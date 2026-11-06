//! Flow weights learnt from the listener's own skips (item 16).
//!
//! Every automatic handover in the history (a song played, the next one picked by the player, on the
//! same device, right after it) becomes a labelled transition: skipped within 30 s or not. A logistic
//! regression of that label on the scorer's cost terms tells how much each term matters to this
//! listener. The fitted weights are blended with the hand-set defaults until 300 labelled transitions
//! exist, never flip a term's sign before that, and are clamped to a sane range afterwards.

use std::collections::HashMap;

use listen_core::analysis::FlowFeatures;
use listen_core::history::{Entry, Picker, PlayStatus};
use listen_core::model::Song;
use serde::{Deserialize, Serialize};

use crate::score::{TimbreStats, Weights, flow_terms, same_artist};

/// A skip this early after a handover counts against the transition.
pub const SKIP_WITHIN_S: f32 = 30.0;
/// Labelled transitions at which the learnt weights fully replace the defaults (and may change sign).
pub const FULL_TRUST: usize = 300;
/// Below this, the defaults are used as they are.
pub const MIN_LABELS: usize = 30;
/// Both outcomes need at least this many examples to fit anything.
const MIN_EACH: usize = 5;
/// Slack for the gap between two entries of one handover (loading, a DJ break).
const GAP_SLACK_S: f32 = 120.0;
/// Ridge strength on the standardised coefficients.
const RIDGE: f64 = 1.0;
const FOLDS: usize = 5;
/// A yes/no term needs this many transitions with and without it to be learnt.
const MIN_TERM_CASES: usize = 10;

/// The terms of one transition, in [`COMPONENTS`] order.
pub const COMPONENTS: [&str; 7] =
    ["tempo", "key", "energy", "timbre", "mixability", "artist_repeat", "album_repeat"];

/// One labelled handover.
#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    /// Unweighted cost terms ([`COMPONENTS`]).
    pub x: [f32; 7],
    /// Skipped within [`SKIP_WITHIN_S`].
    pub skipped: bool,
}

/// The weights of the [`COMPONENTS`] (the scorer's flow terms and repeat penalties).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TermWeights {
    pub tempo: f32,
    pub key: f32,
    pub energy: f32,
    pub timbre: f32,
    pub mixability: f32,
    pub artist_repeat: f32,
    pub album_repeat: f32,
}

impl TermWeights {
    pub fn of(w: &Weights) -> Self {
        Self {
            tempo: w.tempo,
            key: w.key,
            energy: w.energy,
            timbre: w.timbre,
            mixability: w.mixability,
            artist_repeat: w.artist_repeat,
            album_repeat: w.album_repeat,
        }
    }

    fn to_array(self) -> [f32; 7] {
        [
            self.tempo,
            self.key,
            self.energy,
            self.timbre,
            self.mixability,
            self.artist_repeat,
            self.album_repeat,
        ]
    }

    fn from_array(a: [f32; 7]) -> Self {
        Self {
            tempo: a[0],
            key: a[1],
            energy: a[2],
            timbre: a[3],
            mixability: a[4],
            artist_repeat: a[5],
            album_repeat: a[6],
        }
    }

    /// `base` with these weights for the flow terms and repeats (the unanalysed penalty stays).
    pub fn apply(&self, base: &Weights) -> Weights {
        Weights {
            tempo: self.tempo,
            key: self.key,
            energy: self.energy,
            timbre: self.timbre,
            mixability: self.mixability,
            artist_repeat: self.artist_repeat,
            album_repeat: self.album_repeat,
            ..base.clone()
        }
    }
}

/// What `flow-weights.json` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Learnt {
    /// Unix seconds of the fit.
    pub at: i64,
    /// Labelled transitions and how many of them were skipped.
    pub labels: usize,
    pub skips: usize,
    /// Share of the fit in [`Learnt::weights`] (labels / 300, 0 below 30 labels).
    pub trust: f32,
    /// Cross-validated AUC of the fitted model (5 folds) and of today's default cost, on the same
    /// transitions; `None` with too few labels of either kind.
    pub auc_cv: Option<f32>,
    pub auc_default: Option<f32>,
    /// The fitted weights alone (clamped), `None` when nothing could be fitted.
    pub fitted: Option<TermWeights>,
    /// What the scorer uses: defaults blended with the fit by `trust`.
    pub weights: TermWeights,
}

