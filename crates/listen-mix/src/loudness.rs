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
    /// The source's name in logs and in the phone's gain JSON: the same word serde writes.
    pub fn as_str(self) -> &'static str {
        match self {
            GainSource::Off => "off",
            GainSource::Analysis => "analysis",
            GainSource::YouTube => "youtube",
            GainSource::ReplayGain => "replaygain",
            GainSource::Unknown => "unknown",
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

// ---------------------------------------------------------------------------------------------
// Blend gain: an equal-loudness crossfade (item 11).
//
// Two decks playing at once add up: a beat blend with both at full level is about +3 dB louder
// than either, an equal-power fade over a quiet outro sounds like a hole. Instead of one fixed
// trim, the plan carries the local loudness of both decks across the blend ([`BlendLevels`],
// estimated from the song's integrated loudness and its per-bar energy curve, smoothed over 3 s
// like EBU short-term loudness) and the engines trim both decks together so the summed loudness
// follows a straight line from the outgoing level before the blend to the incoming level after it.

/// Width of the loudness window (EBU short-term loudness).
pub const LOCAL_WINDOW_S: f64 = 3.0;
/// Reference stretch before / after the blend for its edge levels.
pub const EDGE_REF_S: f64 = 4.0;
/// Spacing of [`BlendLevels::points`].
pub const LEVEL_STEP_S: f64 = 0.5;
/// Largest blend trim boost (the desktop's deck volume goes up to mpv's 130 % for it, the limiter takes peaks).
pub const BLEND_MAX_BOOST_DB: f32 = 3.0;
/// Largest blend trim cut.
pub const BLEND_MAX_CUT_DB: f32 = -6.0;

/// Share of a bar's (K-weighted) loudness below the bass swap's shelf when nothing else is known (a bass
/// cut at −24 dB then takes about 2 LU).
pub const DEFAULT_BASS_SHARE: f32 = 0.37;
/// Share of the song's bass-heaviest bar that is below 150 Hz (K-weighted loudness). The bar curves only
/// bound it (a low band is never more than its whole bar); fitted on the item-11 bench (bug 148).
const BASS_SHARE_SCALE: f32 = 0.55;
/// How much less a bar's bass counts in K-weighted loudness than in the plain power the energy curve is
/// made of (BS.1770's high-shelf lifts what is above the bass). Fitted with [`BASS_SHARE_SCALE`].
const BASS_WEIGHT_CUT: f32 = 0.7;
/// K-weighted, the song's brightest bar (centroid curve 255) is this much louder against its darkest (0)
/// than their plain power says. Fitted with [`BASS_SHARE_SCALE`].
const BRIGHT_DB: f32 = 2.0;

/// Per-bar loudness of a song in LUFS: the bar curve's dB shape anchored on the integrated loudness.
#[derive(Debug, Clone)]
pub struct LocalLevel {
    /// Bar boundaries in file seconds (one more than `lufs`).
    bounds: Vec<f64>,
    lufs: Vec<f32>,
    /// Share of each bar's loudness below 150 Hz (kick and bassline, 0..1): from the low-band curve
    /// against the energy curve.
    bass: Vec<f32>,
}

impl LocalLevel {
    /// `None` without a plausible loudness. Without a bar curve the song is taken as flat.
    pub fn new(f: &Features) -> Option<LocalLevel> {
        if !plausible(f.lufs) {
            return None;
        }
        let flat = || {
            let (a, b) = (f64::from(f.audio_start), f64::from(f.audio_end));
            Some(LocalLevel {
                bounds: vec![a, b.max(a + 1.0)],
                lufs: vec![f.lufs],
                bass: vec![DEFAULT_BASS_SHARE],
            })
        };
        if f.bars.energy.is_empty() {
            return flat();
        }
        // Same bars as the analyser's `bar_bounds`: the beat grid's bars, else 2 s windows.
        let bounds: Vec<f64> = match f.grid.as_ref().filter(|g| g.bar_count() > 0) {
            Some(g) => (0..=g.bar_count()).map(|b| f64::from(g.bar_time(b))).collect(),
            None => {
                let n = ((f.audio_end - f.audio_start) / 2.0).floor().max(0.0) as usize;
                (0..=n).map(|b| f64::from(f.audio_start) + 2.0 * b as f64).collect()
            }
        };
        let n = f.bars.energy.len().min(bounds.len().saturating_sub(1));
        if n == 0 {
            return flat();
        }
        // The curves are 20·log10(rms / loudest bar) mapped −30..0 dB → 0..255.
        let to_db = |v: u8| (f32::from(v) / 255.0 - 1.0) * 30.0;
        let plain: Vec<f32> = f.bars.energy[..n].iter().map(|&e| to_db(e)).collect();
        let bass = bass_shares(&plain, &f.bars.low);
        // K-weighted, a bass-heavy bar is quieter and a bright one louder against the rest than its plain
        // power says (bug 148: 3 s windows against the audio, sd 1.04 → 0.75 dB on 78 songs, 0.96 → 0.72 on
        // 47 others).
        let bright = |i: usize| f.bars.centroid.get(i).map_or(0.0, |&c| BRIGHT_DB * f32::from(c) / 255.0);
        let db: Vec<f32> = plain
            .iter()
            .zip(&bass)
            .enumerate()
            .map(|(i, (d, s))| d + 10.0 * (1.0 - BASS_WEIGHT_CUT * s).log10() + bright(i))
            .collect();
        // Integrated loudness ≈ power mean of the bars above the relative gate (−10 LU, BS.1770).
        let pmean = |v: &[f32]| v.iter().map(|d| 10f32.powf(d / 10.0)).sum::<f32>() / v.len().max(1) as f32;
        let ungated = 10.0 * pmean(&db).max(1e-12).log10();
        let gated: Vec<f32> = db.iter().copied().filter(|&d| d > ungated - 10.0).collect();
        let integrated = 10.0 * pmean(if gated.is_empty() { &db } else { &gated }).max(1e-12).log10();
        let offset = f.lufs - integrated;
        Some(LocalLevel {
            bounds: bounds[..=n].to_vec(),
            lufs: db.iter().map(|d| d + offset).collect(),
            bass,
        })
    }

    /// The window [`Self::at`] averages over around `t`, clamped to the analysed bars.
    fn window(&self, t: f64) -> (f64, f64) {
        let (a, b) = (t - LOCAL_WINDOW_S / 2.0, t + LOCAL_WINDOW_S / 2.0);
        let (first, last) = (self.bounds[0], self.bounds[self.bounds.len() - 1]);
        if b <= first {
            (first, first + 0.01)
        } else if a >= last {
            (last - 0.01, last)
        } else {
            (a.max(first), b.min(last))
        }
    }

    /// Loudness change (LU, ≤ 0) of a bass shelf at `db` around file time `t`: the share of the window's
    /// loudness below 150 Hz taken down by the shelf.
    pub fn bass_loss_lu(&self, t: f64, db: f32) -> f32 {
        let (a, b) = self.window(t);
        let (mut kept, mut all) = (0.0f64, 0.0f64);
        let cut = 1.0 - 10f64.powf(f64::from(db.min(0.0)) / 10.0);
        for (i, (l, s)) in self.lufs.iter().zip(&self.bass).enumerate() {
            let ov = self.bounds[i + 1].min(b) - self.bounds[i].max(a);
            if ov > 0.0 {
                let p = ov * 10f64.powf(f64::from(*l) / 10.0);
                all += p;
                kept += p * (1.0 - f64::from(*s) * cut);
            }
        }
        if all <= 0.0 { 0.0 } else { (10.0 * (kept / all).max(1e-6).log10()) as f32 }
    }

    /// Loudness (LUFS) around file time `t`: power mean over [`LOCAL_WINDOW_S`], bars weighted by overlap.
    /// Outside the analysed bars the nearest bar holds.
    pub fn at(&self, t: f64) -> f32 {
        let (a, b) = self.window(t);
        let (mut p, mut w) = (0.0f64, 0.0f64);
        for (i, l) in self.lufs.iter().enumerate() {
            let ov = self.bounds[i + 1].min(b) - self.bounds[i].max(a);
            if ov > 0.0 {
                p += ov * 10f64.powf(f64::from(*l) / 10.0);
                w += ov;
            }
        }
        if w <= 0.0 { self.lufs[0] } else { (10.0 * (p / w).log10()) as f32 }
    }

    /// Mean loudness over file seconds `a..b` (power mean of [`Self::at`] every half second).
    pub fn mean(&self, a: f64, b: f64) -> f32 {
        let n = (((b - a) / LEVEL_STEP_S).ceil() as usize).max(1);
        let p: f64 = (0..=n)
            .map(|k| 10f64.powf(f64::from(self.at(a + (b - a) * k as f64 / n as f64)) / 10.0))
            .sum::<f64>()
            / (n + 1) as f64;
        (10.0 * p.log10()) as f32
    }
}

/// Each bar's share of its power below 150 Hz from the plain energy curve (dB below the loudest bar) and
/// the low-band curve (0–255 on the same 30 dB scale, below its own loudest bar). The two curves only give
/// the shares up to one factor per song; the low band is never more than the whole bar, so the bar where
/// it is largest against the energy bounds that factor, and [`BASS_SHARE_SCALE`] of the bound is used.
fn bass_shares(plain_db: &[f32], low: &[u8]) -> Vec<f32> {
    if low.len() < plain_db.len() {
        return vec![DEFAULT_BASS_SHARE; plain_db.len()];
    }
    let rel: Vec<f32> =
        plain_db.iter().zip(low).map(|(e, &l)| (f32::from(l) / 255.0 - 1.0) * 30.0 - e).collect();
    let top = rel.iter().copied().fold(f32::MIN, f32::max);
    rel.iter().map(|r| (BASS_SHARE_SCALE * 10f32.powf((r - top) / 10.0)).clamp(0.0, 0.97)).collect()
}

/// Both decks' local loudness across a blend, before the deck gains.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BlendLevels {
    /// Outgoing level over the [`EDGE_REF_S`] before the blend (LUFS).
    pub out_ref: f32,
    /// Incoming level over the [`EDGE_REF_S`] after the blend.
    pub in_ref: f32,
    /// `(t, out, in)`: wall seconds from the blend start, both decks' loudness there (LUFS; in a plan, after
    /// the decks' filters), every [`LEVEL_STEP_S`]. Linear in between.
    pub points: Vec<(f64, f32, f32)>,
}

