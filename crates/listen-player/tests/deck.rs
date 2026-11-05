//! Deck primitives for DJ mixing, against a real mpv with `--ao=null`.
//! Skipped when no mpv binary is available (`LISTEN_MPV` overrides the path).

use listen_core::settings::{
    DECK_BASS, DECK_HIGHPASS, DECK_LOWPASS, DECK_MIX_LABEL, DECK_TEMPO_FALLBACK, DeckEcho, LEVELLING_LABEL,
    LEVELLING_TARGET, Playback, deck_audio_filters, deck_audio_filters_with, deck_echo_commands,
    with_levelling,
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
    write_wav_at(path, secs, 8000);
}

/// Writes a mono 16-bit PCM WAV at `rate` with a quiet 440 Hz sine.
fn write_wav_at(path: &Path, secs: f32, rate: u32) {
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

/// RMS level (dBFS) of the last frames through the `st` probe after the levelling stage.
async fn probe_rms(f: &Fixture) -> f64 {
    tokio::time::sleep(Duration::from_millis(400)).await;
    let meta = f.player.mpv().get_property("af-metadata/st").await.expect("probe metadata");
    meta["lavfi.astats.Overall.RMS_level"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("no RMS in {meta}"))
}

/// Volume levelling: the limiter stage ends the deck chain, takes the song's gain live as
/// `level_in` once the graph exists, and its delay compensation keeps the cue exact (both
/// decks stay in time). mpv rebuilds the graph from the `af` string on a seek, so the app sends
/// the gain again after every `Restarted`. Needs mpv (not skipped: this is the only check
/// that the gain is audible).
#[tokio::test(flavor = "multi_thread")]
async fn levelling_stage_takes_gain_live() {
    let mut f = deck("lvl").await.expect("this test needs mpv (LISTEN_MPV)");
    let p = Playback::default();
    let probe = ",@st:lavfi=[astats=metadata=1:reset=1:measure_overall=RMS_level:measure_perchannel=none]";
    let chain = with_levelling(&p, deck_audio_filters(&p), 1.0) + probe;
    if f.player.set_af(&chain).await.is_err() {
        let fallback = with_levelling(&p, deck_audio_filters_with(&p, DECK_TEMPO_FALLBACK), 1.0) + probe;
        f.player.set_af(&fallback).await.expect("levelling chain accepted");
    }
    let af = f.player.mpv().get_property("af").await.unwrap();
    let labels: Vec<_> = af.as_array().unwrap().iter().map(|e| e["label"].as_str().unwrap_or("")).collect();
    assert_eq!(labels, ["mix", "ts", LEVELLING_LABEL, "st"]);
    // No graph before a file plays: refused (the app retries).
    assert!(f.player.af_command(LEVELLING_LABEL, "level_in", "0.5", LEVELLING_TARGET).await.is_err());

    f.player.load(&f.track, Some(Duration::from_secs_f64(2.5)), true).await.unwrap();
    wait_for(&mut f.rx, &PlayerEvent::Restarted, &NOT_EXPECTED).await;
    let pts = f.player.audio_pts().await.unwrap().unwrap();
    assert!((pts.pts - 2.5).abs() < 0.01, "cue unaffected by the limiter: {pts:?}");
    f.player.set_paused(false).await.unwrap();
    let unity = probe_rms(&f).await;
    f.player.af_command(LEVELLING_LABEL, "level_in", "0.1", LEVELLING_TARGET).await.expect("live level_in");
    let cut = probe_rms(&f).await;
    assert!((unity - cut - 20.0).abs() < 1.0, "−20 dB live: {unity:.1} → {cut:.1} dBFS");
    // A seek rebuilds the graph from the chain string (level_in=1): the live value is gone.
    f.player.seek(Duration::from_secs(1)).await.unwrap();
    wait_for(&mut f.rx, &PlayerEvent::Restarted, &NOT_EXPECTED).await;
    let rebuilt = probe_rms(&f).await;
    assert!((rebuilt - unity).abs() < 1.0, "rebuilt at the chain's gain: {rebuilt:.1} dBFS");
    f.player.af_command(LEVELLING_LABEL, "level_in", "0.1", LEVELLING_TARGET).await.expect("sent again");
    assert!((probe_rms(&f).await - cut).abs() < 1.0);
    assert!(f.player.af_command(LEVELLING_LABEL, "level_in", "1", "alimiter@x").await.is_err());
}

/// Nothing loaded: no position to report.
#[tokio::test(flavor = "multi_thread")]
async fn audio_pts_is_none_when_idle() {
    let Some(f) = deck("idle").await else { return };
    assert_eq!(f.player.audio_pts().await.unwrap(), None);
}

/// Reads mpv's `--ao=pcm` output (float or 16-bit WAV): first channel, and its rate.
fn read_pcm(path: &Path) -> (Vec<f32>, u32) {
    let d = std::fs::read(path).expect("pcm output");
    let (mut i, mut channels, mut rate, mut bits) = (12, 1usize, 0u32, 16u16);
    let u16_at = |i: usize| u16::from_le_bytes([d[i], d[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]);
    while i + 8 <= d.len() {
        let (id, len) = (&d[i..i + 4], u32_at(i + 4) as usize);
        let body = i + 8;
        if id == b"fmt " {
            channels = u16_at(body + 2) as usize;
            rate = u32_at(body + 4);
            bits = u16_at(body + 14);
        } else if id == b"data" {
            let end = d.len().min(body + len.max(1));
            let data = &d[body..if len == 0 || len == u32::MAX as usize { d.len() } else { end }];
            let width = bits as usize / 8;
            let x = data
                .chunks_exact(width * channels)
                .map(|f| match bits {
                    32 => f32::from_le_bytes([f[0], f[1], f[2], f[3]]),
                    _ => i16::from_le_bytes([f[0], f[1]]) as f32 / 32768.0,
                })
                .collect();
            return (x, rate);
        }
        i = body + len;
    }
    panic!("no data chunk in {path:?}");
}

/// Echo-out on a live deck (bug 66): the send opens a repeat before the stop and the dry goes at the stop,
/// both through the `afade` gates at their scheduled file time, so the stop has no sample step (the
/// stepped `amix` weights jumped ~15x the signal's own slope there). Rendered by mpv to a file.
#[tokio::test(flavor = "multi_thread")]
async fn echo_out_gates_glide_at_their_file_time() {
    let Some(binary) = mpv_binary() else { return };
    let dir = std::env::temp_dir().join(format!("listen-deck-echo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (file, out) = (dir.join("song.wav"), dir.join("out.wav"));
    write_wav_at(&file, 6.0, 48000);
    let extra_args = vec!["--ao=pcm".into(), format!("--ao-pcm-file={}", out.display())];
    let opts = MpvOptions { binary, extra_args, audio_buffer: Some(0.1), ..Default::default() };
    let (player, mut rx) = Player::spawn(&opts).await.expect("mpv starts");
    if player.set_af(&deck_audio_filters(&Playback::default())).await.is_err() {
        let fallback = deck_audio_filters_with(&Playback::default(), DECK_TEMPO_FALLBACK);
        player.set_af(&fallback).await.expect("deck chain accepted");
    }
    // The stop on a crest of the 440 Hz sine (a step there is half its amplitude); the repeats are in phase.
    let (cue, stop, delay) = (1.0, 3.5 + 0.25 / 440.0, 0.5);
    player.load(&Track::url(file.to_string_lossy()), Some(Duration::from_secs_f64(cue)), true).await.unwrap();
    wait_for(&mut rx, &PlayerEvent::Restarted, &NOT_EXPECTED).await;
    // As the app sends them: well before the deck's filter reaches either time.
    let on = DeckEcho { delay_s: delay as f32, taps: [0.5, 0.25, 0.125, 0.0625], send: 1.0, dry: 1.0 };
    let off = DeckEcho { send: 0.0, dry: 0.0, ..on };
    let cmds = [deck_echo_commands(&DeckEcho::IDLE, &on, stop - delay), deck_echo_commands(&on, &off, stop)]
        .concat();
    for (target, cmd, arg) in &cmds {
        player
            .af_command(DECK_MIX_LABEL, cmd, arg, target)
            .await
            .unwrap_or_else(|e| panic!("{target} {cmd}: {e}"));
    }
    player.set_paused(false).await.unwrap();
    wait_for(&mut rx, &PlayerEvent::Ended, &[]).await;
    drop(player);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (x, rate) = read_pcm(&out);
    let _ = std::fs::remove_dir_all(&dir);
    let at = |t: f64| (((t - cue) * rate as f64) as usize).min(x.len());
    let slope = |a: f64, b: f64| x[at(a)..at(b)].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    let rms = |a: f64, b: f64| {
        let s = &x[at(a)..at(b)];
        (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32).sqrt()
    };
    assert!(x.len() as f64 > (5.0 - cue) * rate as f64, "rendered {} samples at {rate} Hz", x.len());
    let natural = slope(1.5, 2.5);
    let dry = rms(1.5, 2.5);
    assert!(dry > 0.05, "dry level {dry}");
    // No step at the stop nor where each repeat hands to the next.
    for t in [stop - delay, stop, stop + delay, stop + 2.0 * delay] {
        let edge = slope(t - 0.05, t + 0.05);
        assert!(edge < 1.3 * natural, "step at {t} s: {edge:.4} vs the sine's own {natural:.4}");
    }
    // Dry alone before the stop, then the repeats at half, a quarter…
    let (r0, r1, r2) = (
        rms(2.6, 2.9),
        rms(stop + 0.05, stop + delay - 0.05),
        rms(stop + delay + 0.05, stop + 2.0 * delay - 0.05),
    );
    assert!((r0 / dry - 1.0).abs() < 0.05, "before the stop {r0} vs {dry}");
    assert!((r1 / dry - 0.5).abs() < 0.05, "first repeat {r1} vs {dry}");
    assert!((r2 / dry - 0.25).abs() < 0.05, "second repeat {r2} vs {dry}");
}
