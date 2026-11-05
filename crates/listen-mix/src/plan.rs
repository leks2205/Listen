//! Transition planner: mix-out and mix-in points, blend length, tempo moves and gain/EQ automation.
//! The app's mix engine executes the plan by following the outgoing deck's position.
//!
//! - `*_out_s`: **outgoing file time** (unaffected by deck speed); `in_*_s`: **incoming file time**.
//! - Envelope times and ramp/blend lengths are **wall seconds**.
//! - Bars count on the outgoing grid; in a beat-matched blend one outgoing bar is `1/k` incoming bars
//!   (`k` ∈ {1, 2, ½}, see [`crate::score::octave_match`]).

use crate::genre::relax_vocal_edge;
use crate::score::octave_match;
use listen_core::analysis::{BeatGrid, Features, SectionKind, cue};
use listen_core::model::SourceKind;
use listen_core::queue::QueueItem;
use listen_core::settings::Mixing;
use serde::{Deserialize, Serialize};

/// Minimum lead before a mix point, so the app can load and line up the second deck.
pub const MIN_LEAD_S: f64 = 5.0;
/// Speed ramps may start this soon (they only need a command sent).
const RAMP_LEAD_S: f64 = 1.0;
/// Grids less sure than this are treated as "no steady beat".
const MIN_GRID_CONFIDENCE: f32 = 0.3;
/// Songs with fewer usable bars are not beat-mixed.
const MIN_BARS: usize = 16;
/// Bass cut on the deck that must not double the low end (dB).
const BASS_CUT_DB: f32 = -24.0;
/// Below this vocal level (0..1) a bar counts as instrumental.
const QUIET_VOCAL: f32 = 0.4;
/// Longest glide of the incoming deck back to 1.0× after a blend (wall s).
const SETTLE_MAX_S: f64 = 32.0;
/// Tempo gaps under this are matched on the incoming deck alone (inaudible).
const TINY_TEMPO_LR: f64 = 0.005;
/// Cue anchors (lyric phrase ends, breakdowns) may start a mix-out this far into the outgoing song.
const EARLY_OUT_FRAC: f64 = 0.6;
/// Cue-ins before a verse or chorus entry within this part (and this many seconds) of the incoming song.
const EARLY_IN_FRAC: f64 = 0.4;
const EARLY_IN_MAX_S: f64 = 90.0;
/// Score per second of the outgoing song cut off, and of the incoming intro skipped.
const CUT_PER_S: f32 = 0.02;
const SKIP_PER_S: f32 = 0.01;

/// How the two songs are joined.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    /// Beat-matched equal-power blend over instrumental parts. Basses swap on the downbeat
    /// `bass_swap_bar` (from the blend start; `None` = no bass swap).
    BeatBlend { bars: u8, bass_swap_bar: Option<u8> },
    /// Short beat-matched crossfade, placed so the vocals don't overlap.
    ShortBlend { bars: u8 },
    /// Outgoing is high-passed away while the incoming starts on a downbeat.
    FilterOut { bars: u8 },
    /// Plain equal-power crossfade (no steady beat on one side).
    Fade { secs: f32 },
    /// The outgoing's last beat before a phrase end is echoed (`delay_s` apart, see [`EqTarget::Echo`]), its
    /// dry signal stops on the downbeat and the incoming starts there at its own tempo (no beat matching).
    EchoOut { delay_s: f32 },
    /// Hard handover on a phrase downbeat (a few ms crossfade), each song at its own tempo.
    Cut,
    /// No mix: play the next song gaplessly.
    Gapless,
}

/// A linear speed change of one deck.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpeedRamp {
    pub from: f64,
    pub to: f64,
    /// Start of the ramp in outgoing file time (`out_stop_s` for the incoming settle).
    pub start_out_s: f64,
    /// Wall seconds.
    pub secs: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Which {
    Out,
    In,
}

/// Where a deck's mix filter should go.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EqTarget {
    /// Low shelf gain in dB (0 = flat).
    Bass(f32),
    /// High-pass cutoff and dry/wet (0 = off).
    Highpass {
        hz: f32,
        mix: f32,
    },
    Lowpass {
        hz: f32,
        mix: f32,
    },
    /// The deck's echo ([`crate::echo`]): repeats `delay_s` apart, each `feedback` times the last, of what
    /// goes into the send (`send` 0..1); `dry` is the direct signal's level. Set at once (`ramp_s` unused).
    /// Idle: send 0, dry 1.
    Echo {
        delay_s: f32,
        feedback: f32,
        send: f32,
        dry: f32,
    },
}

/// Moves `deck`'s filter to `target` over `ramp_s` wall seconds, from when the outgoing reaches `at_out_s`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqEvent {
    pub deck: Which,
    pub at_out_s: f64,
    pub target: EqTarget,
    pub ramp_s: f64,
}

/// Gain automation: `(t, g_out, g_in)` sorted by `t` (wall seconds from `out_mix_start_s`).
/// Power is interpolated linearly between points, so a simultaneous fade keeps `g_out² + g_in²` constant.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub points: Vec<(f64, f32, f32)>,
}

impl Envelope {
    /// Gains at `t`, held before the first and after the last point; empty = outgoing only.
    pub fn gains_at(&self, t: f64) -> (f32, f32) {
        let p = &self.points;
        let (Some(first), Some(last)) = (p.first(), p.last()) else {
            return (1.0, 0.0);
        };
        if t <= first.0 {
            return (first.1, first.2);
        }
        if t >= last.0 {
            return (last.1, last.2);
        }
        let i = p.partition_point(|q| q.0 <= t) - 1;
        let (a, b) = (p[i], p[i + 1]);
        let x = if b.0 > a.0 { ((t - a.0) / (b.0 - a.0)) as f32 } else { 1.0 };
        let mix = |g0: f32, g1: f32| (g0 * g0 + (g1 * g1 - g0 * g0) * x).max(0.0).sqrt();
        (mix(a.1, b.1), mix(a.2, b.2))
    }
}

/// Everything the mix engine needs for one transition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransitionPlan {
    pub kind: Kind,
    /// One line for logs and `--check-mix`.
    pub reason: String,
    /// Planner score of the chosen option (higher is better, same planner version only). Fade −2, gapless −5.
    pub score: f32,
    /// The incoming deck starts (at `in_mix_start_s`) when the outgoing reaches this point.
    pub out_mix_start_s: f64,
    pub in_mix_start_s: f64,
    /// Where the incoming deck is loaded paused: one bar before `in_mix_start_s` (≥ 0), for phase alignment.
    pub in_load_at_s: f64,
    /// Length of the blend in wall seconds.
    pub blend_s: f64,
    /// When the UI, lyrics, scrobbling etc. switch to the incoming song.
    pub handover_out_s: f64,
    /// The outgoing deck is silent from here and can be stopped.
    pub out_stop_s: f64,
    /// Incoming deck speed during the blend.
    pub in_speed: f64,
    /// Outgoing speed change before the blend (towards the shared tempo).
    pub out_ramp: Option<SpeedRamp>,
    /// Incoming speed glide through the blend (from the mix point, wall seconds) when the overlapping
    /// beats drift apart at one constant speed; `in_speed` is then the best constant speed (for engines
    /// without the glide) and `in_settle` starts from the glide's end. See [`TransitionPlan::in_speed_at`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_glide: Option<SpeedRamp>,
    /// Incoming back to 1.0× after the outgoing stops.
    pub in_settle: Option<SpeedRamp>,
    pub gain: Envelope,
    /// Sorted by `at_out_s`.
    pub eq: Vec<EqEvent>,
    /// Best stretch (outgoing file time) for the AI DJ to talk over: no lead vocal on either deck.
    pub voice_window: Option<(f64, f64)>,
    /// Both decks' loudness across the blend for the equal-loudness trim
    /// ([`crate::loudness::blend_trim`]); `None` for handovers or without analysed loudness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<crate::loudness::BlendLevels>,
}

impl TransitionPlan {
    /// "Don't mix": the next song plays gaplessly; the times only mark the outgoing end.
    pub fn gapless(out_end_s: f64, reason: impl Into<String>) -> Self {
        TransitionPlan {
            kind: Kind::Gapless,
            reason: reason.into(),
            score: -5.0,
            out_mix_start_s: out_end_s,
            in_mix_start_s: 0.0,
            in_load_at_s: 0.0,
            blend_s: 0.0,
            handover_out_s: out_end_s,
            out_stop_s: out_end_s,
            in_speed: 1.0,
            out_ramp: None,
            in_glide: None,
            in_settle: None,
            gain: Envelope { points: vec![(0.0, 0.0, 1.0)] },
            eq: Vec::new(),
            voice_window: None,
            levels: None,
        }
    }

    /// Incoming deck speed `blend_wall_s` wall seconds after the mix point, until the outgoing stops
    /// (then `in_settle` takes over).
    pub fn in_speed_at(&self, blend_wall_s: f64) -> f64 {
        match self.in_glide {
            Some(r) if r.secs > 0.0 => r.from + (r.to - r.from) * (blend_wall_s / r.secs).clamp(0.0, 1.0),
            Some(r) => r.to,
            None => self.in_speed,
        }
    }
}

/// AI DJ talk-over armed for this transition ("between songs" breaks are gapless, see [`mix_eligible`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceMode {
    /// Over the end of the outgoing song.
    Outro,
    /// Over the end of the outgoing and the start of the incoming.
    Across,
    /// Over the incoming intro (handled by the app on the incoming deck).
    Intro,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VoiceBreak {
    pub mode: VoiceMode,
    pub clip_secs: f64,
}

#[derive(Debug, Clone)]
pub struct PlanCtx<'a> {
    pub settings: &'a Mixing,
    /// Where the outgoing deck is now (file seconds).
    pub out_position_s: f64,
    pub voice_break: Option<VoiceBreak>,
    /// Genres of the outgoing and incoming song (empty when unknown), for the vocal edge rule.
    pub out_genres: &'a [String],
    pub in_genres: &'a [String],
    /// The outgoing song itself came in through an echo-out: no second one in a row (a cut, a blend or a
    /// fade instead).
    pub no_echo: bool,
}

/// Sources whose decks play the exact file that was analysed: Navidrome (raw file) and YouTube Music
/// (local copy). Jellyfin has no analysis.
pub fn mixable_source(kind: SourceKind) -> bool {
    matches!(kind, SourceKind::Subsonic | SourceKind::YouTubeMusic)
}

