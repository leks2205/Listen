//! Song structure on the bar grid: trim points, per-bar curves, the vocal
//! heuristic, Foote novelty, the phrase lattice and section labels.

use crate::decode::Pcm;
use crate::spectral::{Frames, N_MFCC};
use listen_core::analysis::{BarCurves, BeatGrid, Section, SectionKind};

/// −60 dBFS.
const SILENCE: f32 = 1e-3;

/// First / last moment above −60 dBFS, in 50 ms RMS windows (Mixxx's
/// AutoDJ trim). `(0, 0)` for digital silence.
pub fn trim(pcm: &Pcm) -> (f32, f32) {
    let win = (pcm.sample_rate as usize / 20).max(1);
    let loud = |w: usize| {
        let s = w * win;
        let e = (s + win).min(pcm.left.len());
        let sum: f32 = (s..e).map(|i| pcm.left[i] * pcm.left[i] + pcm.right[i] * pcm.right[i]).sum();
        (sum / (2 * (e - s).max(1)) as f32).sqrt() > SILENCE
    };
    let n = pcm.left.len().div_ceil(win);
    let Some(first) = (0..n).find(|&w| loud(w)) else {
        return (0.0, 0.0);
    };
    let last = (0..n).rev().find(|&w| loud(w)).unwrap_or(first);
    let secs = |w: usize| (w * win).min(pcm.left.len()) as f32 / pcm.sample_rate as f32;
    (secs(first), secs(last + 1))
}

/// Bar boundaries (n + 1 times). Without a grid: 2-second pseudo-bars
/// from the audio start (a 120 BPM 4/4 bar).
pub fn bar_bounds(grid: Option<&BeatGrid>, audio_start: f32, audio_end: f32) -> Vec<f32> {
    match grid {
        Some(g) if g.bar_count() > 0 => (0..=g.bar_count()).map(|b| g.bar_time(b)).collect(),
        _ => {
            let n = ((audio_end - audio_start) / 2.0).floor().max(0.0) as usize;
            (0..=n).map(|b| audio_start + 2.0 * b as f32).collect()
        }
    }
}

/// Raw per-bar measurements (linear units), before 0–255 scaling.
#[derive(Debug, Clone, Default)]
pub struct BarStats {
    /// RMS amplitude, full band and below 150 Hz.
    pub energy: Vec<f32>,
    pub low: Vec<f32>,
    /// Vocal likelihood 0..1, and the raw evidence behind it.
    pub vocal: Vec<f32>,
    pub vocal_raw: Vec<f32>,
    /// Mean spectral centroid, Hz.
    pub centroid: Vec<f32>,
    /// Foote novelty, 0..1 (1 = the song's strongest change).
    pub novelty: Vec<f32>,
    /// Feature vectors for the self-similarity matrix.
    vectors: Vec<Vec<f32>>,
}

fn mean(v: &[f32]) -> f32 {
    if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 }
}

fn percentile(v: &[f32], p: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    s[((s.len() - 1) as f32 * p).round() as usize]
}

/// Raw vocal evidence for frames `r` (about 0..1): how strongly the tonal, centred
/// formant-range energy ([`Frames::voice_tonal`]) flutters at syllable rate (3–8 Hz),
/// gated by its share of the voice band.
pub fn vocal_evidence(frames: &Frames, r: std::ops::Range<usize>) -> f32 {
    if r.len() < 16 {
        return 0.0;
    }
    let voice: f32 = frames.voice_mid[r.clone()].iter().sum();
    let tonal: f32 = frames.voice_tonal[r.clone()].iter().sum();
    if voice <= 1e-9 {
        return 0.0;
    }
    // Share of the voice band (kick and bass don't dilute it).
    let gate = ((tonal / voice - 0.02) / 0.15).clamp(0.0, 1.0);
    let env: Vec<f32> = frames.voice_tonal[r].iter().map(|x| x.sqrt()).collect();
    let m = mean(&env);
    if m <= 0.0 {
        return 0.0;
    }
    let fps = 1.0 / frames.hop_secs;
    let n = env.len() as f32;
    let band = |lo: f32, hi: f32| -> f32 {
        let (k0, k1) = ((lo * n / fps).ceil() as usize, (hi * n / fps).floor() as usize);
        (k0.max(1)..=k1)
            .map(|k| {
                let w = 2.0 * std::f32::consts::PI * k as f32 / n;
                let (mut re, mut im) = (0f32, 0f32);
                for (i, x) in env.iter().enumerate() {
                    re += (x - m) * (w * i as f32).cos();
                    im -= (x - m) * (w * i as f32).sin();
                }
                re * re + im * im
            })
            .sum()
    };
    let syllables = band(3.0, 8.0);
    let modulation = (2.0 * syllables).sqrt() / (n * m);
    // Syllables start and stop abruptly (energy far above 8 Hz); chorus
    // beating and tremolo are smooth.
    let edges = if syllables > 0.0 { (band(10.0, 30.0) / syllables / 0.3).clamp(0.0, 1.0) } else { 0.0 };
    gate * modulation.min(1.5) * edges
}

