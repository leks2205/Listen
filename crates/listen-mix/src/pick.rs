//! What an automatic fill aims for beyond a smooth flow: a tempo (heart rate, cadence), focus, and a
//! slot for a forgotten favourite. One [`Targets`] value goes into every automatic picker (Auto-DJ, the AI
//! DJ candidate filter, [`crate::score::order`]); `Targets::default()` means "pick as before".

use serde::{Deserialize, Serialize};

/// Aim song tempos at `bpm` (± `tol` BPM). With `half_double`, half and double time count as a match
/// (85 BPM fits 170).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoTarget {
    pub bpm: f32,
    pub tol: f32,
    #[serde(default)]
    pub half_double: bool,
}

impl TempoTarget {
    /// Distance in BPM from `song_bpm` to the target, using half/double time when allowed.
    pub fn distance(&self, song_bpm: f32) -> f32 {
        let d = |b: f32| (b - self.bpm).abs();
        if self.half_double { d(song_bpm).min(d(song_bpm * 2.0)).min(d(song_bpm / 2.0)) } else { d(song_bpm) }
    }

    /// The song's tempo is within the tolerance.
    pub fn fits(&self, song_bpm: f32) -> bool {
        self.distance(song_bpm) <= self.tol
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
    fn familiar_slot_every_nth_pick() {
        let t = Targets { familiar_slot: Some(8), ..Default::default() };
        let slots: Vec<usize> = (0..40).filter(|&i| t.familiar_at(i)).collect();
        assert_eq!(slots, [7, 15, 23, 31, 39]);
        assert!(!Targets::default().familiar_at(0));
        assert!(!Targets { familiar_slot: Some(0), ..Default::default() }.familiar_at(0));
        assert!(Targets::default().is_neutral() && !t.is_neutral());
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
