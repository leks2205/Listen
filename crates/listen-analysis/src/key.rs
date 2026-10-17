//! Global key from tuned pitch-class profiles: the treble against key profiles, the bassline
//! against chord-root profiles, and which bass note starts the phrases (separates relative
//! keys and loop rotations). Numbers in docs/dj-transitions.md, "Key detection and mood
//! calibration".

use crate::spectral::{FINE, Frames};
use listen_core::analysis::Camelot;

/// Temperley's major profile.
const MAJOR: [f32; 12] = [5.0, 2.0, 3.5, 2.0, 4.5, 4.0, 2.0, 4.5, 2.0, 3.5, 1.5, 4.0];
/// Temperley's minor with the natural minor's ♭7 (4.0) above the leading
/// tone (2.5).
const MINOR: [f32; 12] = [5.0, 2.0, 3.5, 4.5, 2.0, 4.0, 2.0, 4.5, 3.5, 2.0, 4.0, 2.5];
/// Bass notes = chord roots: I, V, IV, vi (major); i, iv, v, VI, VII, III
/// (minor), the tonic strongest.
const BASS_MAJOR: [f32; 12] = [5.0, 0.0, 1.0, 0.0, 1.0, 3.0, 0.0, 4.0, 0.0, 2.0, 0.0, 0.5];
const BASS_MINOR: [f32; 12] = [5.0, 0.0, 0.5, 2.0, 0.0, 3.0, 0.0, 3.0, 3.0, 0.0, 2.5, 0.5];
/// Weight of the bassline correlation next to the treble one.
const W_BASS: f32 = 0.5;
/// Weight of the tonic's share of the bass at phrase starts.
const W_START: f32 = 0.5;

fn pearson(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let (ma, mb) = (a.iter().sum::<f32>() / 12.0, b.iter().sum::<f32>() / 12.0);
    let (mut num, mut da, mut db) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        num += (x - ma) * (y - mb);
        da += (x - ma).powi(2);
        db += (y - mb).powi(2);
    }
    num / (da * db).sqrt().max(1e-12)
}

/// Song-level pitch-class profiles (C = 0) for key detection.
#[derive(Debug, Clone, Copy, Default)]
pub struct PitchProfile {
    /// Tones 100 Hz–5 kHz (harmony and melody).
    pub treble: [f32; 12],
    /// Tones 30–250 Hz (the bassline).
    pub bass: [f32; 12],
    /// The bass in the first half of each phrase-start bar.
    pub phrase_bass: [f32; 12],
    /// Estimated tuning, cents from A440.
    pub cents: f32,
}

/// Pitch profile of `[start, end)`: chroma summed over audible frames (each normalised to its
/// peak), tuning estimated from the peaks, folded to 12 tuned pitch classes. `phrase_starts`:
/// windows whose bass goes to [`PitchProfile::phrase_bass`] (empty without a beat grid).
pub fn pitch_profile(frames: &Frames, start: f32, end: f32, phrase_starts: &[(f32, f32)]) -> PitchProfile {
    const PER: usize = FINE / 12;
    let range = frames.range(start, end);
    let mut powers: Vec<f32> = frames.power[range.clone()].to_vec();
    powers.sort_by(f32::total_cmp);
    let floor = powers.get(powers.len() / 2).copied().unwrap_or(0.0) * 1e-3;
    let add = |sum: &mut [f32; FINE], c: &[f32; FINE]| {
        let peak = c.iter().copied().fold(0.0, f32::max);
        if peak > 0.0 {
            for (s, x) in sum.iter_mut().zip(c) {
                *s += x / peak;
            }
        }
    };
    let (mut treble, mut bass, mut phrase_bass) = ([0f32; FINE], [0f32; FINE], [0f32; FINE]);
    for i in range {
        if frames.power[i] > floor {
            add(&mut treble, &frames.fine_chroma[i]);
            add(&mut bass, &frames.bass_chroma[i]);
        }
    }
    for &(a, b) in phrase_starts {
        for i in frames.range(a.max(start), b.min(end)) {
            if frames.power[i] > floor {
                add(&mut phrase_bass, &frames.bass_chroma[i]);
            }
        }
    }
    // Circular mean of the position within the semitone.
    let (mut re, mut im) = (0f32, 0f32);
    for (b, x) in treble.iter().zip(&bass).map(|(t, b)| t + b).enumerate() {
        let a = 2.0 * std::f32::consts::PI * (b % PER) as f32 / PER as f32;
        re += x * a.cos();
        im += x * a.sin();
    }
    let shift =
        if re.hypot(im) > 1e-9 { im.atan2(re) / (2.0 * std::f32::consts::PI) * PER as f32 } else { 0.0 };
    // Triangular windows one semitone wide around the tuned centres.
    let fold = |fine: &[f32; FINE]| -> [f32; 12] {
        std::array::from_fn(|pc| {
            let centre = (pc * PER) as f32 + shift;
            fine.iter()
                .enumerate()
                .map(|(b, x)| {
                    let d = (b as f32 - centre).rem_euclid(FINE as f32);
                    let d = d.min(FINE as f32 - d);
                    x * (1.0 - d / (PER as f32 / 2.0)).max(0.0)
                })
                .sum()
        })
    };
    PitchProfile {
        treble: fold(&treble),
        bass: fold(&bass),
        phrase_bass: fold(&phrase_bass),
        cents: shift * 100.0 / PER as f32,
    }
}

