//! Bridge songs: when the next song is too far from the current one to blend well (a tempo gap, clashing
//! keys, a very different sound), a DJ plays a few bars of a third record that sits between the two and
//! mixes out of it again. That record is the "bridge"; here it is an already analysed song, and only a
//! short instrumental stretch of it plays ([`BRIDGE_BARS`] bars: blend in, the groove alone for a few bars,
//! blend out).
//!
//! - [`gap`] says whether a pair is far enough apart to want a bridge.
//! - [`choose`] picks the bridge song and the stretch ([`BridgeCut`]) so that both legs, current → bridge
//!   and bridge → next, are beat-matched blends that cost less than the direct join.
//! - [`clip`] turns a song's analysis into the view of its stretch the planner works on; [`plan_into`] and
//!   [`plan_out_of`] plan the two legs.

use crate::plan::{Kind, Opts, PlanCtx, Side, TransitionPlan, plan, plan_with};
use crate::score::{FlowCtx, TimbreStats, pair_cost};
use listen_core::analysis::{Features, FlowFeatures, SectionKind};
pub use listen_core::queue::BridgeCut;
use listen_core::settings::Mixing;

/// Length of the stretch that plays: up to 8 bars blending in, the bridge alone, up to 8 bars blending out.
pub const BRIDGE_BARS: usize = 24;
/// The incoming bridge glides back to its own tempo over this many bars (short: it is only heard briefly).
const INTO: Opts = Opts { max_bars: 8, settle_bars: 4.0 };
/// Out of the bridge: short blends so they fit in the stretch; the next song settles as usual.
const OUT_OF: Opts = Opts { max_bars: 8, settle_bars: 16.0 };
/// Where the bridge deck is when the second leg gets planned: after the 8-bar blend and the 4-bar settle.
const OUT_PLANNED_AT_BAR: f64 = 12.0;
/// A pair with a flow cost at least this high counts as far apart (≈ 1 is a typical, acceptable pair).
const FAR_COST: f32 = 1.5;
/// Tempo gaps above this many percent want a bridge even when they can still be beat-matched.
const FAR_TEMPO_PCT: f64 = 4.0;
/// Each leg must be at least this much cheaper than the direct join.
const MIN_GAIN: f32 = 0.25;
/// Vocal level (0..1) the blend zones of a stretch must stay under (the planner's "instrumental" bar).
const EDGE_VOCAL: f32 = 0.4;

/// Why `out` → `inn` would sound better with a bridge, or `None` when they join well enough directly.
/// `direct` is the plan for the direct join.
pub fn gap(out: &Features, inn: &Features, direct: &TransitionPlan, settings: &Mixing) -> Option<String> {
    // A bridge is beat-mixed on both sides: it can't help a song without a steady beat.
    let (o, i) = (Side::new(out)?, Side::new(inn)?);
    let (a, c) = (FlowFeatures::of(out), FlowFeatures::of(inn));
    match direct.kind {
        Kind::Fade { .. } | Kind::Gapless => {
            return Some(format!("no beat-matched join ({})", direct.reason));
        }
        // Vocals that can't be kept apart are a matter of the two songs' edges, not of distance: a bridge
        // doesn't change them.
        _ => {}
    }
    let (_, lr) = crate::score::octave_match(o.bpm() as f32, i.bpm() as f32);
    let pct = f64::from(lr.abs()).exp_m1() * 100.0;
    if pct > FAR_TEMPO_PCT.min(f64::from(settings.max_tempo_pct)) {
        return Some(format!("{pct:.1} % tempo gap ({:.0} → {:.0} bpm)", o.bpm(), i.bpm()));
    }
    if let (Some((ka, ca)), Some((kc, cc))) = (a.key, c.key)
        && ca.min(cc) >= 0.5
        && ka.distance(kc) >= 0.35
    {
        return Some(format!("keys clash ({} → {})", ka.label(), kc.label()));
    }
    let ctx =
        FlowCtx { timbre: TimbreStats::from_vectors([&a.timbre[..], &c.timbre[..]]), ..FlowCtx::default() };
    let cost = pair_cost(&a, &c, &ctx);
    (cost >= FAR_COST).then(|| format!("far apart in sound (flow cost {cost:.2})"))
}

/// The chosen bridge: `index` into the candidates, the stretch that plays and a line for the log.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub index: usize,
    pub cut: BridgeCut,
    pub reason: String,
}

