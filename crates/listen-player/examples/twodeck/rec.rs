//! Records the sink's monitor with `pw-record` and maps frames to the host's monotonic clock, so
//! `Instant`-stamped IPC calls can be compared with the graph's output. A read ending at frame `f`
//! arriving at `t` gives `t - f/rate` = offset + delivery delay ≥ 0, so local minima are the offset.
//! The dummy driver runs on CLOCK_MONOTONIC: no drift, bias under one quantum.

use crate::signal::{RATE, Stereo, deinterleave};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Default)]
struct Shared {
    samples: Vec<f32>,
    /// `(end frame, arrival in seconds since t0)` per read.
    chunks: Vec<(u64, f64)>,
}

pub struct Recorder {
    child: Child,
    shared: Arc<Mutex<Shared>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

pub struct Recording {
    pub audio: Stereo,
    /// `(frame, offset)` local minima, sorted by frame.
    offsets: Vec<(u64, f64)>,
}

impl Recorder {
    /// `quantum`: node latency asked of pw-record (frames); sets the cycle, bounds the mapping error.
    pub fn start(sink: &str, quantum: u32, t0: Instant) -> std::io::Result<Self> {
        let mut child = Command::new("pw-record")
            .args(["--target", sink, "-P", "stream.capture.sink=true", "--rate", &RATE.to_string()])
            .args(["--channels", "2", "--format", "f32", "--raw", "--latency", &quantum.to_string(), "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut out = child.stdout.take().expect("piped");
        let shared = Arc::new(Mutex::new(Shared::default()));
        let sh = shared.clone();
        let reader = std::thread::spawn(move || {
            let mut buf = vec![0u8; 1 << 16];
            let mut carry: Vec<u8> = Vec::new();
            let mut frames = 0u64;
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let at = t0.elapsed().as_secs_f64();
                carry.extend_from_slice(&buf[..n]);
                let whole = carry.len() / 8 * 8;
                let mut s = sh.lock().unwrap();
                s.samples.extend(carry[..whole].as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)));
                carry.drain(..whole);
                frames += whole as u64 / 8;
                s.chunks.push((frames, at));
            }
        });
        Ok(Self { child, shared, reader: Some(reader) })
    }

    /// Frames recorded so far (to know where an experiment starts).
    pub fn frames(&self) -> usize {
        self.shared.lock().unwrap().samples.len() / 2
    }

    /// Waits until the stream is linked and data flows.
    pub fn wait_running(&self) {
        let start = Instant::now();
        while self.frames() < RATE as usize / 2 {
            assert!(start.elapsed().as_secs() < 10, "pw-record gets no data; is the sink there?");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    pub fn stop(mut self) -> Recording {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        let s = std::mem::take(&mut *self.shared.lock().unwrap());
        // Local minima of arrival - frame/rate over 0.1 s windows (a dropped cycle spoils only its window).
        let raw: Vec<(u64, f64)> = s.chunks.iter().map(|&(f, at)| (f, at - f as f64 / RATE as f64)).collect();
        let win = RATE as u64 / 10;
        let mut offsets = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            let start = if i == 0 { 0 } else { raw[i - 1].0 };
            let mut j = i;
            let mut best = f64::INFINITY;
            while j < raw.len() && raw[j].0 < start + win {
                best = best.min(raw[j].1);
                j += 1;
            }
            offsets.push((start, best));
            i = j.max(i + 1);
        }
        Recording { audio: deinterleave(&s.samples), offsets }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Recording {
    /// Host time (seconds since t0) at which `frame` left the graph.
    pub fn time_of(&self, frame: f64) -> f64 {
        // The window that contains the frame.
        let idx = self.offsets.partition_point(|&(f, _)| (f as f64) <= frame).saturating_sub(1);
        self.offsets[idx].1 + frame / RATE as f64
    }

    /// Windows whose offset jumped more than 1 ms from the previous one (xruns; nearby timings are suspect).
    pub fn jumps(&self) -> usize {
        self.offsets.windows(2).filter(|w| (w[1].1 - w[0].1).abs() > 0.001).count()
    }

    /// The frame that left the graph at host time `secs` since t0.
    pub fn frame_at(&self, secs: f64) -> f64 {
        // Two fixed-point steps: offsets only change across lost cycles.
        let guess = (secs - self.offsets.first().map_or(0.0, |o| o.1)) * RATE as f64;
        let guess = guess + (secs - self.time_of(guess)) * RATE as f64;
        guess + (secs - self.time_of(guess)) * RATE as f64
    }

    /// Writes `frame offset_ms` lines (debugging the clock mapping).
    pub fn dump_offsets(&self, path: &std::path::Path) {
        let s: String = self.offsets.iter().map(|(f, o)| format!("{f} {:.3}\n", o * 1000.0)).collect();
        let _ = std::fs::write(path, s);
    }

    /// Spread of the per-window offsets: a sanity check of the clock mapping.
    pub fn offset_spread_ms(&self) -> f64 {
        let (min, max) = self
            .offsets
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), o| (a.min(o.1), b.max(o.1)));
        (max - min) * 1000.0
    }

    pub fn save(&self, path: &std::path::Path) {
        let _ = crate::signal::write_wav(path, &self.audio);
    }
}