/// Whether `cur` → `next` may be mixed: both from [`mixable_source`]s, chosen by a DJ (or queued while
/// a DJ is on, if allowed), not consecutive album tracks, and no "between songs" break waiting.
pub fn mix_eligible(
    cur: &QueueItem,
    next: &QueueItem,
    dj_on: bool,
    between_break_armed: bool,
    settings: &Mixing,
) -> bool {
    let (a, b) = (&cur.song, &next.song);
    // Needs album ids and track numbers on both; YouTube Music list/radio songs often lack them.
    let album_run = a.album_id.is_some()
        && a.album_id == b.album_id
        && a.disc_number.unwrap_or(1) == b.disc_number.unwrap_or(1)
        && matches!((a.track_number, b.track_number), (Some(x), Some(y)) if x.abs_diff(y) == 1);
    settings.enabled
        && mixable_source(a.source)
        && mixable_source(b.source)
        && !between_break_armed
        && (next.auto || cur.auto || (dj_on && next.queued && settings.mix_manual_queue))
        && !album_run
}

/// One analysed song with a usable beat grid.
pub(crate) struct Side<'a> {
    pub(crate) f: &'a Features,
    g: &'a BeatGrid,
    /// Complete bars that end before the audio does.
    pub(crate) bars: usize,
    /// Nominal bar length (file seconds).
    pub(crate) bar_s: f64,
}

impl<'a> Side<'a> {
    pub(crate) fn new(f: &'a Features) -> Option<Self> {
        let g = f.grid.as_ref().filter(|g| g.confidence >= MIN_GRID_CONFIDENCE && g.bpm > 0.0)?;
        let bar_s = 60.0 / f64::from(g.bpm) * f64::from(g.beats_per_bar.max(1));
        let mut side = Side { f, g, bars: g.bar_count(), bar_s };
        while side.bars > 0 && side.t(side.bars as f64) > f64::from(f.audio_end) + 0.25 * bar_s {
            side.bars -= 1;
        }
        (side.bars >= MIN_BARS).then_some(side)
    }

    pub(crate) fn bpm(&self) -> f64 {
        f64::from(self.g.bpm)
    }

    /// File time of (fractional) bar `bar`.
    pub(crate) fn t(&self, bar: f64) -> f64 {
        let per = f64::from(self.g.beats_per_bar.max(1));
        f64::from(self.g.time_of((f64::from(self.g.downbeat_phase) + bar * per) as f32))
    }

    fn beat_s(&self) -> f64 {
        60.0 / self.bpm()
    }

    fn phrase(&self) -> usize {
        match self.f.phrase_bars {
            4 | 8 | 16 => usize::from(self.f.phrase_bars),
            _ => 8,
        }
    }

    fn offset(&self) -> usize {
        usize::from(self.f.phrase_offset) % self.phrase()
    }

    pub(crate) fn on_phrase(&self, bar: usize) -> bool {
        bar >= self.offset() && (bar - self.offset()).is_multiple_of(self.phrase())
    }

    /// Half/quarter-phrase points (every 4 bars on the lattice).
    fn on_subphrase(&self, bar: usize) -> bool {
        bar >= self.offset() && (bar - self.offset()).is_multiple_of(4.min(self.phrase()))
    }

    fn outro_start(&self) -> Option<usize> {
        let from_sections = self
            .f
            .sections
            .iter()
            .rev()
            .find(|s| s.kind == SectionKind::Outro)
            .map(|s| usize::from(s.start_bar));
        let from_count = (self.f.outro_instrumental_bars > 0)
            .then(|| self.bars.saturating_sub(usize::from(self.f.outro_instrumental_bars)));
        from_sections.or(from_count).filter(|&b| b < self.bars)
    }

    /// Lead-vocal likelihood of a bar (0..1); without a curve, from the instrumental intro/outro counts.
    pub(crate) fn vocal(&self, bar: usize) -> f32 {
        if let Some(&v) = self.f.bars.vocal.get(bar) {
            return f32::from(v) / 255.0;
        }
        let instrumental = bar < usize::from(self.f.intro_instrumental_bars)
            || bar >= self.bars.saturating_sub(usize::from(self.f.outro_instrumental_bars));
        if instrumental { 0.0 } else { 1.0 }
    }

    /// [`cue`] flags of a bar (none without synced lyrics, see [`crate::lyric`]).
    fn cue(&self, bar: usize) -> u8 {
        self.f.bars.cue.get(bar).copied().unwrap_or(0)
    }

    fn has_lyrics(&self) -> bool {
        !self.f.bars.cue.is_empty()
    }

    /// A sung line or the chorus ends in the bar before `bar`, and `bar` is not sung: a cue-out anchor.
    fn after_phrase(&self, bar: usize) -> bool {
        bar > 0 && self.cue(bar - 1) & (cue::LINE_END | cue::CHORUS_END) != 0 && self.vocal(bar) < QUIET_VOCAL
    }

    fn breakdown_at(&self, bar: usize) -> bool {
        self.on_phrase(bar)
            && self
                .f
                .sections
                .iter()
                .any(|s| s.kind == SectionKind::Breakdown && usize::from(s.start_bar) == bar)
    }

    pub(crate) fn energy(&self, bar: usize) -> f32 {
        self.f.bars.energy.get(bar).map_or(0.5, |&e| f32::from(e) / 255.0)
    }
}

/// The tempo moves for one candidate.
#[derive(Debug, Clone, Copy)]
struct Tempo {
    s_out: f64,
    s_in: f64,
    out_ramp: Option<SpeedRamp>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CandKind {
    Beat,
    Short,
    Filter,
}

#[derive(Debug, Clone, Copy)]
struct Cand {
    kind: CandKind,
    /// Outgoing mix-out bar, incoming mix-in bar, blend length (outgoing bars).
    m: usize,
    i: usize,
    n: usize,
    /// Outgoing bars per incoming bar (1, 2 or ½), from the overlapping bars.
    k: f64,
    /// Incoming/outgoing speed ratio at the blend's start and end ([`Local::glide`]).
    glide: Option<(f64, f64)>,
    score: f32,
    tempo: Tempo,
    clash: f32,
    /// Vocals overlap here, and only the genre rule let the candidate through.
    sampled: bool,
}

/// Limits for one plan: a normal song pair, or one leg of a bridge (see [`crate::bridge`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Opts {
    /// Longest blend in bars.
    pub(crate) max_bars: usize,
    /// Bars the incoming deck takes to glide back to 1.0× after the blend (capped at [`SETTLE_MAX_S`]).
    pub(crate) settle_bars: f64,
}

impl Opts {
    pub(crate) const NORMAL: Opts = Opts { max_bars: 32, settle_bars: 16.0 };
}

/// Plans the transition from `out` into `inn`. Falls back to [`Kind::Fade`] without a usable grid or with
/// too large a tempo gap, and to [`Kind::Gapless`] when it is too late to mix.
pub fn plan(out: &Features, inn: &Features, ctx: &PlanCtx) -> TransitionPlan {
    plan_with(out, inn, ctx, Opts::NORMAL)
}

pub(crate) fn plan_with(out: &Features, inn: &Features, ctx: &PlanCtx, opts: Opts) -> TransitionPlan {
    let mut p = plan_inner(out, inn, ctx, opts);
    p.levels = crate::loudness::plan_levels(&p, out, inn);
    p
}

fn plan_inner(out: &Features, inn: &Features, ctx: &PlanCtx, opts: Opts) -> TransitionPlan {
    let (Some(o), Some(i)) = (Side::new(out), Side::new(inn)) else {
        return fade_plan(out, inn, ctx, "no steady beat on one side");
    };
    let (k, lr) = octave_match(i.g.bpm, o.g.bpm);
    let (k, lr) = (f64::from(k), f64::from(lr));
    let max_lr = (1.0 + f64::from(ctx.settings.max_tempo_pct.max(0.0)) / 100.0).ln();
    // Echo-out or cut: no overlap, so no tempo match needed. Not on bridge legs (they exist to be
    // beat-mixed) and not under an AI DJ talk-over (it wants a blend to talk over).
    let talk = ctx.voice_break.is_some_and(|v| matches!(v.mode, VoiceMode::Outro | VoiceMode::Across));
    let hand = (opts == Opts::NORMAL && !talk).then(|| handovers(&o, &i, ctx).first().copied()).flatten();
    if lr.abs() > max_lr {
        let reason =
            format!("tempo gap {:.1} % ({:.0} → {:.0} bpm)", lr.abs().exp_m1() * 100.0, o.bpm(), i.bpm());
        return match hand {
            Some(h) => build_handover(&o, &i, h, &reason),
            None => fade_plan(out, inn, ctx, &reason),
        };
    }
    match (candidates(&o, &i, k, ctx, opts).first().copied(), hand) {
        (Some(c), Some(h)) if h.score > c.score => build_handover(&o, &i, h, "vocals on both edges"),
        (Some(c), _) => build(&o, &i, c, ctx, opts),
        (None, Some(h)) => build_handover(&o, &i, h, "no beat-matched mix point left"),
        (None, None) => fade_plan(out, inn, ctx, "no beat-matched mix point left"),
    }
}

/// The planner's options for `out` → `inn`, best first: [`plan`]'s own choice, then the other
/// beat-matched candidates and echo-outs/cuts by score (the same constraints as [`plan`]: no handovers
/// under a talk-over, no beat match past the tempo limit), then the plain fade. At most `max` plans; a
/// fade or gapless first choice has no alternatives. For [`crate::verify`], which tries them in order.
pub fn alternatives(out: &Features, inn: &Features, ctx: &PlanCtx, max: usize) -> Vec<TransitionPlan> {
    let first = plan(out, inn, ctx);
    if max <= 1 || matches!(first.kind, Kind::Fade { .. } | Kind::Gapless) {
        return vec![first];
    }
    let (Some(o), Some(i)) = (Side::new(out), Side::new(inn)) else { return vec![first] };
    let (k, lr) = octave_match(i.g.bpm, o.g.bpm);
    let (k, lr) = (f64::from(k), f64::from(lr));
    let max_lr = (1.0 + f64::from(ctx.settings.max_tempo_pct.max(0.0)) / 100.0).ln();
    let talk = ctx.voice_break.is_some_and(|v| matches!(v.mode, VoiceMode::Outro | VoiceMode::Across));
    let cands = if lr.abs() <= max_lr { candidates(&o, &i, k, ctx, Opts::NORMAL) } else { Vec::new() };
    let hands = if talk { Vec::new() } else { handovers(&o, &i, ctx) };
    // Merged by score; on equal scores the blend first (as in `plan`).
    enum Opt {
        C(Cand),
        H(Hand),
    }
    let mut opts: Vec<(f32, Opt)> = cands.into_iter().map(|c| (c.score, Opt::C(c))).collect();
    opts.extend(hands.into_iter().map(|h| (h.score, Opt::H(h))));
    opts.sort_by(|a, b| b.0.total_cmp(&a.0));
    let same = |a: &TransitionPlan, b: &TransitionPlan| {
        std::mem::discriminant(&a.kind) == std::mem::discriminant(&b.kind)
            && (a.out_mix_start_s - b.out_mix_start_s).abs() < 1e-6
            && (a.in_mix_start_s - b.in_mix_start_s).abs() < 1e-6
            && (a.blend_s - b.blend_s).abs() < 1e-6
    };
    let mut plans = vec![first];
    for (_, opt) in opts {
        if plans.len() + 1 >= max {
            break;
        }
        let mut p = match opt {
            Opt::C(c) => build(&o, &i, c, ctx, Opts::NORMAL),
            Opt::H(h) => build_handover(&o, &i, h, "alternative"),
        };
        if plans.iter().any(|q| same(q, &p)) {
            continue;
        }
        p.levels = crate::loudness::plan_levels(&p, out, inn);
        plans.push(p);
    }
    let mut fade = fade_plan(out, inn, ctx, "plain fallback");
    fade.levels = crate::loudness::plan_levels(&fade, out, inn);
    plans.push(fade);
    plans
}