/// Picks the bridge for `out` → `inn` from analysed `candidates`: the one whose legs both flow better than
/// the direct join and both plan as beat-matched blends, with the best instrumental stretch. `None` if
/// nothing fits. `ctx.out_position_s` is where `out` plays now.
pub fn choose(out: &Features, inn: &Features, candidates: &[&Features], ctx: &PlanCtx) -> Option<Choice> {
    let (a, c) = (FlowFeatures::of(out), FlowFeatures::of(inn));
    let flows: Vec<Option<FlowFeatures>> =
        candidates.iter().map(|f| Side::new(f).map(|_| FlowFeatures::of(f))).collect();
    let timbre = TimbreStats::from_vectors(
        [&a.timbre[..], &c.timbre[..]].into_iter().chain(flows.iter().flatten().map(|f| &f.timbre[..])),
    );
    let fctx = FlowCtx { timbre, ..FlowCtx::default() };
    let direct = pair_cost(&a, &c, &fctx);
    let mut best: Option<(f32, Choice)> = None;
    for (index, (f, flow)) in candidates.iter().zip(&flows).enumerate() {
        let Some(b) = flow else { continue };
        // Each leg must be beat-matchable at all.
        if !tempo_fits(a.bpm, b.bpm, ctx.settings) || !tempo_fits(b.bpm, c.bpm, ctx.settings) {
            continue;
        }
        let (ab, bc) = (pair_cost(&a, b, &fctx), pair_cost(b, &c, &fctx));
        if direct - ab.max(bc) < MIN_GAIN {
            continue;
        }
        let Some((cut, cut_score)) = find_cut(f) else { continue };
        let piece = clip(f, &cut);
        let leg_in = plan_into(out, &piece, ctx);
        if !beat_matched(&leg_in) {
            continue;
        }
        let Some(side) = Side::new(&piece) else { continue };
        let later = PlanCtx {
            out_position_s: side.t(f64::from(cut.start_bar) + OUT_PLANNED_AT_BAR),
            voice_break: None,
            out_genres: &[],
            ..ctx.clone()
        };
        let leg_out = plan_out_of(&piece, inn, &later);
        if !beat_matched(&leg_out) {
            continue;
        }
        // Lower is better: the worse leg counts most, a clean stretch and good blends help.
        let cost = ab.max(bc) + 0.25 * (ab + bc) - 0.3 * cut_score - 0.05 * (leg_in.score + leg_out.score)
            + short_leg(&leg_in)
            + short_leg(&leg_out);
        if best.as_ref().is_none_or(|(c0, _)| cost < *c0) {
            let reason = format!(
                "flow {direct:.2} direct, {ab:.2} + {bc:.2} through the bridge; bars {}–{} ({:.0} bpm); in: {}; out: {}",
                cut.start_bar,
                cut.start_bar + cut.bars,
                side.bpm(),
                leg_in.reason,
                leg_out.reason
            );
            best = Some((cost, Choice { index, cut, reason }));
        }
    }
    best.map(|(_, c)| c)
}

/// A leg worth a bridge: beat-matched, and a short blend only where the vocals don't touch.
fn beat_matched(p: &TransitionPlan) -> bool {
    match p.kind {
        Kind::BeatBlend { .. } => true,
        Kind::ShortBlend { .. } => !p.reason.contains("vocals touch"),
        _ => false,
    }
}

/// Extra cost of a leg that is not a long blend (a one- or two-bar cut most).
fn short_leg(p: &TransitionPlan) -> f32 {
    match p.kind {
        Kind::BeatBlend { .. } => 0.0,
        Kind::ShortBlend { bars } if bars >= 4 => 0.15,
        _ => 0.35,
    }
}

/// Both tempos known and within the largest tempo change (at the best octave).
fn tempo_fits(x: Option<f32>, y: Option<f32>, s: &Mixing) -> bool {
    let (Some(x), Some(y)) = (x, y) else { return false };
    let (_, lr) = crate::score::octave_match(x, y);
    f64::from(lr.abs()) <= (1.0 + f64::from(s.max_tempo_pct.max(0.0)) / 100.0).ln()
}

