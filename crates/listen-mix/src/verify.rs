//! Render-and-verify for planned transitions (DJustify-style gates, PLAN item 11): before a plan is
//! armed it is played through on the analysis curves — both decks' loudness (bar curve anchored on the
//! integrated loudness, [`LocalLevel`]), lead-vocal likelihood (with synced lyrics merged in) and the
//! plan's gain envelope, equal-loudness trim, EQ moves and echo repeats — every [`STEP_S`]. A plan fails
//! when the mix falls below the quieter song's own level and the edge-to-edge line, or rises above the
//! louder one and the line, while both sound (by more than [`MAX_LEVEL_LU`]), when both decks are audibly sung at once, or when the outgoing
//! leaves in the middle of a sung line (with synced lyrics). Failing
//! plans give way to the planner's next option ([`crate::plan::alternatives`]), then the plain fade.
//!
//! No audio is needed, so this runs at plan time on both apps (the desktop plans before it fetches
//! the incoming file). [`audio_levels`] measures the same dip and bump on a rendered preview
//! ([`crate::render`]) for `listen --check-mix --render` and the bench.

use crate::loudness::{EDGE_REF_S, LocalLevel, blend_trim};
use crate::plan::{EqTarget, Kind, PlanCtx, TransitionPlan, Which, alternatives};
use listen_core::analysis::{Features, cue};

/// Simulation step (wall s).
pub const STEP_S: f64 = 0.1;
/// Largest hole or bump of the summed loudness during a blend (LU, see [`Check::dip_lu`]).
pub const MAX_LEVEL_LU: f32 = 1.5;
/// The curves' error on a hole (LU): rendered blends' holes measured (stereo BS.1770, decks aligned)
/// against the modelled ones differ by about this much (bug 57: −0.69, +0.58, −0.11 on the lyric-armed
/// BeatBlends with a modelled hole; the bar curve's own error against the audio is 0.8 dB sd mid-song).
/// A modelled hole fails at [`MAX_LEVEL_LU`] minus this, so a hole the curves place just under the gate
/// does not render just over it. On the 427 history pairs 12 plans change, all to another blend.
pub const HOLE_MARGIN_LU: f32 = 0.6;
/// Longest time both decks may be audibly sung at once (s).
pub const MAX_CLASH_S: f32 = 1.0;
/// A deck's vocal counts as audible above this (vocal likelihood × deck gain).
const AUDIBLE_VOCAL: f32 = 0.4;
/// The outgoing "leaves" when its gain falls under this.
const GONE_GAIN: f32 = 0.3;
/// Sung bars on both sides of the outgoing's exit, without a line end between them: a line is cut.
const SUNG: f32 = 0.6;
/// Plans tried per transition (the plain fade comes on top as the last).
pub const MAX_TRIED: usize = 6;

/// The outcome of one plan's simulation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Check {
    /// Largest hole / bump of the summed loudness during the blend (LU,
    /// ≥ 0): how far the mix falls below the quieter / rises above the louder of the two songs' own
    /// (levelled) loudness while both sound. `None` without measured loudness or for handovers (no overlap).
    pub dip_lu: Option<f32>,
    pub bump_lu: Option<f32>,
    /// Seconds with both decks audibly sung (echo repeats count on the outgoing side).
    pub clash_s: f32,
    /// The outgoing is gone while its singer is mid-line (synced lyrics: no line end in the exit bar).
    pub cuts_line: bool,
}

impl Check {
    /// Why the plan fails, `None` if it passes.
    pub fn failure(&self) -> Option<String> {
        let mut why = Vec::new();
        if let Some(d) = self.dip_lu.filter(|&d| d + HOLE_MARGIN_LU > MAX_LEVEL_LU) {
            why.push(format!("loudness dip {d:.1} LU (may render {:.1})", d + HOLE_MARGIN_LU));
        }
        if let Some(b) = self.bump_lu.filter(|&b| b > MAX_LEVEL_LU) {
            why.push(format!("loudness bump {b:.1} LU"));
        }
        if self.clash_s > MAX_CLASH_S {
            why.push(format!("vocals clash {:.1} s", self.clash_s));
        }
        if self.cuts_line {
            why.push("cuts a sung line".to_string());
        }
        (!why.is_empty()).then(|| why.join(", "))
    }

