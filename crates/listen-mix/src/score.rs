//! Song flow: how well song B follows song A, and the order a DJ should
//! play a set of candidates in.
//!
//! Every term is a cost (0 = perfect, about 1 = typical and acceptable). Unknown data gets a neutral
//! value between a good and a bad match, so unanalysed songs are neither preferred nor buried.

use listen_core::analysis::FlowFeatures;
use listen_core::model::Song;

/// Relative weight of each flow term, plus the repetition penalties.
#[derive(Debug, Clone, PartialEq)]
pub struct Weights {
    pub tempo: f32,
    pub key: f32,
    pub energy: f32,
    pub timbre: f32,
    /// Cost for a pair without instrumental outro / intro to blend over.
    pub mixability: f32,
    /// Same artist as the song right before (decays further back).
    pub artist_repeat: f32,
    /// Same album as the song right before.
    pub album_repeat: f32,
    /// Added to the neutral cost of a candidate without analysis.
    pub unanalysed: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            tempo: 1.0,
            key: 0.8,
            energy: 0.6,
            timbre: 0.5,
            mixability: 0.3,
            artist_repeat: 1.5,
            album_repeat: 0.8,
            unanalysed: 0.3,
        }
    }
}

/// Per-dimension mean and spread of timbre vectors, so that no single
/// dimension (e.g. loudness) dominates the distance.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimbreStats {
    pub mean: Vec<f32>,
    pub std: Vec<f32>,
}

impl TimbreStats {
    /// Stats over the vectors of the most common length; `None` if fewer
    /// than three such vectors (too few to estimate a spread).
    pub fn from_vectors<'a>(vectors: impl IntoIterator<Item = &'a [f32]>) -> Option<Self> {
        let vectors: Vec<&[f32]> = vectors.into_iter().filter(|v| !v.is_empty()).collect();
        let dim = {
            let mut lens: Vec<usize> = vectors.iter().map(|v| v.len()).collect();
            lens.sort_unstable();
            // Most common length; ties go to the longer one.
            let mut best = (0, 0);
            for chunk in lens.chunk_by(|a, b| a == b) {
                if chunk.len() >= best.1 {
                    best = (chunk[0], chunk.len());
                }
            }
            best.0
        };
        let same: Vec<&[f32]> = vectors.into_iter().filter(|v| v.len() == dim).collect();
        if same.len() < 3 {
            return None;
        }
        let n = same.len() as f32;
        let mean: Vec<f32> = (0..dim).map(|d| same.iter().map(|v| v[d]).sum::<f32>() / n).collect();
        let std = (0..dim)
            .map(|d| (same.iter().map(|v| (v[d] - mean[d]).powi(2)).sum::<f32>() / n).sqrt().max(1e-6))
            .collect();
        Some(TimbreStats { mean, std })
    }
}

/// What the scorer knows besides the two songs.
#[derive(Debug, Clone, Default)]
pub struct FlowCtx {
    /// Songs played before the "from" song, oldest first (for the energy arc).
    pub recent: Vec<FlowFeatures>,
    /// Steer the energy towards this level (0..1), e.g. from the AI DJ's mood.
    pub target_energy: Option<f32>,
    pub weights: Weights,
    /// Timbre normalisation; when `None`, [`order`] and [`rank`] estimate it
    /// from the candidates.
    pub timbre: Option<TimbreStats>,
    /// Lower-case names of artists played recently (mildly penalised).
    pub recent_artists: Vec<String>,
}

/// Below this tempo confidence a bpm is treated as unknown.
const MIN_BPM_CONFIDENCE: f32 = 0.35;
const NEUTRAL_TEMPO: f32 = 0.5;
const NEUTRAL_KEY: f32 = 0.4;
const NEUTRAL_TIMBRE: f32 = 1.0;
/// ln(1.06): a 6 % tempo gap costs 1.0.
const TEMPO_UNIT: f32 = 0.058_268_91;
/// Half/double-time matches are fine to mix but change the feel.
const OCTAVE_PENALTY: f32 = 0.15;

