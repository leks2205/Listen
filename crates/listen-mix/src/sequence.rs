//! "Order for smooth mixing": reorders a whole playlist or queue so each song flows into the next,
//! using the same transition cost as the DJs ([`crate::score`]).
//!
//! A path through all analysed songs: nearest-neighbour start from the fixed first song, then 2-opt
//! (reverse a stretch) and relocation (move one song) until nothing improves. The cost is asymmetric
//! (the energy term prefers a gentle rise), so both moves are scored in the direction they are played.
//! Unanalysed songs keep their relative order at the end.

use crate::score::{FlowCtx, repeat_penalty, stats_for, step_cost};
use listen_core::analysis::FlowFeatures;
use listen_core::model::Song;

/// Analysed songs beyond this many are left after the ordered part, in their order (the cost matrix
/// grows with the square; 2000 songs take well under a second).
pub const MAX_ORDERED: usize = 2000;
/// Improvement passes at most (each pass is O(n²)); real playlists settle in a handful.
const MAX_PASSES: usize = 200;
/// Smaller gains than this are noise.
const EPS: f32 = 1e-5;

/// Cost of playing `b` right after `a` in a fixed list: flow cost (energy without a history arc) plus
/// the artist/album repeat penalty for neighbours.
fn link_cost(a: &(Song, Option<FlowFeatures>), b: &(Song, Option<FlowFeatures>), ctx: &FlowCtx) -> f32 {
    step_cost(a.1.as_ref(), b.1.as_ref(), &[], ctx, ctx.timbre.as_ref()) + repeat_penalty(&[&a.0], &b.0, ctx)
}

/// Sum of the link costs along `order` (indices into `songs`), and the number of links. Used to compare
/// an order before and after [`order_all`].
pub fn path_cost(songs: &[(Song, Option<FlowFeatures>)], order: &[usize], ctx: &FlowCtx) -> (f32, usize) {
    let ctx = list_ctx(songs, ctx);
    let total = order.windows(2).map(|w| link_cost(&songs[w[0]], &songs[w[1]], &ctx)).sum();
    (total, order.len().saturating_sub(1))
}

/// The scorer context for a fixed list: no recent-artist penalty (every song is in the list anyway),
/// timbre spread from the list when the caller has none.
fn list_ctx(songs: &[(Song, Option<FlowFeatures>)], ctx: &FlowCtx) -> FlowCtx {
    FlowCtx {
        recent: Vec::new(),
        recent_artists: Vec::new(),
        timbre: stats_for(ctx, None, songs),
        ..ctx.clone()
    }
}

/// A smooth playing order of all `songs`: a permutation of `0..songs.len()` that starts with `first`
/// (when given and in range), then the analysed songs in flow order, then the unanalysed ones in
/// their original order. Deterministic.
pub fn order_all(songs: &[(Song, Option<FlowFeatures>)], first: Option<usize>, ctx: &FlowCtx) -> Vec<usize> {
    let first = first.filter(|&f| f < songs.len());
    let ctx = list_ctx(songs, ctx);
    let mut analysed: Vec<usize> =
        (0..songs.len()).filter(|&i| Some(i) != first && songs[i].1.is_some()).collect();
    let overflow = analysed.split_off(analysed.len().min(MAX_ORDERED));
    let unanalysed = (0..songs.len()).filter(|&i| Some(i) != first && songs[i].1.is_none());

    // Node 0 is the fixed start (or a virtual one that costs the same into every song).
    let nodes: Vec<Option<usize>> = std::iter::once(first).chain(analysed.iter().map(|&i| Some(i))).collect();
    let n = nodes.len();
    let mut cost = vec![0.0f32; n * n];
    for (a, na) in nodes.iter().enumerate() {
        for (b, nb) in nodes.iter().enumerate() {
            if a != b && b != 0 {
                cost[a * n + b] = match (na, nb) {
                    (Some(x), Some(y)) => link_cost(&songs[*x], &songs[*y], &ctx),
                    _ => 0.0,
                };
            }
        }
    }
    let path = improve(nearest_neighbour(&cost, n), &cost, n);

    let mut out = Vec::with_capacity(songs.len());
    out.extend(first);
    out.extend(path.into_iter().skip(1).filter_map(|node| nodes[node]));
    out.extend(overflow);
    out.extend(unanalysed);
    out
}