impl BlendLevels {
    /// Levels for a blend that starts at outgoing file time `out_start_s` and incoming `in_start_s`,
    /// lasts `blend_s` wall seconds, with the decks at `out_speed` / `in_speed`.
    pub fn new(
        out: &Features,
        inn: &Features,
        out_start_s: f64,
        in_start_s: f64,
        blend_s: f64,
        out_speed: f64,
        in_speed: f64,
    ) -> Option<BlendLevels> {
        if blend_s <= 0.0 {
            return None;
        }
        let (lo, li) = (LocalLevel::new(out)?, LocalLevel::new(inn)?);
        let out_at = |t: f64| out_start_s + t * out_speed;
        let in_at = |t: f64| in_start_s + t * in_speed;
        let n = ((blend_s / LEVEL_STEP_S).ceil() as usize).max(1);
        let points = (0..=n)
            .map(|k| {
                let t = blend_s * k as f64 / n as f64;
                (t, lo.at(out_at(t)), li.at(in_at(t)))
            })
            .collect();
        Some(BlendLevels {
            out_ref: lo.mean(out_start_s - EDGE_REF_S * out_speed, out_start_s),
            in_ref: li.mean(in_at(blend_s), in_at(blend_s) + EDGE_REF_S * in_speed),
            points,
        })
    }

