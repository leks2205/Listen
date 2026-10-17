//! One STFT pass over the song that yields every frame-level feature the
//! later stages need (bands, centroid, chroma, onset strength, MFCCs).

use crate::decode::Pcm;
use realfft::RealFftPlanner;

pub const N_FFT: usize = 2048;
/// 256 samples at 22050 Hz = 86 frames/s: ~12 ms beat resolution.
pub const HOP: usize = 256;
pub const N_MFCC: usize = 13;
const N_MEL: usize = 40;
/// Bins per octave of [`Frames::fine_chroma`].
pub const FINE: usize = 36;

/// Frame-level features, all vectors of equal length (one per hop).
#[derive(Debug, Clone, Default)]
pub struct Frames {
    pub sample_rate: u32,
    /// Seconds per frame.
    pub hop_secs: f32,
    /// Power below 150 Hz (kick, bass), mid channel.
    pub low: Vec<f32>,
    /// Power in the 300–3400 Hz voice band, mid (L+R) and side (L−R).
    pub voice_mid: Vec<f32>,
    pub voice_side: Vec<f32>,
    /// Power of the centre (|M| − |S| per bin) in spectral peaks between
    /// 500 and 3400 Hz: pitched, centre-panned sound in the formant range
    /// (lead vocals; not drums, bass harmonics or wide pads).
    pub voice_tonal: Vec<f32>,
    /// Total power, mid channel.
    pub power: Vec<f32>,
    /// Spectral centroid, Hz.
    pub centroid: Vec<f32>,
    /// 85 % roll-off frequency, Hz.
    pub rolloff: Vec<f32>,
    /// Spectral flatness 0..1 (noise-like = 1).
    pub flatness: Vec<f32>,
    /// Pitch-class energy (C = 0) of spectral peaks, 100 Hz–5 kHz.
    pub chroma: Vec<[f32; 12]>,
    /// Spectral peaks on the pitch-class circle in thirds of a semitone (bin 0 = C at A440, linearly
    /// interpolated), for tuning estimation before folding to 12. `fine_chroma`: 100 Hz–5 kHz;
    /// `bass_chroma`: 30–250 Hz (the bassline names chord roots).
    pub fine_chroma: Vec<[f32; FINE]>,
    pub bass_chroma: Vec<[f32; FINE]>,
    /// Onset strength: summed positive log-magnitude change.
    pub flux: Vec<f32>,
    /// Same, below 150 Hz only (kicks mark downbeats in pop).
    pub low_flux: Vec<f32>,
    pub mfcc: Vec<[f32; N_MFCC]>,
}

impl Frames {
    pub fn len(&self) -> usize {
        self.power.len()
    }

    pub fn is_empty(&self) -> bool {
        self.power.is_empty()
    }

    /// Frame index of time `t` (clamped).
    pub fn frame_at(&self, t: f32) -> usize {
        ((t / self.hop_secs).round().max(0.0) as usize).min(self.len().saturating_sub(1))
    }

    /// Frames covering `[a, b)` seconds.
    pub fn range(&self, a: f32, b: f32) -> std::ops::Range<usize> {
        let n = self.len();
        let s = ((a / self.hop_secs).floor().max(0.0) as usize).min(n);
        let e = ((b / self.hop_secs).ceil().max(0.0) as usize).clamp(s, n);
        s..e
    }
}

fn hz_to_mel(f: f32) -> f32 {
    2595.0 * (1.0 + f / 700.0).log10()
}

fn mel_to_hz(m: f32) -> f32 {
    700.0 * (10f32.powf(m / 2595.0) - 1.0)
}

/// Triangular mel filters as (first bin, weights).
fn mel_filters(sr: f32, n_bins: usize) -> Vec<(usize, Vec<f32>)> {
    let bin_hz = sr / N_FFT as f32;
    let (lo, hi) = (hz_to_mel(30.0), hz_to_mel(sr / 2.0 * 0.95));
    let points: Vec<f32> =
        (0..N_MEL + 2).map(|i| mel_to_hz(lo + (hi - lo) * i as f32 / (N_MEL + 1) as f32)).collect();
    (0..N_MEL)
        .map(|m| {
            let (a, c, b) = (points[m], points[m + 1], points[m + 2]);
            let first = ((a / bin_hz).floor() as usize).min(n_bins - 1);
            let last = ((b / bin_hz).ceil() as usize).min(n_bins - 1);
            let w = (first..=last)
                .map(|k| {
                    let f = k as f32 * bin_hz;
                    if f <= c { (f - a) / (c - a) } else { (b - f) / (b - c) }.max(0.0)
                })
                .collect();
            (first, w)
        })
        .collect()
}