/// Greedy path from node 0; ties go to the lower node (original order).
fn nearest_neighbour(cost: &[f32], n: usize) -> Vec<usize> {
    let mut used = vec![false; n];
    used[0] = true;
    let mut path = vec![0];
    for _ in 1..n {
        let last = *path.last().unwrap_or(&0);
        let next = (0..n)
            .filter(|&j| !used[j])
            .min_by(|&a, &b| cost[last * n + a].total_cmp(&cost[last * n + b]).then(a.cmp(&b)));
        let Some(next) = next else { break };
        used[next] = true;
        path.push(next);
    }
    path
}

/// 2-opt and single-song relocation on an open path with a fixed start, first improvement, until no
/// move helps (or [`MAX_PASSES`]).
fn improve(mut t: Vec<usize>, cost: &[f32], n: usize) -> Vec<usize> {
    let m = t.len();
    if m < 3 {
        return t;
    }
    let c = |a: usize, b: usize| cost[a * n + b];
    for _ in 0..MAX_PASSES {
        // fwd[k]: cost of t[0..=k] as played; rev[k]: the same links played backwards.
        let mut fwd = vec![0.0f32; m];
        let mut rev = vec![0.0f32; m];
        for k in 1..m {
            fwd[k] = fwd[k - 1] + c(t[k - 1], t[k]);
            rev[k] = rev[k - 1] + c(t[k], t[k - 1]);
        }
        let mut changed = false;
        // 2-opt: reverse t[i+1..=j].
        'two: for i in 0..m - 2 {
            for j in i + 2..m {
                let tail_old = if j + 1 < m { c(t[j], t[j + 1]) } else { 0.0 };
                let tail_new = if j + 1 < m { c(t[i + 1], t[j + 1]) } else { 0.0 };
                let old = c(t[i], t[i + 1]) + (fwd[j] - fwd[i + 1]) + tail_old;
                let new = c(t[i], t[j]) + (rev[j] - rev[i + 1]) + tail_new;
                if new + EPS < old {
                    t[i + 1..=j].reverse();
                    changed = true;
                    break 'two;
                }
            }
        }
        if !changed {
            // Relocation: move t[p] (p ≥ 1) to just after t[q].
            'move_one: for p in 1..m {
                let (prev, node) = (t[p - 1], t[p]);
                let next = t.get(p + 1).copied();
                let removed =
                    c(prev, node) + next.map_or(0.0, |x| c(node, x)) - next.map_or(0.0, |x| c(prev, x));
                for q in 0..m {
                    if q == p || q + 1 == p {
                        // Same place.
                        continue;
                    }
                    // q ≠ p − 1, so q's successor after the removal is still t[q + 1].
                    let after = t.get(q + 1).copied();
                    let added =
                        c(t[q], node) + after.map_or(0.0, |x| c(node, x)) - after.map_or(0.0, |x| c(t[q], x));
                    if added + EPS < removed {
                        t.remove(p);
                        let at = if q < p { q + 1 } else { q };
                        t.insert(at, node);
                        changed = true;
                        break 'move_one;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{flow, song};
    use listen_core::analysis::Camelot;

    fn key(n: u8, minor: bool) -> Camelot {
        Camelot { number: n, minor }
    }

    fn entry(title: &str, artist: &str, bpm: f32, k: u8, energy: f32) -> (Song, Option<FlowFeatures>) {
        (song(title, artist, None, None), Some(flow(Some(bpm), Some(key(k, true)), energy)))
    }

    fn is_permutation(o: &[usize], n: usize) -> bool {
        let mut s = o.to_vec();
        s.sort_unstable();
        s == (0..n).collect::<Vec<_>>()
    }

    #[test]
    fn tempo_ladder_is_restored_and_first_kept() {
        // A shuffled tempo ladder 100 → 130 bpm (keys walking round the wheel).
        let bpms = [100.0, 118.0, 104.0, 130.0, 110.0, 122.0, 106.0, 126.0, 114.0];
        let songs: Vec<_> = bpms
            .iter()
            .enumerate()
            .map(|(i, &b)| {
                let k = ((b - 100.0) / 4.0) as u8 % 12 + 1;
                entry(&format!("s{i}"), &format!("A{i}"), b, k, 0.5)
            })
            .collect();
        let ctx = FlowCtx::default();
        let o = order_all(&songs, Some(0), &ctx);
        assert!(is_permutation(&o, songs.len()));
        assert_eq!(o[0], 0);
        let tempos: Vec<f32> = o.iter().map(|&i| bpms[i]).collect();
        assert!(tempos.windows(2).all(|w| w[1] > w[0]), "{tempos:?}");
        let before = path_cost(&songs, &(0..songs.len()).collect::<Vec<_>>(), &ctx).0;
        let after = path_cost(&songs, &o, &ctx).0;
        assert!(after < before, "{after} < {before}");
        assert_eq!(order_all(&songs, Some(0), &ctx), o, "deterministic");
    }

    #[test]
    fn unanalysed_songs_go_last_in_their_order() {
        let mut songs = vec![
            entry("a", "A", 120.0, 8, 0.5),
            (song("u1", "U", None, None), None),
            entry("b", "B", 140.0, 2, 0.9),
            (song("u2", "V", None, None), None),
            entry("c", "C", 121.0, 8, 0.52),
        ];
        let o = order_all(&songs, Some(0), &FlowCtx::default());
        assert_eq!(o, [0, 4, 2, 1, 3]);
        // An unanalysed first song stays first; the rest still flows.
        songs.swap(0, 1);
        let o = order_all(&songs, Some(0), &FlowCtx::default());
        assert_eq!(o[0], 0);
        assert!(is_permutation(&o, 5));
        assert_eq!(o[4], 3);
    }

    #[test]
    fn same_artist_is_spread_and_no_first_works() {
        let songs = vec![
            entry("a1", "Dup", 120.0, 8, 0.5),
            entry("a2", "Dup", 120.0, 8, 0.5),
            entry("o1", "Else", 120.0, 8, 0.5),
            entry("a3", "Dup", 120.0, 8, 0.5),
            entry("o2", "Other", 120.0, 8, 0.5),
        ];
        let o = order_all(&songs, None, &FlowCtx::default());
        assert!(is_permutation(&o, 5));
        let artists: Vec<&str> = o.iter().map(|&i| songs[i].0.artists[0].name.as_str()).collect();
        assert!(artists.windows(2).all(|w| w[0] != w[1]), "{artists:?}");
    }

    #[test]
    fn tiny_and_empty_inputs() {
        let ctx = FlowCtx::default();
        assert!(order_all(&[], Some(0), &ctx).is_empty());
        let one = vec![entry("a", "A", 120.0, 8, 0.5)];
        assert_eq!(order_all(&one, Some(0), &ctx), [0]);
        assert_eq!(order_all(&one, Some(5), &ctx), [0], "out-of-range first is ignored");
        assert_eq!(path_cost(&one, &[0], &ctx), (0.0, 0));
    }

    #[test]
    fn three_hundred_songs_are_fast_and_better() {
        // Pseudo-random library: tempos 80–160, keys, energies, artists.
        let mut x: u32 = 12345;
        let mut rnd = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        let songs: Vec<_> = (0..300)
            .map(|i| {
                let bpm = 80.0 + (rnd() % 80) as f32;
                let k = (rnd() % 12) as u8 + 1;
                let e = (rnd() % 100) as f32 / 100.0;
                entry(&format!("s{i}"), &format!("A{}", rnd() % 60), bpm, k, e)
            })
            .collect();
        let ctx = FlowCtx::default();
        let t = std::time::Instant::now();
        let o = order_all(&songs, Some(0), &ctx);
        let took = t.elapsed();
        assert!(is_permutation(&o, 300));
        assert_eq!(o[0], 0);
        let before = path_cost(&songs, &(0..300).collect::<Vec<_>>(), &ctx).0;
        let after = path_cost(&songs, &o, &ctx).0;
        assert!(after < 0.5 * before, "{after} vs {before}");
        // Debug builds are ~10× slower than release; the phone budget is 1 s in release.
        assert!(took.as_secs_f32() < 5.0, "{took:?}");
    }
}