/// The best [`BRIDGE_BARS`]-bar stretch of a song to play as a bridge, with its score (higher is better):
/// it starts on a phrase, has no lead vocal where it blends, preferably little vocal overall, and a steady groove
/// (bass and energy present, not a near-silent breakdown).
pub fn find_cut(f: &Features) -> Option<(BridgeCut, f32)> {
    let s = Side::new(f)?;
    // Without a vocal curve the planner only knows the intro and outro; don't guess for the middle.
    if f.bars.vocal.is_empty() || s.bars < BRIDGE_BARS {
        return None;
    }
    let curve = |c: &[u8], b: usize| c.get(b).map_or(0.5, |&v| f32::from(v) / 255.0);
    let mean =
        |from: usize, n: usize, get: &dyn Fn(usize) -> f32| (from..from + n).map(get).sum::<f32>() / n as f32;
    let (start, end) = (f64::from(f.audio_start), f64::from(f.audio_end));
    let mut best: Option<(usize, f32)> = None;
    for s0 in 0..=s.bars - BRIDGE_BARS {
        if !s.on_phrase(s0) || s.t(s0 as f64) < start || s.t((s0 + BRIDGE_BARS) as f64) > end - 0.5 {
            continue;
        }
        let vocal = |b: usize| s.vocal(b);
        // It is mixed in over its first bars (short blends) and out over its last 8; in between a vocal
        // hook may play alone.
        let edge_in = mean(s0, 4, &vocal);
        let edge_out = mean(s0 + BRIDGE_BARS - 8, 8, &vocal);
        let all = mean(s0, BRIDGE_BARS, &vocal);
        if edge_in >= EDGE_VOCAL || edge_out >= EDGE_VOCAL {
            continue;
        }
        let energy = |b: usize| s.energy(b);
        let e = mean(s0, BRIDGE_BARS, &energy);
        let spread = (mean(s0, BRIDGE_BARS, &|b| (s.energy(b) - e).powi(2))).sqrt();
        let low = mean(s0, BRIDGE_BARS, &|b| curve(&f.bars.low, b));
        let score = (1.0 - all) + 0.25 * (2.0 - edge_in - edge_out) + 0.4 * low + 0.3 * e - 0.5 * spread;
        if best.is_none_or(|(_, b)| score > b) {
            best = Some((s0, score));
        }
    }
    let (s0, score) = best?;
    let cut = BridgeCut {
        start_s: s.t(s0 as f64),
        end_s: s.t((s0 + BRIDGE_BARS) as f64),
        start_bar: s0 as u32,
        bars: BRIDGE_BARS as u32,
    };
    Some((cut, score))
}

/// The analysis as the planner should see a bridge: the song starts at the stretch and ends with it, with
/// no intro or outro of its own (so it is mixed in at the stretch's first bar and out before its end).
pub fn clip(f: &Features, cut: &BridgeCut) -> Features {
    let mut c = f.clone();
    c.audio_start = (cut.start_s - 0.01).max(0.0) as f32;
    c.audio_end = (cut.end_s + 0.05) as f32;
    c.intro_instrumental_bars = 0;
    c.outro_instrumental_bars = 0;
    c.sections.retain(|s| s.kind != SectionKind::Outro && s.kind != SectionKind::Intro);
    c
}

/// Plans the first leg: the current song into the bridge's stretch (`bridge` from [`clip`]).
pub fn plan_into(out: &Features, bridge: &Features, ctx: &PlanCtx) -> TransitionPlan {
    let ctx = PlanCtx { voice_break: None, in_genres: &[], ..ctx.clone() };
    plan_with(out, bridge, &ctx, INTO)
}

/// Plans the second leg: out of the bridge's stretch (`bridge` from [`clip`]) into the next song.
pub fn plan_out_of(bridge: &Features, inn: &Features, ctx: &PlanCtx) -> TransitionPlan {
    let ctx = PlanCtx { voice_break: None, ..ctx.clone() };
    plan_with(bridge, inn, &ctx, OUT_OF)
}

