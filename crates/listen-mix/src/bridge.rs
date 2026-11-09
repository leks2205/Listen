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
/// The stretch at slow tempos, where 24 bars would last too long ([`MAX_BRIDGE_S`]): up to 4 bars blending
/// in, up to 8 out.
pub const SHORT_BRIDGE_BARS: usize = 16;
/// A bridge is a short DJ move, never a song of its own: the whole stretch (both blends included) lasts at
/// most this long (24 bars down to 96 bpm, 16 bars down to 64 bpm).
pub const MAX_BRIDGE_S: f64 = 60.0;
/// …and the bridge plays alone (between the end of the blend in and the start of the blend out) at most
/// this long.
pub const MAX_ALONE_S: f64 = 30.0;
/// The incoming bridge glides back to its own tempo over this many bars (short: it is only heard briefly).
const INTO: Opts = Opts { max_bars: 8, settle_bars: 4.0 };
/// Into a [`SHORT_BRIDGE_BARS`] stretch: half as long, so the way out has its 8 bars.
const INTO_SHORT: Opts = Opts { max_bars: 4, settle_bars: 2.0 };
/// Out of the bridge: short blends so they fit in the stretch; the next song settles as usual.
const OUT_OF: Opts = Opts { max_bars: 8, settle_bars: 16.0 };
/// Where the bridge deck is when the second leg gets planned: after the 8-bar blend and the 4-bar settle.
const OUT_PLANNED_AT_BAR: f64 = 12.0;
/// The same for a [`SHORT_BRIDGE_BARS`] stretch: after the 4-bar blend and the 2-bar settle.
const OUT_PLANNED_AT_BAR_SHORT: f64 = 6.0;
/// A pair with a flow cost at least this high counts as far apart (≈ 1 is a typical, acceptable pair).
const FAR_COST: f32 = 1.5;
/// Tempo gaps above this many percent want a bridge even when they can still be beat-matched.
const FAR_TEMPO_PCT: f64 = 4.0;
/// Each leg must be at least this much cheaper (flow cost) than a direct join that can be beat-matched.
const MIN_GAIN: f32 = 0.25;
/// Each leg must be at least this much cheaper than a direct join that can't be beat-matched (an echo-out
/// or a cut otherwise): a bridge is a third song in between, so it has to flow clearly better than the
/// pair, not just as well. Measured with `listen --check-bridges` (150 history pairs, real store, after the
/// scene gate): with no margin 10 pairs were bridged, 4 of them with legs only 0.02–0.09 better than the
/// direct join (a typical acceptable pair costs ≈ 1, so that is noise); the next one gained 0.35. Any
/// margin from 0.1 to 0.35 keeps the same 6 bridges; 0.25 is the margin a beat-matched pair needs too.
pub const MIN_GAIN_UNMATCHED: f32 = 0.25;
/// Weight of the scene score (0..1, `listen_core::scene::Gate::judge`) in the ranking: among bridges that
/// fit the scene, the closest one wins unless another flows clearly better.
const SCENE_WEIGHT: f32 = 0.3;
/// Vocal level (0..1) the blend zones of a stretch must stay under (the planner's "instrumental" bar).
const EDGE_VOCAL: f32 = 0.4;