/// Repeats of an echo-out fall off by this much each.
const ECHO_FEEDBACK: f32 = 0.5;
/// Crossfade of a [`Kind::Cut`] (wall s): short enough to sound like a cut, long enough not to click.
const CUT_FADE_S: f64 = 0.03;
/// Base scores: below every beat-matched blend without a clash, above [`Kind::Fade`] (−2) and a filter-out
/// over clashing vocals. An echo gets [`ECHO_SUNG`] more when the bar before the stop is sung.
const ECHO_BASE: f32 = -1.5;
const ECHO_SUNG: f32 = 0.15;
const CUT_BASE: f32 = -1.45;

/// An echo-out or cut candidate: the outgoing stops on the downbeat of bar `m`, the incoming starts at
/// its bar `i`.
#[derive(Debug, Clone, Copy)]
struct Hand {
    echo: bool,
    m: usize,
    i: usize,
    score: f32,
}

/// Echo delay: one beat, or two beats / half a beat to stay within 0.3–0.75 s.
fn echo_delay(o: &Side) -> f64 {
    let beat = o.beat_s();
    if beat < 0.3 {
        2.0 * beat
    } else if beat > 0.75 {
        beat / 2.0
    } else {
        beat
    }
}

/// How long the echo rings after the dry signal stops (wall s).
fn echo_tail(delay: f64) -> f64 {
    (crate::echo::TAPS as f64 + 0.5) * delay
}

/// The best echo-out or cut: the outgoing stops on a phrase downbeat (or after a sung phrase / at a
/// breakdown late in the song) and the incoming starts on its first phrase downbeat or, with lyrics, right
/// on a verse or chorus entry. An echo suits a loud or sung bar before the stop, a cut a quiet one.
/// Every echo-out and cut candidate, best first; no echo-outs when `ctx.no_echo`.
fn handovers(o: &Side, i: &Side, ctx: &PlanCtx) -> Vec<Hand> {
    let earliest = ctx.out_position_s + MIN_LEAD_S;
    let end = f64::from(o.f.audio_end);
    let outro = o.outro_start();
    let outro_t = outro.map_or(end, |b| o.t(b as f64));
    let delay = echo_delay(o);
    let lo_t = o.t(outro.unwrap_or(o.bars) as f64 - 16.0).max(end - 90.0).max(earliest + delay);
    let early_lo = (EARLY_OUT_FRAC * end).max(earliest + delay);
    let outs: Vec<usize> = (1..=o.bars)
        .filter(|&b| {
            let t = o.t(b as f64);
            t <= end - 0.5
                && ((t >= lo_t - 1e-6 && (o.on_subphrase(b) || Some(b) == outro))
                    || (t >= early_lo && (o.after_phrase(b) || o.breakdown_at(b))))
        })
        .collect();

    let audio_start = f64::from(i.f.audio_start);
    let Some(first_in) = (0..i.bars).find(|&b| i.on_phrase(b) && i.t(b as f64) >= audio_start - 0.05) else {
        return Vec::new();
    };
    let mut ins = vec![first_in];
    if i.has_lyrics() {
        let hi = (EARLY_IN_FRAC * f64::from(i.f.audio_end)).min(EARLY_IN_MAX_S);
        ins.extend(
            (first_in + 1..i.bars)
                .filter(|&b| i.t(b as f64) <= hi && i.cue(b) & (cue::ENTRY | cue::CHORUS) != 0),
        );
    }

    let mut all: Vec<Hand> = Vec::new();
    for &m in &outs {
        let phrase_pen = if o.on_phrase(m) || Some(m) == outro || o.after_phrase(m) { 0.0 } else { -0.25 };
        let cut = (outro_t.min(end) - o.t(m as f64)).max(0.0) as f32;
        let (vo, vo_next, eo) = (o.vocal(m - 1), o.vocal(m), o.energy(m - 1));
        // Singing runs on through the stop: mid-line (lyric_score knows it for sure with lyrics).
        let through = !o.has_lyrics() && vo >= QUIET_VOCAL && vo_next >= QUIET_VOCAL;
        for &inb in &ins {
            let skipped = (i.t(inb as f64) - i.t(first_in as f64)).max(0.0) as f32;
            let jump = (eo - i.energy(inb)).abs();
            let vi = i.vocal(inb);
            let common =
                -CUT_PER_S * cut - SKIP_PER_S * skipped + phrase_pen + lyric_score(o, i, m - 1, 1, inb);
            // Echo: the sung or loud end of a phrase repeats over the incoming start.
            let mut echo = ECHO_BASE + common - 0.25 * jump + 0.3 * (eo - 0.5);
            if vo >= QUIET_VOCAL {
                echo += ECHO_SUNG - 0.2 * vo * vi;
            }
            if through {
                echo -= 0.3;
            }
            // A cut right after a sung bar chops the line's last note; it suits a quiet bar.
            let mut cut_score = CUT_BASE + common - 0.5 * jump - 0.4 * vo;
            if through {
                cut_score -= 0.5;
            }
            // The repeats need the outgoing file to run on (a deck stops at its end, echo and all).
            let room = o.t(m as f64) + echo_tail(delay) <= end + 0.05;
            for (is_echo, score) in [(true, echo), (false, cut_score)] {
                if is_echo && (!room || ctx.no_echo) {
                    continue;
                }
                all.push(Hand { echo: is_echo, m, i: inb, score });
            }
        }
    }
    // Stable: of equal options the first found stays first.
    all.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.m.cmp(&a.m)));
    all
}

fn build_handover(o: &Side, i: &Side, h: Hand, why: &str) -> TransitionPlan {
    let stop = o.t(h.m as f64);
    let in_mix_start_s = i.t(h.i as f64);
    let delay = echo_delay(o);
    let (kind, blend_s, gain, eq, what) = if h.echo {
        let tail = echo_tail(delay);
        let echo = |send: f32, dry: f32| EqTarget::Echo {
            delay_s: delay as f32,
            feedback: ECHO_FEEDBACK,
            send,
            dry,
        };
        let eq = vec![
            // The send takes the last repeat-length before the stop…
            EqEvent { deck: Which::Out, at_out_s: stop - delay, target: echo(1.0, 1.0), ramp_s: 0.0 },
            // …then the dry signal stops and only the repeats ring on, thinning out.
            EqEvent { deck: Which::Out, at_out_s: stop, target: echo(0.0, 0.0), ramp_s: 0.0 },
            EqEvent {
                deck: Which::Out,
                at_out_s: stop,
                target: EqTarget::Highpass { hz: 1000.0, mix: 1.0 },
                ramp_s: tail,
            },
        ];
        // The outgoing deck's level stays up for the repeats; its dry part is muted in its effect.
        let gain = vec![(0.0, 1.0, 0.0), (CUT_FADE_S, 1.0, 1.0)];
        (
            Kind::EchoOut { delay_s: delay as f32 },
            tail,
            gain,
            eq,
            format!("echo-out ({:.2} s repeats)", delay),
        )
    } else {
        let gain = vec![(0.0, 1.0, 0.0), (CUT_FADE_S, 0.0, 1.0)];
        (Kind::Cut, CUT_FADE_S, gain, Vec::new(), "cut".to_string())
    };
    TransitionPlan {
        kind,
        reason: format!(
            "{what} on a downbeat (out bar {}, in bar {}), {:.1} → {:.1} bpm at their own tempos: {why}",
            h.m,
            h.i,
            o.bpm(),
            i.bpm()
        ),
        score: h.score,
        out_mix_start_s: stop,
        in_mix_start_s,
        // One incoming bar of silent pre-roll, as for the blends.
        in_load_at_s: i.t(h.i as f64 - 1.0).clamp(0.0, in_mix_start_s.max(0.0)),
        blend_s,
        handover_out_s: stop,
        out_stop_s: stop + blend_s,
        in_speed: 1.0,
        out_ramp: None,
        in_glide: None,
        in_settle: None,
        gain: Envelope { points: gain },
        eq,
        voice_window: None,
        levels: None,
    }
}

/// How a candidate deals with the tempo gap, or `None` if it can't.
fn tempo_for(o: &Side, m: usize, lr: f64, ctx: &PlanCtx) -> Option<Tempo> {
    // Each deck may move half the allowed gap, at least ±3 %.
    let per_deck = (1.0 + (f64::from(ctx.settings.max_tempo_pct) / 2.0).max(3.0) / 100.0).ln();
    let full = Tempo { s_out: 1.0, s_in: (-lr).exp(), out_ramp: None };
    if !ctx.settings.tempo_ramps || lr.abs() < TINY_TEMPO_LR {
        return (lr.abs() <= per_deck).then_some(full);
    }
    // Meet at the geometric mid tempo: the outgoing ramps over the 8 bars before the blend.
    let mix_t = o.t(m as f64);
    let ramp_start = o.t(m as f64 - 8.0).max(ctx.out_position_s + RAMP_LEAD_S);
    if mix_t - ramp_start >= 2.0 * o.bar_s && lr.abs() / 2.0 <= per_deck {
        let s_out = (lr / 2.0).exp();
        let secs = (mix_t - ramp_start) / ((1.0 + s_out) / 2.0);
        return Some(Tempo {
            s_out,
            s_in: (-lr / 2.0).exp(),
            out_ramp: Some(SpeedRamp { from: 1.0, to: s_out, start_out_s: ramp_start, secs }),
        });
    }
    // No room for the ramp: match the incoming alone if that stays in range.
    (lr.abs() <= per_deck).then_some(full)
}

/// How the overlapping beats of one candidate line up (see [`local_match`]).
#[derive(Debug, Clone, Copy)]
struct Local {
    /// Outgoing bars per incoming bar (1, 2 or ½).
    k: f64,
    /// Log tempo gap of the best constant speed, as in [`octave_match`]: the incoming plays at `e^-lr`
    /// of the outgoing's speed.
    lr: f64,
    /// Incoming/outgoing speed ratio at the start and the end of the blend, when a glide keeps the beats
    /// together better than the constant `e^-lr`.
    glide: Option<(f64, f64)>,
    /// Largest beat offset left inside the blend (s), with the glide if there is one.
    drift_s: f64,
}