/// The octave factor `k` ∈ {1, 2, ½} that brings `from_bpm · k` closest to
/// `to_bpm`, and the remaining log ratio `ln(from_bpm · k / to_bpm)`.
pub fn octave_match(from_bpm: f32, to_bpm: f32) -> (f32, f32) {
    let (from, to) = (from_bpm.max(1.0), to_bpm.max(1.0));
    [1.0f32, 2.0, 0.5]
        .into_iter()
        .map(|k| (k, (from * k / to).ln()))
        .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .unwrap_or((1.0, 0.0))
}

/// Tempo cost of mixing `a` bpm into `b` bpm: linear up to a 6 % gap (=1.0),
/// quadratic beyond (a 12 % gap costs 4), capped at 4.
pub fn tempo_cost(a: f32, b: f32) -> f32 {
    let (k, lr) = octave_match(a, b);
    let x = lr.abs() / TEMPO_UNIT;
    let base = if x <= 1.0 { x } else { x * x };
    base.min(4.0) + if k == 1.0 { 0.0 } else { OCTAVE_PENALTY }
}

fn tempo_term(a: &FlowFeatures, b: &FlowFeatures) -> f32 {
    match (a.bpm, b.bpm) {
        (Some(x), Some(y)) if a.bpm_confidence.min(b.bpm_confidence) >= MIN_BPM_CONFIDENCE => {
            tempo_cost(x, y)
        }
        _ => NEUTRAL_TEMPO,
    }
}

/// Camelot distance, blended towards neutral as the key confidence drops (so an unsure key never looks
/// better than a confident ±1 neighbour).
fn key_term(a: &FlowFeatures, b: &FlowFeatures) -> f32 {
    match (a.key, b.key) {
        (Some((ka, ca)), Some((kb, cb))) => {
            let c = ca.min(cb).clamp(0.0, 1.0);
            c * ka.distance(kb) + (1.0 - c) * NEUTRAL_KEY
        }
        _ => NEUTRAL_KEY,
    }
}

/// The energy step the set wants next: towards the target if one is set,
/// otherwise a gentle rise with a dip after a long climb or at the top.
fn desired_step(history: &[f32], cur: f32, target: Option<f32>) -> f32 {
    if let Some(t) = target {
        return (t - cur).clamp(-0.1, 0.1);
    }
    let climbing = history.len() >= 3 && {
        let tail: Vec<f32> = history[history.len() - 3..].iter().copied().chain([cur]).collect();
        tail.windows(2).all(|w| w[1] > w[0] + 0.01)
    };
    if climbing && cur > 0.6 {
        -0.12
    } else if cur > 0.85 {
        -0.05
    } else {
        0.04
    }
}

fn energy_term(history: &[f32], a: &FlowFeatures, b: &FlowFeatures, target: Option<f32>) -> f32 {
    let want = desired_step(history, a.mood.energy, target);
    (((b.mood.energy - a.mood.energy) - want).abs() / 0.25).min(2.0)
}

/// Standardised RMS distance (≈1 for two random songs of the library).
fn timbre_term(a: &[f32], b: &[f32], stats: Option<&TimbreStats>) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return NEUTRAL_TIMBRE;
    }
    let d = match stats.filter(|s| s.std.len() == a.len()) {
        Some(s) => {
            let sum: f32 = a.iter().zip(b).zip(&s.std).map(|((x, y), sd)| ((x - y) / sd).powi(2)).sum();
            // Two independent standardised vectors are √2 apart on average.
            (sum / a.len() as f32).sqrt() / std::f32::consts::SQRT_2
        }
        None => {
            // No spread known: relative distance, roughly on the same scale.
            let diff: f32 = a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f32>().sqrt();
            let size: f32 =
                a.iter().map(|x| x * x).sum::<f32>().sqrt() + b.iter().map(|x| x * x).sum::<f32>().sqrt();
            if size <= 1e-9 { 0.0 } else { 2.0 * diff / size }
        }
    };
    d.min(2.0)
}