/// Why `out` → `inn` would sound better with a bridge, or `None` when they join well enough directly.
/// `direct` is the plan for the direct join.
pub fn gap(out: &Features, inn: &Features, direct: &TransitionPlan, settings: &Mixing) -> Option<String> {
    // A bridge is beat-mixed on both sides: it can't help a song without a steady beat.
    let (o, i) = (Side::new(out)?, Side::new(inn)?);
    let (a, c) = (FlowFeatures::of(out), FlowFeatures::of(inn));
    match direct.kind {
        // Echo-outs and cuts too: a bridge is preferred to them, so every pair that would get one tries it.
        Kind::Fade { .. } | Kind::Gapless | Kind::EchoOut { .. } | Kind::Cut => {
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
    pub legs: Legs,
}

/// How a chosen bridge mixes: the flow costs of the direct join and of both legs, and how long it plays.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Legs {
    pub direct: f32,
    /// Current song → bridge, bridge → next song.
    pub cost_in: f32,
    pub cost_out: f32,
    /// The kinds of the two legs ("BeatBlend", "ShortBlend").
    pub kind_in: String,
    pub kind_out: String,
    /// The direct join is a beat-matched blend.
    pub direct_matched: bool,
    /// The stretch's length and how much of it plays alone (seconds).
    pub cut_s: f64,
    pub alone_s: f64,
}

/// Whether a far pair tries a bridge every time, whatever the "how often" setting: when the direct join
/// can't be beat-matched (an echo-out, cut or fade otherwise), a bridge is the preferred way; the setting
/// only thins out bridges between pairs that would blend anyway.
pub fn always_try(direct: &TransitionPlan) -> bool {
    !beat_matched(direct)
}

/// Picks the bridge for `out` → `inn` from analysed `candidates`: the one whose legs both flow better than
/// the direct join and both plan as beat-matched blends, with the best instrumental stretch. `None` if
/// nothing fits. `ctx.out_position_s` is where `out` plays now. `direct` is the plan for the direct join:
/// when it isn't beat-matched, legs that flow no worse than the direct pair are enough (a bridge beats an
/// echo-out or a cut); otherwise each leg must be [`MIN_GAIN`] better.
pub fn choose(
    out: &Features,
    inn: &Features,
    direct_plan: &TransitionPlan,
    candidates: &[&Features],
    ctx: &PlanCtx,
) -> Option<Choice> {
    choose_scored(out, inn, direct_plan, candidates, &[], ctx)
}

/// [`choose`] with a scene score per candidate (`scene[i]`, 0..1, from the scene gate; empty = none): the
/// candidates are those that fit the scene, and a closer scene lowers a candidate's cost.
pub fn choose_scored(
    out: &Features,
    inn: &Features,
    direct_plan: &TransitionPlan,
    candidates: &[&Features],
    scene: &[f32],
    ctx: &PlanCtx,
) -> Option<Choice> {
    let min_gain = if always_try(direct_plan) { MIN_GAIN_UNMATCHED } else { MIN_GAIN };
    choose_with_gain(out, inn, direct_plan, candidates, scene, ctx, min_gain)
}

/// [`choose_scored`] with the margin each leg must beat the direct join by (`listen --check-bridges`
/// measures it).
pub fn choose_with_gain(
    out: &Features,
    inn: &Features,
    direct_plan: &TransitionPlan,
    candidates: &[&Features],
    scene: &[f32],
    ctx: &PlanCtx,
    min_gain: f32,
) -> Option<Choice> {
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
        if direct - ab.max(bc) < min_gain {
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
            out_position_s: side.t(f64::from(cut.start_bar) + out_planned_at(cut.bars as usize)),
            voice_break: None,
            out_genres: &[],
            ..ctx.clone()
        };
        let leg_out = plan_out_of(&piece, inn, &later);
        if !beat_matched(&leg_out) {
            continue;
        }
        // Never a song of its own: short alone between the two blends.
        let cut_s = cut.end_s - cut.start_s;
        let alone_s = (cut_s - leg_in.blend_s - leg_out.blend_s).max(0.0);
        if cut_s > MAX_BRIDGE_S + 1e-6 || alone_s > MAX_ALONE_S {
            continue;
        }
        // Lower is better: the worse leg counts most, a clean stretch and good blends help.
        let cost = ab.max(bc) + 0.25 * (ab + bc) - 0.3 * cut_score - 0.05 * (leg_in.score + leg_out.score)
            + short_leg(&leg_in)
            + short_leg(&leg_out)
            - SCENE_WEIGHT * scene.get(index).copied().unwrap_or(0.0);
        if best.as_ref().is_none_or(|(c0, _)| cost < *c0) {
            let reason = format!(
                "flow {direct:.2} direct, {ab:.2} + {bc:.2} through the bridge; bars {}–{} ({:.0} bpm); in: {}; out: {}",
                cut.start_bar,
                cut.start_bar + cut.bars,
                side.bpm(),
                leg_in.reason,
                leg_out.reason
            );
            let kind = |p: &TransitionPlan| {
                format!("{:?}", p.kind).split([' ', '{', '(']).next().unwrap_or("").to_string()
            };
            let legs = Legs {
                direct,
                cost_in: ab,
                cost_out: bc,
                kind_in: kind(&leg_in),
                kind_out: kind(&leg_out),
                direct_matched: beat_matched(direct_plan),
                cut_s,
                alone_s,
            };
            best = Some((cost, Choice { index, cut, reason, legs }));
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

/// How many bars a bridge stretch of a song at `bpm` has: [`BRIDGE_BARS`], or [`SHORT_BRIDGE_BARS`] where
/// that would last longer than [`MAX_BRIDGE_S`]; `None` when even that is too long (below 64 bpm).
pub fn bridge_bars(bpm: f64) -> Option<usize> {
    let bar_s = 240.0 / bpm.max(1.0);
    [BRIDGE_BARS, SHORT_BRIDGE_BARS].into_iter().find(|&n| n as f64 * bar_s <= MAX_BRIDGE_S)
}

/// Where the second leg gets planned in a stretch of `bars` (bars after its start).
fn out_planned_at(bars: usize) -> f64 {
    if bars < BRIDGE_BARS { OUT_PLANNED_AT_BAR_SHORT } else { OUT_PLANNED_AT_BAR }
}

/// How many bars the stretch of a clipped bridge (from [`clip`]) has.
fn clip_bars(bridge: &Features) -> usize {
    let Some(s) = Side::new(bridge) else { return BRIDGE_BARS };
    let bar_s = 240.0 / s.bpm().max(1.0);
    ((f64::from(bridge.audio_end) - f64::from(bridge.audio_start)) / bar_s).round() as usize
}

/// The best stretch of a song to play as a bridge ([`bridge_bars`] long), with its score (higher is
/// better): it starts on a phrase, has no lead vocal where it blends, preferably little vocal overall, and a
/// steady groove (bass and energy present, not a near-silent breakdown).
pub fn find_cut(f: &Features) -> Option<(BridgeCut, f32)> {
    let s = Side::new(f)?;
    let bars = bridge_bars(s.bpm())?;
    // Without a vocal curve the planner only knows the intro and outro; don't guess for the middle.
    if f.bars.vocal.is_empty() || s.bars < bars {
        return None;
    }
    let curve = |c: &[u8], b: usize| c.get(b).map_or(0.5, |&v| f32::from(v) / 255.0);
    let mean =
        |from: usize, n: usize, get: &dyn Fn(usize) -> f32| (from..from + n).map(get).sum::<f32>() / n as f32;
    let (start, end) = (f64::from(f.audio_start), f64::from(f.audio_end));
    let mut best: Option<(usize, f32)> = None;
    for s0 in 0..=s.bars - bars {
        if !s.on_phrase(s0) || s.t(s0 as f64) < start || s.t((s0 + bars) as f64) > end - 0.5 {
            continue;
        }
        let vocal = |b: usize| s.vocal(b);
        // It is mixed in over its first bars (short blends) and out over its last 8; in between a vocal
        // hook may play alone.
        let edge_in = mean(s0, 4, &vocal);
        let edge_out = mean(s0 + bars - 8, 8, &vocal);
        let all = mean(s0, bars, &vocal);
        if edge_in >= EDGE_VOCAL || edge_out >= EDGE_VOCAL {
            continue;
        }
        let energy = |b: usize| s.energy(b);
        let e = mean(s0, bars, &energy);
        let spread = (mean(s0, bars, &|b| (s.energy(b) - e).powi(2))).sqrt();
        let low = mean(s0, bars, &|b| curve(&f.bars.low, b));
        let score = (1.0 - all) + 0.25 * (2.0 - edge_in - edge_out) + 0.4 * low + 0.3 * e - 0.5 * spread;
        if best.is_none_or(|(_, b)| score > b) {
            best = Some((s0, score));
        }
    }
    let (s0, score) = best?;
    let cut = BridgeCut {
        start_s: s.t(s0 as f64),
        end_s: s.t((s0 + bars) as f64),
        start_bar: s0 as u32,
        bars: bars as u32,
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
    plan_with(out, bridge, &ctx, if clip_bars(bridge) < BRIDGE_BARS { INTO_SHORT } else { INTO })
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
        PlanCtx {
            settings: s,
            out_position_s: now,
            voice_break: None,
            out_genres: &[],
            in_genres: &[],
            no_echo: false,
        }
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
    fn a_bridge_is_short_at_any_tempo() {
        // 24 bars down to 96 bpm (60 s), 16 bars below, none below 64 bpm.
        assert_eq!(bridge_bars(128.0), Some(24));
        assert_eq!(bridge_bars(96.0), Some(24));
        assert_eq!(bridge_bars(90.0), Some(16));
        assert_eq!(bridge_bars(70.0), Some(16));
        assert_eq!(bridge_bars(60.0), None);
        // A 72 bpm groove: a 16-bar stretch, under a minute, and both legs fit in it.
        let s = Mixing::default();
        let mut b = FeatBuilder::new(72.0, 96).intro(8).outro(16).build();
        for bar in 32..64 {
            b.bars.vocal[bar] = 0;
            b.bars.energy[bar] = 190;
            b.bars.low[bar] = 220;
        }
        let (cut, _) = find_cut(&b).expect("a stretch");
        assert_eq!(cut.bars as usize, SHORT_BRIDGE_BARS);
        assert!(cut.end_s - cut.start_s <= MAX_BRIDGE_S, "{cut:?}");
        let a = FeatBuilder::new(70.0, 96).intro(8).outro(16).build();
        let c = FeatBuilder::new(76.0, 96).intro(16).outro(8).build();
        let piece = clip(&b, &cut);
        let leg_in = plan_into(&a, &piece, &ctx(&s, 10.0));
        assert!(beat_matched(&leg_in), "{}", leg_in.reason);
        assert!(leg_in.blend_s <= 4.0 * 4.0 * 60.0 / 70.0 / 0.95, "a 4-bar blend in: {}", leg_in.reason);
        let side = Side::new(&piece).unwrap();
        let now = side.t(f64::from(cut.start_bar) + out_planned_at(cut.bars as usize));
        let leg_out = plan_out_of(&piece, &c, &ctx(&s, now));
        assert!(beat_matched(&leg_out), "{}", leg_out.reason);
        assert!(leg_out.out_stop_s <= cut.end_s + 1e-3);
        // Chosen between a 70 and a 76 bpm song: under a minute, at most 30 s alone.
        let direct = plan(&a, &c, &ctx(&s, 10.0));
        if let Some(choice) = choose_with_gain(&a, &c, &direct, &[&b], &[], &ctx(&s, 10.0), 0.0) {
            assert!(
                choice.legs.cut_s <= MAX_BRIDGE_S && choice.legs.alone_s <= MAX_ALONE_S,
                "{:?}",
                choice.legs
            );
        }
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
        let direct = plan(&a, &c, &ctx(&s, 10.0));
        let choice = choose(&a, &c, &direct, &cands, &ctx(&s, 10.0)).expect("a bridge");
        assert_eq!(choice.index, 2, "{}", choice.reason);
        assert!(choice.cut.start_bar >= 32);
    }

    #[test]
    fn nothing_fits_no_bridge() {
        let s = Mixing::default();
        let a = FeatBuilder::new(120.0, 96).intro(8).outro(16).build();
        let c = FeatBuilder::new(133.0, 96).intro(16).outro(8).build();
        let too_fast = with_groove(150.0, 8);
        let direct = plan(&a, &c, &ctx(&s, 10.0));
        assert_eq!(choose(&a, &c, &direct, &[&too_fast], &ctx(&s, 10.0)), None);
    }
    #[test]
    fn a_pair_that_would_echo_out_tries_a_bridge_first() {
        // Close in tempo and key, but sung up to the end and from the start: the direct join is an echo-out
        // or cut. That counts as far apart (whatever the chance setting).
        let s = Mixing::default();
        let mut a = FeatBuilder::new(124.0, 96).intro(8).outro(0).build();
        let mut c = FeatBuilder::new(125.0, 96).intro(0).outro(8).build();
        a.bars.vocal.iter_mut().skip(8).for_each(|v| *v = 255);
        c.bars.vocal.iter_mut().take(88).for_each(|v| *v = 255);
        let direct = plan(&a, &c, &ctx(&s, 10.0));
        assert!(
            matches!(direct.kind, Kind::EchoOut { .. } | Kind::Cut),
            "{:?}: {}",
            direct.kind,
            direct.reason
        );
        assert!(always_try(&direct));
        let why = gap(&a, &c, &direct, &s).expect("an echo-out pair tries a bridge");
        assert!(why.contains("beat-matched"), "{why}");
        // A record that flows only as well as the pair is not enough: a bridge must mix clearly better.
        let between = with_groove(124.5, 8);
        assert_eq!(choose(&a, &c, &direct, &[&between], &ctx(&s, 10.0)), None);
        assert!(choose_with_gain(&a, &c, &direct, &[&between], &[], &ctx(&s, 10.0), 0.0).is_some());
        // A pair that blends anyway keeps the stricter rule and the chance setting.
        let blend = plan(&with_groove(124.0, 8), &with_groove(124.5, 8), &ctx(&s, 10.0));
        assert!(!always_try(&blend), "{}", blend.reason);
    }

    mod scenes {
        use super::*;
        use listen_core::scene::{Gate, Neighbours, Profile, Scene};

        const KPOP: &str = "K-pop, dance-pop, 2020s, South Korea, Korean, girl group, synthesizer";
        const KPOP_B: &str = "k-pop, ballad, 2010s, south korea, korean, female solo, piano, OST";
        const USRAP: &str = "Hip hop, boom bap, 1990s, United States, English, male solo, turntable";

        fn tags(t: &str) -> Profile {
            Profile { scene: Scene::from_tags(t), ..Default::default() }
        }

        /// The gate, then the chooser over what passed; the index is into `cands`.
        fn gated(
            out: (&Features, Profile),
            inn: (&Features, Profile),
            cands: &[(&Features, Profile)],
        ) -> Option<usize> {
            gated_heard(out, inn, cands, &[])
        }

        /// [`gated`] with the bridge rules; `heard[i]`: the listener played candidate `i`.
        fn gated_heard(
            out: (&Features, Profile),
            inn: (&Features, Profile),
            cands: &[(&Features, Profile)],
            heard: &[bool],
        ) -> Option<usize> {
            let s = Mixing::default();
            let n = Neighbours::default();
            let gate = Gate::new(out.1, inn.1, &n);
            let kept: Vec<(usize, f32)> = cands
                .iter()
                .enumerate()
                .filter_map(|(i, (_, p))| {
                    gate.judge_bridge(p, heard.get(i).copied().unwrap_or(false)).ok().map(|x| (i, x))
                })
                .collect();
            let feats: Vec<&Features> = kept.iter().map(|&(i, _)| cands[i].0).collect();
            let scores: Vec<f32> = kept.iter().map(|&(_, x)| x).collect();
            let direct = plan(out.0, inn.0, &ctx(&s, 10.0));
            choose_scored(out.0, inn.0, &direct, &feats, &scores, &ctx(&s, 10.0)).map(|c| kept[c.index].0)
        }

        fn pair() -> (Features, Features) {
            (
                FeatBuilder::new(120.0, 96).intro(8).outro(16).build(),
                FeatBuilder::new(133.0, 96).intro(16).outro(8).build(),
            )
        }

        #[test]
        fn kpop_pair_is_never_bridged_by_us_rap_even_when_it_flows_best() {
            let (a, c) = pair();
            let rap = with_groove(126.5, 8);
            let kpop = with_groove(127.0, 8);
            let s = Mixing::default();
            let direct = plan(&a, &c, &ctx(&s, 10.0));
            // On sound alone the rap record wins.
            let by_sound = choose(&a, &c, &direct, &[&rap, &kpop], &ctx(&s, 10.0)).expect("a bridge");
            assert_eq!(by_sound.index, 0, "{}", by_sound.reason);
            let got =
                gated((&a, tags(KPOP)), (&c, tags(KPOP_B)), &[(&rap, tags(USRAP)), (&kpop, tags(KPOP))]);
            assert_eq!(got, Some(1));
            // With only the rap record there is no bridge at all.
            assert_eq!(gated((&a, tags(KPOP)), (&c, tags(KPOP_B)), &[(&rap, tags(USRAP))]), None);
        }

        #[test]
        fn a_kpop_pair_gets_no_instrumental_house_bridge() {
            let (a, c) = pair();
            let house = with_groove(126.5, 8);
            let house_tags = tags("tech house, dance, electronic, 2020s, instrumental, club");
            // Not even one the listener played: it shares only the generic pop/electronic with K-pop.
            for heard in [false, true] {
                assert_eq!(
                    gated_heard(
                        (&a, tags(KPOP)),
                        (&c, tags(KPOP)),
                        &[(&house, house_tags.clone())],
                        &[heard]
                    ),
                    None
                );
            }
            // A Korean song's own stretch (an instrumental version tagged Korean) is what a bridge should be.
            let inst = with_groove(126.5, 8);
            let inst_tags = tags("K-pop, dance-pop, 2020s, South Korea, Korean, girl group, instrumental");
            assert_eq!(
                gated_heard(
                    (&a, tags(KPOP)),
                    (&c, tags(KPOP)),
                    &[(&house, house_tags), (&inst, inst_tags)],
                    &[]
                ),
                Some(1)
            );
            // Lo-fi hip hop between two Korean hip hop songs: only when the listener played it.
            let krap = tags("Korean hip hop, 2020s, South Korea, Korean, male solo");
            let lofi = tags("lo-fi hip hop, chillhop, 2020s, instrumental, piano");
            assert_eq!(
                gated_heard((&a, krap.clone()), (&c, krap.clone()), &[(&inst, lofi.clone())], &[false]),
                None
            );
            assert_eq!(gated_heard((&a, krap.clone()), (&c, krap), &[(&inst, lofi)], &[true]), Some(0));
        }

        #[test]
        fn a_loose_match_never_heard_is_not_a_bridge() {
            let (a, c) = pair();
            let b = with_groove(126.5, 8);
            // Korean, but known only by its Hangul title (score 0.6 × …): too loose for a record never played.
            let hangul =
                Profile { scene: Scene::from_script("너만을 느끼며", ""), ..Default::default() };
            let mut kpop_era = tags(KPOP);
            kpop_era.scene.era = Some(2020);
            assert_eq!(gated_heard((&a, kpop_era.clone()), (&c, kpop_era), &[(&b, hangul)], &[false]), None);
        }

        #[test]
        fn a_mixed_pair_takes_a_bridge_from_either_side() {
            let (a, c) = pair();
            let rap = with_groove(126.5, 8);
            assert_eq!(gated((&a, tags(KPOP)), (&c, tags(USRAP)), &[(&rap, tags(USRAP))]), Some(0));
            let kpop = with_groove(126.5, 8);
            assert_eq!(gated((&a, tags(KPOP)), (&c, tags(USRAP)), &[(&kpop, tags(KPOP_B))]), Some(0));
            let samba = tags("samba, bossa nova, 1960s, brazil, portuguese, male solo");
            assert_eq!(gated((&a, tags(KPOP)), (&c, tags(USRAP)), &[(&kpop, samba)]), None);
        }

        #[test]
        fn an_unknown_scene_candidate_is_rejected_when_the_pair_is_known() {
            let (a, c) = pair();
            let b = with_groove(126.5, 8);
            assert_eq!(gated((&a, tags(KPOP)), (&c, tags(KPOP_B)), &[(&b, Profile::default())]), None);
            // Nothing known about the pair: the gate stays out of the way.
            assert_eq!(
                gated((&a, Profile::default()), (&c, Profile::default()), &[(&b, Profile::default())]),
                Some(0)
            );
        }

        #[test]
        fn a_same_scene_bridge_is_still_chosen_and_the_closer_scene_wins_a_tie() {
            let (a, c) = pair();
            let b = with_groove(126.5, 8);
            assert_eq!(gated((&a, tags(KPOP)), (&c, tags(KPOP_B)), &[(&b, tags(KPOP))]), Some(0));
            // Same sound: the fully tagged K-pop song beats one known only by its Hangul title.
            let hangul =
                Profile { scene: Scene::from_script("너만을 느끼며", ""), ..Default::default() };
            let b2 = with_groove(126.5, 8);
            assert_eq!(
                gated((&a, tags(KPOP)), (&c, tags(KPOP_B)), &[(&b, hangul), (&b2, tags(KPOP))]),
                Some(1)
            );
        }
    }
}
