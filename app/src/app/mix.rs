//! Runs `listen_mix::plan` transitions on two mpv decks: the incoming song is preloaded paused, started on
//! a deadline, phase-locked with speed nudges (from both decks' `audio-pts`) and faded/EQ'd along the
//! plan; halfway through the blend it becomes the active deck.
//!
//! Anything unexpected (late deck, skip, seek, pause) cancels to mpv's gapless playback.
//! Timing constants: docs/dj-transitions.md.

use super::{Listen, Message, err};
use iced::Task;
use listen_core::analysis::{BeatGrid, Features};
use listen_core::model::Song;
use listen_core::settings::{DECK_BASS, DECK_HIGHPASS, DECK_LOWPASS, DECK_MIX_LABEL};
use listen_mix::plan::{EqTarget, Kind, PlanCtx, TransitionPlan, Which};
use listen_player::{PlayerEvent, PtsSample, Track};
use listen_source::{AudioInput, MusicSource};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The incoming deck is loaded this long (outgoing seconds) before it starts.
const PRELOAD_LEAD_S: f64 = 20.0;
/// A fetch running inside the preload window may finish until this long before the incoming start.
const FETCH_GRACE_S: f64 = 5.0;
/// Not ready this close to the mix point: give up and play gapless.
const READY_DEADLINE_S: f64 = 4.0;
/// The start task is spawned this long (wall seconds) before the start.
const START_WINDOW_S: f64 = 1.5;
/// Filter commands are heard about this long after they are sent
/// (`--audio-buffer=0.1`), so they are sent this much early.
const FILTER_LEAD_S: f64 = 0.24;
/// Speed corrections settle the phase error over this long.
const LOCK_SETTLE_S: f64 = 0.6;
/// Largest speed nudge while the incoming deck is audible / still silent.
const NUDGE_AUDIBLE: f64 = 0.005;
const NUDGE_SILENT: f64 = 0.02;
/// Phase errors from this long after the start count as "locked" in the end-of-mix summary.
const LOCKED_AFTER_S: f64 = 2.0;
/// Phase readings with a longer IPC round trip are discarded.
const MAX_RTT: Duration = Duration::from_millis(2);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    /// A plan exists; mpv's "next" slot is empty so mpv doesn't advance.
    Planned,
    /// The incoming song is being loaded (paused) on the other deck.
    Preloading,
    Ready,
    /// The start task is waiting for the deadline.
    Starting,
    Running,
    /// The outgoing deck stopped; the incoming one glides back to 1.0×.
    Settling,
}

/// One filter value moving over time.
#[derive(Debug, Clone)]
struct EqRamp {
    deck: usize,
    filter: Filter,
    from: f32,
    to: f32,
    start: Instant,
    secs: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    Bass,
    Highpass,
    Lowpass,
}