    fn levels_at(&self, t: f64) -> (f32, f32) {
        let p = &self.points;
        let (Some(first), Some(last)) = (p.first(), p.last()) else { return (self.out_ref, self.in_ref) };
        if t <= first.0 {
            return (first.1, first.2);
        }
        if t >= last.0 {
            return (last.1, last.2);
        }
        let i = p.partition_point(|q| q.0 <= t) - 1;
        let (a, b) = (p[i], p[i + 1]);
        let x = if b.0 > a.0 { ((t - a.0) / (b.0 - a.0)) as f32 } else { 1.0 };
        (a.1 + (b.1 - a.1) * x, a.2 + (b.2 - a.2) * x)
    }

    /// Trim (dB) for both decks at wall second `t` of the blend, given the envelope gains there and
    /// each deck's playback gain (levelling, 0 when off): the summed loudness then follows the line
    /// from the outgoing edge level to the incoming one. Fades in and out with the overlap, so the
    /// trim is 0 dB where only one deck sounds; clamped to [`BLEND_MAX_CUT_DB`]..[`BLEND_MAX_BOOST_DB`].
    pub fn trim_db(&self, t: f64, g_out: f32, g_in: f32, gain_out_db: f32, gain_in_db: f32) -> f32 {
        let blend_s = self.points.last().map_or(0.0, |p| p.0);
        if g_out <= 0.0 || g_in <= 0.0 || blend_s <= 0.0 {
            return 0.0;
        }
        let x = (t / blend_s).clamp(0.0, 1.0) as f32;
        let from = self.out_ref + gain_out_db;
        let to = self.in_ref + gain_in_db;
        let target = from + (to - from) * x;
        let (lo, li) = self.levels_at(t);
        let (po, pi) = (g_out * g_out, g_in * g_in);
        let sum = 10.0
            * (po * 10f32.powf((lo + gain_out_db) / 10.0) + pi * 10f32.powf((li + gain_in_db) / 10.0))
                .max(1e-12)
                .log10();
        overlap_weight(po, pi) * (target - sum).clamp(BLEND_MAX_CUT_DB, BLEND_MAX_BOOST_DB)
    }
}

