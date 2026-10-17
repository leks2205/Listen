//! Drives a real mpv (with `--ao=null`) through the two-slot model.
//! Skipped when no mpv binary is available (`LISTEN_MPV` overrides the path).

use listen_player::{MpvOptions, Player, PlayerEvent, Track};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

fn mpv_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LISTEN_MPV") {
        return Some(p.into());
    }
    std::process::Command::new("mpv").arg("--version").output().ok()?;
    Some("mpv".into())
}

/// Writes a mono 16-bit PCM WAV with a sine tone.
fn write_wav(path: &Path, secs: f32, freq: f32) {
    let rate = 8000u32;
    let n = (secs * rate as f32) as u32;
    let mut data = Vec::with_capacity(44 + n as usize * 2);
    let push32 = |d: &mut Vec<u8>, v: u32| d.extend_from_slice(&v.to_le_bytes());
    data.extend_from_slice(b"RIFF");
    push32(&mut data, 36 + n * 2);
    data.extend_from_slice(b"WAVEfmt ");
    push32(&mut data, 16);
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    data.extend_from_slice(&1u16.to_le_bytes()); // mono
    push32(&mut data, rate);
    push32(&mut data, rate * 2);
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    push32(&mut data, n * 2);
    for i in 0..n {
        let s = (i as f32 / rate as f32 * freq * std::f32::consts::TAU).sin();
        data.extend_from_slice(&((s * 8000.0) as i16).to_le_bytes());
    }
    std::fs::write(path, data).unwrap();
}

