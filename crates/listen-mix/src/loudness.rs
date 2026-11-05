//! Playback and blend gain (contract C5): the one place a per-song gain is computed, for the
//! desktop decks, the Android mixer and DJ blends.
//!
//! A song's loudness comes from the first source that has it:
//! 1. the analysis store's integrated loudness (`Features::lufs`, EBU R128 / BS.1770),
//! 2. YouTube's per-video `loudnessDb` from the player response (relative to YouTube's
//!    −14 LUFS reference, so `lufs ≈ −14 + loudnessDb`),
//! 3. ReplayGain tags (ReplayGain 2 reference −18 LUFS, so `lufs = −18 − gain`),
//! 4. nothing: gain 0 dB.
//!
//! Gain = target − lufs. Cuts are allowed down to [`MIN_GAIN_DB`]; boosts stop at
//! [`MAX_BOOST_DB`] because quiet songs are usually dynamic and the output limiter
//! (desktop mpv `alimiter`, Android master limiter) catches the peaks. With a ReplayGain
//! peak tag and no limiter the boost is also held below [`PEAK_CEILING_DBTP`].

use listen_core::analysis::Features;
use listen_core::model::GainInfo;
use serde::{Deserialize, Serialize};

/// YouTube's loudness reference: `loudnessDb` is the video's level above it.
pub const YTM_REFERENCE_LUFS: f32 = -14.0;
/// ReplayGain 2.0 reference level.
pub const RG_REFERENCE_LUFS: f32 = -18.0;
/// Largest boost for quiet songs (the limiter takes the peaks).
pub const MAX_BOOST_DB: f32 = 6.0;
/// Largest cut.
pub const MIN_GAIN_DB: f32 = -20.0;
/// Peak ceiling when a song's peak is known and no limiter runs (dBFS, sample peak as a
/// stand-in for true peak).
pub const PEAK_CEILING_DBTP: f32 = -1.0;
/// Fastest gain change while levelling is toggled or a source arrives late: 1 dB per 50 ms.
pub const MAX_RAMP_DB_PER_S: f32 = 20.0;

pub use listen_core::settings::Levelling;

/// A ReplayGain tag pair (gain in dB, peak as a linear sample value).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RgTag {
    pub gain_db: f32,
    pub peak: Option<f32>,
}

impl RgTag {
    /// The track tag, or the album tag when `album` and it exists (falls back to the other one).
    pub fn from_info(info: &GainInfo, album: bool) -> Option<RgTag> {
        let track = info.track_gain.map(|g| RgTag { gain_db: g, peak: info.track_peak });
        let alb = info.album_gain.map(|g| RgTag { gain_db: g, peak: info.album_peak });
        if album { alb.or(track) } else { track.or(alb) }
    }
}

/// Where a gain came from (logged, shown in Get info).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GainSource {
    /// Levelling is off.
    Off,
    /// The analysis store's measured loudness.
    Analysis,
    /// YouTube's `loudnessDb`.
    YouTube,
    /// ReplayGain tags.
    ReplayGain,
    /// No loudness known: 0 dB.
    Unknown,
}

impl GainSource {
    pub fn as_str(self) -> &'static str {
        match self {
            GainSource::Off => "off",
            GainSource::Analysis => "analysis",
            GainSource::YouTube => "youtube",
            GainSource::ReplayGain => "replaygain",
            GainSource::Unknown => "none",
        }
    }
}

/// A playback gain in dB and its source.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GainDb {
    pub db: f32,
    pub source: GainSource,
    /// The loudness the gain was computed from (LUFS), when known.
    pub lufs: Option<f32>,
}

impl GainDb {
    pub const UNITY: GainDb = GainDb { db: 0.0, source: GainSource::Off, lufs: None };

    /// Linear amplitude factor.
    pub fn factor(self) -> f32 {
        db_to_factor(self.db)
    }

    /// The song's loudness after this gain, when known.
    pub fn out_lufs(self) -> Option<f32> {
        self.lufs.map(|l| l + self.db)
    }
}

