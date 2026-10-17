//! Beat sources and raw beat times to a [`BeatGrid`]. Sources: the Beat This! model
//! ([`crate::models`]) and an Ellis-style DP tracker as fallback. [`build_grid`] fills
//! gaps, straightens machine-steady tempos and picks the downbeat phase.

use crate::spectral::Frames;
use listen_core::analysis::BeatGrid;

/// Raw output of a beat source, times in seconds of file time.
#[derive(Debug, Clone, Default)]
pub struct BeatEstimate {
    pub beats: Vec<f32>,
    /// Empty when the source doesn't know downbeats.
    pub downbeats: Vec<f32>,
    /// The source's own certainty (0..1), if it has one.
    pub confidence: Option<f32>,
}

/// Systematic lead of the own tracker's beats (the 93 ms STFT window sees
/// an onset before its centre); added so grids from either source line up.
pub const OWN_OFFSET: f32 = 0.013;
/// Same for Beat This! (its beats come ~7 ms after the attack).
pub const BEAT_THIS_OFFSET: f32 = -0.007;

/// Grids below this are dropped (plain fades only).
pub const MIN_CONFIDENCE: f32 = 0.35;
pub const MIN_STABILITY: f32 = 0.9;

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// Onset envelope for tracking: flux minus its 1 s running mean, clipped at
/// zero and scaled to unit standard deviation.
fn onset_envelope(frames: &Frames) -> Vec<f32> {
    let o = &frames.flux;
    let n = o.len();
    let half = (0.5 / frames.hop_secs) as usize;
    let mut prefix = vec![0f64; n + 1];
    for i in 0..n {
        prefix[i + 1] = prefix[i] + f64::from(o[i]);
    }
    let mut env: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(half), (i + half + 1).min(n));
            let mean = ((prefix[b] - prefix[a]) / (b - a) as f64) as f32;
            (o[i] - mean).max(0.0)
        })
        .collect();
    let sd = (env.iter().map(|x| x * x).sum::<f32>() / n.max(1) as f32).sqrt();
    if sd > 0.0 {
        env.iter_mut().for_each(|x| *x /= sd);
    }
    env
}

/// Log-normal tempo prior (octaves from `center`).
fn tempo_prior(bpm: f32, center: f32, sigma_oct: f32) -> f32 {
    let d = (bpm / center).log2() / sigma_oct;
    (-0.5 * d * d).exp()
}

/// Global tempo by enhanced autocorrelation of the onset envelope with a
/// tempo prior (120 BPM ± 1 octave, or the tag hint ± 0.25 octave).
pub fn estimate_tempo(env: &[f32], fps: f32, hint: Option<f32>) -> Option<(f32, f32)> {
    let n = env.len();
    let lag_of = |bpm: f32| fps * 60.0 / bpm;
    let (min_lag, max_lag) = (lag_of(210.0).floor() as usize, (lag_of(55.0).ceil() as usize) * 2);
    if n < max_lag * 2 {
        return None;
    }
    let r: Vec<f32> = (0..=max_lag)
        .map(|l| env[..n - l].iter().zip(&env[l..]).map(|(a, b)| a * b).sum::<f32>() / (n - l) as f32)
        .collect();
    let r0 = r[0].max(1e-9);
    let (center, sigma) = hint.map_or((120.0, 1.0), |h| (h, 0.25));
    let max_single = (lag_of(55.0).ceil() as usize).min(max_lag / 2);
    let score = |l: usize| {
        let enh = r[l] + 0.5 * r[(2 * l).min(max_lag)];
        enh * tempo_prior(fps * 60.0 / l as f32, center, sigma)
    };
    let best = (min_lag..=max_single).max_by(|&a, &b| score(a).total_cmp(&score(b)))?;
    // Parabolic refinement of the raw autocorrelation peak.
    let lag = if best > min_lag && best < max_single {
        let (a, b, c) = (r[best - 1], r[best], r[best + 1]);
        let den = a - 2.0 * b + c;
        if den.abs() > 1e-9 { best as f32 + 0.5 * (a - c) / den } else { best as f32 }
    } else {
        best as f32
    };
    // Peak height relative to zero lag: ~0.1 for weak pulses, >0.4 for four-on-the-floor.
    Some((fps * 60.0 / lag, (r[best] / r0).clamp(0.0, 1.0)))
}

