#![cfg(target_os = "linux")]
//! Opt-in (`LISTEN_VIZ_LIVE=1`, needs PipeWire + mpv): plays a quiet test tone
//! through an mpv stream named "Listen" and checks the analyzer sees it.
use listen_visualizer::{Analyzer, Config, capture};

#[tokio::test]
async fn captures_listens_own_stream() {
    if std::env::var("LISTEN_VIZ_LIVE").is_err() {
        return;
    }
    let mut mpv = tokio::process::Command::new("mpv")
        .args(["--no-config", "--no-video", "--volume=4", "--audio-client-name=Listen"])
        .arg("av://lavfi:sine=frequency=1000:duration=3")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("mpv");
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let task = tokio::spawn(async move { capture::pipewire("Listen", tx).await });
    let mut analyzer = Analyzer::new(Config::default());
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(2500);
    let mut samples = 0usize;
    let mut max_abs = 0.0f32;
    let mut best = 0.0f32;
    while let Ok(Some(chunk)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        samples += chunk.len();
        max_abs = chunk.iter().fold(max_abs, |m, v| m.max(v.abs()));
        analyzer.push_stereo(&chunk);
        // Like the app: a frame per chunk (~every 20 ms).
        best = analyzer.frame().levels.iter().cloned().fold(best, f32::max);
    }
    println!("max |sample| {max_abs}, stream {:?}", capture::find_stream("Listen").await);
    let peak = best;
    println!("captured {samples} samples, peak level {peak:.2}");
    task.abort();
    let _ = mpv.kill().await;
    assert!(samples > 48_000, "got audio");
    assert!(peak > 0.1, "tone visible in the spectrum");
}

/// Opt-in: two decks ("Listen" 1 kHz, "Listen B" 5 kHz) are each captured and tagged with their index.
#[tokio::test]
async fn captures_both_decks() {
    if std::env::var("LISTEN_VIZ_LIVE").is_err() {
        return;
    }
    let deck = |name: &str, hz: u32| {
        tokio::process::Command::new("mpv")
            .args(["--no-config", "--no-video", "--volume=4", &format!("--audio-client-name={name}")])
            .arg(format!("av://lavfi:sine=frequency={hz}:duration=3"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("mpv")
    };
    let (_a, _b) = (deck("Listen", 1000), deck("Listen B", 5000));
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let task = tokio::spawn(async move { capture::pipewire_many(&["Listen", "Listen B"], tx).await });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(2500);
    let mut samples = [0usize; 2];
    while let Ok(Some((deck, chunk))) = tokio::time::timeout_at(deadline, rx.recv()).await {
        samples[deck] += chunk.len();
    }
    task.abort();
    println!("captured per deck: {samples:?}");
    assert!(samples.iter().all(|&n| n > 48_000), "both decks captured");
}