pub fn db_to_factor(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn factor_to_db(f: f32) -> f32 {
    20.0 * f.max(1e-6).log10()
}

/// A plausible integrated loudness (failed or silent analyses report −70 or NaN).
fn plausible(lufs: f32) -> bool {
    lufs.is_finite() && (-40.0..=3.0).contains(&lufs)
}

/// The song's loudness estimate and where it came from.
pub fn song_lufs(
    features: Option<&Features>,
    ytm_loudness_db: Option<f32>,
    replaygain: Option<RgTag>,
) -> Option<(f32, GainSource)> {
    if let Some(l) = features.map(|f| f.lufs).filter(|&l| plausible(l)) {
        return Some((l, GainSource::Analysis));
    }
    if let Some(l) = ytm_loudness_db.map(|d| YTM_REFERENCE_LUFS + d).filter(|&l| plausible(l)) {
        return Some((l, GainSource::YouTube));
    }
    if let Some(l) = replaygain.map(|rg| RG_REFERENCE_LUFS - rg.gain_db).filter(|&l| plausible(l)) {
        return Some((l, GainSource::ReplayGain));
    }
    None
}

/// Playback gain for one song (contract C5). `limiter`: an output limiter follows the gain,
/// so boosts may exceed the known peak.
pub fn track_gain(
    features: Option<&Features>,
    ytm_loudness_db: Option<f32>,
    replaygain: Option<RgTag>,
    target: Levelling,
    limiter: bool,
) -> GainDb {
    let Some(target) = target.target_lufs() else { return GainDb::UNITY };
    let Some((lufs, source)) = song_lufs(features, ytm_loudness_db, replaygain) else {
        return GainDb { db: 0.0, source: GainSource::Unknown, lufs: None };
    };
    let mut db = (target - lufs).clamp(MIN_GAIN_DB, MAX_BOOST_DB);
    if !limiter
        && db > 0.0
        && let Some(peak) = replaygain.and_then(|rg| rg.peak).filter(|p| *p > 0.0)
    {
        db = db.min((PEAK_CEILING_DBTP - factor_to_db(peak)).max(0.0));
    }
    GainDb { db, source, lufs: Some(lufs) }
}

/// One step of a gain ramp from `current_db` towards `target_db` over `dt_s` seconds, at most
/// [`MAX_RAMP_DB_PER_S`] (so toggling the setting mid-song never jumps).
pub fn ramp_toward(current_db: f32, target_db: f32, dt_s: f32) -> f32 {
    let max = MAX_RAMP_DB_PER_S * dt_s.max(0.0);
    current_db + (target_db - current_db).clamp(-max, max)
}

/// Gain ramp from `from_db` to `to_db` as `(seconds, db)` steps of `step_s`, never faster than
/// [`MAX_RAMP_DB_PER_S`]; the last step is `to_db` exactly.
pub fn ramp_steps(from_db: f32, to_db: f32, step_s: f32) -> Vec<(f32, f32)> {
    let step_s = step_s.max(0.005);
    let mut out = Vec::new();
    let (mut db, mut t) = (from_db, 0.0);
    while (to_db - db).abs() > 1e-3 {
        t += step_s;
        db = ramp_toward(db, to_db, step_s);
        out.push((t, db));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::FeatBuilder;

    fn feat(lufs: f32) -> Features {
        let mut f = FeatBuilder::new(120.0, 32).build();
        f.lufs = lufs;
        f
    }

    #[test]
    fn levelling_targets() {
        assert_eq!(Levelling::default(), Levelling::Normal);
        assert_eq!(Levelling::Off.target_lufs(), None);
        assert_eq!(Levelling::Quiet.target_lufs(), Some(-19.0));
        assert_eq!(Levelling::Normal.target_lufs(), Some(-14.0));
        assert_eq!(Levelling::Loud.target_lufs(), Some(-11.0));
        for l in Levelling::ALL {
            assert_eq!(Levelling::from_u8(l.to_u8()), l);
        }
        assert_eq!(Levelling::from_u8(99), Levelling::Normal);
    }

    #[test]
    fn analysis_comes_first() {
        let f = feat(-8.0);
        let g = track_gain(
            Some(&f),
            Some(3.0),
            Some(RgTag { gain_db: -2.0, peak: None }),
            Levelling::Normal,
            true,
        );
        assert_eq!(g.source, GainSource::Analysis);
        assert!((g.db + 6.0).abs() < 1e-4);
        assert_eq!(g.out_lufs(), Some(-14.0));
    }

    #[test]
    fn youtube_then_replaygain_then_nothing() {
        // loudnessDb 4 → −10 LUFS → −4 dB to reach −14.
        let g =
            track_gain(None, Some(4.0), Some(RgTag { gain_db: -9.0, peak: None }), Levelling::Normal, true);
        assert_eq!(g.source, GainSource::YouTube);
        assert!((g.db + 4.0).abs() < 1e-4);
        // RG −9 dB → −9 LUFS → −5 dB to reach −14.
        let g = track_gain(None, None, Some(RgTag { gain_db: -9.0, peak: None }), Levelling::Normal, true);
        assert_eq!(g.source, GainSource::ReplayGain);
        assert!((g.db + 5.0).abs() < 1e-4);
        let g = track_gain(None, None, None, Levelling::Normal, true);
        assert_eq!((g.db, g.source), (0.0, GainSource::Unknown));
    }

    #[test]
    fn implausible_analysis_falls_through() {
        let g = track_gain(Some(&feat(-70.0)), Some(1.0), None, Levelling::Normal, true);
        assert_eq!(g.source, GainSource::YouTube);
        let g = track_gain(Some(&feat(f32::NAN)), None, None, Levelling::Normal, true);
        assert_eq!(g.source, GainSource::Unknown);
    }

    #[test]
    fn off_is_unity() {
        let g = track_gain(Some(&feat(-6.0)), None, None, Levelling::Off, true);
        assert_eq!(g, GainDb::UNITY);
        assert_eq!(g.factor(), 1.0);
    }

    #[test]
    fn boost_is_capped_and_cut_is_bounded() {
        let g = track_gain(Some(&feat(-30.0)), None, None, Levelling::Normal, true);
        assert_eq!(g.db, MAX_BOOST_DB);
        let g = track_gain(Some(&feat(2.0)), None, None, Levelling::Quiet, true);
        assert_eq!(g.db, MIN_GAIN_DB);
    }

    #[test]
    fn known_peak_limits_boost_without_limiter() {
        // −20 LUFS wants +6 dB; peak 0.5 (−6.02 dBFS) allows only +5.02 dB to stay under −1 dBFS.
        let rg = Some(RgTag { gain_db: 2.0, peak: Some(0.5) });
        let g = track_gain(None, None, rg, Levelling::Normal, false);
        assert!((g.db - 5.0206).abs() < 0.01, "{}", g.db);
        let g = track_gain(None, None, rg, Levelling::Normal, true);
        assert_eq!(g.db, 6.0);
        // Cuts are never touched by the peak guard.
        let rg = Some(RgTag { gain_db: -8.0, peak: Some(1.0) });
        assert!((track_gain(None, None, rg, Levelling::Normal, false).db + 4.0).abs() < 1e-4);
    }

    #[test]
    fn levels_a_loud_library_into_a_narrow_band() {
        // The real library spans −15.3 … −5.8 LUFS (p5–p95); after levelling every song lands
        // on the target unless it needs more than the boost cap.
        let out: Vec<f32> = [-15.3, -10.3, -8.4, -7.1, -5.8]
            .iter()
            .map(|&l| track_gain(Some(&feat(l)), None, None, Levelling::Normal, true).out_lufs().unwrap())
            .collect();
        let (lo, hi) = out.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        assert!(hi - lo < 0.01, "{out:?}");
    }

    #[test]
    fn rg_tag_choice() {
        let info = GainInfo {
            track_gain: Some(-3.0),
            track_peak: Some(0.9),
            album_gain: Some(-5.0),
            album_peak: None,
        };
        assert_eq!(RgTag::from_info(&info, false), Some(RgTag { gain_db: -3.0, peak: Some(0.9) }));
        assert_eq!(RgTag::from_info(&info, true), Some(RgTag { gain_db: -5.0, peak: None }));
        let only_track = GainInfo { track_gain: Some(-3.0), ..Default::default() };
        assert_eq!(RgTag::from_info(&only_track, true).map(|r| r.gain_db), Some(-3.0));
        assert_eq!(RgTag::from_info(&GainInfo::default(), false), None);
    }

    #[test]
    fn ramps_never_jump() {
        assert_eq!(ramp_toward(0.0, -6.0, 0.05), -1.0);
        assert_eq!(ramp_toward(-5.5, -6.0, 0.05), -6.0);
        let steps = ramp_steps(0.0, -6.0, 0.05);
        assert_eq!(steps.last().unwrap().1, -6.0);
        assert_eq!(steps.len(), 6);
        let mut prev = 0.0;
        for (_, db) in &steps {
            assert!((db - prev).abs() <= 1.0 + 1e-4);
            prev = *db;
        }
        assert!(ramp_steps(-3.0, -3.0, 0.05).is_empty());
    }

    #[test]
    fn db_factor_roundtrip() {
        assert!((db_to_factor(-6.0206) - 0.5).abs() < 1e-4);
        assert!((factor_to_db(0.5) + 6.0206).abs() < 1e-3);
    }
}