/// Per-bar statistics for `bounds` (see [`bar_bounds`]).
pub fn bar_stats(frames: &Frames, bounds: &[f32]) -> BarStats {
    let mut s = BarStats::default();
    let mut raw_vocal = Vec::new();
    for w in bounds.windows(2) {
        let r = frames.range(w[0], w[1]);
        let n = r.len().max(1) as f32;
        let p: f32 = frames.power[r.clone()].iter().sum::<f32>() / n;
        let low: f32 = frames.low[r.clone()].iter().sum::<f32>() / n;
        s.energy.push(p.sqrt());
        s.low.push(low.sqrt());
        let cen = if p > 0.0 {
            r.clone().map(|i| frames.centroid[i] * frames.power[i]).sum::<f32>() / (p * n)
        } else {
            0.0
        };
        s.centroid.push(cen);
        // Syllable modulation needs a couple of seconds: widen short bars.
        let mid = 0.5 * (w[0] + w[1]);
        let half = (0.5 * (w[1] - w[0])).max(1.0);
        raw_vocal.push(if p.sqrt() > SILENCE {
            vocal_evidence(frames, frames.range(mid - half, mid + half))
        } else {
            0.0
        });

        let mut chroma = [0f32; 12];
        let mut mfcc = [0f32; N_MFCC];
        for i in r.clone() {
            for (a, b) in chroma.iter_mut().zip(&frames.chroma[i]) {
                *a += b;
            }
            for (a, b) in mfcc.iter_mut().zip(&frames.mfcc[i]) {
                *a += b / n;
            }
        }
        let cn = chroma.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
        let mut v: Vec<f32> = chroma.iter().map(|x| x / cn).collect();
        v.extend_from_slice(&mfcc[1..]);
        v.push(20.0 * p.sqrt().max(1e-6).log10());
        v.push(20.0 * low.sqrt().max(1e-6).log10());
        s.vectors.push(v);
    }
    s.vocal = vocal_likelihood(&raw_vocal, &s.energy);
    s.vocal_raw = raw_vocal;
    for (v, voc) in s.vectors.iter_mut().zip(&s.vocal) {
        v.push(*voc);
    }
    s.novelty = foote_novelty(&s.vectors, 4);
    s
}

/// Evidence above this is "vocal likely".
pub const VOCAL_THRESHOLD: f32 = 0.08;

/// Raw vocal evidence to a 0..1 likelihood (0.5 at the threshold), median-of-3 smoothed;
/// the first and last bar need their one neighbour to agree.
fn vocal_likelihood(raw: &[f32], energy: &[f32]) -> Vec<f32> {
    let p: Vec<f32> = raw
        .iter()
        .zip(energy)
        .map(|(r, e)| if *e <= SILENCE { 0.0 } else { 1.0 / (1.0 + (-(r - VOCAL_THRESHOLD) / 0.02).exp()) })
        .collect();
    let n = p.len();
    (0..n)
        .map(|i| {
            if n < 2 {
                return p[i];
            }
            if i == 0 {
                return p[0].min(p[1]);
            }
            if i + 1 == n {
                return p[i].min(p[i - 1]);
            }
            let mut w = [p[i - 1], p[i], p[i + 1]];
            w.sort_by(f32::total_cmp);
            w[1]
        })
        .collect()
}

