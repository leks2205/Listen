//! Tempo matching for the heart-rate / cadence mode (phone and desktop): turn a live rate (heart beats or
//! steps per minute) into a [`TempoTarget`], smooth it, decide when the queue is worth re-ranking, and pick
//! or re-rank songs by how well their tempo fits. Half and double time count (85 BPM fits 170).
//!
//! The fit is judged at the faster of the two pulses ([`fit_distance`]): `TempoTarget::distance` halves a
//! double-time song, so for a 70 BPM target it lets 124–156 BPM in (a 130 BPM song is "5 off"), while the
//! queue re-rank keeps only 132–148. Here a 70 target fits 62–78, 132–148 and 31–39 BPM.
//!
//! Evidence note: music does not measurably change heart rate during exercise (Terry et al. 2020);
//! matching step cadence has better support. This is "music that fits my effort", not training guidance.

use crate::pick::TempoTarget;
use listen_core::analysis::FlowFeatures;
use listen_core::model::Song;
use listen_core::queue::{Picker, Queue, Why, WhyCode};
use listen_core::versions::song_key;
use std::collections::HashSet;

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

/// How far `song_bpm` is from `target`, half and double time counted, measured at the faster pulse:
/// `min(|b − t|, |b − 2t|, |2b − t|)`. A song fits when this is within `target.tol`.
pub fn fit_distance(target: &TempoTarget, song_bpm: f32) -> f32 {
    let (b, t) = (song_bpm, target.bpm);
    let d = (b - t).abs();
    if target.half_double { d.min((b - 2.0 * t).abs()).min((2.0 * b - t).abs()) } else { d }
}

