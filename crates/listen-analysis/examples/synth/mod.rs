//! Synthetic pop songs with known tempo, beat phase, downbeats, key, vocal bars and sections.
//! Default layout (bars): intro 8, verse 16, breakdown 8, chorus 16, outro 8. Round-trips through
//! an MP3 (ffmpeg) and mpv, so encoder delay is part of the test. [`render`] takes any tempo and layout.

use std::f32::consts::PI;

pub const SR: u32 = 44100;
pub const BARS: [(&str, usize); 5] =
    [("intro", 8), ("verse", 16), ("breakdown", 8), ("chorus", 16), ("outro", 8)];

/// One section: what plays in it.
#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub bars: usize,
    pub drums: bool,
    pub bass: bool,
    pub vocal: bool,
}

impl Part {
    pub const fn new(bars: usize, drums: bool, bass: bool, vocal: bool) -> Self {
        Part { bars, drums, bass, vocal }
    }
}

/// The bench layout ([`BARS`]) as parts.
pub fn default_parts() -> Vec<Part> {
    BARS.iter()
        .map(|&(name, bars)| {
            let band = name != "breakdown" && name != "intro";
            Part::new(bars, band, band, matches!(name, "verse" | "breakdown" | "chorus"))
        })
        .collect()
}

pub struct Truth {
    pub bpm: f32,
    pub parts: Vec<Part>,
    /// First downbeat (s).
    pub start: f32,
    pub tonic: u8,
    pub minor: bool,
    pub four_on_floor: bool,
    /// Global tuning offset from A440 (cents); 0 for [`song`].
    #[allow(dead_code)] // read by examples/keys.rs only
    pub cents: f32,
    /// Index into the progression table ([`key_song`] only).
    #[allow(dead_code)]
    pub prog: usize,
}

impl Truth {
    #[allow(dead_code)]
    pub fn beat(&self, i: usize) -> f32 {
        self.start + i as f32 * 60.0 / self.bpm
    }
    #[allow(dead_code)]
    pub fn bars(&self) -> usize {
        self.parts.iter().map(|p| p.bars).sum()
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f32 / (1u64 << 53) as f32
    }
    fn pick(&mut self, n: usize) -> usize {
        ((self.next() * n as f32) as usize).min(n - 1)
    }
}

