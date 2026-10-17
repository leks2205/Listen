//! Prepares a TTS clip for airing: normalises loudness, pads silence and writes a 16-bit PCM WAV.

/// Gemini TTS output when there is no RIFF header: 24 kHz mono s16le.
const RAW_RATE: u32 = 24_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

impl Pcm {
    pub fn duration_secs(&self) -> f32 {
        self.samples.len() as f32 / (self.rate as f32 * f32::from(self.channels.max(1)))
    }
}

/// Reads 16-bit PCM from a WAV, or headerless bytes as Gemini's raw 24 kHz mono output.
pub fn decode(bytes: &[u8]) -> Result<Pcm, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Ok(Pcm { rate: RAW_RATE, channels: 1, samples: to_i16(bytes) });
    }
    let (mut rate, mut channels, mut bits) = (RAW_RATE, 1u16, 16u16);
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let declared = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = at + 8;
        // Streaming encoders write 0 or 0xFFFFFFFF for "until the end".
        let len = if declared == 0 || body + declared > bytes.len() { bytes.len() - body } else { declared };
        match id {
            b"fmt " if len >= 16 => {
                let format = u16::from_le_bytes([bytes[body], bytes[body + 1]]);
                channels = u16::from_le_bytes([bytes[body + 2], bytes[body + 3]]);
                rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().unwrap());
                bits = u16::from_le_bytes([bytes[body + 14], bytes[body + 15]]);
                if format != 1 && format != 0xFFFE {
                    return Err(format!("unsupported WAV format {format}"));
                }
            }
            b"data" => {
                if bits != 16 {
                    return Err(format!("unsupported WAV sample size {bits} bit"));
                }
                return Ok(Pcm {
                    rate,
                    channels: channels.max(1),
                    samples: to_i16(&bytes[body..body + len]),
                });
            }
            _ => {}
        }
        at = body + len + (len & 1);
    }
    Err("WAV without audio data".into())
}

fn to_i16(bytes: &[u8]) -> Vec<i16> {
    bytes.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect()
}

/// Scales the voice to `target_dbfs` RMS plus `gain_db` with a soft peak limiter (speech has 15+ dB
/// crest factor), and pads `pad_secs` of silence at both ends.
pub fn prepare(mut pcm: Pcm, target_dbfs: f32, gain_db: f32, pad_secs: f32) -> Pcm {
    // RMS over the speech only (ignore near-silence between words).
    let loud: Vec<f64> =
        pcm.samples.iter().map(|&s| f64::from(s) / 32768.0).filter(|v| v.abs() > 0.003).collect();
    if !loud.is_empty() {
        let rms = (loud.iter().map(|v| v * v).sum::<f64>() / loud.len() as f64).sqrt();
        let peak = pcm.samples.iter().map(|&s| (f64::from(s) / 32768.0).abs()).fold(0.0, f64::max);
        let want = 10f64.powf(f64::from(target_dbfs + gain_db) / 20.0);
        // Peaks may go up to 9 dB into the limiter, no further.
        let factor = (want / rms.max(1e-6)).min(2.8 / peak.max(1e-6));
        for s in &mut pcm.samples {
            *s = (soft_limit(f64::from(*s) / 32768.0 * factor) * 32767.0).round() as i16;
        }
    }
    let pad = (pcm.rate as f32 * pad_secs) as usize * usize::from(pcm.channels);
    let mut samples = vec![0i16; pad];
    samples.extend_from_slice(&pcm.samples);
    samples.extend(std::iter::repeat_n(0, pad));
    pcm.samples = samples;
    pcm
}

/// Linear below -6 dBFS, then a tanh knee that never exceeds -1 dBFS.
fn soft_limit(x: f64) -> f64 {
    const KNEE: f64 = 0.5;
    const CEILING: f64 = 0.891;
    let a = x.abs();
    if a <= KNEE {
        return x;
    }
    let room = CEILING - KNEE;
    x.signum() * (KNEE + room * ((a - KNEE) / room).tanh())
}

pub fn encode(pcm: &Pcm) -> Vec<u8> {
    let data_len = (pcm.samples.len() * 2) as u32;
    let block = pcm.channels * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&pcm.channels.to_le_bytes());
    out.extend_from_slice(&pcm.rate.to_le_bytes());
    out.extend_from_slice(&(pcm.rate * u32::from(block)).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in &pcm.samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, amp: f32) -> Pcm {
        let samples = (0..rate).map(|i| ((i as f32 * 0.05).sin() * amp * 32767.0) as i16).collect();
        Pcm { rate, channels: 1, samples }
    }

    #[test]
    fn round_trips_and_reads_raw() {
        let pcm = tone(24_000, 0.5);
        assert_eq!(decode(&encode(&pcm)).unwrap(), pcm);
        let raw: Vec<u8> = pcm.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        assert_eq!(decode(&raw).unwrap(), pcm);
        assert!((pcm.duration_secs() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn streaming_header_with_zero_length() {
        let mut bytes = encode(&tone(24_000, 0.2));
        bytes[40..44].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(decode(&bytes).unwrap().samples.len(), 24_000);
    }

    #[test]
    fn prepare_levels_and_pads() {
        let quiet = prepare(tone(24_000, 0.05), -16.0, 0.0, 0.25);
        assert_eq!(quiet.samples.len(), 24_000 + 2 * 6_000);
        assert!(quiet.samples[..6_000].iter().all(|&s| s == 0));
        let body = &quiet.samples[6_000..30_000];
        let rms =
            (body.iter().map(|&s| (f64::from(s) / 32768.0).powi(2)).sum::<f64>() / body.len() as f64).sqrt();
        let db = 20.0 * rms.log10();
        assert!((db + 16.0).abs() < 1.5, "rms {db} dBFS");
        // A loud target can't push peaks past -1 dBFS.
        let loud = prepare(tone(24_000, 0.5), 0.0, 6.0, 0.0);
        assert!(soft_limit(0.3) == 0.3 && soft_limit(-5.0) > -0.892 && soft_limit(5.0) < 0.892);
        let peak = loud.samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(f64::from(peak) / 32768.0 <= 0.892);
    }
}