    /// How far past the gates (0 when it passes): for picking the least bad plan when none passes.
    pub fn badness(&self) -> f32 {
        let over = |v: Option<f32>| v.map_or(0.0, |v| (v - MAX_LEVEL_LU).max(0.0));
        over(self.dip_lu.map(|d| d + HOLE_MARGIN_LU))
            + over(self.bump_lu)
            + (self.clash_s - MAX_CLASH_S).max(0.0)
            + if self.cuts_line { 1.0 } else { 0.0 }
    }
}

/// A plan that failed its check.
#[derive(Debug, Clone, PartialEq)]
pub struct Rejected {
    pub kind: Kind,
    pub out_mix_start_s: f64,
    pub in_mix_start_s: f64,
    pub reason: String,
}

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} at {:.1}s → {:.1}s: {}",
            self.kind, self.out_mix_start_s, self.in_mix_start_s, self.reason
        )
    }
}

/// The plan to arm, its check, and the plans it beat.
#[derive(Debug, Clone)]
pub struct Verified {
    pub plan: TransitionPlan,
    pub check: Check,
    /// Tried before `plan`, in order; empty when the planner's first choice passed.
    pub rejected: Vec<Rejected>,
    /// No option passed: `plan` is the least bad.
    pub none_passed: bool,
}

/// Per-song curves for the simulation (built once per pair).
pub struct Song<'a> {
    f: &'a Features,
    level: Option<LocalLevel>,
    /// Bar boundaries (file s) for the vocal curve; empty without a grid.
    bounds: Vec<f64>,
}

impl<'a> Song<'a> {
    pub fn new(f: &'a Features) -> Self {
        let bounds = match f.grid.as_ref().filter(|g| g.bar_count() > 0) {
            Some(g) => (0..=g.bar_count()).map(|b| f64::from(g.bar_time(b))).collect(),
            None => Vec::new(),
        };
        Song { f, level: LocalLevel::new(f), bounds }
    }

    /// The bar that file time `t` falls in.
    fn bar_at(&self, t: f64) -> Option<usize> {
        let b = self.bounds.partition_point(|&b| b <= t);
        (b >= 1 && b < self.bounds.len()).then(|| b - 1)
    }

    /// Lead-vocal likelihood (0..1) at file time `t`; without a curve, from the instrumental intro and
    /// outro counts (as the planner does); 0 outside the song.
    fn vocal(&self, t: f64) -> f32 {
        let f = self.f;
        if t < f64::from(f.audio_start) || t >= f64::from(f.audio_end) || self.bounds.len() < 2 {
            return 0.0;
        }
        let Some(bar) = self.bar_at(t) else { return 0.0 };
        if let Some(&v) = f.bars.vocal.get(bar) {
            return f32::from(v) / 255.0;
        }
        if !f.bars.vocal.is_empty() {
            return 0.0;
        }
        let bars = self.bounds.len() - 1;
        let instrumental = bar < usize::from(f.intro_instrumental_bars)
            || bar >= bars.saturating_sub(usize::from(f.outro_instrumental_bars));
        if instrumental { 0.0 } else { 1.0 }
    }
}

/// Loudness change (LU, ≤ 0) a deck filter causes on typical pop (roughly pink, K-weighted): a bass
/// shelf takes up to ~2 LU, a pass filter the share of the spectrum it removes (log-frequency).
fn eq_loss_lu(t: EqTarget) -> f32 {
    let share = |hz: f32| ((hz.max(40.0) / 40.0).ln() / (16000.0f32 / 40.0).ln()).clamp(0.0, 0.95);
    let lu = |removed: f32| 10.0 * (1.0 - removed.clamp(0.0, 0.95)).log10();
    match t {
        EqTarget::Bass(db) => (db / 12.0).clamp(-2.0, 0.0),
        EqTarget::Highpass { hz, mix } => lu(mix.clamp(0.0, 1.0) * share(hz)),
        EqTarget::Lowpass { hz, mix } => lu(mix.clamp(0.0, 1.0) * (1.0 - share(hz))),
        EqTarget::Echo { .. } => 0.0,
    }
}

