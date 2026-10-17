//! Offline preview of a [`TransitionPlan`] (`listen --check-mix --render`). Approximate: speed changes
//! are linear resampling (pitch follows tempo, unlike mpv's scaletempo2) and the EQ is one-pole filters.

use crate::plan::{EqTarget, Kind, SpeedRamp, TransitionPlan, Which};

/// Renders the transition from `out` into `inn` (mono, `sr` Hz) with `context_s` seconds of context on
/// either side.
pub fn render(out: &[f32], inn: &[f32], sr: u32, plan: &TransitionPlan, context_s: f64) -> Vec<f32> {
    let sr_f = f64::from(sr.max(1));
    let dt = 1.0 / sr_f;
    if plan.kind == Kind::Gapless {
        // Tail of the outgoing straight into the head of the incoming.
        let from = ((plan.out_stop_s - context_s).max(0.0) * sr_f) as usize;
        let head = ((context_s * sr_f) as usize).min(inn.len());
        return out.get(from..).unwrap_or(&[]).iter().chain(&inn[..head]).copied().collect();
    }

    let first = plan.out_ramp.map_or(plan.out_mix_start_s, |r| r.start_out_s.min(plan.out_mix_start_s));
    let mut out_pos = (first - context_s).max(0.0);
    let mut in_pos: Option<f64> = None;
    let mut wall = 0.0;
    let (mut ramp_wall, mut mix_wall, mut stop_wall) = (None::<f64>, None::<f64>, None::<f64>);
    let mut events = plan.eq.clone();
    events.sort_by(|a, b| a.at_out_s.total_cmp(&b.at_out_s));
    let mut next_event = 0;
    let (mut fx_out, mut fx_in) = (Fx::new(sr_f), Fx::new(sr_f));
    let mut buf = Vec::new();
    // Safety cap: 15 minutes of output.
    let cap = (900.0 * sr_f) as usize;

    while buf.len() < cap {
        if ramp_wall.is_none() && plan.out_ramp.is_some_and(|r| out_pos >= r.start_out_s) {
            ramp_wall = Some(wall);
        }
        if mix_wall.is_none() && out_pos >= plan.out_mix_start_s {
            mix_wall = Some(wall);
            in_pos = Some(plan.in_mix_start_s);
        }
        if mix_wall.is_some() && stop_wall.is_none() && out_pos >= plan.out_stop_s {
            stop_wall = Some(wall);
        }
        if stop_wall.is_some_and(|s| wall - s >= context_s) {
            break;
        }
        while next_event < events.len() && events[next_event].at_out_s <= out_pos && stop_wall.is_none() {
            let e = events[next_event];
            match e.deck {
                Which::Out => fx_out.set(e.target, e.ramp_s),
                Which::In => fx_in.set(e.target, e.ramp_s),
            }
            next_event += 1;
        }

        let (g_out, g_in) = match mix_wall {
            Some(m) => plan.gain.gains_at(wall - m),
            None => (1.0, 0.0),
        };
        let mut y = 0.0;
        if stop_wall.is_none() {
            y += g_out * fx_out.process(sample(out, out_pos * sr_f));
        }
        if let Some(p) = in_pos {
            if p * sr_f >= inn.len() as f64 {
                break;
            }
            y += g_in * fx_in.process(sample(inn, p * sr_f));
        }
        buf.push(y);

        let s_out = speed(plan.out_ramp, ramp_wall, wall, 1.0);
        let s_in = match stop_wall {
            Some(_) => speed(plan.in_settle, stop_wall, wall, plan.in_speed),
            None => plan.in_speed,
        };
        out_pos += s_out * dt;
        if let Some(p) = &mut in_pos {
            *p += s_in * dt;
        }
        if stop_wall.is_none() && out_pos * sr_f >= out.len() as f64 && in_pos.is_none() {
            break; // the plan points past the outgoing audio
        }
        wall += dt;
    }
    buf
}

/// Speed at `wall` given a ramp that began at `began` (or `before` if it
/// hasn't); after the ramp the target holds.
fn speed(ramp: Option<SpeedRamp>, began: Option<f64>, wall: f64, before: f64) -> f64 {
    match (ramp, began) {
        (Some(r), Some(b)) => {
            let x = if r.secs > 0.0 { ((wall - b) / r.secs).clamp(0.0, 1.0) } else { 1.0 };
            r.from + (r.to - r.from) * x
        }
        (Some(r), None) => r.from,
        (None, _) => before,
    }
}

/// Linear interpolation at fractional sample index `i` (0 outside).
fn sample(x: &[f32], i: f64) -> f32 {
    if i < 0.0 {
        return 0.0;
    }
    let k = i.floor() as usize;
    let frac = (i - k as f64) as f32;
    match (x.get(k), x.get(k + 1)) {
        (Some(&a), Some(&b)) => a + (b - a) * frac,
        (Some(&a), None) => a,
        _ => 0.0,
    }
}

/// A parameter that moves linearly to its target.
#[derive(Clone, Copy)]
struct Param {
    value: f32,
    target: f32,
    step: f32,
}

impl Param {
    fn new(v: f32) -> Self {
        Param { value: v, target: v, step: 0.0 }
    }

    fn set(&mut self, target: f32, ramp_samples: f64) {
        self.target = target;
        if ramp_samples < 1.0 {
            self.value = target;
            self.step = 0.0;
        } else {
            self.step = ((target - self.value) / ramp_samples as f32).abs();
        }
    }

    fn tick(&mut self) -> f32 {
        if self.value < self.target {
            self.value = (self.value + self.step).min(self.target);
        } else if self.value > self.target {
            self.value = (self.value - self.step).max(self.target);
        }
        self.value
    }
}