impl Learnt {
    /// The scorer weights with the learnt terms.
    pub fn scorer(&self) -> Weights {
        self.weights.apply(&Weights::default())
    }

    /// The fit carries something learnt (at least 30 labelled handovers). Only such a fit goes to the
    /// other devices, so one without analysed handovers never pushes the defaults over a learnt fit.
    pub fn worth_sharing(&self) -> bool {
        self.trust > 0.0
    }

    /// Whether this fit takes the place of `current` (a local refit or one from another device):
    /// the newest fit wins, but a fit with nothing learnt only replaces another such fit.
    pub fn replaces(&self, current: Option<&Learnt>) -> bool {
        match current {
            None => true,
            Some(c) if !self.worth_sharing() => !c.worth_sharing(),
            Some(c) => !c.worth_sharing() || self.at > c.at,
        }
    }
}

fn automatic(e: &Entry) -> bool {
    e.ctx
        .as_ref()
        .is_some_and(|c| c.picker.automatic() && !matches!(c.picker, Picker::Request | Picker::Other))
}

fn device(e: &Entry) -> &str {
    e.ctx.as_ref().map_or("", |c| c.device.as_str())
}

/// Seconds the entry's song had played when it was written (plays from about 0.41 of the song on).
fn elapsed(e: &Entry) -> f32 {
    e.at_s.unwrap_or(e.duration_secs as f32 * 0.5)
}

/// The labelled handovers of a history (any order; entries of every device). `flow` looks up the
/// analysis of a song; transitions with an unanalysed side are left out.
pub fn transitions(entries: &[Entry], flow: impl Fn(&Song) -> Option<FlowFeatures>) -> Vec<Transition> {
    let mut idx: Vec<usize> = (0..entries.len()).collect();
    idx.sort_by_key(|&i| entries[i].ts);
    let mut feats: HashMap<(String, String), Option<FlowFeatures>> = HashMap::new();
    let mut feat = |e: &Entry| -> Option<FlowFeatures> {
        feats.entry((e.server_id.0.clone(), e.song_id.clone())).or_insert_with(|| flow(&e.to_song())).clone()
    };
    // Per device: the previous entry and the energies of the songs before it in this session.
    let mut last: HashMap<&str, (usize, Vec<f32>)> = HashMap::new();
    let mut raw: Vec<(FlowFeatures, FlowFeatures, Vec<f32>, bool, bool, bool)> = Vec::new();
    for &i in &idx {
        let next = &entries[i];
        let dev = device(next);
        let next_f = feat(next);
        let mut history = Vec::new();
        if let Some((p, energies)) = last.get(dev) {
            let prev = &entries[*p];
            let remaining = (prev.duration_secs as f32 - elapsed(prev)).max(0.0);
            let gap = (next.ts - prev.ts) as f32;
            let joined = gap >= 0.0 && gap <= remaining + elapsed(next) + GAP_SLACK_S;
            if joined {
                history = energies.clone();
                if prev.status == PlayStatus::Played
                    && automatic(next)
                    && let (Some(a), Some(b)) = (feat(prev), next_f.clone())
                {
                    let skipped = next.status == PlayStatus::Skipped && elapsed(next) <= SKIP_WITHIN_S;
                    let (ps, ns) = (prev.to_song(), next.to_song());
                    let album = prev.album_id.is_some() && prev.album_id == next.album_id;
                    raw.push((a, b, energies.clone(), same_artist(&ps, &ns), album, skipped));
                }
                if let Some(f) = feat(prev) {
                    history.push(f.mood.energy);
                }
                let keep = history.len().saturating_sub(4);
                history.drain(..keep);
            }
        }
        last.insert(dev, (i, history));
    }
    let stats =
        TimbreStats::from_vectors(raw.iter().flat_map(|r| [r.0.timbre.as_slice(), r.1.timbre.as_slice()]));
    raw.into_iter()
        .map(|(a, b, hist, artist, album, skipped)| {
            let t = flow_terms(&a, &b, &hist, stats.as_ref());
            Transition {
                x: [t[0], t[1], t[2], t[3], t[4], f32::from(u8::from(artist)), f32::from(u8::from(album))],
                skipped,
            }
        })
        .collect()
}

