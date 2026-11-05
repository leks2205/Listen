//! The echo of an [`EchoOut`](crate::plan::Kind::EchoOut) transition as a sample-level effect, for the
//! offline preview ([`crate::render`]) and the Android mixer. The desktop builds the same thing from
//! ffmpeg filters in the deck's mpv chain (`listen_core::settings::deck_audio_filters`).
//!
//! What goes into the send is repeated [`TAPS`] times, `delay_s` apart, each repeat `feedback` times the
//! one before (a feedback delay cut off after four repeats, as on the desktop). `dry` is the level of the
//! direct signal: an echo-out opens the send for the last beat of a phrase, then mutes the dry signal and
//! closes the send, so only the repeats ring on.

use crate::plan::EqTarget;

/// Repeats of the send.
pub const TAPS: usize = 4;
/// Longest delay between repeats (s).
pub const MAX_DELAY_S: f32 = 1.0;
/// Send and dry levels glide over this long (s) so opening or muting doesn't click.
const GLIDE_S: f32 = 0.005;

/// Echo settings ([`EqTarget::Echo`]); [`EchoParams::IDLE`] passes the signal through unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoParams {
    pub delay_s: f32,
    pub feedback: f32,
    pub send: f32,
    pub dry: f32,
}

impl EchoParams {
    pub const IDLE: EchoParams = EchoParams { delay_s: 0.0, feedback: 0.0, send: 0.0, dry: 1.0 };

    /// The echo part of an EQ target (`None` for the filters).
    pub fn of(target: EqTarget) -> Option<Self> {
        match target {
            EqTarget::Echo { delay_s, feedback, send, dry } => {
                Some(EchoParams { delay_s, feedback, send, dry })
            }
            _ => None,
        }
    }

    /// Level of repeat `k` (1-based) relative to the send.
    pub fn tap_gain(&self, k: usize) -> f32 {
        self.feedback.clamp(0.0, 0.95).powi(k as i32)
    }
}

/// An interleaved multi-channel echo with a ring buffer of `TAPS × MAX_DELAY_S`.
pub struct Echo {
    sr: f32,
    channels: usize,
    params: EchoParams,
    send: f32,
    dry: f32,
    /// Ring of what went into the send (frames × channels); empty until the echo is first used.
    ring: Vec<f32>,
    pos: usize,
}

impl Echo {
    pub fn new(sr: u32, channels: usize) -> Self {
        Echo {
            sr: sr.max(1) as f32,
            channels: channels.max(1),
            params: EchoParams::IDLE,
            send: 0.0,
            dry: 1.0,
            ring: Vec::new(),
            pos: 0,
        }
    }

    pub fn params(&self) -> EchoParams {
        self.params
    }

    /// Nothing to do: dry at full level and no repeats left to play.
    pub fn idle(&self) -> bool {
        self.params.send == 0.0
            && self.params.dry == 1.0
            && self.send == 0.0
            && self.dry == 1.0
            && self.ring.is_empty()
    }

    pub fn set(&mut self, p: EchoParams) {
        let p = EchoParams { delay_s: p.delay_s.clamp(0.0, MAX_DELAY_S), ..p };
        if p.send > 0.0 && self.ring.is_empty() {
            let frames = (TAPS as f32 * MAX_DELAY_S * self.sr) as usize + 1;
            self.ring = vec![0.0; frames * self.channels];
            self.pos = 0;
        }
        self.params = p;
    }

    /// Back to [`EchoParams::IDLE`] at once, dropping the repeats.
    pub fn reset(&mut self) {
        self.params = EchoParams::IDLE;
        self.send = 0.0;
        self.dry = 1.0;
        self.ring = Vec::new();
        self.pos = 0;
    }

    /// Processes interleaved frames in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        if self.idle() {
            return;
        }
        let ch = self.channels;
        let step = 1.0 / (GLIDE_S * self.sr);
        let glide = |v: &mut f32, to: f32| {
            *v = if *v < to { (*v + step).min(to) } else { (*v - step).max(to) };
        };
        let delay = (self.params.delay_s * self.sr).round() as usize;
        let frames = self.ring.len() / ch;
        let gains: [f32; TAPS] = std::array::from_fn(|k| self.params.tap_gain(k + 1));
        for frame in buf.chunks_mut(ch) {
            glide(&mut self.send, self.params.send);
            glide(&mut self.dry, self.params.dry);
            for (c, x) in frame.iter_mut().enumerate() {
                let input = *x;
                let mut wet = 0.0;
                if frames > 0 && delay > 0 {
                    for (k, g) in gains.iter().enumerate() {
                        let back = (k + 1) * delay;
                        if back < frames {
                            wet += g * self.ring[((self.pos + frames - back) % frames) * ch + c];
                        }
                    }
                }
                if frames > 0 {
                    self.ring[self.pos * ch + c] = self.send * input;
                }
                *x = self.dry * input + wet;
            }
            if frames > 0 {
                self.pos = (self.pos + 1) % frames;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn burst(sr: u32, secs: f32, on: (f32, f32)) -> Vec<f32> {
        (0..(secs * sr as f32) as usize)
            .map(|i| {
                let t = i as f32 / sr as f32;
                if t >= on.0 && t < on.1 { (t * 440.0 * std::f32::consts::TAU).sin() } else { 0.0 }
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn idle_echo_is_transparent() {
        let x = burst(8000, 1.0, (0.1, 0.3));
        let mut y = x.clone();
        let mut e = Echo::new(8000, 1);
        e.process(&mut y);
        assert_eq!(x, y);
        assert!(e.ring.is_empty());
    }

    #[test]
    fn repeats_come_delay_apart_and_decay() {
        let sr = 8000;
        let s = |t: f32| (t * sr as f32) as usize;
        let mut y = burst(sr, 3.0, (0.0, 0.1));
        let mut e = Echo::new(sr, 1);
        e.set(EchoParams { delay_s: 0.4, feedback: 0.5, send: 1.0, dry: 1.0 });
        e.process(&mut y[..s(0.1)]);
        e.set(EchoParams { delay_s: 0.4, feedback: 0.5, send: 0.0, dry: 0.0 });
        e.process(&mut y[s(0.1)..]);
        let level: Vec<f32> = (1..=5).map(|k| rms(&y[s(0.4 * k as f32)..s(0.4 * k as f32 + 0.1)])).collect();
        let dry = rms(&burst(sr, 0.1, (0.0, 0.1)));
        for (k, l) in level.iter().take(TAPS).enumerate() {
            let want = dry * 0.5f32.powi(k as i32 + 1);
            assert!((l - want).abs() < 0.1 * want + 0.01, "repeat {}: {l} vs {want}", k + 1);
        }
        assert!(level[4] < 1e-3, "only {TAPS} repeats: {}", level[4]);
        // Between the repeats (and the muted dry signal) it is silent.
        assert!(rms(&y[s(0.15)..s(0.38)]) < 0.02);
    }

    #[test]
    fn stereo_channels_stay_apart() {
        let sr = 8000;
        let mono = burst(sr, 1.0, (0.0, 0.05));
        let mut y: Vec<f32> = mono.iter().flat_map(|&v| [v, 0.0]).collect();
        let mut e = Echo::new(sr, 2);
        e.set(EchoParams { delay_s: 0.2, feedback: 0.5, send: 1.0, dry: 1.0 });
        e.process(&mut y);
        let right: Vec<f32> = y.iter().skip(1).step_by(2).copied().collect();
        assert!(rms(&right) < 1e-6);
        let left: Vec<f32> = y.iter().step_by(2).copied().collect();
        assert!(rms(&left[1600..2000]) > 0.1);
    }
}
