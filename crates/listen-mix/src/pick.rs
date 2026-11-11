//! What an automatic fill aims for beyond a smooth flow: a tempo (heart rate, cadence), focus, and a
//! slot for a forgotten favourite. One [`Targets`] value goes into every automatic picker (Auto-DJ, the AI
//! DJ candidate filter, [`crate::score::order`]); `Targets::default()` means "pick as before".

use listen_core::analysis::FlowFeatures;
use listen_core::model::Song;
use serde::{Deserialize, Serialize};

/// Aim song tempos at `bpm` (± `tol` BPM). With `half_double`, half and double time count as a match
/// (85 BPM fits 170, 140 fits 70), judged at the faster pulse: a double-time song is never halved.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoTarget {
    pub bpm: f32,
    pub tol: f32,
    #[serde(default)]
    pub half_double: bool,
}

impl TempoTarget {
    /// Distance in BPM from `song_bpm` to the target, half/double time counted when allowed, measured
    /// at the faster pulse ([`crate::tempo::fit_distance`]: 130 is 10 off a 70 target, not "65, 5 off").
    pub fn distance(&self, song_bpm: f32) -> f32 {
        crate::tempo::fit_distance(self, song_bpm)
    }

    /// The song's tempo is within the tolerance ([`crate::tempo::fits`]).
    pub fn fits(&self, song_bpm: f32) -> bool {
        crate::tempo::fits(self, song_bpm)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Targets {
    #[serde(default)]
    pub tempo: Option<TempoTarget>,
    /// Focus mode: prefer little lead vocal and steady energy.
    #[serde(default)]
    pub focus: bool,
    /// One forgotten favourite every `n` picks (`Some(8)` = 1 in 8).
    #[serde(default)]
    pub familiar_slot: Option<u8>,
}

impl Targets {
    /// No target: pickers behave exactly as without targets.
    pub fn is_neutral(&self) -> bool {
        *self == Self::default()
    }

    /// Is pick number `index` (0-based, counted over the whole fill) the forgotten-favourite slot?
    /// The last of every `n` picks, so a short fill starts with ordinary picks.
    pub fn familiar_at(&self, index: usize) -> bool {
        match self.familiar_slot {
            Some(n) if n > 0 => (index + 1).is_multiple_of(usize::from(n)),
            _ => false,
        }
    }
}

/// Focus mode: songs with at most this share of sung bars are focus-friendly.
pub const FOCUS_MAX_VOCAL: f32 = 0.35;
/// Focus mode: energy above this (0..1) counts against a song (no big pushes while working).
const FOCUS_CALM_ENERGY: f32 = 0.7;
/// What a song without a vocal curve counts as (unknown: after every measured song).
const FOCUS_UNKNOWN: f32 = 2.0;

/// How badly a song suits focus (lower is better): its sung share, plus energy above
/// [`FOCUS_CALM_ENERGY`]; instrumental versions count as 0, songs without analysis come last.
pub fn focus_cost(song: &Song, f: Option<&FlowFeatures>) -> f32 {
    let energy = f.map_or(0.0, |f| (f.mood.energy - FOCUS_CALM_ENERGY).max(0.0));
    if listen_core::versions::is_instrumental(song) {
        return energy;
    }
    match f.and_then(|f| f.vocal) {
        Some(v) => v + energy,
        None => FOCUS_UNKNOWN,
    }
}

/// Focus mode: the candidates fit for focus, best first (indices into `cands`). Every
/// focus-friendly one (cost up to [`FOCUS_MAX_VOCAL`]) is kept; when fewer than `want` are, the
/// least sung others fill up to `want` (unanalysed songs last), so a fill never comes back empty.
pub fn focus_pool(cands: &[(Song, Option<FlowFeatures>)], want: usize) -> Vec<usize> {
    let mut ranked: Vec<(usize, f32)> =
        cands.iter().enumerate().map(|(i, (s, f))| (i, focus_cost(s, f.as_ref()))).collect();
    ranked.sort_by(|a, b| a.1.total_cmp(&b.1));
    let friendly = ranked.iter().filter(|(_, c)| *c <= FOCUS_MAX_VOCAL).count();
    ranked.into_iter().take(friendly.max(want)).map(|(i, _)| i).collect()
}

/// Focus mode: the `n` least sung analysed songs of `songs` (indices, best first) — what joins an
/// Auto-DJ fill's pool from the listening history. Songs without a vocal curve never join.
pub fn focus_extras(songs: &[(Song, Option<FlowFeatures>)], n: usize) -> Vec<usize> {
    let mut ranked: Vec<(usize, f32)> = songs
        .iter()
        .enumerate()
        .map(|(i, (s, f))| (i, focus_cost(s, f.as_ref())))
        .filter(|(_, c)| *c < FOCUS_UNKNOWN)
        .collect();
    ranked.sort_by(|a, b| a.1.total_cmp(&b.1));
    ranked.into_iter().take(n).map(|(i, _)| i).collect()
}

/// Focus mode: a song worth swapping for its instrumental version (sung or not analysed).
pub fn wants_instrumental(song: &Song, f: Option<&FlowFeatures>) -> bool {
    !listen_core::versions::is_instrumental(song)
        && f.and_then(|f| f.vocal).is_none_or(|v| v > FOCUS_MAX_VOCAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_distance_with_and_without_half_double_time() {
        let t = TempoTarget { bpm: 170.0, tol: 8.0, half_double: true };
        assert_eq!(t.distance(85.0), 0.0);
        assert!(t.fits(164.0) && t.fits(88.0) && !t.fits(120.0));
        let strict = TempoTarget { half_double: false, ..t };
        assert_eq!(strict.distance(85.0), 85.0);
        assert!(!strict.fits(85.0));
        let low = TempoTarget { bpm: 75.0, tol: 5.0, half_double: true };
        assert!(low.fits(150.0) && low.fits(78.0));
    }

    #[test]
    fn tempo_target_fits_agrees_with_tempo_fits() {
        let t = crate::tempo::target(70.0);
        for b in [61.0, 62.0, 78.0, 79.0, 128.0, 130.0, 131.0, 132.0, 148.0, 149.0, 31.0, 39.0, 40.0] {
            assert_eq!(t.fits(b), crate::tempo::fits(&t, b), "{b}");
            assert_eq!(t.distance(b), crate::tempo::fit_distance(&t, b), "{b}");
        }
        assert!(!t.fits(130.0), "a 130 BPM song is 10 off 140, never halved to 65");
    }

    #[test]
    fn familiar_slot_every_nth_pick() {
        let t = Targets { familiar_slot: Some(8), ..Default::default() };
        let slots: Vec<usize> = (0..40).filter(|&i| t.familiar_at(i)).collect();
        assert_eq!(slots, [7, 15, 23, 31, 39]);
        assert!(!Targets::default().familiar_at(0));
        assert!(!Targets { familiar_slot: Some(0), ..Default::default() }.familiar_at(0));
        assert!(Targets::default().is_neutral() && !t.is_neutral());
    }

    #[test]
    fn focus_prefers_little_singing_and_calm_songs() {
        use crate::testutil::{flow, song};
        let with = |vocal: Option<f32>, energy: f32| {
            let mut f = flow(Some(120.0), None, energy);
            f.vocal = vocal;
            Some(f)
        };
        let cands = vec![
            (song("sung", "A", None, None), with(Some(0.8), 0.5)),
            (song("calm", "A", None, None), with(Some(0.1), 0.4)),
            (song("unknown", "A", None, None), None),
            (song("loud instrumental", "A", None, None), with(Some(0.05), 0.95)),
            (song("half", "A", None, None), with(Some(0.3), 0.5)),
            (song("Sung (Inst.)", "A", None, None), None),
        ];
        // calm 0.1, Inst. 0, half 0.3 are friendly; the loud one costs 0.3 too.
        assert_eq!(focus_pool(&cands, 2), [5, 1, 3, 4]);
        // Too few friendly songs: the least sung others fill up, unanalysed last.
        assert_eq!(focus_pool(&cands, 6), [5, 1, 3, 4, 0, 2]);
        assert_eq!(focus_extras(&cands, 3), [5, 1, 3]);
        assert_eq!(focus_extras(&cands, 9), [5, 1, 3, 4, 0]);
        assert!(wants_instrumental(&cands[0].0, cands[0].1.as_ref()));
        assert!(wants_instrumental(&cands[2].0, None));
        assert!(!wants_instrumental(&cands[1].0, cands[1].1.as_ref()));
        assert!(!wants_instrumental(&cands[5].0, None));
    }

    #[test]
    fn serde_defaults() {
        let t: Targets = serde_json::from_str("{}").unwrap();
        assert!(t.is_neutral());
        let t: Targets = serde_json::from_str(r#"{"tempo":{"bpm":120,"tol":4},"focus":true}"#).unwrap();
        assert_eq!(t.tempo.map(|x| x.half_double), Some(false));
        assert!(t.focus);
    }
}