/// Ridge logistic regression (Newton steps) on standardised inputs; returns (intercept, coefficients)
/// on the raw scale.
fn fit_logistic(data: &[Transition]) -> (f64, [f64; 7]) {
    let n = data.len().max(1) as f64;
    let mut mean = [0f64; 7];
    let mut sd = [0f64; 7];
    for t in data {
        for (m, x) in mean.iter_mut().zip(t.x) {
            *m += f64::from(x) / n;
        }
    }
    for t in data {
        for ((v, x), m) in sd.iter_mut().zip(t.x).zip(mean) {
            *v += (f64::from(x) - m).powi(2) / n;
        }
    }
    let sd: Vec<f64> = sd.iter().map(|v| v.sqrt()).collect();
    let z = |t: &Transition, j: usize| if sd[j] > 1e-9 { (f64::from(t.x[j]) - mean[j]) / sd[j] } else { 0.0 };
    const P: usize = 8;
    let mut beta = [0f64; P];
    for _ in 0..30 {
        let mut grad = [0f64; P];
        let mut hess = [[0f64; P]; P];
        for t in data {
            let mut row = [0f64; P];
            row[0] = 1.0;
            for j in 0..7 {
                row[j + 1] = z(t, j);
            }
            let eta: f64 = row.iter().zip(&beta).map(|(a, b)| a * b).sum();
            let p = 1.0 / (1.0 + (-eta).exp());
            let y = if t.skipped { 1.0 } else { 0.0 };
            let w = (p * (1.0 - p)).max(1e-9);
            for a in 0..P {
                grad[a] += (y - p) * row[a];
                for b in 0..P {
                    hess[a][b] += w * row[a] * row[b];
                }
            }
        }
        for a in 1..P {
            grad[a] -= RIDGE * beta[a];
            hess[a][a] += RIDGE;
        }
        hess[0][0] += 1e-9;
        let Some(step) = solve(hess, grad) else { break };
        let mut moved = 0f64;
        for a in 0..P {
            beta[a] += step[a];
            moved = moved.max(step[a].abs());
        }
        if moved < 1e-8 {
            break;
        }
    }
    let mut coef = [0f64; 7];
    let mut intercept = beta[0];
    for j in 0..7 {
        if sd[j] > 1e-9 {
            coef[j] = beta[j + 1] / sd[j];
            intercept -= coef[j] * mean[j];
        }
    }
    (intercept, coef)
}