/// Ellis (2007) dynamic-programming beat tracker on the onset envelope.
pub fn track_own(
    frames: &Frames,
    audio_start: f32,
    audio_end: f32,
    hint: Option<f32>,
) -> Option<BeatEstimate> {
    let fps = 1.0 / frames.hop_secs;
    let range = frames.range(audio_start, audio_end);
    let env_full = onset_envelope(frames);
    let env = &env_full[range.clone()];
    let (bpm, peak) = estimate_tempo(env, fps, hint)?;
    let period = fps * 60.0 / bpm;
    // Smooth with a narrow Gaussian (σ = period/32) as librosa does.
    let half = (period / 8.0).ceil() as isize;
    let kernel: Vec<f32> =
        (-half..=half).map(|k| (-0.5 * (k as f32 * 32.0 / period).powi(2)).exp()).collect();
    let n = env.len();
    let local: Vec<f32> = (0..n as isize)
        .map(|i| {
            kernel
                .iter()
                .enumerate()
                .filter_map(|(j, w)| {
                    let k = i + j as isize - half;
                    (k >= 0 && (k as usize) < n).then(|| w * env[k as usize])
                })
                .sum()
        })
        .collect();
    let tightness = 100.0;
    let (lo, hi) = ((period / 2.0).round() as usize, (2.0 * period).round() as usize);
    let mut cum = vec![0f32; n];
    let mut back = vec![usize::MAX; n];
    for t in 0..n {
        let mut best = (f32::MIN, usize::MAX);
        for d in lo.max(1)..=hi {
            if d > t {
                break;
            }
            let x = (d as f32 / period).ln();
            let v = cum[t - d] - tightness * x * x;
            if v > best.0 {
                best = (v, t - d);
            }
        }
        cum[t] = local[t] + best.0.max(0.0);
        back[t] = if best.0 > 0.0 { best.1 } else { usize::MAX };
    }
    // Last beat: the best score within the final period.
    let tail = n.saturating_sub(hi)..n;
    let mut t = tail.max_by(|&a, &b| cum[a].total_cmp(&cum[b]))?;
    let mut beats = vec![t];
    while back[t] != usize::MAX {
        t = back[t];
        beats.push(t);
    }
    beats.reverse();
    let offset = range.start as f32 * frames.hop_secs;
    Some(BeatEstimate {
        beats: beats.into_iter().map(|b| offset + OWN_OFFSET + b as f32 * frames.hop_secs).collect(),
        downbeats: Vec::new(),
        confidence: Some(((peak - 0.05) / 0.3).clamp(0.0, 1.0)),
    })
}

/// Linear least squares t = a + b·i.
fn fit_line(beats: &[f32]) -> (f32, f32, f32) {
    let n = beats.len() as f64;
    let (mut si, mut st, mut sii, mut sit) = (0f64, 0f64, 0f64, 0f64);
    for (i, &t) in beats.iter().enumerate() {
        let (i, t) = (i as f64, f64::from(t));
        si += i;
        st += t;
        sii += i * i;
        sit += i * t;
    }
    let b = (n * sit - si * st) / (n * sii - si * si).max(1e-12);
    let a = (st - b * si) / n;
    let rms =
        (beats.iter().enumerate().map(|(i, &t)| (f64::from(t) - a - b * i as f64).powi(2)).sum::<f64>() / n)
            .sqrt();
    (a as f32, b as f32, rms as f32)
}