/// A glide is used when it takes this much drift off (s)...
const GLIDE_MIN_GAIN_S: f64 = 0.002;
/// ...and changes the incoming speed by at most this ratio over the blend.
const GLIDE_MAX: f64 = 0.04;
/// Score per second of beat drift left in a blend beyond [`DRIFT_FREE_S`].
const DRIFT_PER_S: f32 = 20.0;
const DRIFT_FREE_S: f64 = 0.004;

/// Picks `k` from the `n` overlapping bars from out bar `m` and in bar `inb` (`k_song`, from the songs'
/// average tempos, unless another octave is clearly closer), then fits the incoming beats to the
/// outgoing ones by least squares, anchored at the shared first downbeat: once at one constant speed
/// ratio, once with a linear glide. A grid whose tempo moves away from its average (a slow intro, a
/// live drummer) would otherwise slide off by tens of ms per beat. `None` without room on the incoming
/// side.
fn local_match(o: &Side, i: &Side, m: usize, inb: usize, n: usize, k_song: f64) -> Option<Local> {
    let (m, inb, nf) = (m as f64, inb as f64, n as f64);
    let out_span = o.t(m + nf) - o.t(m);
    let gap = |k: f64| {
        let in_span = i.t(inb + nf / k) - i.t(inb);
        (in_span > 0.0 && out_span > 0.0).then(|| (out_span / in_span).ln())
    };
    let k = [k_song, 1.0, 2.0, 0.5]
        .into_iter()
        .filter(|&k| inb + nf / k <= i.bars as f64)
        .filter_map(|k| gap(k).map(|g| (k, g)))
        .min_by(|a, b| {
            let pen = |x: &(f64, f64)| x.1.abs() + if x.0 == k_song { 0.0 } else { 0.25f64.ln_1p() };
            pen(a).total_cmp(&pen(b))
        })?
        .0;
    // Every beat of the overlap (quarter bars): outgoing (x) and incoming (y) file time since the first
    // downbeat.
    let pts: Vec<(f64, f64)> = (1..=4 * n)
        .map(|q| {
            let f = q as f64 / 4.0;
            (o.t(m + f) - o.t(m), i.t(inb + f / k) - i.t(inb))
        })
        .collect();
    let sum = |f: &dyn Fn(f64, f64) -> f64| pts.iter().map(|&(x, y)| f(x, y)).sum::<f64>();
    let (xx, xy) = (sum(&|x, _| x * x), sum(&|x, y| x * y));
    // Incoming file seconds per outgoing file second.
    let ratio = xy / xx.max(1e-12);
    if ratio <= 0.0 {
        return None;
    }
    let worst = |f: &dyn Fn(f64) -> f64| pts.iter().map(|&(x, y)| (y - f(x)).abs()).fold(0.0, f64::max);
    let flat = worst(&|x| ratio * x);
    // Below the grid's precision (f32 beat times): the same tempo.
    let lr = if ratio.ln().abs() < 1e-5 { 0.0 } else { -ratio.ln() };
    let mut local = Local { k, lr, glide: None, drift_s: flat };
    // y = a·x + b·x²: the speed ratio glides from a to a + 2b·span.
    let (x3, x4, x2y) = (sum(&|x, _| x.powi(3)), sum(&|x, _| x.powi(4)), sum(&|x, y| x * x * y));
    let det = xx * x4 - x3 * x3;
    if det > 1e-12 {
        let a = (xy * x4 - x2y * x3) / det;
        let b = (xx * x2y - x3 * xy) / det;
        let end = a + 2.0 * b * out_span;
        let curved = worst(&|x| a * x + b * x * x);
        if a > 0.0 && end > 0.0 && (end / a).ln().abs() <= GLIDE_MAX && flat - curved > GLIDE_MIN_GAIN_S {
            local.glide = Some((a, end));
            local.drift_s = curved;
        }
    }
    Some(local)
}

/// Every beat-matched candidate, best first.
fn candidates(o: &Side, i: &Side, k: f64, ctx: &PlanCtx, opts: Opts) -> Vec<Cand> {
    let s = ctx.settings;
    let earliest = ctx.out_position_s + MIN_LEAD_S;
    let end = f64::from(o.f.audio_end);
    let outro = o.outro_start();
    let outro_t = outro.map_or(end, |b| o.t(b as f64));

    // Mix-out window: 16 bars before the outro (within the last 90 s) to the end, not before `earliest`.
    let lo_t = o.t(outro.unwrap_or(o.bars) as f64 - 16.0).max(end - 90.0).max(earliest);
    // DJustify-style cue anchors earlier in the last 40 %: the bar after a sung phrase (when a gap
    // follows) and breakdowns. Cutting the song short costs `CUT_PER_S` per second.
    let early_lo = (EARLY_OUT_FRAC * end).max(earliest);
    let outs: Vec<usize> = (0..o.bars)
        .filter(|&b| {
            let t = o.t(b as f64);
            (t >= lo_t - 1e-6 && (o.on_subphrase(b) || Some(b) == outro))
                || (t >= early_lo && (o.after_phrase(b) || o.breakdown_at(b)))
        })
        .collect();

    // Mix-in: the first phrase downbeat with audio, plus the next phrase for a very long intro.
    let audio_start = f64::from(i.f.audio_start);
    let Some(first_in) = (0..i.bars).find(|&b| i.on_phrase(b) && i.t(b as f64) >= audio_start - 0.05) else {
        return Vec::new();
    };
    let mut ins = vec![first_in];
    if i.f.intro_instrumental_bars > 32 && first_in + i.phrase() < i.bars {
        ins.push(first_in + i.phrase());
    }
    // With lyrics: 1, 2 or 4 bars before a verse or chorus entry early in the song, so a short blend
    // hands over right where the singing starts (the rest of the intro is skipped).
    if i.has_lyrics() {
        let hi = (EARLY_IN_FRAC * f64::from(i.f.audio_end)).min(EARLY_IN_MAX_S);
        for e in (first_in + 1..i.bars).filter(|&b| i.t(b as f64) <= hi) {
            if i.cue(e) & (cue::ENTRY | cue::CHORUS) != 0 {
                ins.extend([1, 2, 4].iter().filter(|&&d| e >= first_in + d).map(|&d| e - d));
            }
        }
        ins.sort_unstable();
        ins.dedup();
    }

    // Each edge is relaxed by its own song's genre: the outro for the outgoing, the intro for the incoming.
    let relax_out = relax_vocal_edge(s.transition_edges, ctx.out_genres);
    let relax_in = relax_vocal_edge(s.transition_edges, ctx.in_genres);

    let voice = ctx.voice_break.filter(|v| matches!(v.mode, VoiceMode::Outro | VoiceMode::Across));
    let long = usize::from(s.long_blend_bars).clamp(8, 32).min(opts.max_bars.max(1));
    let mut lengths = vec![long, 8, 4, 2, 1];
    if voice.is_some() {
        lengths.insert(0, (2 * long).min(32).min(opts.max_bars.max(1))); // talk-over wants room
    }
    lengths.retain(|&n| n <= opts.max_bars.max(1));
    lengths.dedup();

    let mut all: Vec<Cand> = Vec::new();
    let mut consider = |c: Cand| all.push(c);

    let max_lr = (1.0 + f64::from(s.max_tempo_pct.max(0.0)) / 100.0).ln();
    for &m in &outs {
        let phrase_pen = if o.on_phrase(m) || Some(m) == outro || o.after_phrase(m) { 0.0 } else { -0.25 };
        for &inb in &ins {
            let mut kinds: Vec<(CandKind, usize)> =
                lengths.iter().map(|&n| (if n >= 8 { CandKind::Beat } else { CandKind::Short }, n)).collect();
            kinds.push((CandKind::Filter, 2));
            for (kind, n) in kinds {
                if m + n > o.bars {
                    continue;
                }
                // The tempo of the bars that actually overlap, not the songs' averages.
                let Some(local) = local_match(o, i, m, inb, n, k).filter(|l| l.lr.abs() <= max_lr) else {
                    continue;
                };
                let (k, lr) = (local.k, local.lr);
                let in_bar = |j: f64| inb + (j / k).floor() as usize;
                if in_bar(n as f64 - 1.0) >= i.bars {
                    continue;
                }
                let Some(tempo) = tempo_for(o, m, lr, ctx) else { continue };
                let (mut clash, mut cover, mut raw_clash) = (0.0f32, 0.0f32, 0.0f32);
                for j in 0..n {
                    let (vo, vi) = (o.vocal(m + j), i.vocal(in_bar(j as f64)));
                    // Relaxed edges don't count as a clash; `cover` still prefers quieter sections.
                    clash += if relax_out || relax_in { 0.0 } else { vo * vi };
                    raw_clash += vo * vi;
                    cover += (1.0 - vo) * (1.0 - vi);
                }
                // Long blends need (nearly) instrumental overlaps; short ones must fit between vocals.
                if (kind == CandKind::Beat && clash > 0.1 * n as f32)
                    || (kind == CandKind::Short && clash > 0.25)
                {
                    continue;
                }
                let half = n as f64 / 2.0;
                let jump = (o.energy((m + n / 2).saturating_sub(1).max(m)) - i.energy(in_bar(half))).abs();
                let cut = (outro_t.min(end) - o.t(m as f64 + half)).max(0.0) as f32;
                // (Only lyric cue-ins skip intro bars on purpose; the long-intro case keeps its old score.)
                let skipped = if i.has_lyrics() {
                    (i.t(inb as f64) - i.t(first_in as f64)).max(0.0) as f32
                } else {
                    0.0
                };
                let drift = (local.drift_s - DRIFT_FREE_S).max(0.0) as f32;
                let mut score = 0.15 * cover - 0.5 * jump - CUT_PER_S * cut - SKIP_PER_S * skipped
                    + phrase_pen
                    - DRIFT_PER_S * drift;
                score += lyric_score(o, i, m, n, in_bar(n as f64));
                score += match kind {
                    CandKind::Beat | CandKind::Short => -3.0 * clash,
                    // Overlap hurts less when filtered, but it's the least musical option.
                    CandKind::Filter => -1.5 - clash,
                };
                if kind != CandKind::Filter && Some(m) == outro && inb == first_in {
                    score += 1.0; // Mixxx "full intro + outro"
                }
                if let Some(v) = voice {
                    let (a, b) = quiet_run(o, i, m, inb, n, k, ctx.out_position_s);
                    let secs = (b - a) / tempo.s_out;
                    score += 1.5 * (secs / v.clip_secs.max(1.0)).min(1.0) as f32;
                }
                let sampled = kind == CandKind::Beat && raw_clash > clash && raw_clash > 0.1 * n as f32;
                consider(Cand { kind, m, i: inb, n, k, glide: local.glide, score, tempo, clash, sampled });
            }
        }
    }
    // Stable: of equal options the first found stays first.
    all.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.m.cmp(&a.m)).then(b.n.cmp(&a.n)));
    all
}

