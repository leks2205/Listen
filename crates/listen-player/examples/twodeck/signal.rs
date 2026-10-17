//! Test signals (click tracks, tones), WAV I/O, onset detection and statistics.

use std::f32::consts::TAU;
use std::path::Path;

pub const RATE: u32 = 48_000;
/// 120 BPM: a beat every 0.5 s, a bar every 2 s.
pub const BEAT: f64 = 0.5;
pub const BAR: f64 = 2.0;
/// Click onsets above this absolute level count (clicks peak at 0.8).
pub const CLICK_THRESHOLD: f32 = 0.25;

/// Interleaved stereo f32.
pub struct Stereo {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl Stereo {
    pub fn silent(secs: f64) -> Self {
        let n = (secs * RATE as f64) as usize;
        Self { l: vec![0.0; n], r: vec![0.0; n] }
    }
}

/// Adds a click (a 5 ms 2 kHz burst with an abrupt start) at every beat from
/// 0.5 s on, plus a quiet 150 Hz bed so the first sample a deck plays is visible.
pub fn add_clicks(ch: &mut [f32], bed: bool) {
    if bed {
        for (i, s) in ch.iter_mut().enumerate() {
            *s += 0.02 * (i as f32 / RATE as f32 * 150.0 * TAU).sin();
        }
    }
    let burst = (0.005 * RATE as f64) as usize;
    let mut k = 1;
    loop {
        let start = (k as f64 * BEAT * RATE as f64).round() as usize;
        if start + burst >= ch.len() {
            break;
        }
        for j in 0..burst {
            let fade = if j + 24 > burst { (burst - j) as f32 / 24.0 } else { 1.0 };
            ch[start + j] += 0.8 * fade * (j as f32 / RATE as f32 * 2000.0 * TAU).sin();
        }
        k += 1;
    }
}

pub fn add_sine(ch: &mut [f32], freq: f32, amp: f32) {
    for (i, s) in ch.iter_mut().enumerate() {
        *s += amp * (i as f32 / RATE as f32 * freq * TAU).sin();
    }
}

/// Writes 16-bit PCM (what the codecs get as input).
pub fn write_wav(path: &Path, s: &Stereo) -> std::io::Result<()> {
    let n = s.l.len() as u32;
    let mut d = Vec::with_capacity(44 + n as usize * 4);
    let p32 = |d: &mut Vec<u8>, v: u32| d.extend_from_slice(&v.to_le_bytes());
    let p16 = |d: &mut Vec<u8>, v: u16| d.extend_from_slice(&v.to_le_bytes());
    d.extend_from_slice(b"RIFF");
    p32(&mut d, 36 + n * 4);
    d.extend_from_slice(b"WAVEfmt ");
    p32(&mut d, 16);
    p16(&mut d, 1);
    p16(&mut d, 2);
    p32(&mut d, RATE);
    p32(&mut d, RATE * 4);
    p16(&mut d, 4);
    p16(&mut d, 16);
    d.extend_from_slice(b"data");
    p32(&mut d, n * 4);
    for (l, r) in s.l.iter().zip(&s.r) {
        for v in [l, r] {
            d.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
    }
    std::fs::write(path, d)
}

/// Reads a stereo WAV (16-bit PCM or 32-bit float). The data chunk runs to the end of the file, since
/// mpv's `--ao=pcm` may leave its size unset.
pub fn read_wav(path: &Path) -> std::io::Result<Stereo> {
    let b = std::fs::read(path)?;
    let bad = || std::io::Error::other(format!("unsupported wav {}", path.display()));
    let u16at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let (mut i, mut fmt, mut channels, mut bits) = (12, 0u16, 0u16, 0u16);
    while i + 8 <= b.len() {
        let size = u32at(i + 4) as usize;
        match &b[i..i + 4] {
            b"fmt " => {
                fmt = u16at(i + 8);
                channels = u16at(i + 10);
                bits = u16at(i + 22);
                if fmt == 0xFFFE {
                    fmt = u16at(i + 32);
                }
            }
            b"data" => {
                let data = &b[i + 8..];
                if channels != 2 {
                    return Err(bad());
                }
                let samples: Vec<f32> = match (fmt, bits) {
                    (1, 16) => data
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0)
                        .collect(),
                    (3, 32) => data.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect(),
                    _ => return Err(bad()),
                };
                return Ok(deinterleave(&samples));
            }
            _ => {}
        }
        i += 8 + size + (size & 1);
    }
    Err(bad())
}

pub fn deinterleave(samples: &[f32]) -> Stereo {
    Stereo {
        l: samples.iter().step_by(2).copied().collect(),
        r: samples.iter().skip(1).step_by(2).copied().collect(),
    }
}

/// Click onsets (in frames, sub-sample interpolated): the first crossing of
/// [`CLICK_THRESHOLD`] after at least 150 ms below it.
pub fn onsets(ch: &[f32], from: usize) -> Vec<f64> {
    onsets_at(ch, from, CLICK_THRESHOLD, 0.15)
}

pub fn onsets_at(ch: &[f32], from: usize, threshold: f32, refractory_secs: f64) -> Vec<f64> {
    let refractory = (refractory_secs * RATE as f64) as usize;
    let mut out = Vec::new();
    let mut last_loud: Option<usize> = None;
    for i in from.max(1)..ch.len() {
        let a = ch[i].abs();
        if a < threshold {
            continue;
        }
        if last_loud.is_none_or(|l| i - l > refractory) {
            let p = ch[i - 1].abs().min(threshold);
            out.push((i - 1) as f64 + ((threshold - p) / (a - p)).clamp(0.0, 1.0) as f64);
        }
        last_loud = Some(i);
    }
    out
}

/// First frame at or after `from` whose level exceeds `threshold` (start of sound).
pub fn first_sound(ch: &[f32], from: usize, threshold: f32) -> Option<usize> {
    ch.iter().skip(from).position(|s| s.abs() > threshold).map(|p| p + from)
}

/// Peak level in windows of `win` frames.
pub fn envelope(ch: &[f32], win: usize) -> Vec<f32> {
    ch.chunks(win).map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs()))).collect()
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    pub n: usize,
    pub mean: f64,
    pub median: f64,
    pub p95_abs: f64,
    pub min: f64,
    pub max: f64,
    pub sd: f64,
}

