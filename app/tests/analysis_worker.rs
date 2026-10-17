//! The analysis helper process gives the same results as in-process analysis
//! and stops when idle. Needs mpv; skipped quietly where it isn't installed.

use listen_analysis::{Cancel, Opts, Worker, analyze_url, worker::IDLE_EXIT};
use std::path::Path;

fn have_mpv() -> bool {
    std::process::Command::new("mpv").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// 12 s of 120 BPM clicks over a chord, 16-bit mono.
fn write_song(path: &Path) {
    let sr = 22050u32;
    let samples: Vec<i16> = (0..sr as usize * 12)
        .map(|i| {
            let t = i as f32 / sr as f32;
            let tone = 0.2 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                + 0.15 * (2.0 * std::f32::consts::PI * 277.2 * t).sin();
            let since_click = (t % 0.5) * 1000.0;
            let noise = ((i * 7919) % 200) as f32 / 100.0 - 1.0;
            let click = if since_click < 15.0 { 0.6 * (-since_click / 4.0).exp() * noise } else { 0.0 };
            ((tone + click) * 20000.0) as i16
        })
        .collect();
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&sr.to_le_bytes());
    b.extend_from_slice(&(sr * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

#[tokio::test]
async fn helper_matches_in_process_analysis_and_stops() {
    if !have_mpv() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("listen-worker-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let song = dir.join("song.wav");
    write_song(&song);
    let input = song.to_str().unwrap();
    let opts = Opts { bpm_hint: Some(120.0), ..Opts::default() };

    let direct = analyze_url(input, &opts, &Cancel::default()).await.unwrap();
    let mut worker =
        Worker::with_command(env!("CARGO_BIN_EXE_listen").into(), vec!["--analysis-worker".into()]);
    let via_helper = worker.analyze(input, &opts, &Cancel::default()).await.unwrap();
    assert_eq!(direct, via_helper);
    assert!(worker.is_running());
    // Errors cross the process boundary as the same kind.
    let missing = worker.analyze("/nonexistent/song.wav", &opts, &Cancel::default()).await;
    assert!(missing.is_err());
    // Not idle long enough yet: it stays.
    worker.stop_if_idle().await;
    assert!(worker.is_running());
    worker.stop().await;
    assert!(!worker.is_running());
    // A stopped helper starts again on demand.
    assert_eq!(worker.analyze(input, &opts, &Cancel::default()).await.unwrap(), direct);
    worker.stop().await;
    let _ = IDLE_EXIT;
    std::fs::remove_dir_all(&dir).ok();
}