/// 1 where both decks are equally loud in the envelope, towards 0 where one fades out.
fn overlap_weight(po: f32, pi: f32) -> f32 {
    let s = po + pi;
    if s <= 0.0 { 0.0 } else { (4.0 * po * pi / (s * s)).clamp(0.0, 1.0) }
}

/// Blend trim (linear) for one moment of a transition (contract C5): from the plan's
/// [`BlendLevels`] when it has them, else equal-power (two decks at the same level). 1.0 for
/// handovers (echo-out, cut: no overlapping dry signals) and where only one deck sounds.
pub fn blend_trim(
    plan: &crate::plan::TransitionPlan,
    t: f64,
    g_out: f32,
    g_in: f32,
    gain_out_db: f32,
    gain_in_db: f32,
) -> f32 {
    use crate::plan::Kind;
    if matches!(plan.kind, Kind::EchoOut { .. } | Kind::Cut | Kind::Gapless) || g_out <= 0.0 || g_in <= 0.0 {
        return 1.0;
    }
    let db = match &plan.levels {
        Some(l) => l.trim_db(t, g_out, g_in, gain_out_db, gain_in_db),
        None => {
            let (po, pi) = (g_out * g_out, g_in * g_in);
            overlap_weight(po, pi) * (-10.0 * (po + pi).log10()).clamp(BLEND_MAX_CUT_DB, BLEND_MAX_BOOST_DB)
        }
    };
    db_to_factor(db)
}