pub fn stats(v: &[f64]) -> Stats {
    if v.is_empty() {
        return Stats::default();
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    let mean = s.iter().sum::<f64>() / n as f64;
    let sd = (s.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
    let mut abs: Vec<f64> = s.iter().map(|x| x.abs()).collect();
    abs.sort_by(f64::total_cmp);
    let pct = |v: &[f64], p: f64| v[((n as f64 - 1.0) * p).round() as usize];
    Stats { n, mean, median: pct(&s, 0.5), p95_abs: pct(&abs, 0.95), min: s[0], max: s[n - 1], sd }
}

impl std::fmt::Display for Stats {
    /// Values are printed as milliseconds (inputs are seconds).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ms = |x: f64| x * 1000.0;
        write!(
            f,
            "n={} median={:.2} mean={:.2} sd={:.2} p95|x|={:.2} min={:.2} max={:.2} ms",
            self.n,
            ms(self.median),
            ms(self.mean),
            ms(self.sd),
            ms(self.p95_abs),
            ms(self.min),
            ms(self.max)
        )
    }
}

/// Least-squares line `y = a + b x`; returns `(a, b, residuals)`.
pub fn fit(x: &[f64], y: &[f64]) -> (f64, f64, Vec<f64>) {
    let n = x.len() as f64;
    let (mx, my) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
    let sxy: f64 = x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)).sum();
    let sxx: f64 = x.iter().map(|a| (a - mx).powi(2)).sum();
    let b = sxy / sxx;
    let a = my - b * mx;
    let res = x.iter().zip(y).map(|(xi, yi)| yi - (a + b * xi)).collect();
    (a, b, res)
}