/// How much of a sung line still comes through a deck filter (high-passes above the voice's body thin it).
fn eq_vocal(t: EqTarget) -> f32 {
    match t {
        EqTarget::Highpass { hz, mix } => 1.0 - mix.clamp(0.0, 1.0) * ((hz - 300.0) / 3000.0).clamp(0.0, 0.7),
        EqTarget::Lowpass { hz, mix } => 1.0 - mix.clamp(0.0, 1.0) * ((3000.0 - hz) / 3000.0).clamp(0.0, 0.9),
        _ => 1.0,
    }
}

/// Filter state of one deck: (loss LU, vocal factor) at outgoing file time `out_t`, with ramps followed
/// linearly from the previous target of the same kind of filter.
fn eq_state(plan: &TransitionPlan, deck: Which, out_t: f64, out_speed: f64) -> (f32, f32) {
    // Bass, high-pass and low-pass each hold their own state.
    let mut cur: [(f32, f32); 3] = [(0.0, 1.0); 3];
    for e in plan.eq.iter().filter(|e| e.deck == deck && e.at_out_s <= out_t + 1e-9) {
        let slot = match e.target {
            EqTarget::Bass(_) => 0,
            EqTarget::Highpass { .. } => 1,
            EqTarget::Lowpass { .. } => 2,
            EqTarget::Echo { .. } => continue,
        };
        let to = (eq_loss_lu(e.target), eq_vocal(e.target));
        let x = if e.ramp_s > 0.0 {
            (((out_t - e.at_out_s) / out_speed) / e.ramp_s).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let x = x as f32;
        let from = cur[slot];
        cur[slot] = (from.0 + (to.0 - from.0) * x, from.1 + (to.1 - from.1) * x);
    }
    (cur.iter().map(|c| c.0).sum(), cur.iter().map(|c| c.1).product())
}

/// Simulates `plan` from `out` into `inn` with the decks' playback gains (levelling, dB).
/// Takes well under a millisecond (about 0.1 ms on the PC).
pub fn check(plan: &TransitionPlan, out: &Song, inn: &Song, gains_db: (f32, f32)) -> Check {
    let mut c = Check::default();
    if plan.kind == Kind::Gapless {
        return c;
    }
    let handover = matches!(plan.kind, Kind::EchoOut { .. } | Kind::Cut);
    let s_out = plan.out_ramp.map_or(1.0, |r| r.to).max(0.25);
    let s_in = plan.in_speed.max(0.25);
    let out_t = |w: f64| plan.out_mix_start_s + w * s_out;
    let in_t = |w: f64| plan.in_mix_start_s + w * s_in;
    // Echo repeats: the last `delay` before the stop, `delay` apart, each `feedback` times the last.
    let echo = match plan.kind {
        Kind::EchoOut { delay_s } => Some((f64::from(delay_s.max(0.05)), 0.5f32)),
        _ => None,
    };
    let stop_w = (plan.out_stop_s - plan.out_mix_start_s) / s_out;
    let end_w = plan.blend_s + EDGE_REF_S;

    let levels = match (&out.level, &inn.level) {
        (Some(lo), Some(li)) if !handover && plan.blend_s > 0.0 => Some((lo, li)),
        _ => None,
    };
    // The edge-to-edge line the equal-loudness trim aims for (outgoing before, incoming after).
    let line = levels.map(|(lo, li)| {
        let from = lo.mean(plan.out_mix_start_s - EDGE_REF_S * s_out, plan.out_mix_start_s) + gains_db.0;
        let b = in_t(plan.blend_s);
        (from, li.mean(b, b + EDGE_REF_S * s_in) + gains_db.1)
    });
    let (mut dip, mut bump) = (0.0f32, 0.0f32);
    // An echo-out's or cut's dry signal ends at the mix point; a blend's exit is found below.
    let mut gone_at = plan.out_mix_start_s;

    let steps = (end_w / STEP_S).ceil() as usize;
    for k in 0..=steps {
        let w = k as f64 * STEP_S;
        let (to, ti) = (out_t(w), in_t(w));
        let (g_out, g_in) = plan.gain.gains_at(w);
        let trim = blend_trim(plan, w, g_out, g_in, gains_db.0, gains_db.1);
        // An echo-out's dry signal stops at the mix point; the repeats ring on.
        let out_dry = if echo.is_some() { w < 0.0 } else { w < stop_w };
        let (go, gi) = (if out_dry { g_out * trim } else { 0.0 }, g_in * trim);
        let (loss_o, voc_o) = eq_state(plan, Which::Out, to, s_out);
        let (loss_i, voc_i) = eq_state(plan, Which::In, to, s_out);

        // Vocals: the outgoing's dry signal, or an echo repeat of its last beat.
        let vo = if out_dry {
            out.vocal(to) * go * voc_o
        } else if let Some((d, fb)) = echo {
            let j = (w / d).floor() as i32 + 1; // repeat number
            if j <= crate::echo::TAPS as i32 {
                out.vocal(plan.out_mix_start_s - d * s_out / 2.0) * fb.powi(j) * voc_o
            } else {
                0.0
            }
        } else {
            0.0
        };
        let vi = inn.vocal(ti) * gi * voc_i;
        if vo > AUDIBLE_VOCAL && vi > AUDIBLE_VOCAL && w <= plan.blend_s + 1e-9 {
            c.clash_s += STEP_S as f32;
        }
        // The exit: the last moment the outgoing is still clearly there.
        if go >= GONE_GAIN && out_dry && w <= plan.blend_s + 1e-9 {
            gone_at = to;
        }

        // While both decks sound, a hole is the mix below the quieter song's own level (what it would
        // play at alone, levelled) and below the edge line; a bump is above the louder one and above the
        // line. (Lifting two quiet outro/intro parts towards loud edges is the trim's job, not a bump.)
        if let Some((lo, li)) = levels
            && let Some((from, to_lu)) = line
            && w <= plan.blend_s + 1e-9
            && go > 0.0
            && gi > 0.0
        {
            let (own_o, own_i) = (lo.at(to) + gains_db.0, li.at(ti) + gains_db.1);
            let p = |g: f32, lufs: f32| f64::from(g * g) * 10f64.powf(f64::from(lufs) / 10.0);
            let sum = p(go, own_o + loss_o) + p(gi, own_i + loss_i);
            let l = (10.0 * sum.max(1e-12).log10()) as f32;
            let target = from + (to_lu - from) * (w / plan.blend_s).clamp(0.0, 1.0) as f32;
            dip = dip.max((own_o.min(own_i) - l).min(target - l));
            bump = bump.max((l - own_o.max(own_i)).min(l - target));
        }
    }
    if levels.is_some() {
        c.dip_lu = Some(dip);
        c.bump_lu = Some(bump);
    }
    // The outgoing leaves in a sung bar whose line runs on into the next bar (no line or chorus end in
    // it): the line goes with the deck. Only where synced lyrics back the curve (the planner weighs the
    // audio heuristic itself); the same rule as the planner's mid-line penalty, as a hard gate.
    if let Some(last) = out.bar_at(gone_at - 0.05)
        && !out.f.bars.cue.is_empty()
    {
        let sung = |b: usize| out.f.bars.vocal.get(b).is_some_and(|&v| f32::from(v) / 255.0 >= SUNG);
        let ends = out.f.bars.cue.get(last).is_some_and(|&c| c & (cue::LINE_END | cue::CHORUS_END) != 0);
        c.cuts_line = sung(last) && sung(last + 1) && !ends;
    }
    c
}

/// Plans `out` → `inn` and checks the options in the planner's order until one passes (contract with
/// [`crate::plan::plan`]: the same plan when the first choice passes). When none passes, the least bad
/// of them (the plain fade included) is armed.
pub fn plan_verified(out: &Features, inn: &Features, ctx: &PlanCtx, gains_db: (f32, f32)) -> Verified {
    let (so, si) = (Song::new(out), Song::new(inn));
    let mut rejected = Vec::new();
    let mut least: Option<(f32, TransitionPlan, Check)> = None;
    for p in alternatives(out, inn, ctx, MAX_TRIED + 1) {
        let c = check(&p, &so, &si, gains_db);
        let Some(reason) = c.failure() else {
            return Verified { plan: p, check: c, rejected, none_passed: false };
        };
        rejected.push(Rejected {
            kind: p.kind,
            out_mix_start_s: p.out_mix_start_s,
            in_mix_start_s: p.in_mix_start_s,
            reason,
        });
        let bad = c.badness();
        if least.as_ref().is_none_or(|(b, _, _)| bad < *b) {
            least = Some((bad, p, c));
        }
    }
    let (_, mut plan, check) = least.expect("alternatives always returns a plan");
    plan.reason = format!("{} (no option passed the check; least bad)", plan.reason);
    Verified { plan, check, rejected, none_passed: true }
}

/// Short-term loudness (3 s, K-weighted, BS.1770) of mono `samples` every `step_s`, in LUFS: for
/// measuring a rendered preview. Index `k` covers the 3 s ending at `k · step_s`.
pub fn audio_levels(samples: &[f32], sr: u32, step_s: f64) -> Vec<f32> {
    audio_levels_of(&[samples], sr, step_s)
}

/// [`audio_levels`] of several channels (BS.1770 sums the channels' mean squares), e.g. `[left, right]`:
/// on the scale of the analysis' integrated loudness, which a mono downmix is 3–6 dB below depending on
/// the stereo width.
pub fn audio_levels_of(channels: &[&[f32]], sr: u32, step_s: f64) -> Vec<f32> {
    let sr_f = f64::from(sr.max(1));
    let len = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let k: Vec<Vec<f32>> = channels.iter().map(|c| k_weighted(&c[..len], sr_f)).collect();
    let win = (3.0 * sr_f) as usize;
    let hop = ((step_s * sr_f) as usize).max(1);
    // Running sum of squares over the channels.
    let mut cum = Vec::with_capacity(len + 1);
    cum.push(0.0f64);
    for i in 0..len {
        let e: f64 = k.iter().map(|c| f64::from(c[i]) * f64::from(c[i])).sum();
        cum.push(cum[i] + e);
    }
    (0..=len / hop)
        .map(|n| {
            let b = (n * hop).min(len);
            let a = b.saturating_sub(win);
            let ms = if b > a { (cum[b] - cum[a]) / (b - a) as f64 } else { 0.0 };
            (-0.691 + 10.0 * ms.max(1e-12).log10()) as f32
        })
        .collect()
}

/// The two-stage K filter (high shelf + high-pass) of BS.1770 for any sample rate.
fn k_weighted(x: &[f32], sr: f64) -> Vec<f32> {
    use std::f64::consts::PI;
    // Stage 1: high shelf (+4 dB above ~1.7 kHz).
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let kk = (PI * f0 / sr).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + kk / q + kk * kk;
    let shelf = [
        (vh + vb * kk / q + kk * kk) / a0,
        2.0 * (kk * kk - vh) / a0,
        (vh - vb * kk / q + kk * kk) / a0,
        2.0 * (kk * kk - 1.0) / a0,
        (1.0 - kk / q + kk * kk) / a0,
    ];
    // Stage 2: high-pass at ~38 Hz.
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let kk = (PI * f0 / sr).tan();
    let a0 = 1.0 + kk / q + kk * kk;
    let hp = [1.0 / a0, -2.0 / a0, 1.0 / a0, 2.0 * (kk * kk - 1.0) / a0, (1.0 - kk / q + kk * kk) / a0];
    let biquad = |c: [f64; 5], input: &[f64]| {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        input
            .iter()
            .map(|&x0| {
                let y0 = c[0] * x0 + c[1] * x1 + c[2] * x2 - c[3] * y1 - c[4] * y2;
                (x2, x1, y2, y1) = (x1, x0, y1, y0);
                y0
            })
            .collect::<Vec<f64>>()
    };
    let x: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    biquad(hp, &biquad(shelf, &x)).into_iter().map(|v| v as f32).collect()
}

/// Level of a deck that has stopped, for [`audio_dip_bump`] (LUFS).
const SILENT_LUFS: f32 = -70.0;

/// Hole and bump (LU) of a rendered preview, measured like [`Check::dip_lu`]: `mix`, `out_own` and
/// `in_own` are [`audio_levels`] (every `step_s`) of the preview and of each deck's own levelled audio
/// aligned to it (silence where the deck doesn't play); the blend runs from `blend_from_s` for
/// `blend_s` seconds of preview time. The edge line runs from the outgoing's level over the 3 s before
/// the blend to the incoming's over the 3 s ending [`EDGE_REF_S`] after it. Short-term windows are
/// compared at their centres.
pub fn audio_dip_bump(
    mix: &[f32],
    out_own: &[f32],
    in_own: &[f32],
    step_s: f64,
    blend_from_s: f64,
    blend_s: f64,
) -> Option<(f32, f32)> {
    let at = |v: &[f32], t: f64| v.get((t / step_s).round().max(0.0) as usize).copied();
    // The outgoing file may end inside the last window: silent from there.
    let out_at = |t: f64| at(out_own, t).unwrap_or(SILENT_LUFS);
    let from = at(out_own, blend_from_s)?;
    let to = at(in_own, blend_from_s + blend_s + EDGE_REF_S)?;
    let (mut dip, mut bump) = (0.0f32, 0.0f32);
    let n = (blend_s / step_s).ceil() as usize;
    for k in 0..=n {
        let t = blend_from_s + k as f64 * step_s;
        let x = ((t - blend_from_s) / blend_s.max(1e-6)).clamp(0.0, 1.0) as f32;
        // A window ending at `t + 1.5` is centred on `t`.
        let (l, o, i) = (at(mix, t + 1.5)?, out_at(t + 1.5), at(in_own, t + 1.5)?);
        let target = from + (to - from) * x;
        dip = dip.max((o.min(i) - l).min(target - l));
        bump = bump.max((l - o.max(i)).min(l - target));
    }
    Some((dip, bump))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Envelope, EqEvent, plan};
    use crate::testutil::FeatBuilder;
    use listen_core::settings::Mixing;

    fn ctx(s: &Mixing) -> PlanCtx<'_> {
        PlanCtx {
            settings: s,
            out_position_s: 0.0,
            voice_break: None,
            out_genres: &[],
            in_genres: &[],
            no_echo: false,
        }
    }

    /// Sung from the first bar to the last, with synced-lyric cues (no line ends unless set).
    fn sung_through(bpm: f32, bars: usize) -> Features {
        let mut f = FeatBuilder::new(bpm, bars).intro(0).outro(0).build();
        f.bars.cue = vec![0; bars];
        f
    }

    /// A hard cut from `out` at its bar `bar` into the start of `inn`.
    fn cut_at(out: &Features, bar: usize) -> TransitionPlan {
        let t = f64::from(out.grid.as_ref().unwrap().bar_time(bar));
        TransitionPlan {
            kind: Kind::Cut,
            reason: "test".into(),
            score: 0.0,
            out_mix_start_s: t,
            in_mix_start_s: 0.3,
            in_load_at_s: 0.0,
            blend_s: 0.03,
            handover_out_s: t,
            out_stop_s: t + 0.03,
            in_speed: 1.0,
            out_ramp: None,
            in_glide: None,
            in_curve: Vec::new(),
            in_settle: None,
            gain: Envelope { points: vec![(0.0, 1.0, 0.0), (0.03, 0.0, 1.0)] },
            eq: Vec::new(),
            voice_window: None,
            levels: None,
        }
    }

    #[test]
    fn a_clean_first_choice_is_kept() {
        let s = Mixing::default();
        let (a, b) = (FeatBuilder::new(120.0, 64).build(), FeatBuilder::new(122.0, 64).build());
        let v = plan_verified(&a, &b, &ctx(&s), (-5.0, -5.0));
        assert_eq!(v.plan, plan(&a, &b, &ctx(&s)));
        assert!(v.rejected.is_empty() && !v.none_passed, "{:?}", v.rejected);
        assert_eq!(v.check.failure(), None);
        // Instrumental outro into instrumental intro: no vocal clash, levels within the gate.
        assert_eq!(v.check.clash_s, 0.0);
        assert!(v.check.dip_lu.is_some_and(|d| d <= MAX_LEVEL_LU), "{:?}", v.check);
    }

    #[test]
    fn alternatives_start_with_the_plan_and_end_with_the_fade() {
        let s = Mixing::default();
        let (a, b) = (FeatBuilder::new(120.0, 64).build(), FeatBuilder::new(120.0, 64).build());
        let alts = alternatives(&a, &b, &ctx(&s), 5);
        assert_eq!(alts.len(), 5);
        assert_eq!(alts[0], plan(&a, &b, &ctx(&s)));
        assert!(matches!(alts[4].kind, Kind::Fade { .. }));
        // Distinct options.
        for (x, p) in alts.iter().enumerate() {
            for q in &alts[x + 1..] {
                assert_ne!(p, q);
            }
        }
        // Without a grid there is only the fade.
        let n = FeatBuilder::new(120.0, 64).no_grid().build();
        assert_eq!(alternatives(&n, &b, &ctx(&s), 5).len(), 1);
    }

    #[test]
    fn a_filter_that_takes_the_level_away_is_a_dip() {
        let s = Mixing::default();
        let (a, b) = (FeatBuilder::new(120.0, 64).build(), FeatBuilder::new(120.0, 64).build());
        let mut p = alternatives(&a, &b, &ctx(&s), 2).pop().unwrap();
        assert!(matches!(p.kind, Kind::Fade { .. }) && p.levels.is_some());
        let (sa, sb) = (Song::new(&a), Song::new(&b));
        let clean = check(&p, &sa, &sb, (0.0, 0.0));
        assert!(clean.dip_lu.is_some_and(|d| d < 0.5), "{clean:?}");
        // Both decks high-passed at 4 kHz for the whole fade: most of the loudness is gone.
        for deck in [Which::Out, Which::In] {
            p.eq.push(EqEvent {
                deck,
                at_out_s: p.out_mix_start_s,
                target: EqTarget::Highpass { hz: 4000.0, mix: 1.0 },
                ramp_s: 0.0,
            });
        }
        let c = check(&p, &sa, &sb, (0.0, 0.0));
        assert!(c.dip_lu.is_some_and(|d| d > MAX_LEVEL_LU), "{c:?}");
        assert!(c.failure().unwrap().contains("loudness dip"));
    }

    #[test]
    fn a_hole_just_under_the_gate_fails_with_the_curves_error() {
        // Bug 57: the 8-bar BeatBlend ytm:KcZn05qxVgg → ytm:BycLmWI97Nc had a 0.99 LU hole on the curves
        // and 1.57 LU rendered (stereo, decks aligned).
        let hole = |d: f32| Check { dip_lu: Some(d), bump_lu: Some(0.0), ..Check::default() };
        let why = hole(0.99).failure().unwrap();
        assert!(why.contains("loudness dip 1.0 LU (may render 1.6)"), "{why}");
        assert!(hole(0.99).badness() > 0.0);
        assert_eq!(hole(0.85).failure(), None);
        assert_eq!(hole(0.85).badness(), 0.0);
        // Bumps keep the plain gate (no rendered bump over it so far).
        let bump = Check { dip_lu: Some(0.0), bump_lu: Some(1.4), ..Check::default() };
        assert_eq!(bump.failure(), None);
    }

    #[test]
    fn overlapping_singers_clash_and_handovers_have_no_level_check() {
        let s = Mixing::default();
        let (a, b) = (sung_through(120.0, 64), sung_through(120.0, 64));
        let fade = alternatives(&a, &b, &ctx(&s), 2).pop().unwrap();
        let c = check(&fade, &Song::new(&a), &Song::new(&b), (0.0, 0.0));
        assert!(c.clash_s > MAX_CLASH_S, "{c:?}");
        let cut = check(&cut_at(&a, 40), &Song::new(&a), &Song::new(&b), (0.0, 0.0));
        assert_eq!((cut.dip_lu, cut.bump_lu), (None, None));
        assert!(cut.clash_s < 0.2, "{cut:?}");
    }

    #[test]
    fn leaving_mid_line_needs_lyrics_and_no_line_end() {
        let b = sung_through(120.0, 64);
        let mut a = sung_through(120.0, 64);
        let check_cut = |a: &Features| check(&cut_at(a, 40), &Song::new(a), &Song::new(&b), (0.0, 0.0));
        assert!(check_cut(&a).cuts_line);
        // The line ends in the bar before the cut: a clean exit.
        a.bars.cue[39] = cue::LINE_END;
        assert!(!check_cut(&a).cuts_line);
        // Without synced lyrics the heuristic curve is not trusted for this gate.
        a.bars.cue.clear();
        assert!(!check_cut(&a).cuts_line);
    }

    #[test]
    fn when_nothing_passes_the_least_bad_is_armed() {
        // Sung wall to wall with no line ends, too far apart in tempo for a blend: every echo-out and
        // cut leaves mid-line, the fade overlaps the singers.
        let s = Mixing::default();
        let (a, b) = (sung_through(120.0, 64), sung_through(90.0, 48));
        let v = plan_verified(&a, &b, &ctx(&s), (0.0, 0.0));
        assert!(v.none_passed, "{:?} {:?} {:?}", v.plan, v.check, v.rejected);
        assert!(v.plan.reason.contains("least bad"));
        assert!(v.rejected.len() >= 2, "{:?}", v.rejected);
        let worst = v.rejected.iter().map(|r| r.reason.clone()).collect::<Vec<_>>().join(" | ");
        assert!(worst.contains("cuts a sung line") || worst.contains("vocals clash"), "{worst}");
    }

    #[test]
    fn audio_levels_measure_a_dip() {
        // 1 kHz tone at −20 dBFS, with 4 s at −32 dBFS in the middle.
        let sr = 22050u32;
        let tone = |amp: f32, secs: f32, from: usize| -> Vec<f32> {
            (0..(secs * sr as f32) as usize)
                .map(|n| amp * (2.0 * std::f32::consts::PI * 1000.0 * (from + n) as f32 / sr as f32).sin())
                .collect()
        };
        let (loud, quiet) = (0.1 * std::f32::consts::SQRT_2, 0.025 * std::f32::consts::SQRT_2);
        let mut x = tone(loud, 10.0, 0);
        x.extend(tone(quiet, 4.0, x.len()));
        x.extend(tone(loud, 10.0, x.len()));
        let lv = audio_levels(&x, sr, 0.1);
        // Steady part: −20 dBFS RMS ≈ −20.7 LUFS + K-weighting gain at 1 kHz (~+0.7 dB).
        assert!((lv[80] + 20.0).abs() < 0.5, "{}", lv[80]);
        // As a "mix" of two decks that each stay loud on their own: the quiet middle is a hole.
        let steady = audio_levels(&tone(loud, 24.0, 0), sr, 0.1);
        let (dip, bump) = audio_dip_bump(&lv, &steady, &steady, 0.1, 8.0, 8.0).unwrap();
        assert!(dip > 10.0, "{dip}");
        assert!(bump < 0.5, "{bump}");
        // Where one deck is that quiet on its own, the mix following it is no hole.
        let (dip, _) = audio_dip_bump(&lv, &lv, &steady, 0.1, 8.0, 8.0).unwrap();
        assert!(dip < 0.5, "{dip}");
        // An outgoing file that ends before the last window counts as silent there (it used to give None).
        let short = &steady[..160];
        assert!(audio_dip_bump(&lv, short, &steady, 0.1, 8.0, 8.0).is_some());
        // Stereo sums the channels: the same signal on both is 3 dB above its mono level, and above a
        // mono downmix of the two as well.
        let x = tone(loud, 24.0, 0);
        let (mono, stereo) = (audio_levels(&x, sr, 0.1), audio_levels_of(&[&x, &x], sr, 0.1));
        assert!((stereo[150] - mono[150] - 3.01).abs() < 0.01, "{} vs {}", stereo[150], mono[150]);
    }
}