async fn wait_for(rx: &mut UnboundedReceiver<PlayerEvent>, want: &PlayerEvent) -> Vec<PlayerEvent> {
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let ev = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {want:?}; saw {seen:?}"))
            .expect("player events closed");
        let done = &ev == want;
        seen.push(ev);
        if done {
            return seen;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_slot_gapless_playback() {
    let Some(binary) = mpv_binary() else {
        eprintln!("mpv not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("listen-player-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let files: Vec<PathBuf> = (0..3).map(|i| dir.join(format!("t{i}.wav"))).collect();
    for (i, f) in files.iter().enumerate() {
        write_wav(f, 1.0, 330.0 + 110.0 * i as f32);
    }
    let track = |i: usize| Track::url(files[i].to_string_lossy());

    let opts = MpvOptions { binary, null_audio: true, ..Default::default() };
    let (player, mut rx) = Player::spawn(&opts).await.expect("mpv starts");

    player.play(&track(0), Some(&track(1))).await.unwrap();
    wait_for(&mut rx, &PlayerEvent::Paused(false)).await;

    // mpv moves to the prefetched second file on its own…
    wait_for(&mut rx, &PlayerEvent::Advanced).await;
    // …and the app then rotates the slots, queueing the third file.
    player.advanced(Some(&track(2))).await.unwrap();
    wait_for(&mut rx, &PlayerEvent::Advanced).await;
    player.advanced(None).await.unwrap();

    // After the last file the queue ends.
    wait_for(&mut rx, &PlayerEvent::Ended).await;

    player.mpv().command(serde_json::json!(["quit"])).await.ok();
    std::fs::remove_dir_all(dir).ok();
}

/// A failed load gives one Error, no advance to the prefetched entry, and `play` works after it.
#[tokio::test(flavor = "multi_thread")]
async fn failed_track_is_reported_once_without_advancing() {
    let Some(binary) = mpv_binary() else {
        eprintln!("mpv not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("listen-player-fail-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("good.wav");
    write_wav(&good, 1.0, 440.0);
    let bad = Track::url(dir.join("missing.mp3").to_string_lossy());
    let good = Track::url(good.to_string_lossy());

    let opts = MpvOptions { binary, null_audio: true, ..Default::default() };
    let (player, mut rx) = Player::spawn(&opts).await.expect("mpv starts");
    player.play(&bad, Some(&good)).await.unwrap();

    // Collect everything for a few seconds (the good file would have ended by then).
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    while let Ok(Some(ev)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        seen.push(ev);
    }
    let errors = seen.iter().filter(|e| matches!(e, PlayerEvent::Error(_))).count();
    assert_eq!(errors, 1, "one error expected: {seen:?}");
    assert!(!seen.contains(&PlayerEvent::Advanced), "no advance after a failure: {seen:?}");
    assert!(!seen.contains(&PlayerEvent::Ended), "no 'ended' after a failure: {seen:?}");

    // The app's retry/skip: a new play works and advances normally.
    let other = dir.join("other.wav");
    write_wav(&other, 1.0, 550.0);
    player.play(&good, Some(&Track::url(other.to_string_lossy()))).await.unwrap();
    wait_for(&mut rx, &PlayerEvent::Advanced).await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// A voice clip (other sample rate, per-file options) in the next slot advances like a track.
#[tokio::test(flavor = "multi_thread")]
async fn voice_clip_between_songs() {
    let Some(binary) = mpv_binary() else {
        eprintln!("mpv not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("listen-player-clip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, clip, b) = (dir.join("a.wav"), dir.join("clip.wav"), dir.join("b.wav"));
    write_wav(&a, 1.0, 330.0);
    write_wav(&clip, 0.8, 200.0);
    write_wav(&b, 1.0, 440.0);

    let opts = MpvOptions { binary, null_audio: true, ..Default::default() };
    let (player, mut rx) = Player::spawn(&opts).await.expect("mpv starts");
    player.play(&Track::url(a.to_string_lossy()), None).await.unwrap();
    wait_for(&mut rx, &PlayerEvent::Paused(false)).await;
    // Arm the clip while the song plays, then the next song once it airs.
    player.set_next(Some(&Track::voice_clip(clip.to_string_lossy()))).await.unwrap();
    wait_for(&mut rx, &PlayerEvent::Advanced).await;
    let path = player.mpv().get_property("path").await.unwrap();
    assert!(path.as_str().is_some_and(|p| p.ends_with("clip.wav")), "clip airs: {path}");
    player.advanced(Some(&Track::url(b.to_string_lossy()))).await.unwrap();
    wait_for(&mut rx, &PlayerEvent::Advanced).await;
    // The filter chain is back after the clip (per-file options are local).
    let path = player.mpv().get_property("path").await.unwrap();
    assert!(path.as_str().is_some_and(|p| p.ends_with("b.wav")), "song after clip: {path}");
    player.advanced(None).await.unwrap();
    wait_for(&mut rx, &PlayerEvent::Ended).await;
    player.mpv().command(serde_json::json!(["quit"])).await.ok();
    std::fs::remove_dir_all(dir).ok();
}

/// Clearing the next slot right after a gapless switch (before `advanced`) keeps the new song.
#[tokio::test(flavor = "multi_thread")]
async fn clearing_the_next_slot_right_after_a_switch_keeps_the_new_song() {
    let Some(binary) = mpv_binary() else {
        eprintln!("mpv not found; skipping");
        return;
    };
    let dir = std::env::temp_dir().join(format!("listen-player-race-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    write_wav(&a, 0.6, 330.0);
    write_wav(&b, 3.0, 440.0);
    let opts = MpvOptions { binary, null_audio: true, ..Default::default() };
    let (player, mut rx) = Player::spawn(&opts).await.expect("mpv starts");
    player.play(&Track::url(a.to_string_lossy()), Some(&Track::url(b.to_string_lossy()))).await.unwrap();
    wait_for(&mut rx, &PlayerEvent::Advanced).await;
    // The "set_next(None)" lands before `advanced` dropped entry 0.
    player.set_next(None).await.unwrap();
    player.advanced(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let path = player.mpv().get_property("path").await.unwrap();
    assert!(path.as_str().is_some_and(|p| p.ends_with("b.wav")), "b keeps playing: {path}");
    wait_for(&mut rx, &PlayerEvent::Ended).await;
    player.mpv().command(serde_json::json!(["quit"])).await.ok();
    std::fs::remove_dir_all(dir).ok();
}
