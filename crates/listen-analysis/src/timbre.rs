//! Loudness, timbre vector and coarse mood.

use crate::decode::Pcm;
use listen_core::analysis::{BeatGrid, Mood};

/// Integrated loudness (LUFS) and loudness range (LU), EBU R128.
pub fn loudness(pcm: &Pcm, start: f32, end: f32) -> (f32, f32) {
    let sr = pcm.sample_rate;
    let s = ((start * sr as f32) as usize).min(pcm.left.len());
    let e = ((end * sr as f32) as usize).clamp(s, pcm.left.len());
    let Ok(mut meter) = ebur128::EbuR128::new(2, sr, ebur128::Mode::I | ebur128::Mode::LRA) else {
        return (-70.0, 0.0);
    };
    if meter.add_frames_planar_f32(&[&pcm.left[s..e], &pcm.right[s..e]]).is_err() {
        return (-70.0, 0.0);
    }
    let lufs = meter.loudness_global().unwrap_or(-70.0);
    let lra = meter.loudness_range().unwrap_or(0.0);
    let finite = |x: f64, d: f32| if x.is_finite() { x as f32 } else { d };
    (finite(lufs, -70.0).max(-70.0), finite(lra, 0.0))
}

/// bliss-audio's 23-dim feature vector, normalised to −1..1, from 22050 Hz mono.
/// Empty when the audio is too short.
pub fn bliss_vector(mono: &[f32]) -> Vec<f32> {
    bliss_audio::Song::analyze(mono).map(|a| a.as_vec()).unwrap_or_default()
}

/// Mean spectral centroid (Hz) from a [`bliss_vector`]; `None` for an empty vector.
pub fn bliss_centroid_hz(bliss: &[f32]) -> Option<f32> {
    let v = *bliss.get(bliss_audio::AnalysisIndex::MeanSpectralCentroid as usize)?;
    // bliss maps 0 … 11025 Hz (Nyquist at 22050 Hz) to −1 … 1.
    Some((v + 1.0) / 2.0 * 11025.0)
}

/// Brightness 0..1 from the spectral centroid on a log scale: 1.4 kHz → 0, 2.6 kHz → 1.
/// "Dark" is < 0.33, "bright" > 0.66.
pub fn brightness(centroid_hz: f32) -> f32 {
    ((centroid_hz.max(1.0) / 1400.0).log2() / (2600.0f32 / 1400.0).log2()).clamp(0.0, 1.0)
}

/// Coarse mood: energy from loudness and range, danceability from beat strength × steadiness,
/// brightness from the centroid (0.5 without a bliss vector).
pub fn mood(lufs: f32, lra: f32, grid: Option<&BeatGrid>, bliss: &[f32]) -> Mood {
    // Sigmoid centred on −8 LUFS (modern masters); a wide loudness range reads as less driving.
    let loud = 1.0 / (1.0 + (-(lufs + 8.0) / 2.0).exp());
    let steady = 1.0 - (lra / 12.0).clamp(0.0, 1.0);
    Mood {
        energy: (0.75 * loud + 0.25 * steady).clamp(0.0, 1.0),
        danceability: grid
            .map_or(0.0, |g| (g.confidence * ((g.stability - 0.8) / 0.2).clamp(0.0, 1.0)).clamp(0.0, 1.0)),
        brightness: bliss_centroid_hz(bliss).map_or(0.5, brightness),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::sine;

    #[test]
    fn loudness_of_a_sine() {
        // A 1 kHz stereo sine at amplitude 0.05 (−26 dBFS) reads about −26 LUFS.
        let s: Vec<f32> = sine(22050, 1000.0, 5.0).iter().map(|x| x * 0.1).collect();
        let pcm = Pcm { sample_rate: 22050, left: s.clone(), right: s };
        let (lufs, lra) = loudness(&pcm, 0.0, 5.0);
        assert!((-30.0..-22.0).contains(&lufs), "{lufs}");
        assert!(lra < 1.0);
        let v = bliss_vector(&pcm.mono());
        assert_eq!(v.len(), bliss_audio::NUMBER_FEATURES);
        assert!(v.iter().all(|x| x.is_finite()));
        assert!(bliss_vector(&[0.0; 100]).is_empty(), "too short");
        // A 1 kHz sine: centroid ≈ 1 kHz.
        let c = bliss_centroid_hz(&v).unwrap();
        assert!((800.0..1300.0).contains(&c), "{c}");
    }

    #[test]
    fn brightness_spreads_over_pop() {
        // Library centroids: ballad 1.5 kHz, median pop 1.97 kHz, dance-pop 2.4 kHz.
        assert!(brightness(1500.0) < 0.33);
        assert!((0.33..0.66).contains(&brightness(1970.0)), "{}", brightness(1970.0));
        assert!(brightness(2400.0) > 0.66);
        assert_eq!(brightness(400.0), 0.0);
        let m = mood(-8.0, 6.0, None, &[]);
        assert_eq!(m.brightness, 0.5, "no bliss vector → neutral");
    }
}