/// What synced lyrics add to a candidate (0 without them): a bonus when the incoming singing (or its
/// chorus) starts right after the blend, a penalty when the outgoing stops in the middle of a sung line.
fn lyric_score(o: &Side, i: &Side, m: usize, n: usize, in_after: usize) -> f32 {
    let mut score = 0.0;
    let lands = i.cue(in_after);
    if lands & cue::CHORUS != 0 {
        score += 0.5;
    } else if lands & cue::ENTRY != 0 {
        score += 0.3;
    }
    let last = m + n - 1;
    if o.has_lyrics()
        && o.vocal(last) >= QUIET_VOCAL
        && o.vocal(last + 1) >= QUIET_VOCAL
        && o.cue(last) & cue::LINE_END == 0
    {
        score -= 0.5;
    }
    score
}

/// Longest vocal-free stretch (outgoing file time) touching the blend, from up to 16 bars before to its end.
fn quiet_run(o: &Side, i: &Side, m: usize, inb: usize, n: usize, k: f64, now: f64) -> (f64, f64) {
    let quiet = |b: usize| {
        o.vocal(b) < QUIET_VOCAL
            && (b < m || i.vocal(inb + ((b - m) as f64 / k).floor() as usize) < QUIET_VOCAL)
    };
    let first = (m.saturating_sub(16)..m).find(|&b| o.t(b as f64) >= now).unwrap_or(m);
    let (mut best, mut run_start) = ((m, m), None);
    for b in first..m + n {
        match (quiet(b), run_start) {
            (true, None) => run_start = Some(b),
            (false, Some(s)) => {
                if b > m && b - s > best.1 - best.0 {
                    best = (s, b);
                }
                run_start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = run_start
        && m + n - s > best.1 - best.0
    {
        best = (s, m + n);
    }
    (o.t(best.0 as f64), o.t(best.1 as f64))
}

fn build(o: &Side, i: &Side, c: Cand, ctx: &PlanCtx, opts: Opts) -> TransitionPlan {
    let s = ctx.settings;
    let k = c.k;
    let Tempo { s_out, s_in, out_ramp } = c.tempo;
    let (m, n) = (c.m as f64, c.n as f64);
    let out_mix_start_s = o.t(m);
    let out_stop_s = o.t(m + n);
    let blend_s = (out_stop_s - out_mix_start_s) / s_out;
    let beat_wall = o.beat_s() / s_out;
    let swap_at = o.t(m + (c.n / 2) as f64);
    let bass_swap = s.bass_swap && c.n >= 2;

    let mut eq = Vec::new();
    if bass_swap && c.kind != CandKind::Filter {
        eq.push(EqEvent {
            deck: Which::In,
            at_out_s: out_mix_start_s,
            target: EqTarget::Bass(BASS_CUT_DB),
            ramp_s: 0.0,
        });
        eq.push(EqEvent {
            deck: Which::Out,
            at_out_s: swap_at,
            target: EqTarget::Bass(BASS_CUT_DB),
            ramp_s: beat_wall,
        });
        eq.push(EqEvent {
            deck: Which::In,
            at_out_s: swap_at,
            target: EqTarget::Bass(0.0),
            ramp_s: beat_wall,
        });
    }
    let bars = c.n.min(255) as u8;
    let (kind, points, what) = match c.kind {
        CandKind::Beat => (
            Kind::BeatBlend { bars, bass_swap_bar: bass_swap.then_some(bars / 2) },
            vec![(0.0, 1.0, 0.0), (blend_s / 2.0, 1.0, 1.0), (blend_s, 0.0, 1.0)],
            if c.sampled { "beat blend over vocal samples" } else { "beat blend over instrumental parts" },
        ),
        CandKind::Short => (
            Kind::ShortBlend { bars },
            vec![(0.0, 1.0, 0.0), (blend_s, 0.0, 1.0)],
            if c.clash > 0.05 { "short blend, vocals touch" } else { "short blend between the vocals" },
        ),
        CandKind::Filter => {
            eq.push(EqEvent {
                deck: Which::Out,
                at_out_s: out_mix_start_s,
                target: EqTarget::Highpass { hz: 20.0, mix: 1.0 },
                ramp_s: 0.0,
            });
            eq.push(EqEvent {
                deck: Which::Out,
                at_out_s: out_mix_start_s,
                target: EqTarget::Highpass { hz: 800.0, mix: 1.0 },
                ramp_s: blend_s,
            });
            (
                Kind::FilterOut { bars },
                vec![(0.0, 1.0, 1.0), (blend_s, 0.0, 1.0)],
                "filter-out, vocals can't be kept apart",
            )
        }
    };
    eq.sort_by(|a, b| a.at_out_s.total_cmp(&b.at_out_s));

    let in_glide = c.glide.map(|(a, b)| SpeedRamp {
        from: a * s_out,
        to: b * s_out,
        start_out_s: out_mix_start_s,
        secs: blend_s,
    });
    let in_end = in_glide.map_or(s_in, |g| g.to);
    let in_settle = (s.tempo_ramps && (in_end - 1.0).abs() > 1e-4).then(|| SpeedRamp {
        from: in_end,
        to: 1.0,
        start_out_s: out_stop_s,
        // 16 bars, capped: a slow song's 16 bars can take a minute, blocking the next transition.
        secs: (opts.settle_bars * i.bar_s / ((1.0 + in_end) / 2.0)).min(SETTLE_MAX_S),
    });
    let (vw_a, vw_b) = quiet_run(o, i, c.m, c.i, c.n, k, ctx.out_position_s);
    let in_mix_start_s = i.t(c.i as f64);
    TransitionPlan {
        kind,
        reason: format!(
            "{}-bar {what} (out bar {}, in bar {}), {:.1} → {:.1} bpm{}",
            c.n,
            c.m,
            c.i,
            o.bpm(),
            i.bpm(),
            if k == 1.0 { String::new() } else { format!(", ×{k} time") }
        ),
        score: c.score,
        out_mix_start_s,
        in_mix_start_s,
        // A first downbeat can sit a hair before 0 s.
        in_load_at_s: i.t(c.i as f64 - 1.0).clamp(0.0, in_mix_start_s.max(0.0)),
        blend_s,
        handover_out_s: o.t(m + n / 2.0),
        out_stop_s,
        in_speed: s_in,
        out_ramp,
        in_glide,
        in_settle,
        gain: Envelope { points },
        eq,
        voice_window: (vw_b - vw_a > 0.5).then_some((vw_a, vw_b)),
        levels: None,
    }
}

/// MixRamp-style start: the first bar in the last ~20 s after which energy stays well below the median.
fn quiet_tail_start(o: &Side) -> Option<f64> {
    let curve = &o.f.bars.energy;
    if curve.len() < MIN_BARS {
        return None;
    }
    let mut sorted = curve.clone();
    sorted.sort_unstable();
    let median = f32::from(sorted[sorted.len() / 2]);
    let end = f64::from(o.f.audio_end);
    let last = o.bars.min(curve.len());
    let mut start = None;
    for b in (0..last).rev() {
        if o.t(b as f64) < end - 20.0 || f32::from(curve[b]) >= 0.5 * median {
            break;
        }
        start = Some(b);
    }
    start.map(|b| o.t(b as f64)).filter(|&t| t < end - 1.0)
}

/// Plain equal-power crossfade of `fallback_fade_secs` (3–6 s).
fn fade_plan(out: &Features, inn: &Features, ctx: &PlanCtx, why: &str) -> TransitionPlan {
    let end = f64::from(out.audio_end);
    let secs = f64::from(ctx.settings.fallback_fade_secs).clamp(3.0, 6.0);
    let o = Side::new(out);
    let mut start = o.as_ref().and_then(quiet_tail_start).unwrap_or(end - secs);
    // With a grid, start on the nearest downbeat at most one beat after the natural start.
    if let Some(o) = &o {
        let bar = ((start - o.t(0.0)) / o.bar_s).round();
        let snapped = o.t(bar);
        start = if snapped <= start + o.beat_s() { snapped } else { o.t(bar - 1.0) };
    }
    start = start.max(ctx.out_position_s + MIN_LEAD_S);
    let secs = secs.min(end - start);
    if secs < 1.0 {
        return TransitionPlan::gapless(end, format!("too late to mix ({why})"));
    }
    let in_mix_start_s = f64::from(inn.audio_start).max(0.0);
    let pre_roll = Side::new(inn).map_or(1.0, |i| i.bar_s);
    let voice_window = ctx
        .voice_break
        .filter(|v| matches!(v.mode, VoiceMode::Outro | VoiceMode::Across))
        .map(|v| ((start - (v.clip_secs - secs).max(0.0)).max(ctx.out_position_s), start + secs));
    TransitionPlan {
        kind: Kind::Fade { secs: secs as f32 },
        reason: format!("{secs:.1} s fade: {why}"),
        score: -2.0,
        out_mix_start_s: start,
        in_mix_start_s,
        in_load_at_s: (in_mix_start_s - pre_roll).max(0.0),
        blend_s: secs,
        handover_out_s: start + secs / 2.0,
        out_stop_s: start + secs,
        in_speed: 1.0,
        out_ramp: None,
        in_glide: None,
        in_settle: None,
        gain: Envelope { points: vec![(0.0, 1.0, 0.0), (secs, 0.0, 1.0)] },
        eq: Vec::new(),
        voice_window,
        levels: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{FeatBuilder, song};
    use listen_core::model::SourceKind;
    use listen_core::settings::TransitionEdges;

    fn settings() -> Mixing {
        Mixing::default()
    }

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

    fn assert_in_downbeats(f: &Features, t: f64) {
        let g = f.grid.as_ref().unwrap();
        assert!(g.downbeats().any(|d| (f64::from(d) - t).abs() < 1e-3), "{t} is not a downbeat");
    }

    /// `f` merged with lyrics sung over `sung` bar ranges (chorus = `chorus` bar ranges).
    fn with_lyrics(f: &Features, sung: &[(usize, usize)], chorus: &[(usize, usize)]) -> Features {
        let spans = crate::lyric::bar_spans(f);
        let t = |b: usize| if b < spans.len() { spans[b].0 } else { spans[spans.len() - 1].1 };
        let map = listen_core::analysis::VocalMap {
            sung: sung.iter().map(|&(a, b)| (t(a), t(b) - 0.2)).collect(),
            line_ends: sung
                .iter()
                .flat_map(|&(a, b)| (a + 2..=b).step_by(2).map(move |e| t(e) - 0.2))
                .collect(),
            chorus: chorus.iter().map(|&(a, b)| (t(a), t(b) - 0.2)).collect(),
        };
        crate::lyric::with_lyrics(f, &map).expect("lyrics fit")
    }

    #[test]
    fn a_lyric_gap_late_in_the_song_is_a_cue_out() {
        let s = settings();
        // Both sung from start to end; the lyrics show an 8-bar gap at bars 64–72 of the outgoing
        // that the audio curve doesn't see.
        let out = FeatBuilder::new(120.0, 96).intro(0).outro(0).build();
        let inn = FeatBuilder::new(120.0, 96).intro(0).outro(8).build();
        let base = plan(&out, &inn, &ctx(&s, 30.0));
        assert!(!matches!(base.kind, Kind::BeatBlend { .. }), "{}", base.reason);
        let lyr = with_lyrics(&out, &[(0, 64), (72, 96)], &[]);
        let p = plan(&lyr, &inn, &ctx(&s, 30.0));
        assert!(matches!(p.kind, Kind::BeatBlend { bars: 8, .. }), "{}", p.reason);
        let og = lyr.grid.as_ref().unwrap();
        assert!((p.out_mix_start_s - f64::from(og.bar_time(64))).abs() < 1e-3, "{}", p.reason);
        // Before 60 % of the song a gap is no anchor.
        let early = with_lyrics(&out, &[(0, 40), (48, 96)], &[]);
        let p = plan(&early, &inn, &ctx(&s, 30.0));
        assert!(p.out_mix_start_s > f64::from(og.bar_time(56)), "{}", p.reason);
    }

    #[test]
    fn blends_land_on_the_incoming_chorus() {
        let s = settings();
        // The outgoing sings to its end; the incoming has a 16-bar instrumental intro, chorus at 16.
        let out = FeatBuilder::new(120.0, 96).intro(8).outro(0).build();
        let out = with_lyrics(&out, &[(8, 96)], &[]);
        let inn = FeatBuilder::new(120.0, 96).intro(16).outro(8).build();
        let lyr_in = with_lyrics(&inn, &[(16, 88)], &[(16, 32), (56, 72)]);
        let p = plan(&out, &lyr_in, &ctx(&s, 30.0));
        let after = p.in_mix_start_s + p.blend_s * p.in_speed;
        let chorus = f64::from(lyr_in.grid.as_ref().unwrap().bar_time(16));
        assert!((after - chorus).abs() < 0.05, "lands on the chorus: {}", p.reason);
    }

    #[test]
    fn stopping_in_the_middle_of_a_sung_line_costs() {
        let out = FeatBuilder::new(120.0, 64).intro(8).outro(0).build();
        let inn = FeatBuilder::new(120.0, 64).intro(8).outro(8).build();
        let o = with_lyrics(&out, &[(8, 64)], &[]);
        let (os, is) = (Side::new(&o).unwrap(), Side::new(&inn).unwrap());
        // Line ends every two bars (in bars 9, 11, …): stopping after bar 10 cuts a line, after 11 doesn't.
        assert!(lyric_score(&os, &is, 9, 2, 0) < -0.4);
        assert_eq!(lyric_score(&os, &is, 10, 2, 0), 0.0);
        // No lyrics, no opinion.
        assert_eq!(lyric_score(&Side::new(&out).unwrap(), &is, 9, 2, 0), 0.0);
    }

    #[test]
    fn instrumental_outro_and_intro_make_a_long_beat_blend() {
        let s = settings();
        let out = FeatBuilder::new(124.0, 96).intro(8).outro(16).build();
        let inn = FeatBuilder::new(124.0, 96).intro(16).outro(8).build();
        let p = plan(&out, &inn, &ctx(&s, 30.0));
        assert_eq!(p.kind, Kind::BeatBlend { bars: 16, bass_swap_bar: Some(8) }, "{}", p.reason);
        let og = out.grid.as_ref().unwrap();
        // Full intro + outro: starts at the outro, enters at the first downbeat.
        assert!((p.out_mix_start_s - f64::from(og.bar_time(80))).abs() < 1e-3);
        assert!((p.in_mix_start_s - f64::from(inn.grid.as_ref().unwrap().bar_time(0))).abs() < 1e-3);
        assert!(p.in_load_at_s <= p.in_mix_start_s && p.in_load_at_s >= 0.0);
        // Handover in the middle; bass swap on a downbeat there.
        let mid = (p.out_mix_start_s + p.out_stop_s) / 2.0;
        assert!((p.handover_out_s - mid).abs() < 0.05);
        let swaps: Vec<_> = p.eq.iter().filter(|e| e.ramp_s > 0.0).collect();
        assert_eq!(swaps.len(), 2);
        for e in swaps {
            assert_in_downbeats(&out, e.at_out_s);
            assert!((e.at_out_s - p.handover_out_s).abs() < 1e-3);
        }
        assert!(p.eq.iter().any(|e| e.deck == Which::In && e.target == EqTarget::Bass(BASS_CUT_DB)));
        // Equal-power: each ramp is √-shaped, never a dip below unity power.
        let b = p.blend_s;
        for step in 0..=100 {
            let t = b * f64::from(step) / 100.0;
            let (go, gi) = p.gain.gains_at(t);
            let power = go * go + gi * gi;
            assert!((0.999..=2.001).contains(&power), "{t}: {power}");
            if t <= b / 2.0 {
                assert_eq!(go, 1.0);
                assert!((gi * gi - (t / (b / 2.0)) as f32).abs() < 1e-4);
            } else {
                assert!((gi - 1.0).abs() < 1e-6);
            }
        }
        assert_eq!(p.gain.gains_at(b), (0.0, 1.0));
        assert!((p.out_stop_s - f64::from(og.bar_time(96))).abs() < 1e-3);
        assert_eq!(p.in_speed, 1.0);
        assert!(p.out_ramp.is_none() && p.in_settle.is_none());
    }

    #[test]
    fn bass_swap_and_long_bars_follow_the_settings() {
        let s = Mixing { bass_swap: false, long_blend_bars: 8, ..settings() };
        let out = FeatBuilder::new(124.0, 96).intro(8).outro(16).build();
        let inn = FeatBuilder::new(124.0, 96).intro(16).outro(8).build();
        let p = plan(&out, &inn, &ctx(&s, 30.0));
        assert_eq!(p.kind, Kind::BeatBlend { bars: 8, bass_swap_bar: None });
        assert!(p.eq.is_empty());
    }

    #[test]
    fn tempo_is_split_with_ramps_within_three_percent_each() {
        let s = settings();
        let out = FeatBuilder::new(120.0, 96).intro(8).outro(16).build();
        let inn = FeatBuilder::new(126.0, 96).intro(16).outro(8).build();
        let p = plan(&out, &inn, &ctx(&s, 30.0));
        assert!(matches!(p.kind, Kind::BeatBlend { .. }), "{}", p.reason);
        let r = p.out_ramp.expect("outgoing pre-ramp");
        let mid = (120.0f64 * 126.0).sqrt();
        assert!((r.to - mid / 120.0).abs() < 1e-3 && (p.in_speed - mid / 126.0).abs() < 1e-3);
        assert!((r.to - 1.0).abs() <= 0.03 && (p.in_speed - 1.0).abs() <= 0.03);
        // The ramp ends exactly at the mix start, 8 bars after it began.
        assert!(r.start_out_s < p.out_mix_start_s);
        let bar = 4.0 * 60.0 / 120.0;
        assert!((p.out_mix_start_s - r.start_out_s - 8.0 * bar).abs() < 1e-3);
        let settle = p.in_settle.expect("incoming settles back");
        assert_eq!((settle.from, settle.to, settle.start_out_s), (p.in_speed, 1.0, p.out_stop_s));
        // Blend wall time accounts for the outgoing speed.
        assert!((p.blend_s - 16.0 * bar / r.to).abs() < 1e-3);

        // Without ramps the incoming matches the outgoing alone (5 % > 3 %: no).
        let flat = Mixing { tempo_ramps: false, ..settings() };
        let p = plan(&out, &inn, &ctx(&flat, 30.0));
        assert!(matches!(p.kind, Kind::EchoOut { .. } | Kind::Cut), "{}", p.reason);
        let near = FeatBuilder::new(123.0, 96).intro(16).outro(8).build();
        let p = plan(&out, &near, &ctx(&flat, 30.0));
        assert!(matches!(p.kind, Kind::BeatBlend { .. }));
        assert!(p.out_ramp.is_none() && p.in_settle.is_none());
        assert!((p.in_speed - 120.0 / 123.0).abs() < 1e-4);
    }

    #[test]
    fn vocals_on_both_sides_give_a_short_blend_after_the_vocal() {
        let s = settings();
        let out = FeatBuilder::new(124.0, 64).intro(2).outro(2).build();
        let inn = FeatBuilder::new(124.0, 64).intro(2).outro(2).build();
        let p = plan(&out, &inn, &ctx(&s, 30.0));
        let Kind::ShortBlend { bars } = p.kind else { panic!("{:?}: {}", p.kind, p.reason) };
        assert!((1..=4).contains(&bars));
        let (og, ig) = (out.grid.as_ref().unwrap(), inn.grid.as_ref().unwrap());
        // Outgoing vocal ends at bar 62; the incoming vocal starts at its bar 2.
        let out_vocal_end = f64::from(og.bar_time(62));
        let in_vocal_start = f64::from(ig.bar_time(2));
        let in_vocal_wall = p.out_mix_start_s + (in_vocal_start - p.in_mix_start_s) / p.in_speed;
        assert!(
            in_vocal_wall >= out_vocal_end - 1e-3,
            "incoming sings at {in_vocal_wall}, outgoing until {out_vocal_end}"
        );
        assert_eq!(p.gain.gains_at(0.0), (1.0, 0.0));
        // A simultaneous equal-power crossfade.
        let (go, gi) = p.gain.gains_at(p.blend_s / 3.0);
        assert!((go * go + gi * gi - 1.0).abs() < 1e-4);
    }

    #[test]
    fn unavoidable_vocal_clash_echoes_out() {
        let s = settings();
        let out = FeatBuilder::new(124.0, 64).intro(0).outro(0).build();
        let inn = FeatBuilder::new(124.0, 64).intro(0).outro(0).build();
        let p = plan(&out, &inn, &ctx(&s, 30.0));
        let Kind::EchoOut { delay_s } = p.kind else { panic!("{:?}: {}", p.kind, p.reason) };
        // One beat at 124 bpm.
        assert!((f64::from(delay_s) - 60.0 / 124.0).abs() < 1e-3);
        assert!(p.reason.contains("vocals on both edges"), "{}", p.reason);
        assert_in_downbeats(&out, p.out_mix_start_s);
        assert_eq!((p.in_speed, p.out_ramp, p.in_settle), (1.0, None, None));
        assert!(p.eq.iter().all(|e| e.deck == Which::Out));
        let echo: Vec<_> =
            p.eq.iter()
                .filter_map(|e| crate::echo::EchoParams::of(e.target).map(|x| (e.at_out_s, x)))
                .collect();
        assert_eq!(echo.len(), 2);
        // The send opens one repeat before the stop, then the dry signal goes and the repeats ring on.
        assert!((echo[0].0 - (p.out_mix_start_s - f64::from(delay_s))).abs() < 1e-6);
        assert_eq!((echo[0].1.send, echo[0].1.dry), (1.0, 1.0));
        assert_eq!((echo[1].0, echo[1].1.send, echo[1].1.dry), (p.out_mix_start_s, 0.0, 0.0));
        assert!((p.out_stop_s - p.out_mix_start_s - echo_tail(f64::from(delay_s))).abs() < 1e-6);
        assert_eq!(p.handover_out_s, p.out_mix_start_s);
        assert_eq!(p.gain.gains_at(1.0), (1.0, 1.0));

        // Under an AI DJ talk-over it stays a filter-out (the talk wants a blend).
        let c = PlanCtx {
            voice_break: Some(VoiceBreak { mode: VoiceMode::Outro, clip_secs: 6.0 }),
            ..ctx(&s, 30.0)
        };
        let p = plan(&out, &inn, &c);
        assert_eq!(p.kind, Kind::FilterOut { bars: 2 }, "{}", p.reason);
        assert_eq!(p.gain.gains_at(0.0), (1.0, 1.0));
        let hp: Vec<_> = p.eq.iter().filter(|e| matches!(e.target, EqTarget::Highpass { .. })).collect();
        assert_eq!(hp.len(), 2);
        assert_eq!(hp[1].target, EqTarget::Highpass { hz: 800.0, mix: 1.0 });
        assert!((hp[1].ramp_s - p.blend_s).abs() < 1e-9);
        assert_in_downbeats(&out, p.out_mix_start_s);
    }

    #[test]
    fn vocal_edges_block_a_blend_only_where_the_rule_says() {
        use listen_core::settings::TransitionEdges::{Always, Instrumental, Smart};
        let club = vec!["UK Garage".to_string()];
        let pop = vec!["Pop".to_string()];
        let out = FeatBuilder::new(124.0, 64).intro(0).outro(0).build();
        let inn = FeatBuilder::new(124.0, 64).intro(0).outro(0).build();
        let none: Vec<String> = Vec::new();
        for (mode, og, ig, blends) in [
            (Instrumental, &club, &club, false),
            (Always, &none, &none, true),
            (Always, &pop, &pop, true),
            (Smart, &club, &club, true),
            (Smart, &club, &pop, true),
            (Smart, &pop, &club, true),
            (Smart, &pop, &pop, false),
            (Smart, &none, &none, false),
        ] {
            let s = Mixing { transition_edges: mode, ..settings() };
            let c = PlanCtx { out_genres: og, in_genres: ig, ..ctx(&s, 30.0) };
            let p = plan(&out, &inn, &c);
            let beat = matches!(p.kind, Kind::BeatBlend { .. });
            assert_eq!(beat, blends, "{mode:?} {og:?} {ig:?}: {:?} {}", p.kind, p.reason);
            if !blends {
                assert!(matches!(p.kind, Kind::EchoOut { .. }), "{:?}", p.kind);
            }
        }
    }

    #[test]
    fn relaxed_edges_still_prefer_the_quieter_section() {
        // The outgoing song has a long instrumental outro; with relaxed vocals the blend still starts there.
        let s = Mixing { transition_edges: TransitionEdges::Always, ..settings() };
        let out = FeatBuilder::new(124.0, 96).intro(8).outro(16).build();
        let inn = FeatBuilder::new(124.0, 96).intro(16).outro(8).build();
        let p = plan(&out, &inn, &ctx(&s, 30.0));
        let og = out.grid.as_ref().unwrap();
        assert!(matches!(p.kind, Kind::BeatBlend { .. }), "{}", p.reason);
        assert!((p.out_mix_start_s - f64::from(og.bar_time(80))).abs() < 1e-3, "{}", p.reason);
    }

    #[test]
    fn half_time_incoming_settles_within_the_cap() {
        // 118 → 62 bpm (×2 time, 5 % apart): 16 of its 3.9 s bars would be a minute of settling.
        let s = settings();
        let out = FeatBuilder::new(118.0, 96).intro(8).outro(16).build();
        let inn = FeatBuilder::new(62.0, 48).intro(8).outro(4).build();
        let p = plan(&out, &inn, &ctx(&s, 60.0));
        let settle = p.in_settle.expect("tempo-matched, so it settles");
        assert!(settle.secs <= SETTLE_MAX_S, "{}", settle.secs);
    }

    #[test]
    fn large_tempo_gap_cuts_and_no_grid_fades() {
        let s = settings();
        let out = FeatBuilder::new(120.0, 96).intro(8).outro(16).build();
        let inn = FeatBuilder::new(130.8, 96).intro(16).outro(8).build(); // +9 %
        // The last sung bar echoes out at the start of the outro, into the first incoming downbeat, each at
        // its own tempo.
        let p = plan(&out, &inn, &ctx(&s, 30.0));
        assert_eq!(p.kind, Kind::EchoOut { delay_s: 0.5 }, "{}", p.reason);
        assert!(p.reason.contains("tempo gap"), "{}", p.reason);
        let og = out.grid.as_ref().unwrap();
        assert!((p.out_mix_start_s - f64::from(og.bar_time(80))).abs() < 1e-3, "{}", p.reason);
        assert_in_downbeats(&inn, p.in_mix_start_s);
        assert_eq!((p.in_speed, p.out_ramp, p.in_settle), (1.0, None, None));

        // The outgoing came in through an echo-out itself: never two in a row, a cut instead (and no echo
        // among the alternatives render-and-verify may fall back to).
        let after_echo = PlanCtx { no_echo: true, ..ctx(&s, 30.0) };
        let p = plan(&out, &inn, &after_echo);
        assert_eq!(p.kind, Kind::Cut, "{}", p.reason);
        let alts = alternatives(&out, &inn, &after_echo, 8);
        assert!(alts.len() > 1 && alts.iter().all(|a| !matches!(a.kind, Kind::EchoOut { .. })), "{alts:?}");

        // Only instrumental bars in reach: a cut on a phrase downbeat.
        let long_outro = FeatBuilder::new(120.0, 96).intro(8).outro(48).build();
        let p = plan(&long_outro, &inn, &ctx(&s, 30.0));
        assert_eq!(p.kind, Kind::Cut, "{}", p.reason);
        assert_in_downbeats(&long_outro, p.out_mix_start_s);
        assert!((p.out_stop_s - p.out_mix_start_s - CUT_FADE_S).abs() < 1e-9);
        assert_eq!(p.gain.gains_at(CUT_FADE_S), (0.0, 1.0));
        assert!(p.eq.is_empty());

        // Under a talk-over: the old fade.
        let c = PlanCtx {
            voice_break: Some(VoiceBreak { mode: VoiceMode::Across, clip_secs: 6.0 }),
            ..ctx(&s, 30.0)
        };
        let p = plan(&out, &inn, &c);
        assert_eq!(p.kind, Kind::Fade { secs: 4.0 }, "{}", p.reason);
        assert!(p.reason.contains("tempo"));
        assert_eq!(p.in_mix_start_s, f64::from(inn.audio_start));
        assert_in_downbeats(&out, p.out_mix_start_s);
        assert!(p.out_stop_s <= f64::from(out.audio_end) + 1e-6);
        let (go, gi) = p.gain.gains_at(2.0);
        assert!((go * go + gi * gi - 1.0).abs() < 1e-4);

        // Half time is fine.
        let half = FeatBuilder::new(60.0, 48).intro(8).outro(4).build();
        let p = plan(&out, &half, &ctx(&s, 30.0));
        assert!(!matches!(p.kind, Kind::Fade { .. }), "{}", p.reason);

        let ambient = FeatBuilder::new(120.0, 96).no_grid().build();
        let p = plan(&ambient, &inn, &ctx(&s, 30.0));
        assert!(matches!(p.kind, Kind::Fade { .. }));
        assert!((p.out_mix_start_s - (f64::from(ambient.audio_end) - 4.0)).abs() < 1e-6);
        let p = plan(&out, &ambient, &ctx(&s, 30.0));
        assert!(matches!(p.kind, Kind::Fade { .. }));
    }

    #[test]
    fn fade_starts_at_a_quiet_tail() {
        let s = settings();
        let mut out = FeatBuilder::new(120.0, 96).intro(8).outro(0).build();
        // The last 6 bars (12 s) are very quiet.
        for e in out.bars.energy.iter_mut().skip(90) {
            *e = 10;
        }
        let ambient = FeatBuilder::new(120.0, 96).no_grid().build();
        let p = plan(&out, &ambient, &ctx(&s, 30.0));
        let og = out.grid.as_ref().unwrap();
        assert!((p.out_mix_start_s - f64::from(og.bar_time(90))).abs() < 1e-3, "{}", p.out_mix_start_s);
    }

    #[test]
    fn voice_window_lies_in_the_instrumental_part() {
        let s = settings();
        let out = FeatBuilder::new(124.0, 128).intro(8).outro(32).build();
        let inn = FeatBuilder::new(124.0, 128).intro(32).outro(8).build();
        let plain = plan(&out, &inn, &ctx(&s, 30.0));
        assert!(matches!(plain.kind, Kind::BeatBlend { bars: 16, .. }), "{}", plain.reason);
        let c = PlanCtx {
            settings: &s,
            out_position_s: 30.0,
            voice_break: Some(VoiceBreak { mode: VoiceMode::Across, clip_secs: 40.0 }),
            out_genres: &[],
            in_genres: &[],
            no_echo: false,
        };
        let p = plan(&out, &inn, &c);
        assert!(matches!(p.kind, Kind::BeatBlend { bars: 32, .. }), "lengthened: {}", p.reason);
        let (a, b) = p.voice_window.expect("a window");
        assert!(b - a >= 40.0, "{a}..{b}");
        let og = out.grid.as_ref().unwrap();
        let bar_of = |t: f64| og.beat_at(t as f32).floor() as usize / 4;
        // Every bar in the window: no outgoing vocal; within the blend no incoming vocal either.
        let first = bar_of(a + 0.01);
        let last = bar_of(b - 0.01);
        for bar in first..=last {
            assert_eq!(out.bars.vocal[bar], 0, "bar {bar}");
        }
        assert!(b <= p.out_stop_s + 1e-6 && a >= c.out_position_s);
        assert!(a >= p.out_mix_start_s - 1e-6, "window inside the instrumental blend");
    }

    #[test]
    fn mix_point_is_never_before_now_plus_lead() {
        let s = settings();
        let out = FeatBuilder::new(124.0, 96).intro(8).outro(16).build();
        let inn = FeatBuilder::new(124.0, 96).intro(16).outro(8).build();
        let og = out.grid.as_ref().unwrap();
        // Just past the outro start: the planner must pick a later point.
        let now = f64::from(og.bar_time(80)) - 2.0;
        let p = plan(&out, &inn, &ctx(&s, now));
        assert!(p.out_mix_start_s >= now + MIN_LEAD_S - 1e-6, "{} vs {now}", p.out_mix_start_s);
        assert!(!matches!(p.kind, Kind::Gapless));
        if let Some(r) = p.out_ramp {
            assert!(r.start_out_s >= now);
        }
        // Way too late: gapless.
        let now = f64::from(out.audio_end) - 3.0;
        let p = plan(&out, &inn, &ctx(&s, now));
        assert_eq!(p.kind, Kind::Gapless);
        // A few seconds left: a short fade still fits.
        let now = f64::from(out.audio_end) - 7.0;
        let p = plan(&out, &inn, &ctx(&s, now));
        assert!(p.out_mix_start_s >= now + MIN_LEAD_S - 1e-6);
        assert!(
            matches!(p.kind, Kind::Fade { .. } | Kind::ShortBlend { .. } | Kind::FilterOut { .. }),
            "{:?}",
            p.kind
        );
    }

    #[test]
    fn envelope_interpolation_is_monotonic() {
        let e = Envelope { points: vec![(0.0, 1.0, 0.0), (4.0, 1.0, 1.0), (8.0, 0.0, 1.0)] };
        let mut prev = e.gains_at(-1.0);
        assert_eq!(prev, (1.0, 0.0));
        for step in 0..=90 {
            let g = e.gains_at(f64::from(step) / 10.0);
            assert!(g.0 <= prev.0 + 1e-6 && g.1 >= prev.1 - 1e-6, "{step}: {prev:?} → {g:?}");
            prev = g;
        }
        assert_eq!(prev, (0.0, 1.0));
        assert_eq!(Envelope::default().gains_at(3.0), (1.0, 0.0));
        // Duplicate times jump without dividing by zero.
        let jump = Envelope { points: vec![(0.0, 1.0, 0.0), (2.0, 1.0, 0.0), (2.0, 0.0, 1.0)] };
        assert_eq!(jump.gains_at(2.0), (0.0, 1.0));
        assert_eq!(jump.gains_at(1.0), (1.0, 0.0));
    }

    #[test]
    fn eligibility_matrix() {
        let s = settings();
        let item = |track: u32, album: &str, auto: bool, queued: bool| QueueItem {
            uid: u64::from(track),
            song: song(&format!("t{track}"), "A", Some(album), Some(track)),
            auto,
            ai: false,
            queued,
            bridge: None,
            why: None,
        };
        // DJ picks mix.
        assert!(mix_eligible(&item(1, "x", true, false), &item(5, "y", true, false), true, false, &s));
        assert!(mix_eligible(&item(1, "x", false, false), &item(5, "y", true, false), true, false, &s));
        // Leaving a DJ pick into the user's next song mixes too.
        assert!(mix_eligible(&item(1, "x", true, false), &item(5, "y", false, false), true, false, &s));
        // Hand-started album/playlist: gapless.
        assert!(!mix_eligible(&item(1, "x", false, false), &item(5, "y", false, false), true, false, &s));
        // Consecutive album tracks: gapless, even as DJ picks; same disc only.
        assert!(!mix_eligible(&item(3, "x", true, false), &item(4, "x", true, false), true, false, &s));
        assert!(!mix_eligible(&item(4, "x", true, false), &item(3, "x", true, false), true, false, &s));
        assert!(mix_eligible(&item(3, "x", true, false), &item(5, "x", true, false), true, false, &s));
        let mut other_disc = item(4, "x", true, false);
        other_disc.song.disc_number = Some(2);
        assert!(mix_eligible(&item(3, "x", true, false), &other_disc, true, false, &s));
        // "Add to queue" while a DJ is on: per setting, and only with the DJ on.
        let q = item(7, "z", false, true);
        assert!(mix_eligible(&item(1, "x", false, false), &q, true, false, &s));
        assert!(!mix_eligible(&item(1, "x", false, false), &q, false, false, &s));
        let no_manual = Mixing { mix_manual_queue: false, ..settings() };
        assert!(!mix_eligible(&item(1, "x", false, false), &q, true, false, &no_manual));
        // A waiting "between songs" break.
        assert!(!mix_eligible(&item(1, "x", true, false), &item(5, "y", true, false), true, true, &s));
        // Mixing off.
        let off = Mixing { enabled: false, ..settings() };
        assert!(!mix_eligible(&item(1, "x", true, false), &item(5, "y", true, false), true, false, &off));
        // YouTube Music: on its own and across sources; Jellyfin not.
        let source = |mut i: QueueItem, kind| {
            i.song.source = kind;
            i
        };
        let yt = |track, album| source(item(track, album, true, false), SourceKind::YouTubeMusic);
        let nd = item(1, "x", true, false);
        assert!(mix_eligible(&nd, &yt(5, "y"), true, false, &s));
        assert!(mix_eligible(&yt(5, "y"), &nd, true, false, &s));
        assert!(mix_eligible(&yt(5, "y"), &yt(9, "z"), true, false, &s));
        let jf = source(item(5, "y", true, false), SourceKind::Jellyfin);
        assert!(!mix_eligible(&nd, &jf, true, false, &s));
        assert!(!mix_eligible(&jf, &yt(9, "z"), true, false, &s));
        // The album rule still holds for a YouTube Music album…
        assert!(!mix_eligible(&yt(3, "x"), &yt(4, "x"), true, false, &s));
        // …but not without album ids or track numbers (radio, lists).
        let mut a = yt(3, "x");
        let mut b = yt(4, "x");
        (a.song.album_id, b.song.album_id) = (None, None);
        assert!(mix_eligible(&a, &b, true, false, &s));
        let mut a = yt(3, "x");
        let mut b = yt(4, "x");
        (a.song.track_number, b.song.track_number) = (None, None);
        assert!(mix_eligible(&a, &b, true, false, &s));
        // Manual songs stay gapless whatever the source.
        let hand = source(item(5, "y", false, false), SourceKind::YouTubeMusic);
        assert!(!mix_eligible(
            &source(item(1, "x", false, false), SourceKind::YouTubeMusic),
            &hand,
            true,
            false,
            &s
        ));
    }

    /// `f` with its beats replaced: beat `j` lasts `60 / bpm_at(j)` s (the grid's `bpm` field unchanged).
    fn with_tempo(f: &Features, bpm_at: impl Fn(usize) -> f64) -> Features {
        let mut f = f.clone();
        let g = f.grid.as_mut().unwrap();
        let mut t = f64::from(g.beats[0]);
        for j in 1..g.beats.len() {
            t += 60.0 / bpm_at(j - 1);
            g.beats[j] = t as f32;
        }
        f.audio_end = g.beats.last().copied().unwrap() + 0.3;
        f
    }

    /// Plays the blend as the engines do (no phase nudges, 1 ms steps) and returns the largest offset
    /// (ms) between an outgoing beat and the incoming beat that should meet it (same tempo octave).
    fn worst_drift_ms(p: &TransitionPlan, out: &Features, inn: &Features) -> f64 {
        let (go, gi) = (out.grid.as_ref().unwrap(), inn.grid.as_ref().unwrap());
        let s_out = p.out_ramp.map_or(1.0, |r| r.to);
        let (bo, bi) =
            (go.beat_at(p.out_mix_start_s as f32).round(), gi.beat_at(p.in_mix_start_s as f32).round());
        let (mut w, mut pos, mut j, mut worst) = (0.0f64, p.in_mix_start_s, 0.0f32, 0.0f64);
        let dt = 0.001;
        while w <= p.blend_s {
            let out_beat = (f64::from(go.time_of(bo + j)) - p.out_mix_start_s) / s_out;
            if w >= out_beat {
                let off = (f64::from(gi.time_of(bi + j)) - pos) / p.in_speed_at(w);
                worst = worst.max(off.abs() * 1000.0);
                j += 1.0;
            }
            pos += p.in_speed_at(w) * dt;
            w += dt;
        }
        worst
    }

    #[test]
    fn the_tempo_match_follows_the_overlapping_beats_not_the_song_average() {
        let s = settings();
        let out = FeatBuilder::new(120.0, 96).build();
        // Claims 120 bpm, but its first 32 bars (where it mixes in) run at 116.
        let inn = with_tempo(&FeatBuilder::new(120.0, 96).build(), |j| if j < 128 { 116.0 } else { 124.0 });
        let p = plan(&out, &inn, &ctx(&s, 30.0));
        assert!(matches!(p.kind, Kind::BeatBlend { .. }), "{}", p.reason);
        let s_out = p.out_ramp.map_or(1.0, |r| r.to);
        assert!((p.in_speed / s_out - 120.0 / 116.0).abs() < 1e-3, "{} / {s_out}", p.in_speed);
        assert!(p.in_glide.is_none());
        assert!(worst_drift_ms(&p, &out, &inn) <= 4.0, "{:.1} ms", worst_drift_ms(&p, &out, &inn));
    }

    #[test]
    fn an_incoming_song_speeding_up_through_the_blend_gets_a_glide() {
        let s = settings();
        let out = FeatBuilder::new(120.0, 96).build();
        // Speeds up from 117 to 123 bpm over its first 16 bars: the incoming deck slows down through the blend.
        let inn =
            with_tempo(&FeatBuilder::new(120.0, 96).build(), |j| 117.0 + 6.0 * (j.min(64) as f64 / 64.0));
        let p = plan(&out, &inn, &ctx(&s, 30.0));
        assert!(matches!(p.kind, Kind::BeatBlend { .. }), "{}", p.reason);
        let g = p.in_glide.expect("glide");
        assert!(g.to < g.from && g.secs == p.blend_s && g.start_out_s == p.out_mix_start_s, "{g:?}");
        // The settle starts where the glide ends.
        assert_eq!(p.in_settle.map(|r| r.from), Some(g.to));
        let drift = worst_drift_ms(&p, &out, &inn);
        assert!(drift <= 4.0, "{drift:.1} ms");
        // One constant speed would drift apart.
        let flat = TransitionPlan { in_glide: None, ..p.clone() };
        assert!(worst_drift_ms(&flat, &out, &inn) > 2.0 * drift.max(1.0));
    }

    #[test]
    fn in_speed_at_follows_the_glide() {
        let mut p = TransitionPlan::gapless(100.0, "x");
        p.in_speed = 1.02;
        assert_eq!(p.in_speed_at(3.0), 1.02);
        p.in_glide = Some(SpeedRamp { from: 1.0, to: 1.04, start_out_s: 90.0, secs: 10.0 });
        assert!((p.in_speed_at(5.0) - 1.02).abs() < 1e-12);
        assert_eq!((p.in_speed_at(-1.0), p.in_speed_at(20.0)), (1.0, 1.04));
    }
}