fn hz(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

struct Mix {
    l: Vec<f32>,
    r: Vec<f32>,
}

impl Mix {
    fn add(&mut self, at: f32, sig: &[f32], gain_l: f32, gain_r: f32) {
        let s = (at * SR as f32) as usize;
        for (j, x) in sig.iter().enumerate() {
            if s + j < self.l.len() {
                self.l[s + j] += gain_l * x;
                self.r[s + j] += gain_r * x;
            }
        }
    }
}

fn rng(seed: u64) -> Rng {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for _ in 0..4 {
        rng.next();
    }
    rng
}

/// A bench song: random tempo (75–175 BPM), key and groove, default layout.
pub fn song(seed: u64) -> (Truth, Vec<f32>, Vec<f32>) {
    let mut rng = rng(seed);
    let truth = Truth {
        bpm: (75.0 + rng.next() * 100.0).round(),
        parts: default_parts(),
        start: 0.2 + rng.next() * 1.0,
        tonic: rng.pick(12) as u8,
        minor: rng.next() < 0.5,
        four_on_floor: rng.next() < 0.5,
        cents: 0.0,
        prog: 0,
    };
    let (l, r) = render_with(&truth, &mut rng);
    (truth, l, r)
}

/// A song with a given tempo, groove and layout (key and start from `seed`).
#[allow(dead_code)] // used by djset, not by the bench
pub fn render(seed: u64, bpm: f32, four_on_floor: bool, parts: Vec<Part>) -> (Truth, Vec<f32>, Vec<f32>) {
    let mut rng = rng(seed);
    let _ = rng.next();
    let truth = Truth {
        bpm,
        parts,
        start: 0.2 + rng.next() * 1.0,
        tonic: rng.pick(12) as u8,
        minor: rng.next() < 0.5,
        four_on_floor,
        cents: 0.0,
        prog: 0,
    };
    let (l, r) = render_with(&truth, &mut rng);
    (truth, l, r)
}

fn render_with(truth: &Truth, rng: &mut Rng) -> (Vec<f32>, Vec<f32>) {
    let beat = 60.0 / truth.bpm;
    let bars = truth.bars();
    let total = truth.start + bars as f32 * 4.0 * beat + 2.0;
    let n = (total * SR as f32) as usize;
    let mut mix = Mix { l: vec![0.0; n], r: vec![0.0; n] };

    // Instruments.
    let kick: Vec<f32> = (0..(0.3 * SR as f32) as usize)
        .map(|i| {
            let t = i as f32 / SR as f32;
            let f = 45.0 + 75.0 * (-t * 30.0).exp();
            (2.0 * PI * f * t).sin() * (-t * 9.0).exp()
        })
        .collect();
    let mut noise = |len: f32, decay: f32, hp: bool| -> Vec<f32> {
        let mut prev = 0.0;
        (0..(len * SR as f32) as usize)
            .map(|i| {
                let x = rng.next() * 2.0 - 1.0;
                let y = if hp { x - prev } else { x };
                prev = x;
                y * (-(i as f32 / SR as f32) * decay).exp()
            })
            .collect()
    };
    let snare_noise = noise(0.2, 25.0, false);
    let hat = noise(0.05, 80.0, true);
    let snare: Vec<f32> = snare_noise
        .iter()
        .enumerate()
        .map(|(i, x)| {
            let t = i as f32 / SR as f32;
            0.6 * x + 0.5 * (2.0 * PI * 190.0 * t).sin() * (-t * 30.0).exp()
        })
        .collect();
    let tone = |midi: f32, len: f32, harmonics: &[f32], attack: f32, decay: f32, detune: f32| -> Vec<f32> {
        let f = hz(midi) * 2f32.powf(detune / 1200.0);
        (0..(len * SR as f32) as usize)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let env = (t / attack).min(1.0) * (-t * decay).exp() * ((len - t) / 0.02).clamp(0.0, 1.0);
                harmonics
                    .iter()
                    .enumerate()
                    .map(|(h, a)| a * (2.0 * PI * f * (h + 1) as f32 * t).sin())
                    .sum::<f32>()
                    * env
            })
            .collect()
    };

    // Chords: major I–V–vi–IV, minor i–VI–III–VII.
    let prog: [(i32, bool); 4] = if truth.minor {
        [(0, true), (8, false), (3, false), (10, false)]
    } else {
        [(0, false), (7, false), (9, true), (5, false)]
    };
    let tonic = i32::from(truth.tonic);
    let mut bar0 = 0;
    for &Part { bars: len, drums, bass, vocal } in &truth.parts {
        for b in bar0..bar0 + len {
            let t_bar = truth.start + b as f32 * 4.0 * beat;
            let (root, minor) = prog[b % 4];
            let root = tonic + root;
            let third = if minor { 3 } else { 4 };
            // Wide pad: slightly detuned left/right plus a 12 ms Haas delay.
            for iv in [0, third, 7] {
                let m = (60 + (root + iv).rem_euclid(12)) as f32;
                let pad_l = tone(m, 4.0 * beat, &[1.0, 0.3, 0.15], 0.08, 0.2, -2.0);
                let pad_r = tone(m, 4.0 * beat, &[1.0, 0.3, 0.15], 0.08, 0.2, 2.0);
                mix.add(t_bar, &pad_l, 0.05, 0.0);
                mix.add(t_bar + 0.012, &pad_r, 0.0, 0.05);
            }
            for k in 0..8 {
                let t = t_bar + k as f32 * beat / 2.0;
                mix.add(t, &hat, if k % 2 == 0 { 0.12 } else { 0.05 }, if k % 2 == 0 { 0.05 } else { 0.12 });
                if bass {
                    let m = (36 + root.rem_euclid(12)) as f32;
                    mix.add(
                        t,
                        &tone(m, beat / 2.0 * 0.9, &[1.0, 0.5, 0.3, 0.2], 0.005, 4.0, 0.0),
                        0.28,
                        0.28,
                    );
                }
            }
            if drums {
                for q in 0..4 {
                    let t = t_bar + q as f32 * beat;
                    if truth.four_on_floor || q % 2 == 0 {
                        mix.add(t, &kick, 0.8, 0.8);
                    }
                    if q % 2 == 1 {
                        mix.add(t, &snare, 0.35, 0.35);
                    }
                }
            }
            if vocal {
                // Syllables on eighths with gaps, chord tones in octave 5, 5.5 Hz vibrato; centred.
                for k in 0..8 {
                    if rng.next() < 0.25 {
                        continue;
                    }
                    let iv = [0, third, 7, 12][rng.pick(4)];
                    let m = (60 + (root + iv).rem_euclid(12) + 12 * i32::from(rng.next() < 0.3)) as f32;
                    let len = beat / 2.0 * (0.6 + 0.3 * rng.next());
                    let f = hz(m);
                    let syl: Vec<f32> = (0..(len * SR as f32) as usize)
                        .map(|i| {
                            let t = i as f32 / SR as f32;
                            let vib = 1.0 + 0.017 * (2.0 * PI * 5.5 * t).sin();
                            let env = (t / 0.02).min(1.0) * ((len - t) / 0.04).clamp(0.0, 1.0);
                            let ph = 2.0 * PI * f * vib * t;
                            env * (ph.sin()
                                + 0.6 * (2.0 * ph).sin()
                                + 0.4 * (3.0 * ph).sin()
                                + 0.25 * (4.0 * ph).sin())
                        })
                        .collect();
                    mix.add(t_bar + k as f32 * beat / 2.0, &syl, 0.12, 0.12);
                }
            }
        }
        bar0 += len;
    }
    let peak = mix.l.iter().chain(&mix.r).fold(0f32, |m, x| m.max(x.abs())).max(1e-6);
    let g = 0.9 / peak;
    mix.l.iter_mut().chain(mix.r.iter_mut()).for_each(|x| *x *= g);
    (mix.l, mix.r)
}