/// One deck's mix filter: bass shelf → high-pass → low-pass.
struct Fx {
    sr: f64,
    bass_db: Param,
    hp_hz: Param,
    hp_mix: Param,
    lp_hz: Param,
    lp_mix: Param,
    lo: f32,
    hp_lo: f32,
    lp: f32,
}

/// Shelf corner of the bass swap (kick and bassline).
const BASS_HZ: f32 = 150.0;

impl Fx {
    fn new(sr: f64) -> Self {
        Fx {
            sr,
            bass_db: Param::new(0.0),
            hp_hz: Param::new(20.0),
            hp_mix: Param::new(0.0),
            lp_hz: Param::new(20_000.0),
            lp_mix: Param::new(0.0),
            lo: 0.0,
            hp_lo: 0.0,
            lp: 0.0,
        }
    }

    fn set(&mut self, target: EqTarget, ramp_s: f64) {
        let n = ramp_s * self.sr;
        match target {
            EqTarget::Bass(db) => self.bass_db.set(db, n),
            EqTarget::Highpass { hz, mix } => {
                self.hp_hz.set(hz, n);
                self.hp_mix.set(mix, n);
            }
            EqTarget::Lowpass { hz, mix } => {
                self.lp_hz.set(hz, n);
                self.lp_mix.set(mix, n);
            }
        }
    }

    fn coef(&self, hz: f32) -> f32 {
        1.0 - (-2.0 * std::f64::consts::PI * f64::from(hz) / self.sr).exp() as f32
    }

    fn process(&mut self, x: f32) -> f32 {
        let gain = 10f32.powf(self.bass_db.tick() / 20.0);
        self.lo += self.coef(BASS_HZ) * (x - self.lo);
        let mut y = x + (gain - 1.0) * self.lo;

        let (hz, mix) = (self.hp_hz.tick(), self.hp_mix.tick());
        self.hp_lo += self.coef(hz) * (y - self.hp_lo);
        y += mix * ((y - self.hp_lo) - y);

        let (hz, mix) = (self.lp_hz.tick(), self.lp_mix.tick());
        self.lp += self.coef(hz.min(self.sr as f32 * 0.45)) * (y - self.lp);
        y + mix * (self.lp - y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Envelope, EqEvent};

    fn tone(hz: f32, secs: f32, sr: u32) -> Vec<f32> {
        (0..(secs * sr as f32) as usize)
            .map(|i| (i as f32 * hz * std::f32::consts::TAU / sr as f32).sin())
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn fade(eq: Vec<EqEvent>) -> TransitionPlan {
        TransitionPlan {
            kind: Kind::Fade { secs: 2.0 },
            reason: String::new(),
            score: 0.0,
            out_mix_start_s: 4.0,
            in_mix_start_s: 0.0,
            in_load_at_s: 0.0,
            blend_s: 2.0,
            handover_out_s: 5.0,
            out_stop_s: 6.0,
            in_speed: 1.0,
            out_ramp: None,
            in_settle: None,
            gain: Envelope { points: vec![(0.0, 1.0, 0.0), (2.0, 0.0, 1.0)] },
            eq,
            voice_window: None,
        }
    }

    #[test]
    fn crossfade_preview_has_the_right_shape() {
        let sr = 8000;
        let (out, inn) = (tone(440.0, 8.0, sr), tone(660.0, 8.0, sr));
        let y = render(&out, &inn, sr, &fade(vec![]), 1.0);
        // 1 s before + 2 s blend + 1 s after.
        assert!((y.len() as f32 / sr as f32 - 4.0).abs() < 0.01, "{}", y.len());
        let s = sr as usize;
        assert!((rms(&y[..s / 2]) - rms(&out)).abs() < 0.01);
        assert!((rms(&y[y.len() - s / 2..]) - rms(&inn)).abs() < 0.01);
    }

    #[test]
    fn bass_cut_and_highpass_remove_low_end() {
        let sr = 8000;
        let low = tone(30.0, 8.0, sr);
        let cut = EqEvent { deck: Which::Out, at_out_s: 0.0, target: EqTarget::Bass(-24.0), ramp_s: 0.0 };
        let y = render(&low, &low, sr, &fade(vec![cut]), 1.0);
        assert!(rms(&y[..sr as usize]) < 0.3 * rms(&low), "{}", rms(&y[..sr as usize]));
        let hp = EqEvent {
            deck: Which::Out,
            at_out_s: 0.0,
            target: EqTarget::Highpass { hz: 800.0, mix: 1.0 },
            ramp_s: 0.0,
        };
        let y = render(&low, &low, sr, &fade(vec![hp]), 1.0);
        assert!(rms(&y[..sr as usize]) < 0.15 * rms(&low));
    }

    #[test]
    fn speed_ramp_shortens_the_outgoing() {
        let sr = 8000;
        let out = tone(440.0, 10.0, sr);
        let mut plan = fade(vec![]);
        plan.out_ramp = Some(SpeedRamp { from: 1.0, to: 1.5, start_out_s: 2.0, secs: 1.0 });
        let y = render(&out, &out, sr, &plan, 0.0);
        // From 2 s: 1 s ramp covers 1.25 s of file; then 0.75 s more at 1.5× → mix at wall ≈ 1.5 s.
        // Blend 2 s of wall; stop when outgoing reaches 6 s: 2 s of file at 1.5× ≈ 1.33 s.
        let secs = y.len() as f32 / sr as f32;
        assert!((secs - (1.5 + 4.0 / 3.0)).abs() < 0.02, "{secs}");
    }
}
