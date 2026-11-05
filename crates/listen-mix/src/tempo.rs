//! Tempo matching for the phone's heart-rate / cadence mode: turn a live rate (heart beats or steps per
//! minute) into a [`TempoTarget`], smooth it, decide when the queue is worth re-ranking, and pick or
//! re-rank songs by how well their tempo fits. Half and double time count (85 BPM fits 170).
//!
//! Evidence note: music does not measurably change heart rate during exercise (Terry et al. 2020);
//! matching step cadence has better support. This is "music that fits my effort", not training guidance.

use crate::pick::TempoTarget;
use listen_core::analysis::FlowFeatures;
use listen_core::model::Song;
use listen_core::queue::{Picker, Queue, Why, WhyCode};

/// Targets are clamped to this range (BPM).
pub const MIN_BPM: f32 = 70.0;
pub const MAX_BPM: f32 = 180.0;
/// A song fits when its tempo (or half/double) is within this many BPM of the target.
pub const TOLERANCE: f32 = 8.0;
/// The queue is re-ranked only when the smoothed target moved more than this since the last re-rank.
pub const RERANK_STEP: f32 = 5.0;
/// A re-rank keeps queued automatic songs within this many BPM (half the fit tolerance), so after the
/// target drifts up to [`RERANK_STEP`] without a re-rank the queue still sits within the tolerance.
pub const KEEP_TOLERANCE: f32 = TOLERANCE / 2.0;
/// Time constant of the smoothing (seconds).
pub const SMOOTH_S: f64 = 30.0;
/// Below this beat-grid confidence a song's tempo counts as unknown (as in the flow scorer).
const MIN_CONFIDENCE: f32 = 0.35;

/// The tempo target for a heart rate or cadence of `rate` per minute: clamped to 70–180, ± 8 BPM, half
/// and double time count.
pub fn target(rate: f32) -> TempoTarget {
    let bpm = if rate.is_finite() { rate.clamp(MIN_BPM, MAX_BPM) } else { MIN_BPM };
    TempoTarget { bpm, tol: TOLERANCE, half_double: true }
}

/// A song's tempo from its analysis when the beat grid is sure enough.
pub fn flow_bpm(f: &FlowFeatures) -> Option<f32> {
    f.bpm.filter(|b| *b > 0.0 && f.bpm_confidence >= MIN_CONFIDENCE)
}

/// Smooths the live rate (exponential, 30-s time constant) and says when the target moved enough to
/// re-rank the queue.
#[derive(Debug, Clone, Default)]
pub struct Tracker {
    smoothed: Option<f32>,
    last_t: f64,
    ranked: Option<f32>,
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a reading `rate` per minute taken at `t_s` (any monotonic seconds). Implausible readings
    /// (outside 30–240) are ignored. Returns the smoothed rate.
    pub fn push(&mut self, rate: f32, t_s: f64) -> Option<f32> {
        if !rate.is_finite() || !(30.0..=240.0).contains(&rate) {
            return self.smoothed;
        }
        self.smoothed = Some(match self.smoothed {
            None => rate,
            Some(s) => {
                let dt = (t_s - self.last_t).max(0.0);
                let a = (1.0 - (-dt / SMOOTH_S).exp()) as f32;
                s + a * (rate - s)
            }
        });
        self.last_t = t_s;
        self.smoothed
    }

    /// The smoothed rate, if any reading came in.
    pub fn smoothed(&self) -> Option<f32> {
        self.smoothed
    }

    /// The current target (from the smoothed rate).
    pub fn target(&self) -> Option<TempoTarget> {
        self.smoothed.map(target)
    }

    /// The target to re-rank for, when it is the first one or moved more than [`RERANK_STEP`] since
    /// the last re-rank (compared after clamping). Marks it as ranked.
    pub fn due(&mut self) -> Option<TempoTarget> {
        let t = self.target()?;
        let moved = self.ranked.is_none_or(|r| (t.bpm - r).abs() > RERANK_STEP);
        moved.then(|| {
            self.ranked = Some(t.bpm);
            t
        })
    }

    /// Forget the last re-rank (mode switched off/on): the next [`Self::due`] fires.
    pub fn reset_ranked(&mut self) {
        self.ranked = None;
    }
}

/// Candidate indices for a tempo-targeted fill: the songs that fit, closest first; when fewer than
/// `min_keep` fit, the closest of the rest fill up (songs without a known tempo last, in their order).
/// Feed the result into the flow ordering so the fill still mixes well.
pub fn fit_candidates(bpms: &[Option<f32>], target: &TempoTarget, min_keep: usize) -> Vec<usize> {
    let mut known: Vec<(f32, usize)> =
        bpms.iter().enumerate().filter_map(|(i, b)| b.map(|b| (target.distance(b), i))).collect();
    known.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let fitting = known.iter().take_while(|(d, _)| *d <= target.tol).count();
    let want = fitting.max(min_keep);
    let mut out: Vec<usize> = known.into_iter().take(want).map(|(_, i)| i).collect();
    if out.len() < want {
        out.extend(
            bpms.iter().enumerate().filter(|(_, b)| b.is_none()).map(|(i, _)| i).take(want - out.len()),
        );
    }
    out
}