/// Eight-bar progressions as (root above the tonic, minor chord). Major: I–V–vi–IV, I–IV–V–IV,
/// IV–V–iii–vi ("royal road"), I–vi–IV–V, I–iii–IV–V. Minor: i–VI–III–VII, i–iv–VII–III (… V–i),
/// i–VI–VII–i, i–i–iv–v, i–VII–VI–V (Andalusian), i–iv–i–v.
#[allow(dead_code)]
pub const PROGRESSIONS: [(bool, [(i32, bool); 8]); 11] = [
    (false, [(0, false), (7, false), (9, true), (5, false), (0, false), (7, false), (9, true), (5, false)]),
    (false, [(0, false), (5, false), (7, false), (5, false), (0, false), (5, false), (7, false), (0, false)]),
    (false, [(5, false), (7, false), (4, true), (9, true), (5, false), (7, false), (0, false), (0, false)]),
    (false, [(0, false), (9, true), (5, false), (7, false), (0, false), (9, true), (5, false), (7, false)]),
    (false, [(0, false), (4, true), (5, false), (7, false), (0, false), (2, true), (5, false), (7, false)]),
    (true, [(0, true), (8, false), (3, false), (10, false), (0, true), (8, false), (3, false), (10, false)]),
    (true, [(0, true), (5, true), (10, false), (3, false), (0, true), (5, true), (7, false), (0, true)]),
    (true, [(0, true), (8, false), (10, false), (0, true), (0, true), (8, false), (10, false), (0, true)]),
    (true, [(0, true), (0, true), (5, true), (7, true), (0, true), (0, true), (5, true), (7, true)]),
    (true, [(0, true), (10, false), (8, false), (7, false), (0, true), (10, false), (8, false), (7, false)]),
    (true, [(0, true), (5, true), (0, true), (7, true), (0, true), (5, true), (0, true), (7, true)]),
];