/// The plan's [`BlendLevels`] from both songs' analyses (outgoing speed after its ramp).
pub fn plan_levels(
    plan: &crate::plan::TransitionPlan,
    out: &Features,
    inn: &Features,
) -> Option<BlendLevels> {
    use crate::plan::Kind;
    if matches!(plan.kind, Kind::EchoOut { .. } | Kind::Cut | Kind::Gapless) {
        return None;
    }
    let out_speed = plan.out_ramp.map_or(1.0, |r| r.to);
    let mut levels = BlendLevels::new(
        out,
        inn,
        plan.out_mix_start_s,
        plan.in_mix_start_s,
        plan.blend_s,
        out_speed,
        plan.in_speed,
    )?;
    // What the decks' filters (bass swap, sweeps) take off, so the trim makes up for it (bug 148).
    let (lo, li) = (LocalLevel::new(out), LocalLevel::new(inn));
    let (s_out, s_in) = (out_speed.max(0.25), plan.in_speed.max(0.25));
    for (t, o, i) in &mut levels.points {
        let (to, ti) = (plan.out_mix_start_s + *t * s_out, plan.in_mix_start_s + *t * s_in);
        use crate::verify::{bass_loss, eq_state};
        *o += eq_state(plan, crate::plan::Which::Out, to, s_out, &bass_loss(lo.as_ref(), to)).0;
        *i += eq_state(plan, crate::plan::Which::In, to, s_out, &bass_loss(li.as_ref(), ti)).0;
    }
    Some(levels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::FeatBuilder;

    #[test]
    fn as_str_matches_serde_name() {
        use GainSource::*;
        for v in [Off, Analysis, YouTube, ReplayGain, Unknown] {
            assert_eq!(serde_json::to_value(v).unwrap(), serde_json::json!(v.as_str()), "{v:?}");
        }
    }

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

    fn flat(lufs: f32, energy: u8) -> Features {
        let mut f = feat(lufs);
        f.bars.energy = vec![energy; f.bars.energy.len()];
        f.bars.low = vec![energy; f.bars.low.len()];
        f
    }

    fn levels(lo: f32, li: f32, blend_s: f64) -> BlendLevels {
        BlendLevels::new(&flat(lo, 200), &flat(li, 200), 30.0, 10.0, blend_s, 1.0, 1.0).unwrap()
    }

    #[test]
    fn local_level_follows_the_bar_curve() {
        // Flat curve: the integrated loudness everywhere.
        let l = LocalLevel::new(&flat(-9.0, 180)).unwrap();
        for t in [0.0, 10.0, 33.3, 60.0, 500.0] {
            assert!((l.at(t) + 9.0).abs() < 0.01, "{t}: {}", l.at(t));
        }
        // FeatBuilder: 8-bar intro/outro at 120 (−15.9 dB), body at 200 (−6.5 dB): 9.4 dB apart in plain
        // power, the body (most of the song) near the integrated loudness. The body's bass is 2.35 dB
        // stronger against the rest than the intro's, so K-weighted the two are a little closer.
        let mut f = FeatBuilder::new(120.0, 96).build();
        f.lufs = -9.0;
        let l = LocalLevel::new(&f).unwrap();
        let bar = f.grid.as_ref().unwrap().bar_time(1) - f.grid.as_ref().unwrap().bar_time(0);
        let intro = l.at(f64::from(2.0 * bar));
        let body = l.at(f64::from(16.0 * bar));
        assert!((body + 9.0).abs() < 1.0, "body {body}");
        let (sb, si) = (BASS_SHARE_SCALE, BASS_SHARE_SCALE * 10f32.powf(-0.235));
        let closer = 10.0 * ((1.0 - BASS_WEIGHT_CUT * si) / (1.0 - BASS_WEIGHT_CUT * sb)).log10();
        assert!((body - intro - (9.4 - closer)).abs() < 0.3, "intro {intro} body {body} closer {closer}");
        // No loudness: nothing to go on. No curve: flat at the integrated loudness.
        assert!(LocalLevel::new(&flat(-70.0, 200)).is_none());
        let mut f = feat(-9.0);
        f.bars.energy.clear();
        let l = LocalLevel::new(&f).unwrap();
        assert!((l.at(5.0) + 9.0).abs() < 1e-4 && (l.at(1e4) + 9.0).abs() < 1e-4);
    }

    #[test]
    fn the_bass_swap_takes_more_from_a_bass_heavy_bar() {
        // 32 bars at the same plain power; bars 16.. have their bass 12 dB down.
        let mut f = flat(-9.0, 220);
        for (i, l) in f.bars.low.iter_mut().enumerate() {
            if i >= 16 {
                *l = 220 - 102;
            }
        }
        let l = LocalLevel::new(&f).unwrap();
        let bar = f64::from(f.grid.as_ref().unwrap().bar_time(1) - f.grid.as_ref().unwrap().bar_time(0));
        let (heavy, light) = (l.bass_loss_lu(4.0 * bar, -24.0), l.bass_loss_lu(28.0 * bar, -24.0));
        let full = 10.0 * (1.0 - BASS_SHARE_SCALE * (1.0 - 10f32.powf(-2.4))).log10();
        assert!((heavy - full).abs() < 0.05, "{heavy} vs {full}");
        assert!(light > -0.3 && light < 0.0, "{light}");
        assert_eq!(l.bass_loss_lu(4.0 * bar, 0.0), 0.0);
        // K-weighted, the bass-heavy half is quieter than the light half at the same plain power.
        assert!(l.at(4.0 * bar) < l.at(28.0 * bar) - 0.5);
        // No low-band curve: a typical share.
        f.bars.low.clear();
        let l = LocalLevel::new(&f).unwrap();
        let typical = 10.0 * (1.0 - DEFAULT_BASS_SHARE * (1.0 - 10f32.powf(-2.4))).log10();
        assert!((l.bass_loss_lu(4.0 * bar, -24.0) - typical).abs() < 0.05);
    }

    #[test]
    fn the_trim_makes_up_for_what_the_filters_take() {
        use crate::plan::{EqEvent, EqTarget, Which};
        // Bug 148: a ShortBlend's bass swap takes the outgoing's kick and bassline at half way; the trim
        // must not keep cutting as if the outgoing were still whole.
        let (a, b) = (flat(-9.0, 220), flat(-9.0, 220));
        let mixing = listen_core::settings::Mixing::default();
        let ctx = crate::plan::PlanCtx {
            settings: &mixing,
            out_position_s: 30.0,
            voice_break: None,
            out_genres: &[],
            in_genres: &[],
            no_echo: false,
        };
        let mut p = crate::plan::plan(&a, &b, &ctx);
        p.eq.clear();
        let bare = plan_levels(&p, &a, &b).unwrap();
        let half = p.out_mix_start_s + p.blend_s / 2.0;
        p.eq.push(EqEvent { deck: Which::Out, at_out_s: half, target: EqTarget::Bass(-24.0), ramp_s: 0.0 });
        let swapped = plan_levels(&p, &a, &b).unwrap();
        let loss = LocalLevel::new(&a).unwrap().bass_loss_lu(half + 1.0, -24.0);
        for (x, y) in bare.points.iter().zip(&swapped.points) {
            let after = x.0 >= p.blend_s / 2.0 - 1e-6;
            assert!((y.1 - (x.1 + if after { loss } else { 0.0 })).abs() < 0.05, "{x:?} {y:?}");
            assert_eq!(x.2, y.2);
        }
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let t = p.blend_s * 0.75;
        assert!(swapped.trim_db(t, h, h, 0.0, 0.0) > bare.trim_db(t, h, h, 0.0, 0.0) + 0.5);
    }

    #[test]
    fn equal_loudness_trim() {
        let l = levels(-10.0, -10.0, 8.0);
        assert!((l.out_ref + 10.0).abs() < 0.01 && (l.in_ref + 10.0).abs() < 0.01);
        // Both decks at full level: −3 dB keeps the sum where each one was.
        assert!((l.trim_db(4.0, 1.0, 1.0, 0.0, 0.0) + 3.01).abs() < 0.05);
        // Equal-power midpoint: already right.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!(l.trim_db(4.0, h, h, 0.0, 0.0).abs() < 0.05);
        // One deck alone: no trim.
        assert_eq!(l.trim_db(4.0, 1.0, 0.0, 0.0, 0.0), 0.0);
        assert_eq!(l.trim_db(4.0, 0.0, 1.0, 0.0, 0.0), 0.0);
        // Fading out with the overlap: small where one deck is nearly gone.
        assert!(l.trim_db(0.5, 1.0, 0.1, 0.0, 0.0).abs() < 0.1);

        // 6 LU apart (−8 out, −14 in), equal-power midpoint: the sum follows the straight line, so at
        // half way it should be −11 LUFS. Without a trim it is 10·log10((10^-0.8 + 10^-1.4)/2) = −10.03.
        let l = levels(-8.0, -14.0, 8.0);
        let t = l.trim_db(4.0, h, h, 0.0, 0.0);
        assert!((t + 0.97).abs() < 0.1, "{t}");
        // Levelling brings both to −14: back to the equal case.
        assert!(l.trim_db(4.0, h, h, -6.0, 0.0).abs() < 0.05);
    }

    #[test]
    fn trim_is_bounded() {
        // A 20 LU hole in both decks (quiet breakdowns) is not filled by more than +3 dB.
        let mut l = levels(-10.0, -10.0, 8.0);
        for p in &mut l.points {
            p.1 = -30.0;
            p.2 = -30.0;
        }
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!((l.trim_db(4.0, h, h, 0.0, 0.0) - BLEND_MAX_BOOST_DB).abs() < 1e-4);
        for p in &mut l.points {
            p.1 = 0.0;
            p.2 = 0.0;
        }
        assert!((l.trim_db(4.0, h, h, 0.0, 0.0) - BLEND_MAX_CUT_DB).abs() < 1e-4);
    }

    #[test]
    fn plans_carry_levels_and_handovers_skip_the_trim() {
        use crate::plan::{Envelope, Kind, PlanCtx, plan};
        let (a, b) = (FeatBuilder::new(124.0, 64).build(), FeatBuilder::new(124.0, 64).build());
        let mixing = listen_core::settings::Mixing::default();
        let ctx = PlanCtx {
            settings: &mixing,
            out_position_s: 30.0,
            voice_break: None,
            out_genres: &[],
            in_genres: &[],
            no_echo: false,
        };
        let p = plan(&a, &b, &ctx);
        assert!(!matches!(p.kind, Kind::EchoOut { .. } | Kind::Cut | Kind::Gapless), "{}", p.reason);
        let l = p.levels.as_ref().expect("a blend between analysed songs has levels");
        assert!((l.points.last().unwrap().0 - p.blend_s).abs() < 1e-9);
        // Survives a JSON round trip (Android gets plans as JSON), and old plans without it load.
        let back: crate::plan::TransitionPlan =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        let (x, y) = (back.levels.unwrap(), l.clone());
        assert_eq!(x.points.len(), y.points.len());
        assert!(
            x.points.iter().zip(&y.points).all(|(u, v)| (u.0 - v.0).abs() < 1e-9 && u.1 == v.1 && u.2 == v.2)
        );

        let mut cut = p.clone();
        cut.kind = Kind::Cut;
        assert_eq!(blend_trim(&cut, 1.0, 1.0, 1.0, 0.0, 0.0), 1.0);
        // Without levels: equal power (both full → −3 dB, equal-power midpoint → unity).
        let mut bare = p.clone();
        bare.levels = None;
        bare.gain = Envelope { points: vec![(0.0, 1.0, 0.0), (4.0, 1.0, 1.0), (8.0, 0.0, 1.0)] };
        assert!((factor_to_db(blend_trim(&bare, 4.0, 1.0, 1.0, 0.0, 0.0)) + 3.01).abs() < 0.05);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!(factor_to_db(blend_trim(&bare, 4.0, h, h, 0.0, 0.0)).abs() < 0.05);
    }
}