/// Removes double detections and fills dropped beats, so consecutive entries
/// are one beat apart.
fn regularise(raw: &[f32]) -> Vec<f32> {
    let mut ibis: Vec<f32> = raw.windows(2).map(|w| w[1] - w[0]).collect();
    let m = median(&mut ibis);
    if m <= 0.0 {
        return raw.to_vec();
    }
    let mut out: Vec<f32> = Vec::with_capacity(raw.len());
    for &t in raw {
        let Some(&last) = out.last() else {
            out.push(t);
            continue;
        };
        let gap = t - last;
        if gap < 0.6 * m {
            continue; // extra detection
        }
        let k = (gap / m).round();
        if k >= 2.0 && (gap / k - m).abs() < 0.15 * m {
            for j in 1..k as usize {
                out.push(last + gap * j as f32 / k);
            }
        }
        out.push(t);
    }
    out
}

/// 1 − robust coefficient of variation of the beat intervals.
pub fn stability(beats: &[f32]) -> f32 {
    let mut ibis: Vec<f32> = beats.windows(2).map(|w| w[1] - w[0]).collect();
    if ibis.len() < 4 {
        return 0.0;
    }
    let m = median(&mut ibis.clone());
    let mut dev: Vec<f32> = ibis.iter_mut().map(|x| (*x - m).abs()).collect();
    (1.0 - 1.4826 * median(&mut dev) / m.max(1e-6)).clamp(0.0, 1.0)
}

/// Straightens jitter: a single line when the tempo is machine-steady (the
/// norm in pop), otherwise a local ±8-beat linear fit that follows drift.
fn straighten(beats: &[f32]) -> Vec<f32> {
    if beats.len() < 8 {
        return beats.to_vec();
    }
    let (a, b, rms) = fit_line(beats);
    if rms < 0.015 {
        return (0..beats.len()).map(|i| a + b * i as f32).collect();
    }
    (0..beats.len())
        .map(|i| {
            let (s, e) = (i.saturating_sub(8), (i + 9).min(beats.len()));
            let (a, b, _) = fit_line(&beats[s..e]);
            a + b * (i - s) as f32
        })
        .collect()
}

/// Mean of `v` over a ±40 ms window around `t`.
fn around(frames: &Frames, v: &[f32], t: f32) -> f32 {
    let r = frames.range(t - 0.04, t + 0.04);
    if r.is_empty() { 0.0 } else { v[r.clone()].iter().copied().fold(0.0, f32::max) }
}

/// Downbeat phase without a model: kicks (low-band onsets) and chord
/// changes land on the first beat of the bar in pop.
fn downbeat_phase_own(frames: &Frames, beats: &[f32], per_bar: usize) -> usize {
    let chroma_change = |t: f32| {
        let before = frames.range(t - 0.25, t);
        let after = frames.range(t, t + 0.25);
        if before.is_empty() || after.is_empty() {
            return 0.0;
        }
        let mean = |r: std::ops::Range<usize>| {
            let mut c = [0f32; 12];
            for i in r {
                for (a, b) in c.iter_mut().zip(&frames.chroma[i]) {
                    *a += b;
                }
            }
            let norm = c.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
            c.map(|x| x / norm)
        };
        let (a, b) = (mean(before), mean(after));
        1.0 - a.iter().zip(&b).map(|(x, y)| x * y).sum::<f32>()
    };
    let low_mean = frames.low_flux.iter().sum::<f32>() / frames.len().max(1) as f32;
    let mut scores = vec![0f32; per_bar];
    let mut chroma_scores = vec![0f32; per_bar];
    for (i, &t) in beats.iter().enumerate() {
        scores[i % per_bar] += around(frames, &frames.low_flux, t) / low_mean.max(1e-9);
        chroma_scores[i % per_bar] += chroma_change(t);
    }
    let norm = |v: &mut Vec<f32>| {
        let s: f32 = v.iter().sum::<f32>().max(1e-9);
        v.iter_mut().for_each(|x| *x /= s);
    };
    norm(&mut scores);
    norm(&mut chroma_scores);
    (0..per_bar)
        .max_by(|&a, &b| (scores[a] + chroma_scores[a]).total_cmp(&(scores[b] + chroma_scores[b])))
        .unwrap_or(0)
}