fn mix_term(a: &FlowFeatures, b: &FlowFeatures) -> f32 {
    (2.0 - f32::from(u8::from(a.mixable_out)) - f32::from(u8::from(b.mixable_in))) / 2.0
}

fn cost_with(
    a: &FlowFeatures,
    b: &FlowFeatures,
    history: &[f32],
    ctx: &FlowCtx,
    stats: Option<&TimbreStats>,
) -> f32 {
    let w = &ctx.weights;
    w.tempo * tempo_term(a, b)
        + w.key * key_term(a, b)
        + w.energy * energy_term(history, a, b, ctx.target_energy)
        + w.timbre * timbre_term(&a.timbre, &b.timbre, stats)
        + w.mixability * mix_term(a, b)
}

/// Flow cost of playing `b` right after `a` (lower is better, ≥ 0).
/// `ctx.recent` is the history before `a` (energy arc).
pub fn pair_cost(a: &FlowFeatures, b: &FlowFeatures, ctx: &FlowCtx) -> f32 {
    let history: Vec<f32> = ctx.recent.iter().map(|f| f.mood.energy).collect();
    cost_with(a, b, &history, ctx, ctx.timbre.as_ref())
}

/// The cost used when either side has no analysis: middle of the road.
fn neutral_cost(w: &Weights) -> f32 {
    w.tempo * NEUTRAL_TEMPO
        + w.key * NEUTRAL_KEY
        + w.energy * 0.4
        + w.timbre * NEUTRAL_TIMBRE
        + w.mixability * 0.5
}

fn same_artist(a: &Song, b: &Song) -> bool {
    a.artists.iter().any(|x| {
        b.artists.iter().any(|y| match (&x.id, &y.id) {
            (Some(i), Some(j)) => i == j,
            _ => x.name.eq_ignore_ascii_case(&y.name),
        })
    })
}

/// Penalty for `next` given the songs before it (most recent last).
fn repeat_penalty(before: &[&Song], next: &Song, ctx: &FlowCtx) -> f32 {
    let w = &ctx.weights;
    let mut p = 0.0;
    for (back, prev) in before.iter().rev().take(3).enumerate() {
        if same_artist(prev, next) {
            p += w.artist_repeat * if back == 0 { 1.0 } else { 0.4 };
        }
        if back == 0 && prev.album_id.is_some() && prev.album_id == next.album_id {
            p += w.album_repeat;
        }
    }
    if next.artists.iter().any(|a| ctx.recent_artists.iter().any(|r| r.eq_ignore_ascii_case(&a.name))) {
        p += 0.5 * w.artist_repeat;
    }
    p
}

/// One step: flow (or neutral) cost plus the unanalysed penalty.
fn step_cost(
    a: Option<&FlowFeatures>,
    b: Option<&FlowFeatures>,
    history: &[f32],
    ctx: &FlowCtx,
    stats: Option<&TimbreStats>,
) -> f32 {
    let w = &ctx.weights;
    match (a, b) {
        (Some(a), Some(b)) => cost_with(a, b, history, ctx, stats),
        (None, Some(_)) => neutral_cost(w),
        (_, None) => neutral_cost(w) + w.unanalysed,
    }
}

fn stats_for(
    ctx: &FlowCtx,
    current: Option<&FlowFeatures>,
    cands: &[(Song, Option<FlowFeatures>)],
) -> Option<TimbreStats> {
    ctx.timbre.clone().or_else(|| {
        TimbreStats::from_vectors(
            current.into_iter().chain(cands.iter().filter_map(|c| c.1.as_ref())).map(|f| f.timbre.as_slice()),
        )
    })
}

