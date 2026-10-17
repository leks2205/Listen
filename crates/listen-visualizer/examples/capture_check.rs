//! Checks on a real PipeWire system that the visualizer never records the
//! microphone: a silent (volume 0) test stream "ListenCaptureCheck" plays for 3 s,
//! the capture records it, then the stream ends. Prints only where the
//! recorder is linked from and how many samples arrived; audio is discarded.

#[cfg(target_os = "linux")]
use listen_visualizer::capture;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};
#[cfg(target_os = "linux")]
use tokio::process::Command;

#[cfg(target_os = "linux")]
async fn topology() -> Vec<(String, String)> {
    let out = Command::new("pw-dump").output().await.unwrap();
    let objects: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    let name_of = |id: u64| {
        objects
            .iter()
            .find(|o| o["id"].as_u64() == Some(id))
            .map(|o| {
                let p = &o["info"]["props"];
                format!(
                    "{} ({})",
                    p["node.name"].as_str().unwrap_or("?"),
                    p["media.class"].as_str().unwrap_or("?")
                )
            })
            .unwrap_or_else(|| id.to_string())
    };
    let recorders: Vec<u64> = objects
        .iter()
        .filter(|o| {
            let n = o["info"]["props"]["node.name"].as_str().unwrap_or("");
            n == "listen-visualizer" || n == "old-style-recorder"
        })
        .filter_map(|o| o["id"].as_u64())
        .collect();
    objects
        .iter()
        .filter(|o| o["type"] == "PipeWire:Interface:Link")
        .filter(|o| o["info"]["input-node-id"].as_u64().is_some_and(|i| recorders.contains(&i)))
        .map(|o| {
            (
                name_of(o["info"]["input-node-id"].as_u64().unwrap()),
                name_of(o["info"]["output-node-id"].as_u64().unwrap()),
            )
        })
        .collect()
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() {
    let wav = std::env::temp_dir().join("listen-capture-check.wav");
    // 3 s of a 440 Hz tone (played at volume 0).
    let rate = 48000u32;
    let n = rate * 3;
    let mut d = Vec::new();
    d.extend_from_slice(b"RIFF");
    d.extend_from_slice(&(36 + n * 2).to_le_bytes());
    d.extend_from_slice(b"WAVEfmt ");
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&rate.to_le_bytes());
    d.extend_from_slice(&(rate * 2).to_le_bytes());
    d.extend_from_slice(&2u16.to_le_bytes());
    d.extend_from_slice(&16u16.to_le_bytes());
    d.extend_from_slice(b"data");
    d.extend_from_slice(&(n * 2).to_le_bytes());
    for i in 0..n {
        let s = ((i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16;
        d.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(&wav, d).unwrap();

    let old_style = std::env::args().any(|a| a == "--old");
    let mut player = Command::new("pw-play")
        .args(["--volume", "0", "-P", "{ node.name = ListenCaptureCheck }"])
        .arg(&wav)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let mut old_recorder = None;
    if old_style {
        // Old-style recorder: `--target` only, no dont-reconnect.
        tokio::time::sleep(Duration::from_millis(800)).await;
        let node = capture::find_stream("ListenCaptureCheck").await.expect("test stream");
        old_recorder = Some(
            Command::new("pw-record")
                .args(["--target", &node.serial, "-P", "{ node.name = old-style-recorder }", "/dev/null"])
                .kill_on_drop(true)
                .spawn()
                .unwrap(),
        );
    } else {
        tokio::spawn(async move { capture::pipewire("ListenCaptureCheck", tx).await });
    }
    let started = Instant::now();
    let mut samples_while_playing = 0usize;
    let mut samples_after = 0usize;
    let mut stream_ended = None;
    let mut foreign_links = 0usize;
    let mut polls = 0usize;
    let mut last_print = Instant::now();
    while started.elapsed() < Duration::from_secs(8) {
        tokio::time::sleep(Duration::from_millis(50)).await;
        while let Ok(chunk) = rx.try_recv() {
            if stream_ended.is_some() {
                samples_after += chunk.len();
            } else {
                samples_while_playing += chunk.len();
            }
        }
        if stream_ended.is_none() && player.try_wait().ok().flatten().is_some() {
            stream_ended = Some(started.elapsed());
        }
        let t = topology().await;
        polls += 1;
        foreign_links += t.iter().filter(|(_, from)| !from.starts_with("ListenCaptureCheck")).count();
        if last_print.elapsed() > Duration::from_millis(500) {
            last_print = Instant::now();
            println!(
                "{:>4.1}s stream {}: {:?}",
                started.elapsed().as_secs_f32(),
                if stream_ended.is_some() { "ended  " } else { "playing" },
                t
            );
        }
    }
    println!("samples while playing: {samples_while_playing}, after the stream ended: {samples_after}");
    println!("topology polls: {polls}, links from anything other than the test stream: {foreign_links}");
    drop(old_recorder);
    let _ = std::fs::remove_file(&wav);
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("capture_check needs PipeWire (Linux)");
}