/// The direct plan, for [`gap`] (same as [`plan`]).
pub fn plan_direct(out: &Features, inn: &Features, ctx: &PlanCtx) -> TransitionPlan {
    plan(out, inn, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::FeatBuilder;
    use listen_core::analysis::Camelot;

    fn ctx(s: &Mixing, now: f64) -> PlanCtx<'_> {
        PlanCtx { settings: s, out_position_s: now, voice_break: None, out_genres: &[], in_genres: &[] }
    }

    /// A song whose bars 32..64 are an instrumental groove (vocals elsewhere in the body).
    fn with_groove(bpm: f32, key: u8) -> Features {
        let mut f = FeatBuilder::new(bpm, 96).intro(8).outro(16).build();
        for b in 32..64 {
            f.bars.vocal[b] = 0;
            f.bars.energy[b] = 190;
            f.bars.low[b] = 220;
        }
        f.key = Some((Camelot { number: key, minor: true }, 0.9));
        f
    }

    #[test]
    fn a_close_pair_needs_no_bridge() {
        let s = Mixing::default();
        let a = FeatBuilder::new(124.0, 96).intro(8).outro(16).build();
        let c = FeatBuilder::new(125.0, 96).intro(16).outro(8).build();
        let direct = plan(&a, &c, &ctx(&s, 10.0));
        assert_eq!(gap(&a, &c, &direct, &s), None, "{}", direct.reason);
    }

    #[test]
    fn a_tempo_gap_and_a_key_clash_want_a_bridge() {
        let s = Mixing::default();
        let a = FeatBuilder::new(118.0, 96).intro(8).outro(16).build();
        let c = FeatBuilder::new(132.0, 96).intro(16).outro(8).build();
        let direct = plan(&a, &c, &ctx(&s, 10.0));
        let why = gap(&a, &c, &direct, &s).expect("a 12 % gap is far");
        assert!(why.contains("beat-matched") || why.contains("tempo"), "{why}");

        let mut c = FeatBuilder::new(124.0, 96).intro(16).outro(8).build();
        c.key = Some((Camelot { number: 2, minor: false }, 0.9));
        let direct = plan(&a, &c, &ctx(&s, 10.0));
        let a = FeatBuilder::new(124.0, 96).intro(8).outro(16).build();
        assert!(gap(&a, &c, &direct, &s).is_some_and(|w| w.contains("keys")));
    }

    #[test]
    fn the_cut_is_the_instrumental_groove_on_a_phrase() {
        let f = with_groove(126.0, 8);
        let (cut, _) = find_cut(&f).expect("a stretch");
        assert_eq!(cut.bars as usize, BRIDGE_BARS);
        assert!(cut.start_bar >= 32 && cut.start_bar as usize + BRIDGE_BARS <= 64, "{cut:?}");
        assert_eq!(cut.start_bar % 8, 0);
        let g = f.grid.as_ref().unwrap();
        assert!((f64::from(g.bar_time(cut.start_bar as usize)) - cut.start_s).abs() < 1e-3);
    }

    #[test]
    fn a_song_sung_throughout_has_no_cut() {
        let mut f = FeatBuilder::new(126.0, 96).intro(0).outro(0).build();
        f.bars.vocal.iter_mut().for_each(|v| *v = 255);
        assert_eq!(find_cut(&f), None);
    }

    #[test]
    fn the_clip_is_mixed_in_at_its_start_and_out_before_its_end() {
        let s = Mixing::default();
        let a = FeatBuilder::new(124.0, 96).intro(8).outro(16).build();
        let b = with_groove(127.0, 8);
        let c = FeatBuilder::new(130.0, 96).intro(16).outro(8).build();
        let (cut, _) = find_cut(&b).unwrap();
        let piece = clip(&b, &cut);
        let leg_in = plan_into(&a, &piece, &ctx(&s, 10.0));
        assert!(beat_matched(&leg_in), "{}", leg_in.reason);
        assert!(
            (leg_in.in_mix_start_s - cut.start_s).abs() < 1e-3,
            "{} vs {}",
            leg_in.in_mix_start_s,
            cut.start_s
        );
        assert!(leg_in.blend_s <= 8.0 * 4.0 * 60.0 / 124.0 / 0.95, "short blends only: {}", leg_in.reason);
        let side = Side::new(&piece).unwrap();
        let now = side.t(f64::from(cut.start_bar) + OUT_PLANNED_AT_BAR);
        let leg_out = plan_out_of(&piece, &c, &ctx(&s, now));
        assert!(beat_matched(&leg_out), "{}", leg_out.reason);
        assert!(leg_out.out_stop_s <= cut.end_s + 1e-3, "{} > {}", leg_out.out_stop_s, cut.end_s);
        assert!(leg_out.out_mix_start_s >= now + crate::plan::MIN_LEAD_S - 1e-6);
    }

    #[test]
    fn choose_takes_the_song_in_between() {
        let s = Mixing::default();
        let a = FeatBuilder::new(120.0, 96).intro(8).outro(16).build();
        let c = FeatBuilder::new(133.0, 96).intro(16).outro(8).build();
        let between = with_groove(126.5, 8);
        let too_fast = with_groove(140.0, 8);
        let sung = {
            let mut f = FeatBuilder::new(126.0, 96).intro(0).outro(0).build();
            f.bars.vocal.iter_mut().for_each(|v| *v = 255);
            f
        };
        let cands = [&too_fast, &sung, &between];
        let choice = choose(&a, &c, &cands, &ctx(&s, 10.0)).expect("a bridge");
        assert_eq!(choice.index, 2, "{}", choice.reason);
        assert!(choice.cut.start_bar >= 32);
    }

    #[test]
    fn nothing_fits_no_bridge() {
        let s = Mixing::default();
        let a = FeatBuilder::new(120.0, 96).intro(8).outro(16).build();
        let c = FeatBuilder::new(133.0, 96).intro(16).outro(8).build();
        let too_fast = with_groove(150.0, 8);
        assert_eq!(choose(&a, &c, &[&too_fast], &ctx(&s, 10.0)), None);
    }
}