/// Foote (2000) novelty: a Gaussian-tapered checkerboard kernel of `half`
/// bars per side slid along the diagonal of the bar self-similarity matrix.
pub fn foote_novelty(vectors: &[Vec<f32>], half: usize) -> Vec<f32> {
    let n = vectors.len();
    if n == 0 {
        return Vec::new();
    }
    let dim = vectors[0].len();
    // z-score every dimension so loudness and chroma weigh alike.
    let mut z = vectors.to_vec();
    for d in 0..dim {
        let col: Vec<f32> = vectors.iter().map(|v| v[d]).collect();
        let m = mean(&col);
        let sd = (col.iter().map(|x| (x - m).powi(2)).sum::<f32>() / n as f32).sqrt().max(1e-6);
        for v in z.iter_mut() {
            v[d] = (v[d] - m) / sd;
        }
    }
    let norms: Vec<f32> = z.iter().map(|v| v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9)).collect();
    let sim =
        |i: usize, j: usize| z[i].iter().zip(&z[j]).map(|(a, b)| a * b).sum::<f32>() / (norms[i] * norms[j]);
    let h = half as isize;
    let sigma = half as f32 / 2.0;
    let mut nov: Vec<f32> = (0..n as isize)
        .map(|k| {
            let mut acc = 0f32;
            for a in -h..h {
                for b in -h..h {
                    let (i, j) = (k + a, k + b);
                    if i < 0 || j < 0 || i >= n as isize || j >= n as isize {
                        continue;
                    }
                    let sign = if (a < 0) == (b < 0) { 1.0 } else { -1.0 };
                    let (fa, fb) = (a as f32 + 0.5, b as f32 + 0.5);
                    let taper = (-(fa * fa + fb * fb) / (2.0 * sigma * sigma)).exp();
                    acc += sign * taper * sim(i as usize, j as usize);
                }
            }
            acc.max(0.0)
        })
        .collect();
    let max = nov.iter().copied().fold(0.0, f32::max);
    if max > 0.0 {
        nov.iter_mut().for_each(|x| *x /= max);
    }
    nov
}

/// Phrase length (8, 16 or 4 bars) and offset maximising the novelty on
/// the lattice. 8 is the pop default; 16 and 4 must clearly win.
pub fn phrase_lattice(novelty: &[f32]) -> (u8, u8) {
    let n = novelty.len();
    if n < 16 {
        return (8, 0);
    }
    // Skip the edges, where the kernel only half overlaps the song.
    let score = |p: usize, o: usize| {
        let pts: Vec<f32> = (o..n).step_by(p).filter(|&b| b >= 2 && b + 2 < n).map(|b| novelty[b]).collect();
        mean(&pts)
    };
    let best =
        |p: usize| (0..p).map(|o| (score(p, o), o)).max_by(|a, b| a.0.total_cmp(&b.0)).unwrap_or((0.0, 0));
    let (s8, o8) = best(8);
    let (s16, o16) = best(16);
    let (s4, o4) = best(4);
    if n >= 48 && s16 > 1.25 * s8 && o16 % 8 == o8 {
        (16, o16 as u8)
    } else if s4 > 1.15 * s8 {
        (4, o4 as u8)
    } else {
        (8, o8 as u8)
    }
}