/// The reason shown for a tempo pick.
pub fn why(picker: Picker, song_bpm: f32) -> Why {
    Why::new(picker, WhyCode::MatchesTempo { bpm: song_bpm })
}

/// What [`rerank_queue`] did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rerank {
    /// Automatic items taken out because their tempo does not fit (or is unknown).
    pub removed: Vec<u64>,
    /// Automatic items that stay, now ordered closest-tempo first.
    pub kept: usize,
}

/// Re-ranks the upcoming *automatic* items for `target`: the ones within [`KEEP_TOLERANCE`] stay, closest tempo first
/// (in the slots automatic items already held), the others (and automatic DJ bridges, whose neighbours
/// may change) are taken out for the next tempo-targeted fill to replace. Songs the user added are never
/// touched, nor is the current song. With `keep_next` the item right after the current one stays too
/// (the player has it loaded or a mix armed).
pub fn rerank_queue(
    q: &mut Queue,
    target: &TempoTarget,
    keep_next: bool,
    bpm_of: impl Fn(&Song) -> Option<f32>,
) -> Rerank {
    let Some(pos) = q.current_position() else { return Rerank::default() };
    let first = pos + 1 + usize::from(keep_next);
    let mut slots: Vec<usize> = Vec::new();
    let mut keep: Vec<(f32, usize, u64)> = Vec::new();
    let mut removed = Vec::new();
    for at in first..q.len() {
        let Some(it) = q.item_at(at) else { continue };
        if !it.auto || it.queued {
            continue;
        }
        match bpm_of(&it.song).map(|b| target.distance(b)) {
            Some(d) if it.bridge.is_none() && d <= KEEP_TOLERANCE.min(target.tol) => {
                slots.push(at);
                keep.push((d, at, it.uid));
            }
            _ => removed.push(it.uid),
        }
    }
    keep.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    // Place kept items into their slots, closest first, by swapping: an unplaced kept item always sits
    // at or after the slot being filled, and a swap leaves every other item (manual ones too) in place.
    for (&slot, &(_, _, uid)) in slots.iter().zip(&keep) {
        let Some(from) = (0..q.len()).find(|&i| q.item_at(i).is_some_and(|it| it.uid == uid)) else {
            continue;
        };
        if from > slot {
            q.move_item(from, slot);
            q.move_item(slot + 1, from);
        }
    }
    q.remove_many(&removed);
    Rerank { removed, kept: keep.len() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::song;

    #[test]
    fn target_clamps_and_counts_half_time() {
        assert_eq!(target(50.0).bpm, 70.0);
        assert_eq!(target(200.0).bpm, 180.0);
        assert_eq!(target(f32::NAN).bpm, 70.0);
        let t = target(170.0);
        assert!(t.half_double && t.fits(85.0) && t.fits(166.0) && !t.fits(120.0));
    }

    #[test]
    fn tracker_smooths_and_reranks_only_on_big_moves() {
        let mut tr = Tracker::new();
        assert!(tr.due().is_none(), "nothing before the first reading");
        tr.push(100.0, 0.0);
        assert_eq!(tr.due().map(|t| t.bpm), Some(100.0));
        // A jump to 150 moves the smoothed value only part way after 5 s (≈ 15 %).
        let s = tr.push(150.0, 5.0).unwrap();
        assert!((s - 107.7).abs() < 0.5, "{s}");
        assert!(tr.due().is_some(), "moved 7.7 > 5");
        let s2 = tr.push(150.0, 7.0).unwrap();
        assert!(s2 - s < 5.0);
        assert!(tr.due().is_none(), "small step: no re-rank");
        // After 2 minutes at 150 it is close to 150.
        let mut t = 7.0;
        while t < 127.0 {
            t += 5.0;
            tr.push(150.0, t);
        }
        assert!((tr.smoothed().unwrap() - 150.0).abs() < 3.0);
        assert!(tr.due().is_some());
        // Implausible readings are ignored.
        assert_eq!(tr.push(10.0, t + 5.0), tr.smoothed());
        tr.reset_ranked();
        assert!(tr.due().is_some());
    }

    #[test]
    fn fit_candidates_prefers_fitting_then_closest() {
        let t = target(120.0);
        let bpms = [Some(150.0), Some(61.0), None, Some(118.0), Some(100.0), Some(240.0)];
        // Fitting: 118 (2), 61 (×2 = 122 → 2), 240 (/2 = 120 → 0).
        assert_eq!(fit_candidates(&bpms, &t, 0), vec![5, 1, 3]);
        assert_eq!(fit_candidates(&bpms, &t, 5), vec![5, 1, 3, 4, 0]);
        assert_eq!(fit_candidates(&bpms, &t, 6), vec![5, 1, 3, 4, 0, 2]);
    }

    fn queue(spec: &[(&str, bool, bool)]) -> Queue {
        // (title, auto, queued); bpm is parsed from the title ("x120").
        let mut q = Queue::new();
        q.set(vec![song("now", "a", None, None)], 0);
        for (title, auto, queued) in spec {
            let s = song(title, "a", None, None);
            if *queued {
                q.enqueue_next(vec![s]);
            } else {
                q.add_last(vec![s], *auto);
            }
        }
        q
    }

    fn bpm(s: &Song) -> Option<f32> {
        s.title.split('x').nth(1).and_then(|b| b.parse().ok())
    }

    fn titles(q: &Queue) -> Vec<String> {
        (0..q.len()).filter_map(|i| q.item_at(i).map(|it| it.song.title.clone())).collect()
    }

    #[test]
    fn rerank_keeps_manual_items_and_current() {
        let mut q = queue(&[
            ("mine x90", false, true),
            ("a x100", true, false),
            ("b x153", true, false),
            ("user x60", false, false),
            ("c x75", true, false),
            ("d x148", true, false),
            ("e", true, false),
        ]);
        let before = titles(&q);
        assert_eq!(before[1], "mine x90");
        let r = rerank_queue(&mut q, &target(150.0), false, bpm);
        assert_eq!(r.kept, 3);
        assert_eq!(r.removed.len(), 2, "100 and unknown out");
        // 75 (×2 = 150), 148 and 153 are within 4 BPM; closest first into the automatic slots, manual items in place.
        assert_eq!(titles(&q), ["now", "mine x90", "c x75", "user x60", "d x148", "b x153"]);
        assert_eq!(q.current().unwrap().song.title, "now");
    }

    #[test]
    fn rerank_can_keep_the_loaded_next_item() {
        let mut q = queue(&[("a x100", true, false), ("b x150", true, false)]);
        let r = rerank_queue(&mut q, &target(150.0), true, bpm);
        assert!(r.removed.is_empty());
        assert_eq!(titles(&q), ["now", "a x100", "b x150"]);
        let r = rerank_queue(&mut q, &target(150.0), false, bpm);
        assert_eq!(r.removed.len(), 1);
        assert_eq!(titles(&q), ["now", "b x150"]);
    }

    /// The acceptance curve: 70 → 150 → 90 BPM; after each re-rank and fill the queue's tempos sit
    /// within 8 BPM of the target on average (half/double time counted).
    #[test]
    fn scripted_curve_queue_follows() {
        let library: Vec<f32> = (0..120).map(|i| 60.0 + (i as f32 * 37.0) % 130.0).collect();
        let mut tr = Tracker::new();
        let mut q = Queue::new();
        q.set(vec![song("now", "a", None, None)], 0);
        let mut t = 0.0;
        let mut reranks = 0;
        for &(rate, secs) in &[(70.0, 180.0), (150.0, 300.0), (90.0, 300.0)] {
            let end = t + secs;
            while t < end {
                t += 5.0;
                tr.push(rate, t);
                if let Some(target) = tr.due() {
                    reranks += 1;
                    rerank_queue(&mut q, &target, false, bpm);
                    // Fill back to 5 upcoming from the library with the target.
                    let bpms: Vec<Option<f32>> = library.iter().map(|b| Some(*b)).collect();
                    let need = 5 - q.upcoming().min(5);
                    let have: Vec<String> = titles(&q);
                    let picks: Vec<Song> = fit_candidates(&bpms, &target, need)
                        .into_iter()
                        .map(|i| song(&format!("s{i} x{}", library[i]), "a", None, None))
                        .filter(|s| !have.contains(&s.title))
                        .take(need)
                        .collect();
                    q.add_last(picks, true);
                }
            }
            let target = tr.target().unwrap();
            let ds: Vec<f32> =
                (1..q.len()).filter_map(|i| bpm(&q.item_at(i)?.song)).map(|b| target.distance(b)).collect();
            let mean = ds.iter().sum::<f32>() / ds.len() as f32;
            assert!(mean <= 8.0, "at {rate}: mean distance {mean} {ds:?}");
        }
        assert!(reranks < 40, "re-ranks only on > 5 BPM moves: {reranks}");
    }
}