/// Phrase-start windows for [`pitch_profile`]: the first half of every 4th bar from the
/// phrase lattice's offset (pop loops are 4 bars).
pub fn phrase_starts(bounds: &[f32], phrase_bars: u8, phrase_offset: u8) -> Vec<(f32, f32)> {
    let step = usize::from(phrase_bars.clamp(1, 4));
    (usize::from(phrase_offset) % step..bounds.len().saturating_sub(1))
        .step_by(step)
        .map(|b| (bounds[b], 0.5 * (bounds[b] + bounds[b + 1])))
        .collect()
}

/// Score of every key, best first: (score, tonic pitch class, minor).
fn scores(p: &PitchProfile) -> Vec<(f32, u8, bool)> {
    let has_bass = p.bass.iter().any(|&x| x > 0.0);
    let start_total: f32 = p.phrase_bass.iter().sum();
    let mut out = Vec::with_capacity(24);
    for tonic in 0..12u8 {
        let rot = |profile: &[f32; 12]| -> [f32; 12] {
            std::array::from_fn(|i| profile[(i + 12 - tonic as usize) % 12])
        };
        for (minor, profile, bass) in [(false, &MAJOR, &BASS_MAJOR), (true, &MINOR, &BASS_MINOR)] {
            let mut s = pearson(&p.treble, &rot(profile));
            if has_bass {
                s += W_BASS * pearson(&p.bass, &rot(bass));
            }
            if start_total > 0.0 {
                s += W_START * p.phrase_bass[tonic as usize] / start_total;
            }
            out.push((s, tonic, minor));
        }
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    out
}

/// Best key and a 0..1 confidence: its lead over the runner-up (any other key, so close
/// relatives read as unsure) times how well it fits at all.
pub fn detect(p: &PitchProfile) -> Option<(Camelot, f32)> {
    if p.treble.iter().all(|&x| x <= 0.0) {
        return None;
    }
    let s = scores(p);
    let (best, tonic, minor) = s[0];
    let fit = (pearson(
        &p.treble,
        &std::array::from_fn(|i| {
            let profile = if minor { &MINOR } else { &MAJOR };
            profile[(i + 12 - tonic as usize) % 12]
        }),
    ))
    .clamp(0.0, 1.0);
    let margin = ((best - s[1].0) / 0.3).clamp(0.0, 1.0);
    Some((Camelot::from_pitch(tonic, minor), (fit * (0.25 + 0.75 * margin)).clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::Pcm;
    use crate::spectral;
    use crate::test_util::chord_progression;

    fn key_of(pcm: &Pcm) -> (Camelot, f32) {
        let f = spectral::analyze(pcm);
        detect(&pitch_profile(&f, 0.0, pcm.duration(), &[])).unwrap()
    }

    #[test]
    fn c_major_progression() {
        // C – F – G – C (I IV V I).
        let pcm = chord_progression(&[&[60, 64, 67], &[65, 69, 72], &[67, 71, 74], &[60, 64, 67]], 2.0, 3);
        let (key, conf) = key_of(&pcm);
        assert_eq!(key.label(), "8B", "C major");
        assert!(conf > 0.3, "{conf}");
    }

    #[test]
    fn a_minor_progression() {
        // Am – Dm – E – Am (i iv V i), with the leading tone G#.
        let pcm = chord_progression(&[&[57, 60, 64], &[62, 65, 69], &[64, 68, 71], &[57, 60, 64]], 2.0, 3);
        assert_eq!(key_of(&pcm).0.label(), "8A", "A minor");
    }

    #[test]
    fn natural_minor_progression() {
        // Am – G – F – G – Am (i VII VI VII i): no leading tone, the case
        // Temperley's profile called C major.
        let am = [45, 57, 60, 64];
        let pcm =
            chord_progression(&[&am, &[43, 55, 59, 62], &[41, 53, 57, 60], &[43, 55, 59, 62], &am], 2.0, 3);
        assert_eq!(key_of(&pcm).0.label(), "8A", "A minor");
    }

    #[test]
    fn detuned_song_keeps_its_key() {
        // C major 40 cents sharp: without tuning estimation the peaks
        // straddle C/C# and the key wobbles.
        let c = [48, 60, 64, 67];
        let mut pcm = chord_progression(&[&c, &[53, 65, 69, 72], &[55, 67, 71, 74], &c], 2.0, 3);
        // Resample by 2^(40/1200) (linear interpolation): every pitch +40 ct.
        let ratio = 2f32.powf(40.0 / 1200.0);
        let shift = |x: &[f32]| -> Vec<f32> {
            (0..(x.len() as f32 / ratio) as usize - 1)
                .map(|i| {
                    let p = i as f32 * ratio;
                    let (j, fr) = (p as usize, p.fract());
                    x[j] * (1.0 - fr) + x[(j + 1).min(x.len() - 1)] * fr
                })
                .collect()
        };
        pcm.left = shift(&pcm.left);
        pcm.right = shift(&pcm.right);
        let f = spectral::analyze(&pcm);
        let p = pitch_profile(&f, 0.0, pcm.duration(), &[]);
        assert!((p.cents - 40.0).abs() < 8.0, "{}", p.cents);
        assert_eq!(detect(&p).unwrap().0.label(), "8B");
    }

    #[test]
    fn phrase_start_bass_breaks_the_relative_tie() {
        // The same chord loop heard as A minor (Am F C G) or C major: the
        // bass at phrase starts decides.
        let am = [45, 57, 60, 64];
        let loop_ = [&am[..], &[41, 57, 60, 65], &[48, 55, 60, 64], &[43, 55, 59, 62]];
        let pcm = chord_progression(&loop_, 2.0, 4);
        let f = spectral::analyze(&pcm);
        let bounds: Vec<f32> = (0..=16).map(|b| b as f32 * 2.0).collect();
        let starts = phrase_starts(&bounds, 4, 0);
        assert_eq!(starts.len(), 4);
        let p = pitch_profile(&f, 0.0, pcm.duration(), &starts);
        assert_eq!(detect(&p).unwrap().0.label(), "8A");
        let starts = phrase_starts(&bounds, 4, 2);
        let p = pitch_profile(&f, 0.0, pcm.duration(), &starts);
        assert_eq!(detect(&p).unwrap().0.label(), "8B", "loop heard from the C chord");
    }

    #[test]
    fn silence_has_no_key() {
        let pcm = Pcm { sample_rate: 22050, left: vec![0.0; 22050], right: vec![0.0; 22050] };
        let f = spectral::analyze(&pcm);
        assert!(detect(&pitch_profile(&f, 0.0, 1.0, &[])).is_none());
    }
}