impl Filter {
    fn target(self) -> &'static str {
        match self {
            Filter::Bass => DECK_BASS,
            Filter::Highpass => DECK_HIGHPASS,
            Filter::Lowpass => DECK_LOWPASS,
        }
    }

    fn param(self) -> &'static str {
        match self {
            Filter::Bass => "g",
            _ => "f",
        }
    }

    /// The transparent setting (filters stay in the chain; toggling `mix`
    /// clicks, so the passes idle at their extremes instead).
    fn neutral(self) -> f32 {
        match self {
            Filter::Bass => 0.0,
            Filter::Highpass => 10.0,
            Filter::Lowpass => 20000.0,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

fn eq_value(target: EqTarget) -> (Filter, f32) {
    match target {
        EqTarget::Bass(db) => (Filter::Bass, db),
        EqTarget::Highpass { hz, mix } => {
            (Filter::Highpass, if mix <= 0.0 { Filter::Highpass.neutral() } else { hz })
        }
        EqTarget::Lowpass { hz, mix } => {
            (Filter::Lowpass, if mix <= 0.0 { Filter::Lowpass.neutral() } else { hz })
        }
    }
}

/// The incoming song's audio for the second deck ([`MusicSource::mix_input`]). Fetched when the pair is
/// planned (YouTube Music takes seconds) and kept across re-plans of the same song.
#[derive(Debug, Default)]
struct Fetch {
    /// Fetched: (queue uid, what the deck plays).
    done: Option<(u64, AudioInput)>,
    /// A fetch under way for this uid.
    busy: Option<u64>,
    /// Songs whose fetched file is pinned in their source's cache, see [`Listen::mix_sync_pins`].
    pins: Vec<(u64, Song)>,
}

impl Fetch {
    fn input(&self, uid: u64) -> Option<&AudioInput> {
        self.done.as_ref().filter(|(u, _)| *u == uid).map(|(_, i)| i)
    }

    /// The fetch for `uid` is still under way.
    fn fetching(&self, uid: u64) -> bool {
        self.busy == Some(uid)
    }
}

#[derive(Debug, Default)]
pub struct MixState {
    pub phase: Phase,
    plan: Option<Arc<TransitionPlan>>,
    out_uid: u64,
    in_uid: u64,
    out_deck: usize,
    in_deck: usize,
    /// Bumped on every new plan and cancel (drops late task results).
    token: u64,
    /// `token` for tasks that act on their own (the start task's unpause).
    live: Arc<AtomicU64>,
    /// The incoming deck's `loadfile` was accepted (an earlier `Restarted` is the previous file's).
    loaded: bool,
    out_grid: Option<BeatGrid>,
    in_grid: Option<BeatGrid>,
    /// A pair not to mix again (the planner said gapless, or a mix failed).
    skip: Option<(u64, u64)>,
    /// Mix gain per deck (1 = full).
    gains: [f32; 2],
    /// Last reported position per deck (file seconds, when).
    pos: [(f64, Option<Instant>); 2],
    /// Speed sent per deck.
    speed: [f64; 2],
    /// Filter values per deck: [bass dB, highpass Hz, lowpass Hz].
    eq: [[f32; 3]; 2],
    eq_done: Vec<bool>,
    ramps: Vec<EqRamp>,
    out_ramp_started: Option<Instant>,
    settle_started: Option<Instant>,
    handed_over: bool,
    /// The outgoing deck was stopped.
    out_stopped: bool,
    /// Speed correction of the incoming deck (multiplier − 1).
    nudge: f64,
    lock_next: Option<Instant>,
    lock_busy: bool,
    started_at: Option<Instant>,
    /// Phase errors (ms) once the lock has settled, for the log.
    phase_errs: Vec<f64>,
    /// Learned delay between the unpause command and sound.
    unpause_latency: f64,
    /// The incoming file's length as its deck reported it before the handover.
    in_duration: Option<Duration>,
    /// The incoming song's audio end per its analysis; the shown duration if the deck never reported one.
    in_audio_end: Option<f64>,
    fetch: Fetch,
    /// What the incoming deck was given to load (logs, tests).
    in_loaded: Option<AudioInput>,
    /// Volume last sent to the non-active deck.
    pub(super) idle_last_sent: Option<f32>,
    /// The song that came in through a mix (gets no fade-in of its own).
    pub entered_uid: Option<u64>,
}

impl MixState {
    pub fn gain(&self, deck: usize) -> f32 {
        if self.phase == Phase::Idle { 1.0 } else { self.gains[deck.min(1)] }
    }

    /// A mix owns the transition to the next queue item (mpv must not advance).
    pub fn owns_next(&self) -> bool {
        self.phase != Phase::Idle && !self.handed_over
    }

    pub fn plan(&self) -> Option<&TransitionPlan> {
        self.plan.as_deref()
    }

    /// The pair being mixed (outgoing uid, incoming uid).
    pub fn pair(&self) -> Option<(u64, u64)> {
        (self.phase != Phase::Idle).then_some((self.out_uid, self.in_uid))
    }
}

#[derive(Debug, Clone)]
pub enum MixMsg {
    Loaded(u64, Result<(), String>),
    /// The incoming deck was unpaused for sound at this instant.
    Started(u64, Result<Instant, String>),
    /// Both decks' positions, read back to back.
    Phase(u64, Option<PtsSample>, Option<PtsSample>),
    Done(Result<(), String>),
    /// The incoming song's deck input was fetched (keyed by queue uid: a fetch outlives re-plans).
    Input(u64, Result<Option<AudioInput>, String>),
}

/// Runs a source's [`MusicSource::mix_input`] (separate so tests can call it without iced).
pub(super) async fn fetch_input(
    source: Arc<dyn MusicSource>,
    song: Song,
) -> Result<Option<AudioInput>, String> {
    let started = Instant::now();
    let r = source.mix_input(&song).await.map_err(err);
    tracing::debug!("mix input for “{}” in {:.1}s: {r:?}", song.title, started.elapsed().as_secs_f32());
    r
}

fn bars_ratio(out: &BeatGrid, out_speed: f64, inn: &BeatGrid, in_speed: f64) -> f64 {
    // Incoming beats per outgoing beat: 1, 2 or ½ (half/double time).
    let r = (f64::from(inn.bpm) * in_speed) / (f64::from(out.bpm) * out_speed).max(1.0);
    [0.5, 1.0, 2.0]
        .into_iter()
        .min_by(|a, b| (r / a).ln().abs().total_cmp(&(r / b).ln().abs()))
        .unwrap_or(1.0)
}

impl Listen {
    /// DJ transitions can happen (the second deck runs).
    pub fn mixing_possible(&self) -> bool {
        let s = &self.config.settings;
        !self.mock_playback()
            && !self.dlna_casting()
            && s.mixing.enabled
            && (s.auto_dj.enabled || self.ai_dj_on())
    }

    /// The second deck is needed: mixing is on, or it is playing.
    pub fn deck_b_needed(&self) -> bool {
        self.mixing_possible() || self.decks.active == 1 || self.mix.phase != Phase::Idle
    }

    fn deck_task<F, Fut>(&self, deck: usize, f: F) -> Task<Message>
    where
        F: FnOnce(listen_player::Player) -> Fut,
        Fut: Future<Output = Result<(), listen_player::MpvError>> + Send + 'static,
    {
        match self.decks.d[deck.min(1)].player.clone() {
            Some(p) => Task::perform(f(p), |r| Message::Mix(MixMsg::Done(r.map_err(err)))),
            None => Task::none(),
        }
    }

    /// Records a deck's position report.
    pub(super) fn mix_position(&mut self, deck: usize, pos: Duration) {
        self.mix.pos[deck.min(1)] = (pos.as_secs_f64(), Some(Instant::now()));
    }

    /// A deck's position now (extrapolated a little since its last report).
    fn deck_now(&self, deck: usize) -> f64 {
        let (p, at) = self.mix.pos[deck];
        let ahead = at.map_or(0.0, |t| t.elapsed().as_secs_f64().min(0.25));
        p + ahead * self.mix.speed[deck].max(0.5)
    }

    /// Plans a mix into the next song (after a track or queue change, or a new analysis).
    pub(super) fn mix_consider(&mut self) -> Task<Message> {
        // Called on every track and queue change: the pins follow the decks.
        self.mix_sync_pins();
        // Paused: planned again on resume (a plan's clock is the outgoing deck).
        // Not while a "between songs" DJ clip airs: mpv plays the clip, not `cur`.
        if self.mix.phase != Phase::Idle
            || !self.mixing_possible()
            || !self.playback.playing
            || self.dj_airing()
        {
            return Task::none();
        }
        let (Some(cur), Some(next)) = (self.queue.current().cloned(), self.queue.peek_next().cloned()) else {
            return Task::none();
        };
        if self.queue.repeat == listen_core::queue::Repeat::One
            || self.mix.skip == Some((cur.uid, next.uid))
            || self.decks.idle_player().is_none()
        {
            return Task::none();
        }
        let dj_on = self.ai_dj_on() || self.config.settings.auto_dj.enabled;
        let between = self.dj_between_before(next.uid);
        if !listen_mix::plan::mix_eligible(&cur, &next, dj_on, between, &self.config.settings.mixing) {
            tracing::debug!("mix “{}” → “{}”: not eligible (gapless)", cur.song.title, next.song.title);
            return Task::none();
        }
        // The files that play: an own copy where there is one.
        let cur_song = self.substitutes.used(&cur.song).unwrap_or_else(|| cur.song.clone());
        let next_song = self.playable(&next.song);
        let Some(source) = self.sources.get(&next_song.server_id).cloned() else {
            return Task::none(); // planned once its server is connected
        };
        let Some(analysis) = self.analysis.clone() else { return Task::none() };
        let (Some(a), Some(b)) = (analysis.lookup(&cur_song), analysis.lookup(&next_song)) else {
            tracing::debug!("mix “{}” → “{}”: waiting for the analysis", cur.song.title, next.song.title);
            return Task::none(); // planned when the analysis arrives
        };
        let (Some(fa), Some(fb)) = (a.features(), b.features()) else {
            self.mix.skip = Some((cur.uid, next.uid));
            return Task::none();
        };
        let out_now = self.playback.position.as_secs_f64();
        let ctx = PlanCtx {
            settings: &self.config.settings.mixing,
            out_position_s: out_now,
            voice_break: self.dj_voice_break(next.uid),
        };
        let plan = listen_mix::plan::plan(fa, fb, &ctx);
        if plan.kind == Kind::Gapless {
            tracing::info!("mix “{}” → “{}”: gapless ({})", cur.song.title, next.song.title, plan.reason);
            self.mix.skip = Some((cur.uid, next.uid));
            return Task::none();
        }
        tracing::info!(
            "mix “{}” → “{}”: {:?} at {:.1}s → {:.1}s, blend {:.1}s ({})",
            cur.song.title,
            next.song.title,
            plan.kind,
            plan.out_mix_start_s,
            plan.in_mix_start_s,
            plan.blend_s,
            plan.reason
        );
        self.start_plan(cur.uid, next.uid, plan, fa, fb);
        // The mix brings the file in, not the stream: note which one it is.
        self.substitutes.mark(&next.song, (next_song.id != next.song.id).then(|| next_song.clone()));
        let fetch = self.mix_fetch(source, next.uid, next_song);
        Task::batch([fetch, self.refresh_next()])
    }

    /// Starts fetching the incoming deck's input unless done or under way; the result arrives as
    /// [`MixMsg::Input`].
    fn mix_fetch(&mut self, source: Arc<dyn MusicSource>, uid: u64, song: Song) -> Task<Message> {
        if self.mix.fetch.input(uid).is_some() {
            self.mix_sync_pins();
            return Task::none();
        }
        if self.mix.fetch.busy == Some(uid) {
            return Task::none();
        }
        self.mix.fetch.busy = Some(uid);
        Task::perform(fetch_input(source, song), move |r| Message::Mix(MixMsg::Input(uid, r)))
    }

    /// Pins exactly the fetched files a deck holds or will load (incoming, outgoing until stopped, and a
    /// current song that came in through a mix); releases the rest.
    fn mix_sync_pins(&mut self) {
        let active = self.mix.phase != Phase::Idle;
        let current = self.queue.current().map(|i| i.uid);
        let mut want: Vec<(u64, Song)> = std::mem::take(&mut self.mix.fetch.pins)
            .into_iter()
            .filter_map(|(uid, song)| {
                let keep = (current == Some(uid) && self.mix.entered_uid == Some(uid))
                    || (active && !self.mix.out_stopped && uid == self.mix.out_uid)
                    || (active && uid == self.mix.in_uid);
                if keep {
                    return Some((uid, song));
                }
                if let Some(s) = self.sources.get(&song.server_id) {
                    tracing::debug!("mix: unpin “{}”", song.title);
                    s.pin_mix(&song, false);
                }
                None
            })
            .collect();
        let in_uid = self.mix.in_uid;
        if active
            && matches!(self.mix.fetch.input(in_uid), Some(AudioInput::File(_)))
            && !want.iter().any(|(u, _)| *u == in_uid)
            && let Some(item) = self.queue.iter().find(|i| i.uid == in_uid)
            && let song = self.playable(&item.song)
            && let Some(s) = self.sources.get(&song.server_id)
        {
            tracing::debug!("mix: pin “{}”", song.title);
            s.pin_mix(&song, true);
            want.push((in_uid, song));
        }
        self.mix.fetch.pins = want;
    }

    fn start_plan(&mut self, out_uid: u64, in_uid: u64, plan: TransitionPlan, fa: &Features, fb: &Features) {
        let out_deck = self.decks.active;
        let in_deck = 1 - out_deck;
        let token = self.mix.token + 1;
        let latency = if self.mix.unpause_latency > 0.0 { self.mix.unpause_latency } else { 0.005 };
        let mut gains = [1.0; 2];
        gains[in_deck] = 0.0;
        let neutral = [Filter::Bass.neutral(), Filter::Highpass.neutral(), Filter::Lowpass.neutral()];
        // Right after a song change mpv still reports the old song's position; start from the plan's.
        let mut pos = self.mix.pos;
        pos[out_deck] = (self.playback.position.as_secs_f64(), self.fader.pos_at);
        self.mix = MixState {
            phase: Phase::Planned,
            eq_done: vec![false; plan.eq.len()],
            plan: Some(Arc::new(plan)),
            out_uid,
            in_uid,
            out_deck,
            in_deck,
            token,
            out_grid: fa.grid.clone(),
            in_grid: fb.grid.clone(),
            skip: self.mix.skip,
            gains,
            pos,
            speed: [1.0; 2],
            eq: [neutral; 2],
            unpause_latency: latency,
            entered_uid: self.mix.entered_uid,
            live: self.mix.live.clone(),
            in_audio_end: Some(f64::from(fb.audio_end)),
            fetch: std::mem::take(&mut self.mix.fetch),
            ..MixState::default()
        };
        self.mix.live.store(token, Ordering::SeqCst);
    }

    /// Every fader tick while a mix is planned or running.
    pub(super) fn mix_tick(&mut self) -> Task<Message> {
        let Some(plan) = self.mix.plan.clone() else { return Task::none() };
        let (out_deck, in_deck) = (self.mix.out_deck, self.mix.in_deck);
        // Paused some other way than the play button (which cancels itself).
        if !self.playback.playing {
            return self.mix_cancel("paused");
        }
        // The pair changed under us (queue edit, DJ break armed between them).
        if !self.mix.handed_over
            && (self.queue.current().map(|i| i.uid) != Some(self.mix.out_uid)
                || self.queue.peek_next().map(|i| i.uid) != Some(self.mix.in_uid)
                || self.dj_between_before(self.mix.in_uid))
        {
            return self.mix_cancel("the next song changed");
        }
        let out_t = self.deck_now(out_deck);
        let out_speed = self.mix.speed[out_deck];
        let mut tasks = Vec::new();

        // The outgoing tempo moves towards the shared tempo before the blend.
        if let Some(r) = plan.out_ramp
            && !self.mix.out_stopped
            && out_t >= r.start_out_s
        {
            let started = *self.mix.out_ramp_started.get_or_insert_with(Instant::now);
            let p = (started.elapsed().as_secs_f64() / r.secs.max(0.01)).min(1.0);
            tasks.push(self.set_deck_speed(out_deck, r.from + (r.to - r.from) * p));
        }

        // Where the incoming deck starts so its mix-in point meets the outgoing mix point.
        let preroll_out = (plan.in_mix_start_s - plan.in_load_at_s) / plan.in_speed * out_speed;
        let start_out = plan.out_mix_start_s - preroll_out;
        match self.mix.phase {
            Phase::Planned if out_t >= start_out - PRELOAD_LEAD_S => {
                // Never blend from a half-fetched file. A running fetch is waited for until
                // `FETCH_GRACE_S` before the start; after that this pair plays gapless.
                if self.mix.fetch.input(self.mix.in_uid).is_none() {
                    if self.mix.fetch.fetching(self.mix.in_uid) && out_t < start_out - FETCH_GRACE_S {
                        return Task::batch(tasks);
                    }
                    return self.mix_fail("the incoming song wasn't fetched in time");
                }
                tasks.push(self.mix_preload());
            }
            Phase::Preloading if out_t >= plan.out_mix_start_s - READY_DEADLINE_S => {
                return self.mix_fail("the incoming song wasn't ready in time");
            }
            Phase::Ready => {
                let wall = (start_out - out_t) / out_speed.max(0.5);
                if wall < -0.2 {
                    return self.mix_fail("missed the start");
                }
                if wall <= START_WINDOW_S {
                    tasks.push(self.mix_start(start_out));
                }
            }
            Phase::Starting if out_t > plan.out_mix_start_s + 1.0 => {
                return self.mix_fail("the incoming deck didn't start");
            }
            Phase::Running | Phase::Settling => tasks.push(self.mix_automate(&plan, out_t)),
            _ => {}
        }
        let _ = in_deck;
        Task::batch(tasks)
    }

    fn set_deck_speed(&mut self, deck: usize, speed: f64) -> Task<Message> {
        if (self.mix.speed[deck] - speed).abs() < 0.0002 {
            return Task::none();
        }
        self.mix.speed[deck] = speed;
        self.deck_task(deck, move |p| async move { p.set_speed(speed).await })
    }

    fn set_filter(&mut self, deck: usize, filter: Filter, value: f32) -> Task<Message> {
        let current = &mut self.mix.eq[deck][filter.index()];
        // NaN: unknown (a fresh file), always sent.
        let changed = current.is_nan()
            || match filter {
                Filter::Bass => (*current - value).abs() >= 0.5,
                _ => (*current / value).ln().abs() >= 0.03,
            };
        if !changed {
            return Task::none();
        }
        *current = value;
        let arg = format!("{value:.2}");
        self.deck_task(deck, move |p| async move {
            p.af_command(DECK_MIX_LABEL, filter.param(), &arg, filter.target()).await
        })
    }

    /// Loads the incoming song paused at its cue, silent, at its speed and with its starting EQ.
    fn mix_preload(&mut self) -> Task<Message> {
        let Some(plan) = self.mix.plan.clone() else { return Task::none() };
        let Some(next) = self.queue.peek_next().map(|i| i.song.clone()) else {
            return self.mix_fail("no next song");
        };
        let Some(input) = self.mix.fetch.input(self.mix.in_uid).cloned() else {
            return self.mix_fail("the incoming song wasn't fetched");
        };
        // A cache can only drop pinned files, but a user can still delete it.
        if let AudioInput::File(path) = &input
            && !path.exists()
        {
            self.mix.fetch.done = None;
            return self.mix_fail("the fetched file is gone");
        }
        self.mix.phase = Phase::Preloading;
        let in_deck = self.mix.in_deck;
        tracing::debug!("mix preload “{}” on deck {in_deck} at {:.2}s", next.title, plan.in_load_at_s);
        let out_deck = self.mix.out_deck;
        // The outgoing deck must not advance: a prefetch may have refilled the next slot since
        // `refresh_next` cleared it.
        let mut tasks = vec![self.deck_task(out_deck, |p| async move { p.set_next(None).await })];
        // Its filters are set once the file is loaded (`mix_deck_ready`).
        self.mix.eq[in_deck] = [f32::NAN; 3];
        self.mix.speed[in_deck] = f64::NAN;
        tasks.push(self.set_deck_speed(in_deck, plan.in_speed));
        self.mix.gains[in_deck] = 0.0;
        self.mix.idle_last_sent = None;
        tasks.push(self.push_idle_volume(true));
        let token = self.mix.token;
        let start = Duration::from_secs_f64(plan.in_load_at_s.max(0.0));
        let track = Track::url(input.as_mpv_arg());
        self.mix.in_loaded = Some(input);
        tasks.push(match self.decks.d[in_deck].player.clone() {
            Some(p) => {
                Task::perform(async move { p.load(&track, Some(start), true).await.map_err(err) }, move |r| {
                    Message::Mix(MixMsg::Loaded(token, r))
                })
            }
            None => return self.mix_fail("the second deck isn't running"),
        });
        Task::batch(tasks)
    }

    /// The incoming deck loaded its file: sets its starting EQ (e.g. bass cut) and every other filter to
    /// neutral.
    fn mix_deck_ready(&mut self) -> Task<Message> {
        tracing::debug!("mix deck {} ready", self.mix.in_deck);
        self.mix.phase = Phase::Ready;
        let Some(plan) = self.mix.plan.clone() else { return Task::none() };
        let in_deck = self.mix.in_deck;
        let mut start = [Filter::Bass.neutral(), Filter::Highpass.neutral(), Filter::Lowpass.neutral()];
        for (i, ev) in plan.eq.iter().enumerate() {
            if ev.deck == Which::In && ev.at_out_s <= plan.out_mix_start_s + 0.01 {
                let (filter, value) = eq_value(ev.target);
                self.mix.eq_done[i] = true;
                start[filter.index()] = value;
            }
        }
        self.mix.eq[in_deck] = [f32::NAN; 3];
        let tasks: Vec<_> = [Filter::Bass, Filter::Highpass, Filter::Lowpass]
            .into_iter()
            .map(|f| self.set_filter(in_deck, f, start[f.index()]))
            .collect();
        Task::batch(tasks)
    }

    /// A stopped deck has no filter chain; its next file starts from scratch (see `mix_deck_ready`).
    fn forget_deck_filters(&mut self, deck: usize) {
        self.mix.eq[deck] = [f32::NAN; 3];
        self.mix.ramps.retain(|r| r.deck != deck);
    }

    /// Unpauses the incoming deck when the outgoing one reaches `start_out`.
    fn mix_start(&mut self, start_out: f64) -> Task<Message> {
        let (Some(out_p), Some(in_p)) =
            (self.decks.d[self.mix.out_deck].player.clone(), self.decks.d[self.mix.in_deck].player.clone())
        else {
            return self.mix_fail("a deck is missing");
        };
        self.mix.phase = Phase::Starting;
        // The CPU goes to the transition, not the library analysis.
        self.analysis_hold(true);
        let token = self.mix.token;
        let speed = self.mix.speed[self.mix.out_deck];
        let latency = self.mix.unpause_latency;
        let live = self.mix.live.clone();
        Task::perform(
            async move {
                let sample =
                    out_p.audio_pts().await.map_err(err)?.ok_or("the outgoing deck has no position")?;
                let wait = ((start_out - sample.pts) / speed).max(0.0);
                let when = sample.at + Duration::from_secs_f64(wait);
                tokio::time::sleep_until((when - Duration::from_secs_f64(latency)).into()).await;
                // Cancelled while waiting: the deck may hold the next plan's song by now.
                if live.load(Ordering::SeqCst) != token {
                    return Err("cancelled".to_string());
                }
                in_p.set_paused(false).await.map_err(err)?;
                Ok(when)
            },
            move |r| Message::Mix(MixMsg::Started(token, r)),
        )
    }

    /// Gains, EQ, tempo, phase lock, handover and the end of the blend.
    fn mix_automate(&mut self, plan: &TransitionPlan, out_t: f64) -> Task<Message> {
        let (out_deck, in_deck) = (self.mix.out_deck, self.mix.in_deck);
        let out_speed = self.mix.speed[out_deck];
        let mut tasks = Vec::new();
        let t = (out_t - plan.out_mix_start_s) / out_speed.max(0.5);
        if !self.mix.out_stopped {
            // The pre-roll bar stays silent whatever the envelope starts with.
            let (g_out, g_in) = if t < 0.0 { (1.0, 0.0) } else { plan.gain.gains_at(t) };
            // Both decks at full level mid-blend add up; trim a little.
            let trim = if g_out > 0.0 && g_in > 0.0 { 0.85 } else { 1.0 };
            self.mix.gains[out_deck] = g_out * trim;
            self.mix.gains[in_deck] = g_in * trim;
        } else {
            self.mix.gains[in_deck] = 1.0;
        }
        tasks.push(self.push_volume(false));
        tasks.push(self.push_idle_volume(false));

        // EQ events, sent early by the output latency.
        let lead = FILTER_LEAD_S * out_speed;
        for i in 0..plan.eq.len() {
            let ev = plan.eq[i];
            if self.mix.eq_done[i] || out_t < ev.at_out_s - lead {
                continue;
            }
            self.mix.eq_done[i] = true;
            let deck = if ev.deck == Which::Out { out_deck } else { in_deck };
            let (filter, to) = eq_value(ev.target);
            let from = self.mix.eq[deck][filter.index()];
            self.mix.ramps.retain(|r| !(r.deck == deck && r.filter == filter));
            if ev.ramp_s <= 0.0 || from.is_nan() {
                tasks.push(self.set_filter(deck, filter, to));
            } else {
                self.mix.ramps.push(EqRamp {
                    deck,
                    filter,
                    from,
                    to,
                    start: Instant::now(),
                    secs: ev.ramp_s,
                });
            }
        }
        let ramps = self.mix.ramps.clone();
        for r in &ramps {
            let p = (r.start.elapsed().as_secs_f64() / r.secs).min(1.0) as f32;
            let v = match r.filter {
                // Frequencies glide on a log scale, gains linearly in dB.
                Filter::Bass => r.from + (r.to - r.from) * p,
                _ => (r.from.ln() + (r.to.ln() - r.from.ln()) * p).exp(),
            };
            tasks.push(self.set_filter(r.deck, r.filter, v));
        }
        self.mix.ramps.retain(|r| r.start.elapsed().as_secs_f64() < r.secs);

        // The incoming speed: matched during the blend, back to 1.0× after.
        let base = match (plan.in_settle, self.mix.settle_started) {
            (Some(r), Some(started)) => {
                let p = (started.elapsed().as_secs_f64() / r.secs.max(0.01)).min(1.0);
                r.from + (r.to - r.from) * p
            }
            _ => plan.in_speed,
        };
        tasks.push(self.set_deck_speed(in_deck, base * (1.0 + self.mix.nudge)));

        // Keep the beats together.
        let beat_mixed =
            matches!(plan.kind, Kind::BeatBlend { .. } | Kind::ShortBlend { .. } | Kind::FilterOut { .. });
        if beat_mixed
            && !self.mix.out_stopped
            && !self.mix.lock_busy
            && self.mix.lock_next.is_some_and(|at| Instant::now() >= at)
            && let (Some(out_p), Some(in_p)) =
                (self.decks.d[out_deck].player.clone(), self.decks.d[in_deck].player.clone())
        {
            self.mix.lock_busy = true;
            let token = self.mix.token;
            tasks.push(Task::perform(
                async move {
                    // Both reads at once so the samples are as close together as possible.
                    let (a, b) = tokio::join!(out_p.audio_pts(), in_p.audio_pts());
                    (a.ok().flatten(), b.ok().flatten())
                },
                move |(a, b)| Message::Mix(MixMsg::Phase(token, a, b)),
            ));
        }

        // The incoming song takes over (UI, queue, lyrics, scrobbling).
        if !self.mix.handed_over && out_t >= plan.handover_out_s {
            tasks.push(self.mix_handover());
        }
        // The outgoing deck is silent: stop it, the incoming settles.
        if !self.mix.out_stopped && out_t >= plan.out_stop_s {
            if !self.mix.handed_over {
                tasks.push(self.mix_handover());
            }
            tasks.push(self.mix_stop_out());
        }
        if self.mix.phase == Phase::Settling {
            let settled = match (plan.in_settle, self.mix.settle_started) {
                (Some(r), Some(started)) => started.elapsed().as_secs_f64() >= r.secs,
                _ => true,
            };
            if settled && self.mix.ramps.is_empty() {
                tasks.push(self.mix_finish());
            }
        }
        Task::batch(tasks)
    }

    /// The incoming deck becomes the active one.
    fn mix_handover(&mut self) -> Task<Message> {
        self.mix.handed_over = true;
        let in_deck = self.mix.in_deck;
        tracing::info!("mix handover to deck {in_deck}");
        // The AI DJ notes the finished song (no "between songs" clip here).
        let _ = self.dj_on_advanced();
        self.queue.advance();
        let sleep = self.sleep_advanced();
        if self.queue.current().map(|i| i.uid) != Some(self.mix.in_uid) {
            tracing::warn!("mix handover: the queue moved elsewhere");
        }
        self.decks.active = in_deck;
        std::mem::swap(&mut self.fader.last_sent, &mut self.mix.idle_last_sent);
        self.mix.entered_uid = Some(self.mix.in_uid);
        self.bump_token();
        self.playback.position = Duration::from_secs_f64(self.deck_now(in_deck).max(0.0));
        self.playback.duration =
            self.mix.in_duration.or_else(|| self.mix.in_audio_end.map(Duration::from_secs_f64));
        self.playback.reset_scrobble();
        self.seek_preview = None;
        self.playback.playing = true;
        self.fader.pos_at = Some(Instant::now());
        self.mix_sync_pins();
        let current = self.queue.current().map(|i| (i.song.server_id.clone(), i.song.image.clone()));
        Task::batch([
            match current {
                Some((server, image)) => self.request_image_on(&server, image.as_ref()),
                None => Task::none(),
            },
            self.scrobble(listen_source::ScrobbleEvent::NowPlaying),
            self.on_track_changed(),
            self.refresh_next(),
            sleep,
        ])
    }

    fn reset_deck_filters(&mut self, deck: usize) -> Task<Message> {
        let mut tasks = Vec::new();
        for f in [Filter::Bass, Filter::Highpass, Filter::Lowpass] {
            tasks.push(self.set_filter(deck, f, f.neutral()));
        }
        Task::batch(tasks)
    }

    fn mix_stop_out(&mut self) -> Task<Message> {
        self.mix.out_stopped = true;
        let out_deck = self.mix.out_deck;
        self.mix.gains[out_deck] = 0.0;
        self.mix.settle_started = Some(Instant::now());
        self.mix.phase = Phase::Settling;
        self.forget_deck_filters(out_deck);
        self.mix_sync_pins();
        let stop = self.deck_task(out_deck, |p| async move { p.stop().await });
        let speed = self.set_deck_speed(out_deck, 1.0);
        Task::batch([stop, speed])
    }

    /// The transition is complete.
    fn mix_finish(&mut self) -> Task<Message> {
        let in_deck = self.mix.in_deck;
        let filters = self.reset_deck_filters(in_deck);
        self.mix.nudge = 0.0;
        let speed = self.set_deck_speed(in_deck, 1.0);
        let entered = self.mix.entered_uid;
        self.log_phase_summary();
        self.mix = MixState {
            entered_uid: entered,
            unpause_latency: self.mix.unpause_latency,
            pos: self.mix.pos,
            speed: self.mix.speed,
            eq: self.mix.eq,
            token: self.mix.token + 1,
            live: self.mix.live.clone(),
            fetch: std::mem::take(&mut self.mix.fetch),
            ..MixState::default()
        };
        self.mix.live.store(self.mix.token, Ordering::SeqCst);
        tracing::info!("mix finished");
        self.analysis_hold(false);
        Task::batch([filters, speed, self.push_volume(true), self.mix_consider()])
    }

    /// Stops a mix after a user action (seek, pause, other song, queue edit); the pair may be re-planned.
    pub(super) fn mix_cancel(&mut self, reason: &str) -> Task<Message> {
        self.mix_abort(reason, false)
    }

    /// Stops a mix that went wrong; this pair then plays gapless.
    fn mix_fail(&mut self, reason: &str) -> Task<Message> {
        self.mix_abort(reason, true)
    }

    /// Stops a mix and falls back to normal playback.
    fn mix_abort(&mut self, reason: &str, failed: bool) -> Task<Message> {
        if self.mix.phase == Phase::Idle {
            return Task::none();
        }
        tracing::info!("mix cancelled: {reason}");
        self.log_phase_summary();
        self.analysis_hold(false);
        let (out_deck, in_deck) = (self.mix.out_deck, self.mix.in_deck);
        let mut tasks = Vec::new();
        // The deck that keeps playing goes back to neutral; the other one stops.
        let (keep, drop) = if self.mix.handed_over { (in_deck, out_deck) } else { (out_deck, in_deck) };
        if failed && !self.mix.handed_over {
            self.mix.skip = Some((self.mix.out_uid, self.mix.in_uid));
        }
        if !self.mix.out_stopped || !self.mix.handed_over {
            tasks.push(self.deck_task(drop, |p| async move { p.stop().await }));
        }
        self.forget_deck_filters(drop);
        tasks.push(self.reset_deck_filters(keep));
        for deck in [out_deck, in_deck] {
            tasks.push(self.set_deck_speed(deck, 1.0));
        }
        let handed_over = self.mix.handed_over;
        self.mix = MixState {
            skip: self.mix.skip,
            // Before the handover it still marks the current song: no fade-in dip now.
            entered_uid: self.mix.entered_uid,
            unpause_latency: self.mix.unpause_latency,
            pos: self.mix.pos,
            speed: self.mix.speed,
            eq: self.mix.eq,
            token: self.mix.token + 1,
            live: self.mix.live.clone(),
            fetch: std::mem::take(&mut self.mix.fetch),
            ..MixState::default()
        };
        self.mix.live.store(self.mix.token, Ordering::SeqCst);
        self.mix_sync_pins();
        tasks.push(self.push_volume(true));
        if !handed_over {
            tasks.push(self.refresh_next());
        }
        Task::batch(tasks)
    }

    /// "Next" during a mix: jumps into the incoming song while both decks play. Returns whether the skip
    /// is done; otherwise the mix is dropped and the caller skips as usual.
    pub(super) fn mix_skip_next(&mut self) -> (Task<Message>, bool) {
        match self.mix.phase {
            Phase::Idle => (Task::none(), false),
            Phase::Running if !self.mix.handed_over => {
                let handover = self.mix_handover();
                let stop = self.mix_stop_out();
                (Task::batch([handover, stop, self.mix_finish()]), true)
            }
            _ => (self.mix_cancel("skipped"), false),
        }
    }

    /// Volume of the non-active deck (only while mixing).
    pub(super) fn push_idle_volume(&mut self, force: bool) -> Task<Message> {
        if self.mix.phase == Phase::Idle {
            return Task::none();
        }
        let deck = 1 - self.decks.active;
        let v = self.deck_volume(deck);
        if !force && self.mix.idle_last_sent.is_some_and(|l| (l - v).abs() < 0.15) {
            return Task::none();
        }
        self.mix.idle_last_sent = Some(v);
        self.deck_task(deck, move |p| async move { p.set_volume(v).await })
    }

    pub(super) fn on_mix(&mut self, msg: MixMsg) -> Task<Message> {
        match msg {
            MixMsg::Loaded(token, Ok(())) if token == self.mix.token => self.mix.loaded = true,
            MixMsg::Loaded(token, Err(e)) if token == self.mix.token => {
                return self.mix_fail(&format!("loading failed: {e}"));
            }
            MixMsg::Started(token, result) if token == self.mix.token => match result {
                Ok(at) => {
                    self.mix.phase = Phase::Running;
                    self.mix.started_at = Some(at);
                    self.mix.lock_next = Some(Instant::now() + Duration::from_millis(150));
                    if let Some(plan) = &self.mix.plan {
                        let preroll = (plan.in_mix_start_s - plan.in_load_at_s) / plan.in_speed;
                        let blend_at = at + Duration::from_secs_f64(preroll.max(0.0));
                        let now = Instant::now();
                        let starts_in = if blend_at >= now {
                            blend_at.duration_since(now).as_secs_f64()
                        } else {
                            -now.duration_since(blend_at).as_secs_f64()
                        };
                        tracing::info!("mix blend: starts in {starts_in:.2} s, lasts {:.1} s", plan.blend_s);
                    }
                }
                Err(e) => return self.mix_fail(&format!("start failed: {e}")),
            },
            MixMsg::Phase(token, a, b) if token == self.mix.token => {
                self.mix.lock_busy = false;
                self.mix_phase_lock(a, b);
            }
            MixMsg::Done(Err(e)) => tracing::debug!("mix deck command: {e}"),
            MixMsg::Input(uid, result) => {
                if self.mix.fetch.busy == Some(uid) {
                    self.mix.fetch.busy = None;
                }
                let waiting = self.mix.phase == Phase::Planned && self.mix.in_uid == uid;
                match result {
                    // A late result of an earlier pair doesn't replace the current one's.
                    Ok(Some(input))
                        if uid == self.mix.in_uid || self.mix.fetch.input(self.mix.in_uid).is_none() =>
                    {
                        self.mix.fetch.done = Some((uid, input));
                        self.mix_sync_pins();
                    }
                    // A failed fetch gives the pair up (no retries against YouTube's budget).
                    Ok(None) if waiting => return self.mix_fail("the incoming song can't be mixed"),
                    Err(e) if waiting => {
                        return self.mix_fail(&format!("fetching the incoming song failed: {e}"));
                    }
                    Ok(_) => {}
                    Err(e) => tracing::debug!("mix fetch failed: {e}"),
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// How well the beats stayed together once the lock had settled.
    fn log_phase_summary(&self) {
        let mut errs: Vec<f64> = self.mix.phase_errs.iter().map(|e| e.abs()).collect();
        if errs.is_empty() {
            return;
        }
        errs.sort_by(f64::total_cmp);
        let at = |p: f64| errs[((errs.len() - 1) as f64 * p).round() as usize];
        tracing::info!(
            "mix phase lock: {} readings, |error| median {:.1} ms, p95 {:.1} ms, max {:.1} ms",
            errs.len(),
            at(0.5),
            at(0.95),
            at(1.0)
        );
    }

    /// Turns two position readings into a speed nudge for the incoming deck.
    fn mix_phase_lock(&mut self, a: Option<PtsSample>, b: Option<PtsSample>) {
        let since_start = self.mix.started_at.map_or(0.0, |t| t.elapsed().as_secs_f64());
        let interval = if since_start < 2.0 { 0.25 } else { 1.0 };
        self.mix.lock_next = Some(Instant::now() + Duration::from_secs_f64(interval));
        let (Some(a), Some(b), Some(plan)) = (a, b, self.mix.plan.clone()) else { return };
        let (Some(go), Some(gi)) = (self.mix.out_grid.as_ref(), self.mix.in_grid.as_ref()) else { return };
        if a.rtt > MAX_RTT || b.rtt > MAX_RTT {
            return;
        }
        let (so, si) = (self.mix.speed[self.mix.out_deck], self.mix.speed[self.mix.in_deck]);
        let now = Instant::now();
        let (po, pi) = (a.pts_at(now, so), b.pts_at(now, si));
        // `audio-pts` advances once per buffer (a PipeWire quantum, ~21 ms at 48 kHz), so under load a
        // reading can lag by one quantum; the stamp skew is logged to tell that apart.
        tracing::trace!(
            "mix phase reads {:.2} ms apart, rtt {:.2}/{:.2} ms",
            a.at.max(b.at).duration_since(a.at.min(b.at)).as_secs_f64() * 1000.0,
            a.rtt.as_secs_f64() * 1000.0,
            b.rtt.as_secs_f64() * 1000.0
        );
        let k = bars_ratio(go, so, gi, si / (1.0 + self.mix.nudge));
        let bo = f64::from(go.beat_at(po as f32) - go.beat_at(plan.out_mix_start_s as f32));
        let bi = f64::from(gi.beat_at(pi as f32) - gi.beat_at(plan.in_mix_start_s as f32));
        let mut e = bi - k * bo;
        e -= e.round();
        let beat_secs = 60.0 / (f64::from(gi.bpm) * si).max(1.0);
        let err_secs = e * beat_secs;
        let silent = self.mix.gains[self.mix.in_deck] < 0.05;
        let max = if silent { NUDGE_SILENT } else { NUDGE_AUDIBLE };
        // The nudge holds until the next reading: spread it over ≥1.4 intervals so it can't overshoot.
        let over = LOCK_SETTLE_S.max(1.4 * interval);
        self.mix.nudge = (-err_secs / over).clamp(-max, max);
        let locked = since_start >= LOCKED_AFTER_S;
        tracing::debug!(
            "mix phase error {:+.2} ms{} → nudge {:+.2} %",
            err_secs * 1000.0,
            if locked { " (locked)" } else { "" },
            self.mix.nudge * 100.0
        );
        if locked {
            self.mix.phase_errs.push(err_secs * 1000.0);
        }
        // The first reading after the start tells how late the unpause landed.
        if since_start < 0.4 {
            let late = err_secs.clamp(-0.05, 0.05);
            self.mix.unpause_latency =
                (self.mix.unpause_latency * 0.7 + (self.mix.unpause_latency - late) * 0.3).clamp(0.0, 0.1);
        }
    }

    /// Events of the non-active deck (incoming before the handover, outgoing tail after).
    pub(super) fn mix_on_idle_event(&mut self, deck: usize, event: PlayerEvent) -> Task<Message> {
        match event {
            PlayerEvent::Position(pos) => self.mix_position(deck, pos),
            PlayerEvent::Duration(d) if deck == self.mix.in_deck && !self.mix.handed_over => {
                self.mix.in_duration = Some(d);
            }
            PlayerEvent::Restarted
                if self.mix.phase == Phase::Preloading && self.mix.loaded && deck == self.mix.in_deck =>
            {
                return self.mix_deck_ready();
            }
            PlayerEvent::Error(e) if self.mix.phase != Phase::Idle && deck == self.mix.in_deck => {
                return self.mix_fail(&format!("the incoming song failed: {e}"));
            }
            PlayerEvent::Ended
                if deck == self.mix.out_deck && self.mix.handed_over && !self.mix.out_stopped =>
            {
                return self.mix_stop_out();
            }
            _ => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests;