/// Gaussian elimination with partial pivoting.
fn solve<const P: usize>(mut a: [[f64; P]; P], mut b: [f64; P]) -> Option<[f64; P]> {
    for c in 0..P {
        let piv = (c..P).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[piv][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, piv);
        b.swap(c, piv);
        let pivot_row = a[c];
        for r in c + 1..P {
            let f = a[r][c] / pivot_row[c];
            for (x, p) in a[r][c..].iter_mut().zip(&pivot_row[c..]) {
                *x -= f * p;
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = [0f64; P];
    for r in (0..P).rev() {
        let s: f64 = (r + 1..P).map(|k| a[r][k] * x[k]).sum();
        x[r] = (b[r] - s) / a[r][r];
    }
    Some(x)
}

/// Terms that vary enough in `data` to be learnt; the others keep their default weight (a history
/// without same-artist handovers says nothing about the artist repeat penalty).
fn informative(data: &[Transition]) -> [bool; 7] {
    std::array::from_fn(|j| {
        if j >= 5 {
            // Yes/no terms: enough of both.
            let on = data.iter().filter(|t| t.x[j] > 0.5).count();
            on >= MIN_TERM_CASES && data.len() - on >= MIN_TERM_CASES
        } else {
            let n = data.len().max(1) as f32;
            let m = data.iter().map(|t| t.x[j]).sum::<f32>() / n;
            let var = data.iter().map(|t| (t.x[j] - m).powi(2)).sum::<f32>() / n;
            var.sqrt() > 0.05
        }
    })
}

/// Area under the ROC curve of `score` (higher = more likely skipped); ties count half.
pub fn auc(scored: &[(f64, bool)]) -> Option<f32> {
    let pos = scored.iter().filter(|s| s.1).count();
    let neg = scored.len() - pos;
    if pos == 0 || neg == 0 {
        return None;
    }
    let mut s: Vec<(f64, bool)> = scored.to_vec();
    s.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Rank sum of the positives with average ranks for ties.
    let mut rank_sum = 0f64;
    let mut i = 0;
    while i < s.len() {
        let mut j = i;
        while j + 1 < s.len() && s[j + 1].0 == s[i].0 {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        rank_sum += avg * s[i..=j].iter().filter(|x| x.1).count() as f64;
        i = j + 1;
    }
    let u = rank_sum - (pos * (pos + 1)) as f64 / 2.0;
    Some((u / (pos * neg) as f64) as f32)
}

fn dot(c: &[f64; 7], x: &[f32; 7]) -> f64 {
    c.iter().zip(x).map(|(a, b)| a * f64::from(*b)).sum()
}

/// Fits the weights to `data` (from [`transitions`]); `at` is stamped into the result.
pub fn learn(data: &[Transition], at: i64) -> Learnt {
    let defaults = TermWeights::of(&Weights::default());
    let d = defaults.to_array();
    let labels = data.len();
    let skips = data.iter().filter(|t| t.skipped).count();
    let d64: [f64; 7] = d.map(f64::from);
    let auc_default = auc(&data.iter().map(|t| (dot(&d64, &t.x), t.skipped)).collect::<Vec<_>>());
    let fittable = labels >= MIN_LABELS && skips >= MIN_EACH && labels - skips >= MIN_EACH;
    let auc_cv = fittable
        .then(|| {
            let mut scored = Vec::with_capacity(labels);
            for k in 0..FOLDS {
                let train: Vec<Transition> =
                    data.iter().enumerate().filter(|(i, _)| i % FOLDS != k).map(|(_, t)| t.clone()).collect();
                let (b0, c) = fit_logistic(&train);
                scored.extend(
                    data.iter()
                        .enumerate()
                        .filter(|(i, _)| i % FOLDS == k)
                        .map(|(_, t)| (b0 + dot(&c, &t.x), t.skipped)),
                );
            }
            auc(&scored)
        })
        .flatten();
    let fitted = fittable
        .then(|| {
            let (_, c) = fit_logistic(data);
            let info = informative(data);
            // Same mean cost as the defaults over the terms the data says something about, so they stay
            // comparable with the others and with what the scorer adds.
            let mean: Vec<f64> =
                (0..7).map(|j| data.iter().map(|t| f64::from(t.x[j])).sum::<f64>() / labels as f64).collect();
            let fit_mean: f64 = (0..7).filter(|&j| info[j]).map(|j| c[j] * mean[j]).sum();
            let def_mean: f64 = (0..7).filter(|&j| info[j]).map(|j| d64[j] * mean[j]).sum();
            // A fit in which higher cost does not mean more skips says nothing about the weights.
            (fit_mean > 1e-9).then(|| {
                let k = def_mean / fit_mean;
                let floor = |j: usize| if labels >= FULL_TRUST { -d[j] } else { 0.0 };
                TermWeights::from_array(std::array::from_fn(|j| {
                    if info[j] { ((k * c[j]) as f32).clamp(floor(j), 3.0 * d[j]) } else { d[j] }
                }))
            })
        })
        .flatten();
    let trust = match fitted {
        Some(_) => (labels as f32 / FULL_TRUST as f32).min(1.0),
        None => 0.0,
    };
    let weights = match fitted {
        Some(f) => {
            let f = f.to_array();
            TermWeights::from_array(std::array::from_fn(|j| (1.0 - trust) * d[j] + trust * f[j]))
        }
        None => defaults,
    };
    Learnt { at, labels, skips, trust, auc_cv, auc_default, fitted, weights }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::analysis::{Camelot, Mood};
    use listen_core::history::{PlayCtx, PlayMode};
    use listen_core::model::{ArtistRef, ServerId, SourceKind};

    /// A deterministic pseudo-random stream.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f32) / (1u64 << 31) as f32
        }
    }

    fn synthetic(n: usize, seed: u64, key_matters: bool) -> Vec<Transition> {
        let mut r = Rng(seed);
        (0..n)
            .map(|_| {
                let x = [r.next() * 2.0, r.next() * 1.5, r.next() * 2.0, r.next() * 2.0, r.next(), 0.0, 0.0];
                // Skips follow the tempo gap (and the key clash if asked), plus noise.
                let logit =
                    -2.0 + 1.6 * x[0] + if key_matters { 2.0 * x[1] } else { 0.0 } + (r.next() - 0.5) * 2.0;
                Transition { x, skipped: r.next() < 1.0 / (1.0 + (-logit).exp()) }
            })
            .collect()
    }

    #[test]
    fn few_labels_keep_the_defaults() {
        let l = learn(&synthetic(20, 1, true), 0);
        assert_eq!(l.weights, TermWeights::of(&Weights::default()));
        assert_eq!((l.trust, l.fitted), (0.0, None));
        assert_eq!(l.scorer(), Weights::default(), "the scorer then works exactly as today");
    }

    #[test]
    fn the_fit_finds_the_terms_that_predict_skips() {
        let data = synthetic(600, 7, true);
        let l = learn(&data, 0);
        assert_eq!(l.trust, 1.0);
        let f = l.fitted.unwrap();
        assert!(f.key > f.energy && f.key > f.timbre, "key matters here: {f:?}");
        assert!(f.tempo > f.energy, "{f:?}");
        let (cv, def) = (l.auc_cv.unwrap(), l.auc_default.unwrap());
        assert!(cv > 0.75, "cv auc {cv}");
        assert!(cv >= def - 0.02, "the fit predicts at least as well as the defaults ({cv} vs {def})");
    }

    #[test]
    fn signs_do_not_flip_before_300_labels() {
        // Here a key clash goes with fewer skips: the fit would want a negative key weight.
        let mut data = synthetic(200, 3, false);
        let mut r = Rng(11);
        for t in &mut data {
            t.skipped = t.x[1] < 0.5 && r.next() < 0.6 || t.x[0] > 1.6;
        }
        let l = learn(&data, 0);
        assert!(l.trust > 0.0 && l.trust < 1.0);
        for (w, name) in l.weights.to_array().iter().zip(COMPONENTS) {
            assert!(*w >= 0.0, "{name} flipped with {} labels", l.labels);
        }
        // With 300+ the same pattern may turn the key weight negative.
        let mut more = synthetic(900, 3, false);
        for t in &mut more {
            t.skipped = t.x[1] < 0.5 && r.next() < 0.6 || t.x[0] > 1.6;
        }
        let l = learn(&more, 0);
        assert!(l.weights.key < 0.0, "{:?}", l.weights);
        assert!(l.weights.key >= -Weights::default().key, "clamped");
    }

    #[test]
    fn terms_the_history_says_nothing_about_keep_their_default() {
        // No same-artist or same-album handover at all (x[5], x[6] always 0).
        let l = learn(&synthetic(600, 5, true), 0);
        let (f, d) = (l.fitted.unwrap(), Weights::default());
        assert_eq!((f.artist_repeat, f.album_repeat), (d.artist_repeat, d.album_repeat));
    }

    #[test]
    fn auc_counts_ties_half() {
        assert_eq!(auc(&[(1.0, true), (0.0, false)]), Some(1.0));
        assert_eq!(auc(&[(0.5, true), (0.5, false)]), Some(0.5));
        assert_eq!(auc(&[(0.0, true), (1.0, false)]), Some(0.0));
        assert_eq!(auc(&[(0.0, false)]), None);
    }

    fn flow(bpm: f32, key: u8) -> FlowFeatures {
        FlowFeatures {
            bpm: Some(bpm),
            bpm_confidence: 1.0,
            key: Some((Camelot { number: key, minor: false }, 1.0)),
            lufs: -9.0,
            mood: Mood { energy: 0.5, danceability: 0.5, brightness: 0.5 },
            timbre: vec![0.0; 4],
            mixable_in: true,
            mixable_out: true,
            vocal: None,
        }
    }

    fn entry(id: &str, ts: i64, status: PlayStatus, at_s: f32, picker: Picker, device: &str) -> Entry {
        Entry {
            ts,
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            song_id: id.into(),
            status,
            title: id.into(),
            artists: vec![ArtistRef { id: None, name: format!("Band {id}") }],
            album: None,
            album_id: None,
            image: None,
            duration_secs: 200,
            at_s: Some(at_s),
            frac: Some(at_s / 200.0),
            ctx: Some(PlayCtx { mode: PlayMode::AutoDj, picker, device: device.into() }),
        }
    }

    #[test]
    fn handovers_are_labelled_from_the_history() {
        use PlayStatus::{Played, Skipped};
        let h = vec![
            entry("a", 1000, Played, 90.0, Picker::User, "pc"),
            // Next song (auto) skipped 10 s in: 110 s left of a + 10 s.
            entry("b", 1120, Skipped, 10.0, Picker::AutoDj, "pc"),
            // After a skip: not a handover.
            entry("c", 1220, Played, 90.0, Picker::AutoDj, "pc"),
            // Played through after a handover: label "not skipped".
            entry("d", 1420, Played, 90.0, Picker::AiDj, "pc"),
            // Picked by hand: not the scorer's choice.
            entry("e", 1620, Played, 90.0, Picker::User, "pc"),
            // Skipped late (60 s): not an early skip.
            entry("f", 1790, Skipped, 60.0, Picker::Radio, "pc"),
            // Another device at the same time does not join the pc's chain.
            entry("x", 1121, Played, 90.0, Picker::AutoDj, "phone"),
            // A day later: no handover.
            entry("g", 90_000, Played, 90.0, Picker::AutoDj, "pc"),
            // Unanalysed next song: left out.
            entry("u", 90_200, Played, 90.0, Picker::AutoDj, "pc"),
        ];
        let t = transitions(&h, |s| (s.id != "u").then(|| flow(100.0, 8)));
        assert_eq!(t.iter().map(|t| t.skipped).collect::<Vec<_>>(), [true, false, false], "a→b, c→d, e→f");
        // Order of the file does not matter.
        let mut rev = h.clone();
        rev.reverse();
        assert_eq!(transitions(&rev, |s| (s.id != "u").then(|| flow(100.0, 8))).len(), 3);
        // Tempo gap shows in the terms.
        let t = transitions(&h[..2], |s| Some(flow(if s.id == "a" { 100.0 } else { 120.0 }, 8)));
        assert!(t[0].x[0] > 1.0, "{:?}", t[0].x);
    }

    #[test]
    fn the_newest_fit_wins_but_an_empty_one_never_replaces_a_learnt_one() {
        let fit = |at, trust| Learnt { at, trust, ..learn(&[], at) };
        let desktop = fit(100, 0.5);
        let phone_newer = fit(200, 0.2);
        let phone_empty = fit(300, 0.0);
        assert!(desktop.replaces(None));
        assert!(phone_newer.replaces(Some(&desktop)), "newest fit wins");
        assert!(!desktop.replaces(Some(&phone_newer)), "an older fit does not");
        assert!(!desktop.replaces(Some(&desktop)), "the same fit changes nothing");
        assert!(!phone_empty.replaces(Some(&desktop)), "nothing learnt never overwrites a fit");
        assert!(desktop.replaces(Some(&phone_empty)), "an older learnt fit replaces an empty one");
        assert!(fit(400, 0.0).replaces(Some(&phone_empty)), "empty refits replace each other");
        assert!(!phone_empty.worth_sharing() && desktop.worth_sharing());
    }
}