/// Sections on the phrase lattice (8-bar steps at most), split at clear novelty. Intro/Outro:
/// quiet (< 60 % median energy), vocal-free ends; Breakdown: bass < 40 % of median; Build:
/// rising energy after a breakdown; Drop: first full-bass phrase after either.
pub fn sections(stats: &BarStats, phrase_bars: u8, phrase_offset: u8) -> Vec<Section> {
    let n = stats.energy.len();
    if n == 0 {
        return Vec::new();
    }
    // Boundaries step 8 bars at most: a breakdown is often 8 bars even in 16-bar phrases.
    let p = usize::from(phrase_bars.clamp(1, 8));
    let o = usize::from(phrase_offset) % p;
    let lattice: Vec<usize> = (o..n).step_by(p).filter(|&b| b > 0).collect();
    let lattice_nov: Vec<f32> = lattice.iter().map(|&b| stats.novelty[b]).collect();
    let cut = percentile(&lattice_nov, 0.5).max(0.2);
    let mut bounds = vec![0];
    bounds.extend(lattice.iter().copied().filter(|&b| stats.novelty[b] >= cut || b == o));
    bounds.push(n);
    bounds.dedup();

    struct Seg {
        start: usize,
        end: usize,
        energy: f32,
        low: f32,
        vocal: f32,
        rising: bool,
    }
    let segs: Vec<Seg> = bounds
        .windows(2)
        .map(|w| {
            let (s, e) = (w[0], w[1]);
            let en = &stats.energy[s..e];
            let half = en.len() / 2;
            Seg {
                start: s,
                end: e,
                energy: mean(en),
                low: mean(&stats.low[s..e]),
                vocal: mean(
                    &stats.vocal[s..e].iter().map(|v| if *v >= 0.5 { 1.0 } else { 0.0 }).collect::<Vec<_>>(),
                ),
                rising: half > 0 && mean(&en[half..]) > 1.3 * mean(&en[..half]),
            }
        })
        .collect();
    let med_e = percentile(&stats.energy, 0.5);
    let med_low = percentile(&stats.low, 0.5);
    let quiet_instrumental = |s: &Seg| s.energy < 0.6 * med_e && s.vocal < 0.5;
    let mut kinds = vec![SectionKind::Body; segs.len()];
    let intro = segs.iter().take_while(|s| quiet_instrumental(s)).count();
    let outro = segs.iter().rev().take_while(|s| quiet_instrumental(s)).count().min(segs.len() - intro);
    for k in kinds.iter_mut().take(intro) {
        *k = SectionKind::Intro;
    }
    for k in kinds.iter_mut().rev().take(outro) {
        *k = SectionKind::Outro;
    }
    for i in intro..segs.len() - outro {
        let prev = i.checked_sub(1).map(|j| kinds[j]);
        let after_break = matches!(prev, Some(SectionKind::Breakdown | SectionKind::Build));
        kinds[i] = if segs[i].low < 0.4 * med_low {
            if prev == Some(SectionKind::Breakdown) && segs[i].rising {
                SectionKind::Build
            } else {
                SectionKind::Breakdown
            }
        } else if after_break && segs[i].low >= med_low {
            SectionKind::Drop
        } else if prev == Some(SectionKind::Breakdown) && segs[i].rising {
            SectionKind::Build
        } else {
            SectionKind::Body
        };
    }
    // Merge neighbours of the same kind.
    let mut out: Vec<Section> = Vec::new();
    for (s, kind) in segs.iter().zip(kinds) {
        match out.last_mut() {
            Some(last) if last.kind == kind => last.bars += (s.end - s.start) as u16,
            _ => out.push(Section { start_bar: s.start as u16, bars: (s.end - s.start) as u16, kind }),
        }
    }
    out
}

/// Vocal-free bars at the start and at the end.
pub fn instrumental_edges(vocal: &[f32]) -> (u16, u16) {
    let is_vocal = |v: &&f32| **v >= 0.5;
    let start = vocal.iter().take_while(|v| !is_vocal(v)).count();
    let end = vocal.iter().rev().take_while(|v| !is_vocal(v)).count().min(vocal.len() - start);
    (start as u16, end as u16)
}