/// How strongly the onsets line up with the beats (0..1): beat-time onset
/// strength vs half-beat points.
fn pulse_clarity(frames: &Frames, env: &[f32], beats: &[f32]) -> f32 {
    if beats.len() < 8 {
        return 0.0;
    }
    let (mut on, mut off) = (0f32, 0f32);
    for w in beats.windows(2) {
        on += around(frames, env, w[0]);
        off += around(frames, env, 0.5 * (w[0] + w[1]));
    }
    ((on - off) / (on + off).max(1e-9)).clamp(0.0, 1.0)
}

/// Turns raw beats into a grid; `None` when the beat is too uncertain or
/// unsteady to mix on.
pub fn build_grid(est: &BeatEstimate, frames: &Frames, audio_start: f32, audio_end: f32) -> Option<BeatGrid> {
    grid_candidate(est, frames, audio_start, audio_end)
        .filter(|g| g.confidence >= MIN_CONFIDENCE && g.stability >= MIN_STABILITY)
}

/// [`build_grid`] without the confidence gate (for tuning).
pub fn grid_candidate(
    est: &BeatEstimate,
    frames: &Frames,
    audio_start: f32,
    audio_end: f32,
) -> Option<BeatGrid> {
    let raw: Vec<f32> =
        est.beats.iter().copied().filter(|&t| t >= audio_start - 0.05 && t <= audio_end + 0.05).collect();
    if raw.len() < 16 {
        return None;
    }
    let stab = stability(&raw);
    let beats = straighten(&regularise(&raw));
    let (_, period, _) = fit_line(&beats);
    let bpm = 60.0 / period.max(1e-3);

    // Bar length and phase from the source's downbeats when it has them.
    let near = |t: f32| est.downbeats.iter().any(|d| (d - t).abs() < 0.07);
    let (per_bar, phase) = if est.downbeats.len() >= 4 {
        let idx: Vec<usize> = beats.iter().enumerate().filter(|(_, t)| near(**t)).map(|(i, _)| i).collect();
        let mut counts = [0usize; 8];
        for w in idx.windows(2) {
            if let Some(c) = counts.get_mut(w[1] - w[0]) {
                *c += 1;
            }
        }
        let per_bar = if counts[3] > counts[4] { 3 } else { 4 };
        let mut votes = vec![0usize; per_bar];
        for i in &idx {
            votes[i % per_bar] += 1;
        }
        let phase = (0..per_bar).max_by_key(|&p| votes[p]).unwrap_or(0);
        (per_bar, phase)
    } else {
        (4, downbeat_phase_own(frames, &beats, 4))
    };

    let env = onset_envelope(frames);
    let clarity = pulse_clarity(frames, &env, &beats);
    let confidence = match est.confidence {
        Some(c) => 0.5 * c + 0.5 * clarity,
        None => clarity,
    }
    .clamp(0.0, 1.0);
    let grid = BeatGrid {
        beats,
        downbeat_phase: phase as u8,
        beats_per_bar: per_bar as u8,
        bpm,
        confidence,
        stability: stab,
    };
    Some(grid)
}

/// How far a beat may move onto its attack (the sources are quantised to
/// 20 ms frames, Beat This!, or 11.6 ms hops, the own tracker).
const REFINE_REACH: f32 = 0.025;
/// Energy rise (dB, a 2 ms window against the 2 ms before it) that counts
/// as an attack.
const REFINE_MIN_RISE_DB: f32 = 9.0;