/// Whether `song_bpm` fits `target` ([`fit_distance`] within the tolerance).
pub fn fits(target: &TempoTarget, song_bpm: f32) -> bool {
    fit_distance(target, song_bpm) <= target.tol
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
        bpms.iter().enumerate().filter_map(|(i, b)| b.map(|b| (fit_distance(target, b), i))).collect();
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
        match bpm_of(&it.song).map(|b| fit_distance(target, b)) {
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

/// What [`tempo_fill`] chose.
#[derive(Debug, Clone, Default)]
pub struct TempoFill {
    /// Fitting pool songs (closest tempo first), then the store's fitting songs, then (only when both
    /// were short) the padding.
    pub songs: Vec<Song>,
    /// How many of `songs` fit the target.
    pub fitting: usize,
    /// How many of those came from the store.
    pub from_store: usize,
    /// How many songs that do not fit (or have no known tempo) were padded in.
    pub padded: usize,
}

impl TempoFill {
    /// The counts as JSON for logs and labs: `{"target", "want", "fitting", "from_store", "padded", "note"}`.
    pub fn stats(&self, target: &TempoTarget, want: usize) -> serde_json::Value {
        serde_json::json!({
            "target": target.bpm, "want": want, "fitting": self.fitting, "from_store": self.from_store,
            "padded": self.padded, "note": self.padded_note(target),
        })
    }

    /// The log line for a padded fill, `None` when every song fits.
    pub fn padded_note(&self, target: &TempoTarget) -> Option<String> {
        (self.padded > 0).then(|| {
            format!(
                "heart-rate tempo: only {} songs fit {:.0}, padded with {} nearest",
                self.fitting, target.bpm, self.padded
            )
        })
    }
}

/// Pool indices by [`fit_distance`]: the fitting ones (closest first) and the rest (closest first, then
/// unknown tempo in pool order).
fn split(bpms: &[Option<f32>], target: &TempoTarget) -> (Vec<usize>, Vec<usize>) {
    let mut known: Vec<(f32, usize)> =
        bpms.iter().enumerate().filter_map(|(i, b)| b.map(|b| (fit_distance(target, b), i))).collect();
    known.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let n = known.iter().take_while(|(d, _)| *d <= target.tol).count();
    let fit = known[..n].iter().map(|k| k.1).collect();
    let rest = known[n..]
        .iter()
        .map(|k| k.1)
        .chain(bpms.iter().enumerate().filter(|(_, b)| b.is_none()).map(|(i, _)| i))
        .collect();
    (fit, rest)
}

/// A store song's tempo: the confident beat grid, else the song's BPM tag.
pub fn store_bpm(song: &Song, f: &FlowFeatures) -> Option<f32> {
    flow_bpm(f).or(song.bpm.filter(|b| *b > 0).map(|b| b as f32))
}

/// The songs a tempo-targeted fill (Auto-DJ pick, AI DJ pool) chooses from: at least `want` when there
/// are enough anywhere.
///
/// The fetched pool rarely holds enough songs at the target tempo (a similar-songs pool of ~40 has few at
/// 62–78 or 132–148 BPM), so a short list is topped up first with fitting songs from the analysis store
/// (closest to the current song's sound, then closest tempo); only when the store has too few as well are
/// the closest misfits padded in, and [`TempoFill::padded_note`] says so.
///
/// `pool`: the fetched candidates (already gated by the caller), `bpm_of` their tempo. `store` (only
/// called when the pool is short): analysed songs from the store and the history (connected servers only,
/// any order, may repeat pool songs); those whose [`song_key`] (or `ytm:` record id, or plain id) is in
/// `skip` (excluded, blocked, queued, played, worn out) are never taken, nor a second copy of a song.
/// `here`: the current song's analysis, for the sound ranking. When at least `want` pool songs fit, every
/// fitting pool song comes back (closest first).
pub fn tempo_fill(
    pool: &[Song],
    bpm_of: impl Fn(&Song) -> Option<f32>,
    store: impl FnOnce() -> Vec<(Song, FlowFeatures)>,
    skip: &HashSet<String>,
    target: &TempoTarget,
    want: usize,
    here: Option<&FlowFeatures>,
) -> TempoFill {
    use crate::score::{TimbreStats, Weights, flow_terms};
    let bpms: Vec<Option<f32>> = pool.iter().map(&bpm_of).collect();
    let (fit, rest) = split(&bpms, target);
    let mut out = TempoFill {
        songs: fit.iter().map(|&i| pool[i].clone()).collect(),
        fitting: fit.len(),
        ..Default::default()
    };
    if out.songs.len() >= want {
        return out;
    }
    // Top up from the store: fitting songs not in the pool and not skipped, one per song key.
    let mut seen: HashSet<String> = pool.iter().map(song_key).collect();
    let mut extra: Vec<(Song, FlowFeatures, f32)> = Vec::new();
    for (song, f) in store() {
        let Some(d) = store_bpm(&song, &f).map(|b| fit_distance(target, b)).filter(|d| *d <= target.tol)
        else {
            continue;
        };
        let key = song_key(&song);
        let ytm_id = format!("ytm:{}", song.id);
        if skip.contains(&key) || skip.contains(&ytm_id) || skip.contains(&song.id) || !seen.insert(key) {
            continue;
        }
        extra.push((song, f, d));
    }
    // Closest to the current song's sound (energy and timbre, as the flow scorer weighs them), then tempo.
    let w = Weights::default();
    let stats = TimbreStats::from_vectors(
        here.into_iter().chain(extra.iter().map(|e| &e.1)).map(|f| f.timbre.as_slice()),
    );
    let mut scored: Vec<(f32, f32, usize)> = extra
        .iter()
        .enumerate()
        .map(|(i, (_, f, d))| {
            let sound = here.map_or(0.0, |h| {
                let t = flow_terms(h, f, &[], stats.as_ref());
                w.energy * t[2] + w.timbre * t[3]
            });
            (sound, *d, i)
        })
        .collect();
    scored.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
    let room = want - out.songs.len();
    let taken = scored.len().min(room);
    out.songs.extend(scored.iter().take(room).map(|&(_, _, i)| extra[i].0.clone()));
    out.fitting += taken;
    out.from_store = taken;
    if out.songs.len() >= want {
        return out;
    }
    // The store is short too: the closest misfits (then unknown tempo) so the DJ never runs dry.
    let room = want - out.songs.len();
    out.songs.extend(rest.iter().take(room).map(|&i| pool[i].clone()));
    out.padded = room.min(rest.len());
    out
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
            let ds: Vec<f32> = (1..q.len())
                .filter_map(|i| bpm(&q.item_at(i)?.song))
                .map(|b| fit_distance(&target, b))
                .collect();
            let mean = ds.iter().sum::<f32>() / ds.len() as f32;
            assert!(mean <= 8.0, "at {rate}: mean distance {mean} {ds:?}");
        }
        assert!(reranks < 40, "re-ranks only on > 5 BPM moves: {reranks}");
    }

    // ---- tempo_fill (the fixed fit rule and the store top-up) ----

    fn fsong(id: &str, bpm: Option<u32>) -> Song {
        let mut s = song(&format!("Song {id}"), &format!("Artist {id}"), None, None);
        s.id = id.into();
        s.bpm = bpm;
        s
    }

    fn flow(bpm: f32, energy: f32) -> FlowFeatures {
        FlowFeatures {
            bpm: Some(bpm),
            bpm_confidence: 0.9,
            key: None,
            lufs: -10.0,
            mood: listen_core::analysis::Mood { energy, danceability: 0.5, brightness: 0.5 },
            timbre: vec![energy, 0.5, 0.2],
            mixable_in: true,
            mixable_out: true,
            vocal: None,
        }
    }

    fn tag(s: &Song) -> Option<f32> {
        s.bpm.map(|b| b as f32)
    }

    /// The pool of the phone's bug: 2 songs fit 70 BPM, 10 sit at 121–130 (outside 62–78 and 132–148).
    fn bug_pool() -> Vec<Song> {
        let mut p = vec![fsong("fit1", Some(72)), fsong("fit2", Some(140))];
        p.extend((0..10).map(|i| fsong(&format!("miss{i}"), Some(121 + i))));
        p
    }

    #[test]
    fn fit_is_judged_at_the_faster_tempo() {
        let t = target(70.0);
        // The songs of the bug report: 130 and 128 BPM do not fit 70 (they are 10 and 12 off 140).
        assert!(!fits(&t, 130.0) && !fits(&t, 128.0));
        assert!(t.fits(130.0), "TempoTarget::distance halves 130 to 65 and lets it in");
        for b in [62.0, 78.0, 132.0, 148.0, 31.0, 39.0] {
            assert!(fits(&t, b), "{b}");
        }
        for b in [61.0, 79.0, 131.0, 149.0, 30.0, 40.0, 100.0] {
            assert!(!fits(&t, b), "{b}");
        }
        // Target 150: half time 71–79 (142–158 doubled), double time 292–308.
        let t = target(150.0);
        assert!(fits(&t, 75.0) && fits(&t, 300.0) && !fits(&t, 70.0) && !fits(&t, 140.0));
        // Without half/double time only the plain distance counts.
        let strict = TempoTarget { half_double: false, ..target(70.0) };
        assert!(!fits(&strict, 140.0) && fits(&strict, 75.0));
    }

    #[test]
    fn the_rerank_and_the_fill_agree_on_130_and_128_at_70() {
        let mut q = queue(&[("a x130", true, false), ("b x128", true, false), ("c x140", true, false)]);
        let r = rerank_queue(&mut q, &target(70.0), false, bpm);
        assert_eq!(r.removed.len(), 2);
        assert_eq!(titles(&q), ["now", "c x140"]);
        let bpms = [Some(130.0), Some(128.0), Some(140.0)];
        assert_eq!(fit_candidates(&bpms, &target(70.0), 0), vec![2], "130 and 128 do not fit");
    }

    #[test]
    fn short_pool_is_topped_up_from_the_store_with_fitting_songs_only() {
        let t = target(70.0);
        let store: Vec<(Song, FlowFeatures)> = (0..15)
            .map(|i| (fsong(&format!("st{i}"), None), flow(64.0 + i as f32, 0.3 + i as f32 * 0.03)))
            .collect();
        let bpm_of =
            |s: &Song| tag(s).or_else(|| store.iter().find(|(x, _)| x.id == s.id).and_then(|(_, f)| f.bpm));
        let here = flow(70.0, 0.3);
        let r = tempo_fill(&bug_pool(), bpm_of, || store.clone(), &HashSet::new(), &t, 12, Some(&here));
        assert_eq!(r.songs.len(), 12);
        assert_eq!((r.fitting, r.from_store, r.padded), (12, 10, 0));
        assert!(r.songs.iter().all(|s| fits(&t, bpm_of(s).unwrap())));
        assert!(r.padded_note(&t).is_none());
        // Closest sound first: the far ones (energy 0.6–0.72) stay out.
        let ids: Vec<&str> = r.songs.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"st0") && ids.contains(&"st1"), "{ids:?}");
        assert!(!ids.contains(&"st14") && !ids.contains(&"st13"), "{ids:?}");
        assert_eq!(r.stats(&t, 12)["from_store"], 10);
    }

    #[test]
    fn empty_store_pads_with_the_nearest_misfits_and_says_so() {
        let t = target(70.0);
        let r = tempo_fill(&bug_pool(), tag, Vec::new, &HashSet::new(), &t, 12, None);
        assert_eq!(r.songs.len(), 12);
        assert_eq!((r.fitting, r.from_store, r.padded), (2, 0, 10));
        assert_eq!(
            r.padded_note(&t).unwrap(),
            "heart-rate tempo: only 2 songs fit 70, padded with 10 nearest"
        );
        // Fitting songs lead; the padding follows, closest (130 → 10 off 140) first.
        assert_eq!(r.songs[0].id, "fit2");
        assert_eq!(r.songs[1].id, "fit1");
        assert_eq!(r.songs[2].id, "miss9");
    }

    #[test]
    fn excluded_queued_and_pool_songs_never_come_from_the_store() {
        let t = target(70.0);
        let store: Vec<(Song, FlowFeatures)> = vec![
            (fsong("snoozed", None), flow(70.0, 0.5)),
            (fsong("queued", None), flow(70.0, 0.5)),
            (fsong("ytmplayed", None), flow(70.0, 0.5)),
            (fsong("fit1", Some(72)), flow(72.0, 0.5)), // already in the pool
            (fsong("ok", None), flow(71.0, 0.5)),
            (fsong("ok", None), flow(71.0, 0.5)), // the same song twice
            (fsong("misfit", None), flow(100.0, 0.5)),
        ];
        let skip: HashSet<String> = [
            song_key(&fsong("snoozed", None)),
            song_key(&fsong("queued", None)),
            "ytm:ytmplayed".to_string(),
        ]
        .into();
        let r = tempo_fill(&bug_pool(), tag, || store, &skip, &t, 12, None);
        let ids: Vec<&str> = r.songs.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(&ids[..3], ["fit2", "fit1", "ok"]);
        assert_eq!(ids.iter().filter(|i| **i == "fit1").count(), 1);
        assert_eq!(ids.iter().filter(|i| **i == "ok").count(), 1);
        for bad in ["snoozed", "queued", "ytmplayed", "misfit"] {
            assert!(!ids.contains(&bad), "{bad} in {ids:?}");
        }
        assert_eq!((r.fitting, r.from_store, r.padded), (3, 1, 9));
    }

    #[test]
    fn half_and_double_time_fit_in_the_fill() {
        let t = target(150.0);
        let store: Vec<(Song, FlowFeatures)> = vec![
            (fsong("half", None), flow(75.0, 0.5)),
            (fsong("double", None), flow(300.0, 0.5)),
            (fsong("no", None), flow(100.0, 0.5)),
        ];
        let r = tempo_fill(&[], tag, || store, &HashSet::new(), &t, 5, None);
        let ids: Vec<&str> = r.songs.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids.len(), 2, "store misfits never pad: {ids:?}");
        assert!(ids.contains(&"half") && ids.contains(&"double"));
        assert_eq!((r.fitting, r.padded), (2, 0));
    }

    #[test]
    fn enough_fitting_pool_songs_stay_as_before_and_the_store_is_not_asked() {
        let t = target(120.0);
        let p: Vec<Song> = (0..20).map(|i| fsong(&format!("p{i}"), Some(112 + i))).collect();
        let r = tempo_fill(&p, tag, || panic!("store asked"), &HashSet::new(), &t, 5, None);
        assert_eq!(r.songs.len(), 17, "every fitting pool song (112–128)");
        assert_eq!((r.from_store, r.padded), (0, 0));
        assert_eq!(r.songs[0].id, "p8");
    }

    #[test]
    fn a_store_song_without_a_confident_grid_uses_its_bpm_tag() {
        let mut f = flow(100.0, 0.5);
        f.bpm_confidence = 0.1;
        assert_eq!(store_bpm(&fsong("x", Some(140)), &f), Some(140.0));
        assert_eq!(store_bpm(&fsong("x", None), &f), None);
        f.bpm_confidence = 0.9;
        assert_eq!(store_bpm(&fsong("x", Some(140)), &f), Some(100.0));
    }
}