/// Denser arrangement for the key bench: a random [`PROGRESSIONS`] entry, saw pads, root/fifth bass,
/// a vocal line ending phrases on the tonic, and in a third of the songs a tuning offset up to ±30 cents.
/// Layout: intro 4, verse 16, chorus 16, outro 4 bars.
#[allow(dead_code)]
pub fn key_song(seed: u64) -> (Truth, Vec<f32>, Vec<f32>) {
    let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
    for _ in 0..4 {
        rng.next();
    }
    let prog = rng.pick(PROGRESSIONS.len());
    let (minor, chords) = PROGRESSIONS[prog];
    let truth = Truth {
        bpm: (80.0 + rng.next() * 80.0).round(),
        parts: vec![
            Part::new(4, false, false, false),
            Part::new(16, true, true, true),
            Part::new(16, true, true, true),
            Part::new(4, false, true, false),
        ],
        start: 0.2 + rng.next(),
        tonic: rng.pick(12) as u8,
        minor,
        four_on_floor: rng.next() < 0.5,
        cents: if rng.next() < 0.33 { (rng.next() * 2.0 - 1.0) * 30.0 } else { 0.0 },
        prog,
    };
    let beat = 60.0 / truth.bpm;
    let parts = [("intro", 4), ("verse", 16), ("chorus", 16), ("outro", 4)];
    let bars: usize = parts.iter().map(|p| p.1).sum();
    let n = ((truth.start + bars as f32 * 4.0 * beat + 2.0) * SR as f32) as usize;
    let mut mix = Mix { l: vec![0.0; n], r: vec![0.0; n] };
    let tune = 2f32.powf(truth.cents / 1200.0);
    let saw: Vec<f32> = (1..=10).map(|h| 1.0 / h as f32).collect();
    // Tone with `harmonics`, attack/decay envelope, vibrato depth (ratio).
    let tone = |midi: f32, len: f32, harmonics: &[f32], attack: f32, decay: f32, vib: f32| -> Vec<f32> {
        let f = hz(midi) * tune;
        (0..(len * SR as f32) as usize)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let env = (t / attack).min(1.0) * (-t * decay).exp() * ((len - t) / 0.03).clamp(0.0, 1.0);
                let ph = 2.0 * PI * f * (t + vib * (1.0 - (2.0 * PI * 5.5 * t).cos()) / (2.0 * PI * 5.5));
                harmonics.iter().enumerate().map(|(h, a)| a * (ph * (h + 1) as f32).sin()).sum::<f32>() * env
            })
            .collect()
    };
    let noise = |rng: &mut Rng, len: f32, decay: f32, hp: bool| -> Vec<f32> {
        let mut prev = 0.0;
        (0..(len * SR as f32) as usize)
            .map(|i| {
                let x = rng.next() * 2.0 - 1.0;
                let y = if hp { x - prev } else { x };
                prev = x;
                y * (-(i as f32 / SR as f32) * decay).exp()
            })
            .collect()
    };
    let kick: Vec<f32> = (0..(0.3 * SR as f32) as usize)
        .map(|i| {
            let t = i as f32 / SR as f32;
            (2.0 * PI * (45.0 + 75.0 * (-t * 30.0).exp()) * t).sin() * (-t * 9.0).exp()
        })
        .collect();
    let hat = noise(&mut rng, 0.06, 60.0, true);
    let snare = noise(&mut rng, 0.25, 18.0, false);
    let scale: [i32; 7] = if minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] };
    let tonic = i32::from(truth.tonic);
    let mut degree = 0i32; // melody position in scale steps above the tonic (octave 5)
    let mut bar0 = 0;
    for (part, len) in parts {
        let band = part == "verse" || part == "chorus";
        for b in bar0..bar0 + len {
            let t_bar = truth.start + b as f32 * 4.0 * beat;
            let (root, minor_chord) = chords[b % 8];
            let third = if minor_chord { 3 } else { 4 };
            for (j, iv) in [0, third, 7].into_iter().enumerate() {
                let m = (55 + (tonic + root + iv - 55).rem_euclid(12)) as f32;
                let pad = tone(m, 4.0 * beat, &saw[..6], 0.15, 0.1, 0.0);
                let pan = [0.6, 0.3, 0.45][j];
                mix.add(t_bar, &pad, 0.04 * (1.0 - pan), 0.04 * pan);
            }
            if part != "intro" {
                for k in 0..8 {
                    let fifth = k == 6 && rng.next() < 0.4;
                    let m = (36 + (tonic + root + if fifth { 7 } else { 0 }).rem_euclid(12)) as f32;
                    mix.add(
                        t_bar + k as f32 * beat / 2.0,
                        &tone(m, beat * 0.45, &saw[..5], 0.005, 3.0, 0.0),
                        0.2,
                        0.2,
                    );
                }
            }
            for k in 0..8 {
                let t = t_bar + k as f32 * beat / 2.0;
                mix.add(t, &hat, 0.15, 0.12);
                if band && k % 2 == 0 {
                    let q = k / 2;
                    if truth.four_on_floor || q % 2 == 0 {
                        mix.add(t, &kick, 0.8, 0.8);
                    }
                    if q % 2 == 1 {
                        mix.add(t, &snare, 0.45, 0.45);
                    }
                }
            }
            if band {
                // Vocal: steps or chord-tone jumps; the last bar of each 4-bar phrase holds the tonic.
                let phrase_end = b % 4 == 3;
                let mut t = 0.0;
                while t < 4.0 {
                    if phrase_end && t >= 2.0 {
                        degree = if degree > 3 { 7 } else { 0 };
                        let m = (67 + tonic.rem_euclid(12) + 12 * (degree / 7)) as f32;
                        mix.add(
                            t_bar + t * beat,
                            &tone(m, 2.0 * beat, &[1.0, 0.6, 0.4, 0.25, 0.1], 0.03, 0.2, 0.012),
                            0.13,
                            0.13,
                        );
                        break;
                    }
                    let dur = if rng.next() < 0.5 { 0.5 } else { 1.0 };
                    if rng.next() < 0.6 {
                        degree += [-1, 1, -2, 2][rng.pick(4)];
                    } else {
                        // Jump to a chord tone near the current note.
                        let target = [0, third, 7][rng.pick(3)] + root;
                        let pos = scale.iter().position(|&s| s == target.rem_euclid(12)).unwrap_or(0) as i32;
                        degree = (degree.div_euclid(7)) * 7 + pos;
                    }
                    degree = degree.clamp(-2, 9);
                    let m = 67
                        + tonic.rem_euclid(12)
                        + 12 * degree.div_euclid(7)
                        + scale[degree.rem_euclid(7) as usize];
                    if rng.next() < 0.85 {
                        let syl =
                            tone(m as f32, dur * beat * 0.9, &[1.0, 0.6, 0.4, 0.25, 0.1], 0.02, 0.5, 0.012);
                        mix.add(t_bar + t * beat, &syl, 0.13, 0.13);
                    }
                    t += dur;
                }
            }
            if part == "chorus" {
                // Sustained saw lead on chord tones, half notes.
                for h in 0..2 {
                    let iv = [0, third, 7][rng.pick(3)];
                    let m = (72 + (tonic + root + iv).rem_euclid(12)) as f32;
                    let lead = tone(m, 2.0 * beat, &saw, 0.01, 0.3, 0.004);
                    mix.add(t_bar + h as f32 * 2.0 * beat, &lead, 0.03, 0.05);
                }
            }
        }
        bar0 += len;
    }
    let peak = mix.l.iter().chain(&mix.r).fold(0f32, |m, x| m.max(x.abs())).max(1e-6);
    let g = 0.9 / peak;
    mix.l.iter_mut().chain(mix.r.iter_mut()).for_each(|x| *x *= g);
    (truth, mix.l, mix.r)
}