/// Candidate indices ordered by the cost of playing each right after
/// `current` (single step; for rank fusion in the AI DJ pool). Ties keep
/// the input order.
pub fn rank(
    current: (&Song, Option<&FlowFeatures>),
    cands: &[(Song, Option<FlowFeatures>)],
    ctx: &FlowCtx,
) -> Vec<usize> {
    let stats = stats_for(ctx, current.1, cands);
    let history: Vec<f32> = ctx.recent.iter().map(|f| f.mood.energy).collect();
    let mut scored: Vec<(f32, usize)> = cands
        .iter()
        .enumerate()
        .map(|(i, (song, f))| {
            let c = step_cost(current.1, f.as_ref(), &history, ctx, stats.as_ref())
                + repeat_penalty(&[current.0], song, ctx);
            (c, i)
        })
        .collect();
    scored.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

const BEAM_WIDTH: usize = 4;

/// Picks and orders up to `n` candidates to follow `current` so the whole run flows (beam search,
/// width 4). Deterministic: equal costs are broken by candidate index.
pub fn order(
    current: (&Song, Option<&FlowFeatures>),
    cands: &[(Song, Option<FlowFeatures>)],
    n: usize,
    ctx: &FlowCtx,
) -> Vec<usize> {
    let n = n.min(cands.len());
    let stats = stats_for(ctx, current.1, cands);
    let base_history: Vec<f32> = ctx.recent.iter().map(|f| f.mood.energy).collect();

    struct Beam {
        path: Vec<usize>,
        cost: f32,
    }
    let mut beams = vec![Beam { path: Vec::new(), cost: 0.0 }];
    for _ in 0..n {
        let mut next: Vec<Beam> = Vec::new();
        for beam in &beams {
            let (last, before): (Option<&FlowFeatures>, Vec<&Song>) = {
                let songs: Vec<&Song> =
                    std::iter::once(current.0).chain(beam.path.iter().map(|&i| &cands[i].0)).collect();
                let last = match beam.path.last() {
                    Some(&i) => cands[i].1.as_ref(),
                    None => current.1,
                };
                (last, songs)
            };
            // Energies before `last`: the recent history, current, then the path minus its end.
            let mut history = base_history.clone();
            if !beam.path.is_empty() {
                history.extend(current.1.map(|f| f.mood.energy));
                history.extend(
                    beam.path[..beam.path.len() - 1]
                        .iter()
                        .filter_map(|&i| cands[i].1.as_ref().map(|f| f.mood.energy)),
                );
            }
            for (i, (song, f)) in cands.iter().enumerate() {
                if beam.path.contains(&i) {
                    continue;
                }
                let c = step_cost(last, f.as_ref(), &history, ctx, stats.as_ref())
                    + repeat_penalty(&before, song, ctx);
                let mut path = beam.path.clone();
                path.push(i);
                next.push(Beam { path, cost: beam.cost + c });
            }
        }
        next.sort_by(|a, b| a.cost.total_cmp(&b.cost).then_with(|| a.path.cmp(&b.path)));
        next.truncate(BEAM_WIDTH);
        if next.is_empty() {
            break;
        }
        beams = next;
    }
    beams.into_iter().next().map(|b| b.path).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{flow, song};
    use listen_core::analysis::Camelot;

    fn key(n: u8, minor: bool) -> Camelot {
        Camelot { number: n, minor }
    }

    #[test]
    fn tempo_cost_scale_and_octaves() {
        assert!(tempo_cost(120.0, 120.0) < 1e-6);
        assert!((tempo_cost(120.0, 127.2) - 1.0).abs() < 0.01, "6 % ≈ 1");
        assert!((tempo_cost(127.2, 120.0) - 1.0).abs() < 0.01, "symmetric");
        // Steeper beyond 6 %: twice the gap costs four times as much.
        assert!((tempo_cost(120.0, 120.0 * 1.06f32.powi(2)) - 4.0).abs() < 0.05);
        // Half / double time: only the small octave penalty.
        assert!((tempo_cost(120.0, 240.0) - OCTAVE_PENALTY).abs() < 1e-4);
        assert!((tempo_cost(140.0, 70.0) - OCTAVE_PENALTY).abs() < 1e-4);
        assert!(tempo_cost(120.0, 61.0) < 0.5);
        let (k, lr) = octave_match(64.0, 128.0);
        assert_eq!(k, 2.0);
        assert!(lr.abs() < 1e-6);
        assert!(tempo_cost(100.0, 150.0) >= 4.0, "a fifth apart is a clash");
    }

    #[test]
    fn unknown_or_unsure_tempo_is_neutral() {
        let a = flow(Some(120.0), None, 0.5);
        let mut b = flow(Some(150.0), None, 0.5);
        b.bpm_confidence = 0.1;
        assert_eq!(tempo_term(&a, &b), NEUTRAL_TEMPO);
        b.bpm = None;
        assert_eq!(tempo_term(&a, &b), NEUTRAL_TEMPO);
    }

    #[test]
    fn key_cost_follows_camelot_and_confidence() {
        let mut a = flow(Some(120.0), Some(key(8, true)), 0.5);
        let mut b = flow(Some(120.0), Some(key(8, true)), 0.5);
        assert!(key_term(&a, &b) < 1e-6);
        b.key = Some((key(9, true), 1.0));
        let near = key_term(&a, &b);
        b.key = Some((key(3, false), 1.0));
        let clash = key_term(&a, &b);
        assert!(near < NEUTRAL_KEY && NEUTRAL_KEY < clash, "{near} {clash}");
        // An unsure key moves towards neutral, from either side.
        a.key = Some((key(8, true), 0.2));
        let unsure = key_term(&a, &b);
        assert!(unsure < clash && unsure > NEUTRAL_KEY);
        a.key = None;
        assert_eq!(key_term(&a, &b), NEUTRAL_KEY);
    }

    #[test]
    fn energy_arc_rises_gently_and_dips_after_a_climb() {
        let ctx = FlowCtx::default();
        let a = flow(Some(120.0), None, 0.5);
        let small_rise = pair_cost(&a, &flow(Some(120.0), None, 0.55), &ctx);
        let big_jump = pair_cost(&a, &flow(Some(120.0), None, 0.95), &ctx);
        let drop = pair_cost(&a, &flow(Some(120.0), None, 0.2), &ctx);
        assert!(small_rise < big_jump && small_rise < drop);
        // After a long climb to 0.8 a dip is wanted.
        let climb =
            FlowCtx { recent: [0.5, 0.6, 0.7].map(|e| flow(None, None, e)).to_vec(), ..FlowCtx::default() };
        let top = flow(Some(120.0), None, 0.8);
        assert!(
            pair_cost(&top, &flow(Some(120.0), None, 0.68), &climb)
                < pair_cost(&top, &flow(Some(120.0), None, 0.85), &climb)
        );
        // A target pulls towards it.
        let calm = FlowCtx { target_energy: Some(0.2), ..FlowCtx::default() };
        assert!(
            pair_cost(&a, &flow(Some(120.0), None, 0.4), &calm)
                < pair_cost(&a, &flow(Some(120.0), None, 0.6), &calm)
        );
    }

    #[test]
    fn timbre_handles_stats_and_bad_vectors() {
        let stats = TimbreStats::from_vectors([[0.0, 0.0].as_slice(), &[1.0, 10.0], &[2.0, 20.0]]).unwrap();
        // The second dimension has 10× the spread, so it counts 10× less.
        let d1 = timbre_term(&[0.0, 0.0], &[1.0, 0.0], Some(&stats));
        let d2 = timbre_term(&[0.0, 0.0], &[0.0, 1.0], Some(&stats));
        assert!((d1 / d2 - 10.0).abs() < 0.01);
        assert_eq!(timbre_term(&[], &[], Some(&stats)), NEUTRAL_TIMBRE);
        assert_eq!(timbre_term(&[1.0], &[1.0, 2.0], None), NEUTRAL_TIMBRE);
        assert_eq!(timbre_term(&[1.0, 2.0], &[1.0, 2.0], None), 0.0);
        // Too few vectors for a spread.
        assert!(TimbreStats::from_vectors([[1.0].as_slice(), &[2.0]]).is_none());
    }

    #[test]
    fn mixability_is_a_bonus() {
        let ctx = FlowCtx::default();
        let a = flow(Some(120.0), None, 0.5);
        let mut b = a.clone();
        let plain = pair_cost(&a, &b, &ctx);
        let mut a2 = a.clone();
        a2.mixable_out = true;
        b.mixable_in = true;
        assert!(pair_cost(&a2, &b, &ctx) < plain);
    }

    fn cand(title: &str, artist: &str, f: Option<FlowFeatures>) -> (Song, Option<FlowFeatures>) {
        (song(title, artist, None, None), f)
    }

    #[test]
    fn beam_order_prefers_a_flowing_chain() {
        let cur = song("now", "Z", None, None);
        let cf = flow(Some(120.0), Some(key(8, true)), 0.5);
        let cands = vec![
            cand("far", "A", Some(flow(Some(150.0), Some(key(2, false)), 0.9))),
            cand("step3", "B", Some(flow(Some(127.0), Some(key(10, true)), 0.62))),
            cand("step1", "C", Some(flow(Some(122.0), Some(key(8, true)), 0.54))),
            cand("step2", "D", Some(flow(Some(124.5), Some(key(9, true)), 0.58))),
        ];
        let ctx = FlowCtx::default();
        let o = order((&cur, Some(&cf)), &cands, 3, &ctx);
        assert_eq!(o, [2, 3, 1]);
        assert_eq!(order((&cur, Some(&cf)), &cands, 3, &ctx), o, "deterministic");
        assert_eq!(order((&cur, Some(&cf)), &cands, 10, &ctx).len(), 4, "n is capped");
        assert_eq!(rank((&cur, Some(&cf)), &cands, &ctx)[0], 2);
        assert_eq!(*rank((&cur, Some(&cf)), &cands, &ctx).last().unwrap(), 0);
    }

    #[test]
    fn unanalysed_songs_sit_in_the_middle() {
        let cur = song("now", "Z", None, None);
        let cf = flow(Some(120.0), Some(key(8, true)), 0.5);
        let good = cand("good", "A", Some(flow(Some(121.0), Some(key(8, true)), 0.53)));
        let bad = cand("bad", "B", Some(flow(Some(140.0), Some(key(2, false)), 0.95)));
        let unknown = cand("unknown", "C", None);
        let ctx = FlowCtx::default();
        let r = rank((&cur, Some(&cf)), &[bad, unknown, good], &ctx);
        assert_eq!(r, [2, 1, 0]);
        // Unknown current song: everything is neutral, analysed ones first.
        let r = rank((&cur, None), &[cand("u", "C", None), cand("a", "A", Some(cf.clone()))], &ctx);
        assert_eq!(r, [1, 0]);
    }

    #[test]
    fn repeating_the_artist_or_album_costs() {
        let mut cur = song("now", "Same", Some("alb"), Some(1));
        cur.artists[0].id = Some("ar1".into());
        let f = flow(Some(120.0), Some(key(8, true)), 0.5);
        let mut same_artist = cand("x", "SAME", Some(f.clone()));
        same_artist.0.artists[0].id = None; // compared by name, case-insensitively
        let mut same_album = cand("y", "Other", Some(f.clone()));
        same_album.0.album_id = Some("alb".into());
        let fresh = cand("z", "Fresh", Some(f.clone()));
        let ctx = FlowCtx::default();
        let r = rank((&cur, Some(&f)), &[same_artist.clone(), same_album, fresh], &ctx);
        assert_eq!(r, [2, 1, 0]);
        // Two songs by one artist are spread out rather than played back to back.
        let a1 = cand("a1", "Dup", Some(f.clone()));
        let a2 = cand("a2", "Dup", Some(f.clone()));
        let other = cand("o", "Else", Some(f.clone()));
        let o = order((&cur, Some(&f)), &[a1, a2, other], 3, &ctx);
        assert_eq!(o[1], 2, "{o:?}");
        let recent = FlowCtx { recent_artists: vec!["fresh".into()], ..FlowCtx::default() };
        let r = rank(
            (&cur, Some(&f)),
            &[cand("z", "Fresh", Some(f.clone())), cand("w", "New", Some(f.clone()))],
            &recent,
        );
        assert_eq!(r, [1, 0]);
    }
}