/// Moves a frame-quantised grid onto its attacks (sub-ms): each beat's shift to the steepest
/// high-passed energy rise within ±25 ms, median over ±8 beats, then straightened again.
/// Unchanged without enough clear attacks.
pub fn refine_to_onsets(grid: &mut BeatGrid, mono: &[f32], sample_rate: u32) {
    let beats = &grid.beats;
    if beats.len() < 16 || mono.is_empty() {
        return;
    }
    let sr = sample_rate as f32;
    let mut prefix = Vec::with_capacity(mono.len() + 1);
    let (mut prev, mut acc) = (0f32, 0f64);
    prefix.push(0f64);
    for &x in mono {
        let d = x - prev;
        prev = x;
        acc += f64::from(d * d);
        prefix.push(acc);
    }
    let win = ((0.002 * sr) as usize).max(1);
    let level = |s: usize| -> f32 {
        let (a, b) = (s.min(mono.len()), (s + win).min(mono.len()));
        if b <= a {
            return -120.0;
        }
        (10.0 * ((prefix[b] - prefix[a]) / (b - a) as f64 + 1e-12).log10()) as f32
    };
    let reach = (REFINE_REACH * sr) as usize;
    let step = ((0.0005 * sr) as usize).max(1);
    let mut shifts: Vec<Option<f32>> = Vec::with_capacity(beats.len());
    for &t in beats {
        let centre = (t.max(0.0) * sr) as usize;
        let mut best: Option<(f32, usize)> = None;
        let mut s = centre.saturating_sub(reach).max(3 * win);
        while s <= centre + reach && s + win <= mono.len() {
            let rise = level(s) - level(s - win);
            if best.is_none_or(|(r, _)| rise > r) {
                best = Some((rise, s));
            }
            s += step;
        }
        shifts.push(best.filter(|(r, _)| *r >= REFINE_MIN_RISE_DB).map(|(_, s)| s as f32 / sr - t));
    }
    if shifts.iter().flatten().count() < beats.len() / 4 {
        return;
    }
    let refined: Vec<f32> = (0..beats.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(8), (i + 9).min(beats.len()));
            let mut near: Vec<f32> = shifts[a..b].iter().flatten().copied().collect();
            if near.len() < 3 {
                near = shifts.iter().flatten().copied().collect();
            }
            beats[i] + median(&mut near)
        })
        .collect();
    grid.beats = straighten(&refined);
    let (_, period, _) = fit_line(&grid.beats);
    grid.bpm = 60.0 / period.max(1e-3);
}