/// Writes the song as a 192 kbit/s MP3 via ffmpeg.
pub fn write_mp3(path: &std::path::Path, l: &[f32], r: &[f32]) -> std::io::Result<()> {
    encode(path, l, r, &[])
}

/// Encodes by extension (`.mp3`: LAME 192k with ID3v2.3, `.flac`: Vorbis
/// comments) with the given tags (`title`, `artist`, `album`, `track`, …).
pub fn encode(path: &std::path::Path, l: &[f32], r: &[f32], tags: &[(&str, String)]) -> std::io::Result<()> {
    use std::io::Write;
    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-loglevel", "error", "-y", "-f", "f32le", "-ar", "44100", "-ac", "2", "-i", "pipe:0"]);
    if path.extension().is_some_and(|e| e == "flac") {
        cmd.args(["-c:a", "flac", "-sample_fmt", "s16"]);
    } else {
        cmd.args(["-c:a", "libmp3lame", "-b:a", "192k", "-id3v2_version", "3"]);
    }
    for (k, v) in tags {
        cmd.arg("-metadata").arg(format!("{k}={v}"));
    }
    let mut child = cmd.arg(path).stdin(std::process::Stdio::piped()).spawn()?;
    let mut bytes = Vec::with_capacity(l.len() * 8);
    for (a, b) in l.iter().zip(r) {
        bytes.extend_from_slice(&a.to_le_bytes());
        bytes.extend_from_slice(&b.to_le_bytes());
    }
    child.stdin.take().expect("stdin").write_all(&bytes)?;
    let status = child.wait()?;
    if status.success() { Ok(()) } else { Err(std::io::Error::other("ffmpeg failed")) }
}
