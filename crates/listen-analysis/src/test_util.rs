//! Synthetic test signals.

use crate::decode::Pcm;
use std::f32::consts::PI;

pub fn sine(sr: u32, hz: f32, secs: f32) -> Vec<f32> {
    (0..(sr as f32 * secs) as usize).map(|i| 0.5 * (2.0 * PI * hz * i as f32 / sr as f32).sin()).collect()
}

/// Deterministic white noise (xorshift).
pub fn noise(sr: u32, secs: f32, amp: f32, seed: u64) -> Vec<f32> {
    let mut x = seed.max(1);
    (0..(sr as f32 * secs) as usize)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            amp * ((x >> 11) as f32 / (1u64 << 53) as f32 * 2.0 - 1.0)
        })
        .collect()
}

/// Clicks at `bpm` from `first` seconds; every beat `i % 4 == accent` also
/// gets a 60 Hz kick (the downbeat).
pub fn click_track(bpm: f32, secs: f32, first: f32, accent: usize) -> Pcm {
    let sr = 22050u32;
    let n = (sr as f32 * secs) as usize;
    let mut out = vec![0f32; n];
    let hiss = noise(sr, 0.02, 0.4, 3);
    let period = 60.0 / bpm;
    let mut i = 0;
    loop {
        let t = first + i as f32 * period;
        let s = (t * sr as f32) as usize;
        if s >= n {
            break;
        }
        for (j, h) in hiss.iter().enumerate() {
            if let Some(o) = out.get_mut(s + j) {
                *o += h * (1.0 - j as f32 / hiss.len() as f32);
            }
        }
        if i % 4 == accent {
            for j in 0..(0.15 * sr as f32) as usize {
                let tt = j as f32 / sr as f32;
                if let Some(o) = out.get_mut(s + j) {
                    *o += 0.8 * (2.0 * PI * 60.0 * tt).sin() * (-tt * 25.0).exp();
                }
            }
        }
        i += 1;
    }
    Pcm { sample_rate: sr, left: out.clone(), right: out }
}

fn midi_hz(m: u8) -> f32 {
    440.0 * 2f32.powf((f32::from(m) - 69.0) / 12.0)
}

/// Chords (MIDI notes) of `secs` each, repeated; tones with a few harmonics.
pub fn chord_progression(chords: &[&[u8]], secs: f32, repeats: usize) -> Pcm {
    let sr = 22050u32;
    let per = (sr as f32 * secs) as usize;
    let mut out = Vec::with_capacity(per * chords.len() * repeats);
    for _ in 0..repeats {
        for chord in chords {
            for i in 0..per {
                let t = i as f32 / sr as f32;
                let env = (t * 20.0).min(1.0) * ((secs - t) * 20.0).min(1.0);
                let mut v = 0.0;
                for &m in *chord {
                    let f = midi_hz(m);
                    for (h, a) in [(1.0, 1.0), (2.0, 0.4), (3.0, 0.2)] {
                        v += a * (2.0 * PI * f * h * t).sin();
                    }
                }
                // A soft bass note an octave below the root.
                let root = midi_hz(chord[0] - 12);
                v += 0.6 * (2.0 * PI * root * t).sin();
                out.push(0.08 * env * v);
            }
        }
    }
    Pcm { sample_rate: sr, left: out.clone(), right: out }
}

/// 16-bit mono WAV.
pub fn write_wav(path: &std::path::Path, sr: u32, samples: &[f32]) {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
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
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}