/// Computes all frame features. Cost: two real FFTs per frame (~0.1 s for a
/// 4-minute song in release builds).
pub fn analyze(pcm: &Pcm) -> Frames {
    let sr = pcm.sample_rate as f32;
    let n = pcm.left.len();
    let n_frames = if n == 0 { 0 } else { n / HOP + 1 };
    let n_bins = N_FFT / 2 + 1;
    let bin_hz = sr / N_FFT as f32;
    let window: Vec<f32> = (0..N_FFT)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos())
        .collect();
    let bin = |hz: f32| ((hz / bin_hz).round() as usize).min(n_bins - 1);
    let (low_hi, voice_lo, voice_hi) = (bin(150.0), bin(300.0), bin(3400.0));
    // Chroma from spectral peaks only (drums and noise stay out), from
    // 100 Hz: below that, bins are wider than a semitone and kicks dominate.
    let (chroma_lo, chroma_hi) = (bin(100.0).max(1), bin(5000.0));
    let fine_lo = bin(30.0).max(1);
    // Tonal centre energy from 500 Hz: above bass harmonics, in the formants.
    let tonal_from = bin(500.0) - voice_lo;
    let mels = mel_filters(sr, n_bins);
    // DCT-II basis for the MFCCs.
    let dct: Vec<[f32; N_MEL]> = (0..N_MFCC)
        .map(|c| {
            std::array::from_fn(|m| (std::f32::consts::PI * c as f32 * (m as f32 + 0.5) / N_MEL as f32).cos())
        })
        .collect();

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N_FFT);
    let (mut buf, mut spec) = (fft.make_input_vec(), fft.make_output_vec());
    let mut scratch = fft.make_scratch_vec();
    let mut mag = vec![0f32; n_bins];
    let mut prev_log = vec![0f32; n_bins];
    let mut centre = vec![0f32; voice_hi - voice_lo + 1];

    let mut f = Frames { sample_rate: pcm.sample_rate, hop_secs: HOP as f32 / sr, ..Default::default() };
    for v in [&mut f.low, &mut f.voice_mid, &mut f.voice_side, &mut f.power, &mut f.centroid] {
        v.reserve(n_frames);
    }

    for i in 0..n_frames {
        let start = (i * HOP) as isize - (N_FFT / 2) as isize;
        // Mid channel.
        let fill = |buf: &mut [f32], side: bool| {
            for (j, (b, w)) in buf.iter_mut().zip(&window).enumerate() {
                let s = start + j as isize;
                *b = if s < 0 || s as usize >= n {
                    0.0
                } else {
                    let (l, r) = (pcm.left[s as usize], pcm.right[s as usize]);
                    w * if side { 0.5 * (l - r) } else { 0.5 * (l + r) }
                };
            }
        };
        fill(&mut buf, false);
        fft.process_with_scratch(&mut buf, &mut spec, &mut scratch).expect("fft sizes");
        let norm = 2.0 / N_FFT as f32;
        for (m, c) in mag.iter_mut().zip(&spec) {
            *m = c.norm() * norm;
        }

        let (mut power, mut low, mut voice, mut weighted, mut flux, mut low_flux) =
            (0f32, 0f32, 0f32, 0f32, 0f32, 0f32);
        let mut chroma = [0f32; 12];
        let (mut log_sum, mut lin_sum) = (0f32, 0f32);
        for (k, &m) in mag.iter().enumerate() {
            let p = m * m;
            power += p;
            weighted += p * k as f32 * bin_hz;
            if k <= low_hi {
                low += p;
            }
            if (voice_lo..=voice_hi).contains(&k) {
                voice += p;
            }
            if (chroma_lo..=chroma_hi).contains(&k) && m > mag[k - 1] && m >= mag[k + 1] && m > 1e-5 {
                // Parabolic interpolation on log magnitude for the true peak frequency.
                let (a, b, c) = (mag[k - 1].max(1e-9).ln(), m.ln(), mag[k + 1].max(1e-9).ln());
                let den = a - 2.0 * b + c;
                let delta = if den.abs() > 1e-9 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
                let hz = (k as f32 + delta) * bin_hz;
                let pc = ((12.0 * (hz / 440.0).log2()).round() as i32 + 69).rem_euclid(12) as usize;
                chroma[pc] += m;
            }
            let lg = (1.0 + 1000.0 * m).ln();
            let d = (lg - prev_log[k]).max(0.0);
            flux += d;
            if (1..=low_hi).contains(&k) {
                low_flux += d;
            }
            prev_log[k] = lg;
            if k > 0 {
                log_sum += (p + 1e-12).ln();
                lin_sum += p + 1e-12;
            }
        }
        // Fine chroma: every spectral peak (no prominence filter), soft-binned in 1/3 semitones.
        let mut fine = [0f32; FINE];
        let mut bass = [0f32; FINE];
        for k in fine_lo..=chroma_hi {
            let m = mag[k];
            if !(m > mag[k - 1] && m >= mag[k + 1] && m > 1e-5) {
                continue;
            }
            let (a, b, c) = (mag[k - 1].max(1e-9).ln(), m.ln(), mag[k + 1].max(1e-9).ln());
            let den = a - 2.0 * b + c;
            let delta = if den.abs() > 1e-9 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
            let hz = (k as f32 + delta) * bin_hz;
            let pos = ((12.0 * (hz / 440.0).log2() + 69.0) * (FINE / 12) as f32).rem_euclid(FINE as f32);
            let (i0, frac) = (pos.floor() as usize % FINE, pos.fract());
            for (target, lo, hi) in [(&mut fine, 100.0, 5000.0), (&mut bass, 30.0, 250.0)] {
                if (lo..hi).contains(&hz) {
                    target[i0] += m * (1.0 - frac);
                    target[(i0 + 1) % FINE] += m * frac;
                }
            }
        }
        let rolloff = {
            let target = 0.85 * power;
            let mut acc = 0.0;
            let mut k = 0;
            while k < n_bins && acc < target {
                acc += mag[k] * mag[k];
                k += 1;
            }
            k as f32 * bin_hz
        };
        let nb = (n_bins - 1) as f32;
        let flatness = ((log_sum / nb).exp() / (lin_sum / nb)).clamp(0.0, 1.0);
        let mel_log: [f32; N_MEL] = std::array::from_fn(|m| {
            let (first, w) = &mels[m];
            let e: f32 = w.iter().enumerate().map(|(j, w)| w * mag[first + j] * mag[first + j]).sum();
            (e + 1e-10).ln()
        });
        let mfcc: [f32; N_MFCC] =
            std::array::from_fn(|c| dct[c].iter().zip(&mel_log).map(|(a, b)| a * b).sum());

        // Side channel: only the voice band is needed.
        fill(&mut buf, true);
        fft.process_with_scratch(&mut buf, &mut spec, &mut scratch).expect("fft sizes");
        let mut side_voice = 0f32;
        for k in voice_lo..=voice_hi {
            let s = spec[k].norm() * norm;
            side_voice += s * s;
            centre[k - voice_lo] = (mag[k] - s).max(0.0);
        }
        let mut tonal = 0f32;
        for j in tonal_from.max(1)..centre.len() - 1 {
            let c = centre[j];
            if c > centre[j - 1] && c >= centre[j + 1] {
                let (a, b) = (j.saturating_sub(8), (j + 9).min(centre.len()));
                let local = centre[a..b].iter().sum::<f32>() / (b - a) as f32;
                if c > 4.0 * local {
                    tonal += c * c;
                }
            }
        }

        f.low.push(low);
        f.voice_mid.push(voice);
        f.voice_side.push(side_voice);
        f.voice_tonal.push(tonal);
        f.power.push(power);
        f.centroid.push(if power > 1e-10 { weighted / power } else { 0.0 });
        f.rolloff.push(rolloff);
        f.flatness.push(flatness);
        f.chroma.push(chroma);
        f.fine_chroma.push(fine);
        f.bass_chroma.push(bass);
        f.flux.push(if i == 0 { 0.0 } else { flux });
        f.low_flux.push(if i == 0 { 0.0 } else { low_flux });
        f.mfcc.push(mfcc);
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::sine;

    #[test]
    fn bands_and_chroma_follow_the_tone() {
        let sr = 22050;
        let tone = |hz| {
            let s = sine(sr, hz, 1.0);
            Pcm { sample_rate: sr, left: s.clone(), right: s }
        };
        let bass = analyze(&tone(80.0));
        let mid = bass.len() / 2;
        assert!(bass.low[mid] > 0.9 * bass.power[mid]);
        let a = analyze(&tone(440.0));
        assert!(a.voice_mid[mid] > 0.9 * a.power[mid]);
        assert!(a.voice_side[mid] < 1e-6, "mono has no side");
        let best = (0..12).max_by(|&x, &y| a.chroma[mid][x].total_cmp(&a.chroma[mid][y])).unwrap();
        assert_eq!(best, 9, "A");
        assert!((a.centroid[mid] - 440.0).abs() < 60.0, "{}", a.centroid[mid]);
    }
}