/// 0–255 curves: loudness on a 30 dB scale below the song's loudest bar,
/// centroid and novelty min–max, vocal likelihood as is.
pub fn curves(stats: &BarStats) -> BarCurves {
    let db_scale = |v: &[f32]| {
        let max = v.iter().copied().fold(0.0, f32::max).max(1e-9);
        v.iter()
            .map(|x| {
                ((1.0 + 20.0 * (x.max(1e-9) / max).log10() / 30.0).clamp(0.0, 1.0) * 255.0).round() as u8
            })
            .collect()
    };
    let unit = |v: &[f32]| v.iter().map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
    let minmax = |v: &[f32]| {
        let (lo, hi) = (v.iter().copied().fold(f32::MAX, f32::min), v.iter().copied().fold(0.0, f32::max));
        v.iter().map(|x| (((x - lo) / (hi - lo).max(1e-9)).clamp(0.0, 1.0) * 255.0).round() as u8).collect()
    };
    BarCurves {
        energy: db_scale(&stats.energy),
        low: db_scale(&stats.low),
        vocal: unit(&stats.vocal),
        centroid: minmax(&stats.centroid),
        novelty: unit(&stats.novelty),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectral;
    use crate::test_util::{noise, sine};

    #[test]
    fn trim_finds_the_padding() {
        let sr = 22050;
        let mut left = vec![0.0; sr as usize * 2];
        left.extend(sine(sr, 220.0, 3.0));
        left.extend(vec![0.0; sr as usize * 3 / 2]);
        let pcm = Pcm { sample_rate: sr, left: left.clone(), right: left };
        let (a, b) = trim(&pcm);
        assert!((a - 2.0).abs() < 0.06, "{a}");
        assert!((b - 5.0).abs() < 0.06, "{b}");
        let silent = Pcm { sample_rate: sr, left: vec![0.0; 1000], right: vec![0.0; 1000] };
        assert_eq!(trim(&silent), (0.0, 0.0));
    }

    #[test]
    fn novelty_steps_give_the_phrase_offset() {
        // 64 bars; the material changes every 8 bars starting at bar 3.
        let vectors: Vec<Vec<f32>> = (0..64)
            .map(|b: usize| {
                let block = (b + 5) / 8; // changes at 3, 11, 19, …
                (0..6)
                    .map(|d| if (block + d).is_multiple_of(3) { 1.0 } else { 0.0 } + 0.01 * (b % 2) as f32)
                    .collect()
            })
            .collect();
        let nov = foote_novelty(&vectors, 4);
        assert_eq!(phrase_lattice(&nov), (8, 3));
    }

    #[test]
    fn sections_label_intro_breakdown_drop_outro() {
        // 8-bar phrases: quiet intro, body, breakdown (no bass), drop, body, quiet outro.
        let plan = [(0.2, 0.1), (1.0, 1.0), (0.8, 0.1), (1.2, 1.3), (1.0, 1.0), (0.2, 0.1)];
        let mut s = BarStats::default();
        for (e, l) in plan {
            for _ in 0..8 {
                s.energy.push(e);
                s.low.push(l);
                s.vocal.push(0.0);
                s.centroid.push(1000.0);
            }
        }
        s.novelty = (0..48).map(|b| if b % 8 == 0 { 1.0 } else { 0.05 }).collect();
        let kinds: Vec<_> = sections(&s, 8, 0).iter().map(|x| (x.start_bar, x.kind)).collect();
        use SectionKind::*;
        assert_eq!(kinds, [(0, Intro), (8, Body), (16, Breakdown), (24, Drop), (32, Body), (40, Outro)]);
    }

    #[test]
    fn a_lone_vocal_edge_bar_is_smoothed_away() {
        let (lo, hi) = (0.0, 0.3);
        let energy = vec![1.0; 6];
        // Instrumental intro/outro with a stray "voice" in the very last bar.
        let p = vocal_likelihood(&[lo, lo, hi, hi, lo, hi], &energy);
        assert!(p[5] < 0.1 && p[0] < 0.1, "{p:?}");
        // A vocal right to the end stays.
        let p = vocal_likelihood(&[hi, hi, hi, hi, hi, hi], &energy);
        assert!(p[0] > 0.9 && p[5] > 0.9, "{p:?}");
    }

    #[test]
    fn vocal_curve_follows_a_centred_modulated_voice() {
        // 4 s of uncorrelated stereo noise (a wide pad), then 4 s of a centred 6 Hz-gated
        // voice-band tone (syllables) over the same noise.
        let sr = 22050;
        let (pad_l, pad_r) = (noise(sr, 8.0, 0.05, 7), noise(sr, 8.0, 0.05, 11));
        let tone = sine(sr, 500.0, 8.0);
        let mut left = Vec::new();
        let mut right = Vec::new();
        for (i, t) in tone.iter().enumerate() {
            let secs = i as f32 / sr as f32;
            let gate = if secs >= 4.0 && (secs * 6.0).fract() < 0.6 { 0.3 } else { 0.0 };
            left.push(pad_l[i] + gate * t);
            right.push(pad_r[i] + gate * t);
        }
        let pcm = Pcm { sample_rate: sr, left, right };
        let frames = spectral::analyze(&pcm);
        let bounds: Vec<f32> = (0..=8).map(|b| b as f32).collect(); // 1 s "bars"
        let s = bar_stats(&frames, &bounds);
        let (before, after) = (mean(&s.vocal[..3]), mean(&s.vocal[5..]));
        assert!(after > 0.5 && before < 0.5, "{:?}", s.vocal);
        assert_eq!(instrumental_edges(&s.vocal).0 as usize, s.vocal.iter().position(|v| *v >= 0.5).unwrap());
    }

    #[test]
    fn edges_and_curves() {
        assert_eq!(instrumental_edges(&[0.1, 0.2, 0.9, 0.8, 0.1]), (2, 1));
        assert_eq!(instrumental_edges(&[0.1, 0.1]), (2, 0));
        let s = BarStats {
            energy: vec![1.0, 0.1, 0.001],
            low: vec![1.0; 3],
            vocal: vec![0.0, 0.5, 1.0],
            vocal_raw: vec![],
            centroid: vec![100.0, 200.0, 300.0],
            novelty: vec![0.0, 1.0, 0.5],
            vectors: vec![],
        };
        let c = curves(&s);
        assert_eq!(c.energy, [255, 85, 0]);
        assert_eq!(c.vocal, [0, 128, 255]);
        assert_eq!(c.centroid, [0, 128, 255]);
    }
}
