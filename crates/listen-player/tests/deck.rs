//! Deck primitives for DJ mixing, against a real mpv with `--ao=null`.
//! Skipped when no mpv binary is available (`LISTEN_MPV` overrides the path).

use listen_core::settings::{
    DECK_BASS, DECK_HIGHPASS, DECK_LOWPASS, DECK_MIX_LABEL, DECK_TEMPO_FALLBACK, Playback,
    deck_audio_filters, deck_audio_filters_with,
};
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

/// Writes a mono 16-bit 8 kHz PCM WAV with a quiet sine.
fn write_wav(path: &Path, secs: f32) {
    let rate = 8000u32;
    let n = (secs * rate as f32) as u32;
    let mut data = Vec::with_capacity(44 + n as usize * 2);
    let push32 = |d: &mut Vec<u8>, v: u32| d.extend_from_slice(&v.to_le_bytes());
    data.extend_from_slice(b"RIFF");
    push32(&mut data, 36 + n * 2);
    data.extend_from_slice(b"WAVEfmt ");
    push32(&mut data, 16);
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    push32(&mut data, rate);
    push32(&mut data, rate * 2);
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    push32(&mut data, n * 2);
    for i in 0..n {
        let s = (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin();
        data.extend_from_slice(&((s * 4000.0) as i16).to_le_bytes());
    }
    std::fs::write(path, data).unwrap();
}

/// Waits for `want`, failing on anything in `forbidden`.
async fn wait_for(rx: &mut UnboundedReceiver<PlayerEvent>, want: &PlayerEvent, forbidden: &[PlayerEvent]) {
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let ev = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {want:?}; saw {seen:?}"))
            .expect("player events closed");
        assert!(!forbidden.contains(&ev), "unexpected {ev:?}; saw {seen:?}");
        if &ev == want {
            return;
        }
        seen.push(ev);
    }
}

struct Fixture {
    dir: PathBuf,
    player: Player,
    rx: UnboundedReceiver<PlayerEvent>,
    track: Track,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn deck(name: &str) -> Option<Fixture> {
    let Some(binary) = mpv_binary() else {
        eprintln!("mpv not found; skipping");
        return None;
    };
    let dir = std::env::temp_dir().join(format!("listen-deck-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("song.wav");
    write_wav(&file, 6.0);
    let opts = MpvOptions { binary, null_audio: true, audio_buffer: Some(0.2), ..Default::default() };
    let (player, rx) = Player::spawn(&opts).await.expect("mpv starts");
    // mpv builds without librubberband reject the default chain; the app falls back the same way.
    if player.set_af(&deck_audio_filters(&Playback::default())).await.is_err() {
        let fallback = deck_audio_filters_with(&Playback::default(), DECK_TEMPO_FALLBACK);
        player.set_af(&fallback).await.expect("deck chain accepted");
    }
    Some(Fixture { dir, player, rx, track: Track::url(file.to_string_lossy()) })
}

const NOT_EXPECTED: [PlayerEvent; 2] = [PlayerEvent::Advanced, PlayerEvent::Ended];

/// A deck paused at a cue stays still, reports `Restarted`, and starts exactly there on unpause.
#[tokio::test(flavor = "multi_thread")]
async fn load_paused_at_cue_then_start() {
    let Some(mut f) = deck("load").await else { return };
    f.player.load(&f.track, Some(Duration::from_secs_f64(2.5)), true).await.unwrap();
    wait_for(&mut f.rx, &PlayerEvent::Restarted, &NOT_EXPECTED).await;

    let pts = f.player.audio_pts().await.unwrap().expect("audio-pts once loaded");
    // Exact with scaletempo2; rubberband reports up to ~6 ms past the cue while
    // paused (its priming), so the mix engine re-reads after starting a deck.
    assert!((pts.pts - 2.5).abs() < 0.01, "cued at 2.5 s: {pts:?}");
    assert!(pts.rtt < Duration::from_millis(500));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let still = f.player.audio_pts().await.unwrap().unwrap();
    assert_eq!(still.pts, pts.pts, "paused deck does not move");

    f.player.set_paused(false).await.unwrap();
    wait_for(&mut f.rx, &PlayerEvent::Paused(false), &[PlayerEvent::Ended]).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let moving = f.player.audio_pts().await.unwrap().unwrap();
    assert!(moving.pts > 2.6 && moving.pts < 3.5, "plays on from the cue: {moving:?}");
}

/// Exact relative seeks land where asked and report `Restarted` again.
#[tokio::test(flavor = "multi_thread")]
async fn relative_exact_seek_restarts() {
    let Some(mut f) = deck("seek").await else { return };
    f.player.load(&f.track, Some(Duration::from_secs(1)), true).await.unwrap();
    wait_for(&mut f.rx, &PlayerEvent::Restarted, &NOT_EXPECTED).await;
    f.player.seek_relative_exact(0.0123).await.unwrap();
    wait_for(&mut f.rx, &PlayerEvent::Restarted, &NOT_EXPECTED).await;
    let pts = f.player.audio_pts().await.unwrap().unwrap();
    assert!((pts.pts - 1.0123).abs() < 0.0005, "sample-exact seek: {pts:?}");
}

/// Speed goes through the permanent `@ts` filter; mix filters take commands by graph name.
#[tokio::test(flavor = "multi_thread")]
async fn speed_and_af_commands_on_the_deck_chain() {
    let Some(mut f) = deck("af").await else { return };
    f.player.load(&f.track, None, false).await.unwrap();
    wait_for(&mut f.rx, &PlayerEvent::Restarted, &[PlayerEvent::Ended]).await;

    for speed in [1.03, 1.0, 0.97] {
        f.player.set_speed(speed).await.unwrap();
        let now = f.player.mpv().get_property("speed").await.unwrap();
        assert_eq!(now.as_f64(), Some(speed));
    }
    // The chain is untouched by speed changes (no auto filter swapping in `af`).
    let af = f.player.mpv().get_property("af").await.unwrap();
    let labels: Vec<_> = af.as_array().unwrap().iter().map(|e| e["label"].as_str().unwrap_or("")).collect();
    assert_eq!(labels, ["mix", "ts"]);

    let cmds = [
        (DECK_BASS, "g", "-24"),
        (DECK_BASS, "g", "0"),
        (DECK_HIGHPASS, "f", "300"),
        (DECK_HIGHPASS, "f", "10"),
        (DECK_LOWPASS, "f", "1000"),
        (DECK_LOWPASS, "w", "0.5"),
    ];
    for (target, cmd, arg) in cmds {
        f.player
            .af_command(DECK_MIX_LABEL, cmd, arg, target)
            .await
            .unwrap_or_else(|e| panic!("{target} {cmd}={arg}: {e}"));
    }
    // A name that isn't in the graph is an error, not a silent no-op.
    assert!(f.player.af_command(DECK_MIX_LABEL, "g", "-6", "lo").await.is_err());
}

/// Nothing loaded: no position to report.
#[tokio::test(flavor = "multi_thread")]
async fn audio_pts_is_none_when_idle() {
    let Some(f) = deck("idle").await else { return };
    assert_eq!(f.player.audio_pts().await.unwrap(), None);
}