/// Brings the grid to the tagged tempo octave when the tag says half or
/// double (the tag is what listeners and DJs count).
pub fn apply_hint(grid: &mut BeatGrid, hint: f32) {
    let ratio = hint / grid.bpm;
    if (ratio - 2.0).abs() < 0.08 {
        let mut beats = Vec::with_capacity(grid.beats.len() * 2);
        for w in grid.beats.windows(2) {
            beats.extend([w[0], 0.5 * (w[0] + w[1])]);
        }
        beats.extend(grid.beats.last());
        grid.beats = beats;
        grid.downbeat_phase *= 2;
        grid.bpm *= 2.0;
    } else if (ratio - 0.5).abs() < 0.02 {
        // Keep the downbeats: they are strong beats at half tempo.
        let phase = usize::from(grid.downbeat_phase) % 2;
        grid.beats = grid.beats.iter().skip(phase).step_by(2).copied().collect();
        grid.downbeat_phase = (grid.downbeat_phase / 2) % (grid.beats_per_bar.max(1));
        grid.bpm /= 2.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::Pcm;
    use crate::spectral;
    use crate::test_util::click_track;

    #[test]
    fn own_tracker_finds_120_bpm_and_the_accented_downbeat() {
        // 120 BPM, first beat at 1.0 s; every 4th click accented with a kick.
        let pcm = click_track(120.0, 40.0, 1.0, 0);
        let frames = spectral::analyze(&pcm);
        let est = track_own(&frames, 0.9, 40.0, None).unwrap();
        let grid = build_grid(&est, &frames, 0.9, 40.0).expect("steady click is gridded");
        assert!((grid.bpm - 120.0).abs() < 0.5, "{}", grid.bpm);
        assert!(grid.stability > 0.95, "{}", grid.stability);
        let first_down = grid.downbeats().next().unwrap();
        // Downbeats are at 1.0, 3.0, 5.0 …
        let off = ((first_down - 1.0) / 2.0).fract();
        assert!(!(0.02..=0.98).contains(&off), "downbeat at {first_down}");
    }

    #[test]
    fn downbeat_phase_follows_the_accent() {
        // Accent on the second click: downbeats at 1.5, 3.5, …
        let pcm = click_track(120.0, 40.0, 1.0, 1);
        let frames = spectral::analyze(&pcm);
        let est = track_own(&frames, 0.9, 40.0, None).unwrap();
        let grid = build_grid(&est, &frames, 0.9, 40.0).unwrap();
        let d = grid.downbeats().next().unwrap();
        let off = ((d - 1.5) / 2.0).rem_euclid(1.0);
        assert!(!(0.02..=0.98).contains(&off), "downbeat at {d}");
    }

    #[test]
    fn silence_has_no_grid() {
        let pcm = Pcm { sample_rate: 22050, left: vec![0.0; 22050 * 20], right: vec![0.0; 22050 * 20] };
        let frames = spectral::analyze(&pcm);
        let grid = track_own(&frames, 0.0, 20.0, None).and_then(|e| build_grid(&e, &frames, 0.0, 20.0));
        assert!(grid.is_none());
    }

    #[test]
    fn regularise_fills_and_dedups() {
        let raw = [0.0, 0.5, 1.0, 2.0, 2.5, 2.52, 3.0, 3.5];
        let r = regularise(&raw);
        assert_eq!(r, [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5]);
    }

    #[test]
    fn refining_moves_a_quantised_grid_onto_the_clicks() {
        // Clicks every 0.5 s from 1.0 s; the grid is 8 ms late throughout
        // (what frame quantisation does at a commensurate tempo).
        let pcm = click_track(120.0, 40.0, 1.0, 0);
        let mut g = BeatGrid {
            beats: (0..78).map(|i| 1.008 + i as f32 * 0.5).collect(),
            downbeat_phase: 0,
            beats_per_bar: 4,
            bpm: 120.0,
            confidence: 1.0,
            stability: 1.0,
        };
        refine_to_onsets(&mut g, &pcm.mono(), pcm.sample_rate);
        let worst =
            g.beats.iter().enumerate().map(|(i, t)| (t - (1.0 + i as f32 * 0.5)).abs()).fold(0.0, f32::max);
        assert!(worst < 0.001, "worst beat {:.2} ms off", worst * 1000.0);
        assert!((g.bpm - 120.0).abs() < 0.05, "{}", g.bpm);
        // Silence: nothing to snap to, the grid stays.
        let quiet = vec![0.0; 22050 * 40];
        let before = g.beats.clone();
        refine_to_onsets(&mut g, &quiet, 22050);
        assert_eq!(g.beats, before);
    }

    #[test]
    fn hint_doubles_and_halves() {
        let mut g = BeatGrid {
            beats: (0..9).map(|i| i as f32).collect(),
            downbeat_phase: 1,
            beats_per_bar: 4,
            bpm: 60.0,
            confidence: 1.0,
            stability: 1.0,
        };
        apply_hint(&mut g, 120.0);
        assert_eq!(g.beats.len(), 17);
        assert_eq!(g.bpm, 120.0);
        assert_eq!(g.downbeats().next(), Some(1.0));
        apply_hint(&mut g, 60.0);
        assert_eq!(g.bpm, 60.0);
        assert_eq!(g.beats.first(), Some(&0.0));
    }
}
