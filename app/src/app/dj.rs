//! The AI DJ: when a set's last song starts, Gemini picks the next 2–5 songs from a library pool and writes
//! the break, which is spoken and aired. The break sits at random between songs (clip in mpv's "next" slot),
//! over the song's end, the next song's start, or across the change; those three play on a second mpv under
//! which the music is ducked (see `fade`). Any failure falls back to the Auto-DJ; playback never waits.

use super::{Listen, Message, Setting, Sources, err, stream_track};
use iced::Task;
use listen_core::model::{ImageRef, ServerId, Song};
use listen_core::versions::{dedupe_versions, song_key};
use listen_dj::prompt::Turn;
use listen_dj::quota::{self, Quota};
use listen_dj::{BreakMode, DjError, Gemini, Persona, PlanRequest, Played};
use listen_player::Track;
use rand::seq::{IndexedRandom, SliceRandom};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// Keyring entry of the Gemini API key.
const KEY_ENTRY: &str = "gemini-api-key";
/// Songs offered to the model per set.
const POOL: usize = 40;
/// Voice level before the user's offset (music with ReplayGain sits around here).
const VOICE_DBFS: f32 = -17.0;
/// Don't put the clip in the next slot this close to the end of a song
/// (mpv may already be switching to the prefetched track).
const ARM_MARGIN: Duration = Duration::from_secs(6);

/// A break waiting for or on air.
#[derive(Debug, Clone, Default)]
pub enum Break {
    #[default]
    None,
    /// Airs before the queue item `before_uid`: as a clip, or (no clip:
    /// quota used up / speech failed) as text on screen.
    Armed { before_uid: u64, clip: Option<PathBuf>, line: String, mode: BreakMode, clip_secs: f32 },
    /// Between songs: the clip is playing in the music player (the queue
    /// still points at the song before it).
    Airing { clip: PathBuf },
    /// The voice player is talking over the music.
    Overlap { clip: PathBuf, mode: BreakMode },
}

#[derive(Debug, Default)]
pub struct DjState {
    /// The song the DJ was started from (the mood to keep).
    pub seed: Option<Song>,
    /// What the Auto-DJ would have played after the seed (more of its mood).
    seed_context: Vec<Song>,
    mood: Option<String>,
    /// The first set's mood: the session's anchor.
    seed_mood: Option<String>,
    /// Finished songs since the last break.
    played: Vec<Played>,
    last_line: Option<String>,
    busy: bool,
    pub brk: Break,
    /// The line on screen: (text, spoken).
    pub caption: Option<(String, bool)>,
    /// Next plan is the first after switching on (intro line).
    first: bool,
    sets_since_talk: u32,
    failures: u32,
    backoff_until: Option<Instant>,
    /// Version keys of songs the DJ queued recently, so sets don't repeat.
    history: VecDeque<String>,
    /// Songs the listener queued themselves (oldest first): they steer the
    /// DJ together with the seed.
    pub listener_picks: Vec<Song>,
    /// The clip is what mpv has in its "next" slot.
    clip_in_slot: bool,
    /// Bumped when the DJ is re-seeded or switched off (drops late plans).
    generation: u64,
    pub(super) key: Option<String>,
    /// Text of the API key field in settings.
    pub key_input: String,
    /// Status line in the settings (key saved, test result, errors).
    pub status: Option<String>,
    quota: Quota,
    /// A voice preview is being generated.
    pub previewing: bool,
    /// Stop the voice player (set where no task can be returned; see `update`).
    pub(super) stop_voice: bool,
    /// Queue item the DJ talked over the end of (not "played since the break").
    talked_over: Option<u64>,
    /// How the current queue item came in (uid, mix phrase; `None` = not
    /// mixed), for the "played" list (see `dj_mix`).
    pub(super) entered: Option<(u64, Option<String>)>,
}

/// What mpv's "next" slot should hold.
pub enum Slot {
    Clip(Track),
    Song(Box<Song>),
}

/// A finished plan, ready to queue.
#[derive(Debug, Clone)]
pub struct Planned {
    songs: Vec<Song>,
    mood: String,
    line: String,
    clip: Option<PathBuf>,
    /// Spoken length of the clip.
    clip_secs: f32,
    /// The seed context, when it had to be looked up for this plan.
    seed_context: Vec<Song>,
    mode: BreakMode,
    /// Speech was wanted but failed.
    tts_error: Option<DjError>,
}

impl DjState {
    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }
}

pub async fn resolve_slot(sources: &Sources, slot: Option<Slot>) -> Result<Option<Track>, String> {
    match slot {
        Some(Slot::Clip(track)) => Ok(Some(track)),
        Some(Slot::Song(song)) => stream_track(sources, &song).await.map(Some),
        None => Ok(None),
    }
}

impl Listen {
    pub fn ai_dj_on(&self) -> bool {
        self.config.settings.ai_dj.enabled && !self.demo
    }

    pub fn dj_persona(&self) -> &'static Persona {
        Persona::by_id(&self.config.settings.ai_dj.persona)
    }

    /// The queue item a pending break airs before (for the queue marker).
    pub fn dj_break_before(&self) -> Option<u64> {
        match &self.dj.brk {
            Break::Armed { before_uid, .. } => Some(*before_uid),
            _ => None,
        }
    }

    /// A "between songs" clip is armed before queue item `uid` (that pair
    /// plays gapless with the clip instead of a DJ transition).
    pub(super) fn dj_between_before(&self, uid: u64) -> bool {
        matches!(&self.dj.brk, Break::Armed { before_uid, mode: BreakMode::Between, clip: Some(_), .. } if *before_uid == uid)
            && self.ai_dj_on()
    }

    /// A talk-over break planned before queue item `uid`, for the mix planner.
    pub(super) fn dj_voice_break(&self, uid: u64) -> Option<listen_mix::plan::VoiceBreak> {
        use listen_mix::plan::{VoiceBreak, VoiceMode};
        let Break::Armed { before_uid, mode, clip: Some(_), clip_secs, .. } = &self.dj.brk else {
            return None;
        };
        if *before_uid != uid || !self.ai_dj_on() {
            return None;
        }
        let mode = match mode {
            BreakMode::Outro => VoiceMode::Outro,
            BreakMode::Across => VoiceMode::Across,
            BreakMode::Intro => VoiceMode::Intro,
            BreakMode::Between => return None,
        };
        Some(VoiceBreak { mode, clip_secs: f64::from(*clip_secs) })
    }

    pub fn dj_airing(&self) -> bool {
        matches!(self.dj.brk, Break::Airing { .. })
    }

    /// The DJ is talking over the music (voice player).
    pub fn dj_overlapping(&self) -> bool {
        matches!(self.dj.brk, Break::Overlap { .. })
    }

    fn dj_dir(&self) -> Option<PathBuf> {
        self.paths.as_ref().map(|p| p.cache_dir.join("dj"))
    }

    fn quota_file(&self) -> Option<PathBuf> {
        self.paths.as_ref().map(|p| p.data_dir.join("ai-dj-quota.json"))
    }

    /// Start-up: API key (env var first, then keyring), quota counter, and
    /// clips left over from the last run.
    pub(super) fn dj_boot(&mut self) {
        if self.demo {
            return;
        }
        self.dj.key = std::env::var("LISTEN_GEMINI_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| self.secrets.as_ref().and_then(|s| s.get(&ServerId(KEY_ENTRY.into()))));
        if let Some(file) = self.quota_file() {
            self.dj.quota = Quota::load(&file);
        }
        if let Some(dir) = self.dj_dir()
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with("break-") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }

    fn save_quota(&mut self) {
        if let Some(file) = self.quota_file() {
            self.dj.quota.save(&file);
        }
    }

    fn roll_quota(&mut self) {
        self.dj.quota.roll(quota::pacific_day(SystemTime::now()));
    }

    fn drop_break(&mut self) {
        match std::mem::take(&mut self.dj.brk) {
            Break::Armed { clip: Some(clip), .. } | Break::Airing { clip } => {
                let _ = std::fs::remove_file(clip);
            }
            Break::Overlap { clip, .. } => {
                // Cut the voice and put the music straight back up.
                self.dj.stop_voice = true;
                self.duck(false, true);
                let _ = std::fs::remove_file(clip);
            }
            _ => {}
        }
        self.dj.clip_in_slot = false;
    }

    /// Switches the DJ on (seeded with the current song) or off.
    pub(super) fn dj_switch(&mut self, on: bool) -> Task<Message> {
        self.config.settings.ai_dj.enabled = on;
        self.save_config();
        self.dj.generation += 1;
        self.dj.busy = false;
        self.drop_break();
        self.dj.caption = None;
        if !on {
            self.voice = None; // closes the voice player
            return self.refresh_next();
        }
        if !self.dj.has_key() {
            self.toast_warning("AI DJ: add a Gemini API key in Settings → AI DJ first.".into());
            self.config.settings.ai_dj.enabled = false;
            self.save_config();
            return Task::none();
        }
        // Songs the listener queued while the DJ was off still count.
        let picks = std::mem::take(&mut self.dj.listener_picks);
        self.dj_reseed();
        self.dj.listener_picks = picks;
        // Auto-DJ songs queued before are replaced by the DJ's own sets; what
        // they were tells the DJ more about the mood.
        let auto: Vec<(u64, Song)> = self
            .queue
            .iter()
            .skip(self.queue.current_position().map_or(0, |p| p + 1))
            .filter(|i| i.auto)
            .map(|i| (i.uid, i.song.clone()))
            .collect();
        for (uid, song) in auto {
            self.queue.remove(uid);
            self.dj.seed_context.push(song);
        }
        Task::batch([self.refresh_next(), self.dj_maybe_plan()])
    }

    /// The current song becomes the new mood seed.
    pub(super) fn dj_reseed(&mut self) {
        self.dj.generation += 1;
        self.dj.busy = false;
        self.dj.seed = self.queue.current().map(|i| i.song.clone());
        self.dj.seed_context.clear();
        self.dj.listener_picks.clear();
        self.dj.mood = None;
        self.dj.seed_mood = None;
        self.dj.played.clear();
        self.dj.first = true;
        self.dj.sets_since_talk = 0;
        self.dj.failures = 0;
        self.dj.backoff_until = None;
        self.drop_break();
        self.dj.caption = None;
    }

    /// Notes that the current song ended (naturally or skipped).
    fn dj_finished(&mut self, skipped: bool) {
        if let Some(item) = self.queue.current() {
            if self.dj.talked_over.take() == Some(item.uid) {
                return; // the DJ talked over its end: already part of the last break
            }
            let mixed_in =
                self.dj.entered.as_ref().filter(|(uid, _)| *uid == item.uid).and_then(|(_, how)| how.clone());
            self.dj.played.push(Played { song: item.song.clone(), skipped, by_user: !item.auto, mixed_in });
            if self.dj.played.len() > 12 {
                self.dj.played.remove(0);
            }
        }
    }

    /// User pressed "next".
    pub(super) fn dj_on_next(&mut self) {
        if !self.ai_dj_on() {
            return;
        }
        if self.dj_airing() {
            // Skipping the break itself: the set after it starts.
            self.drop_break();
            self.dj.caption = None;
        } else {
            self.dj_finished(true);
        }
    }

    /// A different track is started by hand (or after a failure).
    pub(super) fn dj_interrupt(&mut self) {
        if self.dj_airing() || self.dj_overlapping() {
            self.drop_break();
            self.dj.caption = None;
        }
        if self.ducked() {
            self.duck(false, true);
        }
    }

    /// mpv moved on. Returns the task when this was the clip starting to air
    /// (the queue must NOT advance then).
    pub(super) fn dj_on_advanced(&mut self) -> Option<Task<Message>> {
        if let Break::Airing { .. } = self.dj.brk {
            // The clip finished; the queue advances to the new set as usual.
            self.drop_break();
            self.dj.caption = None;
            return None;
        }
        if !self.ai_dj_on() {
            return None;
        }
        self.dj_finished(false);
        let Break::Armed { clip: Some(clip), line, mode: BreakMode::Between, .. } = self.dj.brk.clone()
        else {
            return None;
        };
        if !self.dj.clip_in_slot {
            return None;
        }
        tracing::info!("AI DJ break on air (between songs)");
        self.dj.clip_in_slot = false;
        self.dj.brk = Break::Airing { clip };
        self.dj.caption = Some((line.clone(), true));
        self.dj.last_line = Some(line);
        self.dj.played.clear();
        self.dj.sets_since_talk = 0;
        self.bump_token();
        self.playback.position = Duration::ZERO;
        self.playback.duration = None;
        self.fader.pos_at = None;
        let player = self.decks.active_player()?;
        let sources = self.sources.clone();
        let next = self.queue.peek_next().map(|i| i.song.clone());
        Some(Task::perform(
            async move {
                let track = match next {
                    Some(n) => stream_track(&sources, &n).await.ok(),
                    None => None,
                };
                player.advanced(track.as_ref()).await.map_err(err)
            },
            Message::PlayerDone,
        ))
    }

    /// The clip failed to play: go on with the next set.
    pub(super) fn dj_clip_failed(&mut self) -> Option<Task<Message>> {
        if !self.dj_airing() {
            return None;
        }
        tracing::warn!("AI DJ clip failed to play; skipping the break");
        self.drop_break();
        self.dj.caption = None;
        Some(match self.queue.skip_next() {
            Some(_) => self.start_current(),
            None => self.stop_player(),
        })
    }

    /// What goes into mpv's "next" slot: the armed clip right before its
    /// set, else the next song.
    pub(super) fn next_slot(&mut self) -> Option<Slot> {
        let next = self.queue.peek_next()?;
        let remaining = self.duration().saturating_sub(self.playback.position);
        if let Break::Armed { before_uid, clip: Some(clip), mode: BreakMode::Between, .. } = &self.dj.brk
            && next.uid == *before_uid
            && self.ai_dj_on()
            && (remaining > ARM_MARGIN || self.playback.position.is_zero())
        {
            self.dj.clip_in_slot = true;
            return Some(Slot::Clip(Track::voice_clip(clip.to_string_lossy())));
        }
        self.dj.clip_in_slot = false;
        Some(Slot::Song(Box::new(next.song.clone())))
    }

    /// Called on every track change (instead of the Auto-DJ while on).
    pub(super) fn dj_track_changed(&mut self) -> Task<Message> {
        if self.dj_airing() {
            return Task::none();
        }
        self.dj_note_entered();
        let current = self.queue.current().map(|i| i.uid);
        let mut start = Task::none();
        match self.dj.brk.clone() {
            // The new set starts with the DJ talking over its first song.
            Break::Armed { before_uid, clip: Some(clip), line, mode: BreakMode::Intro, .. }
                if Some(before_uid) == current && self.voice.is_some() && !self.dlna_casting() =>
            {
                start = self.start_overlap(clip, line, BreakMode::Intro);
            }
            // The set started without the clip airing (no voice, too late to
            // arm, or skipped into): show the line instead.
            Break::Armed { before_uid, line, .. } if Some(before_uid) == current => {
                self.drop_break();
                self.dj.caption = Some((line.clone(), false));
                self.dj.last_line = Some(line);
                self.dj.played.clear();
                self.dj.sets_since_talk = 0;
            }
            // Still talking across the change: the new song starts turned down.
            Break::Overlap { .. } => {}
            _ => {
                self.dj.caption = None;
                if self.ducked() {
                    self.duck(false, true);
                }
            }
        }
        Task::batch([start, self.dj_maybe_plan()])
    }

    /// Called with each position report: starts a break that talks over the
    /// end of the song at the right moment.
    pub(super) fn dj_check_overlap(&mut self) -> Option<Task<Message>> {
        let Break::Armed { before_uid, clip: Some(clip), line, mode, clip_secs } = &self.dj.brk else {
            return None;
        };
        if !matches!(mode, BreakMode::Outro | BreakMode::Across)
            || self.dlna_casting()
            || !self.playback.playing
            || self.voice.is_none()
            || self.queue.peek_next().map(|i| i.uid) != Some(*before_uid)
        {
            return None;
        }
        // With a DJ transition into the next song, the song "ends" where the
        // instrumental stretch of the blend ends (the planner's voice window).
        let total = match self.mix.plan().filter(|_| self.mix.owns_next()) {
            Some(plan) => plan.voice_window.map_or(plan.handover_out_s, |(_, end)| end) as f32,
            None => self.duration().as_secs_f32(),
        };
        let remaining = total - self.playback.position.as_secs_f32();
        if total <= 0.0 || remaining > lead_secs(*mode, *clip_secs) {
            return None;
        }
        let (clip, line, mode) = (clip.clone(), line.clone(), *mode);
        self.dj.talked_over = self.queue.current().map(|i| i.uid);
        Some(self.start_overlap(clip, line, mode))
    }

    /// The voice player starts talking over the music, which is turned down.
    fn start_overlap(&mut self, clip: PathBuf, line: String, mode: BreakMode) -> Task<Message> {
        tracing::info!("AI DJ break on air ({})", mode.name());
        // Over an intro the song starts already turned down; otherwise the knob turns.
        self.duck(true, mode == BreakMode::Intro);
        self.dj.brk = Break::Overlap { clip: clip.clone(), mode };
        self.dj.caption = Some((line.clone(), false));
        self.dj.last_line = Some(line);
        self.dj.played.clear();
        self.dj.sets_since_talk = 0;
        let volume = f32::from(self.playback.volume);
        let muted = self.playback.muted;
        Task::batch([
            self.push_volume(true),
            self.voice_task(move |v| async move {
                v.set_volume(volume).await?;
                v.set_muted(muted).await?;
                v.play(&Track::url(clip.to_string_lossy()), None).await
            }),
        ])
    }

    /// Messages from the voice player.
    pub(super) fn on_voice(&mut self, msg: crate::playback::PlayerMsg) -> Task<Message> {
        use crate::playback::PlayerMsg;
        use listen_player::PlayerEvent;
        match msg {
            PlayerMsg::Ready(player) => {
                tracing::info!("AI DJ voice player ready");
                self.voice = Some(player);
                let s = &self.config.settings.playback;
                let audio = listen_player::AudioSettings {
                    replay_gain: "no".into(),
                    audio_device: s.audio_device.clone(),
                    ..Default::default()
                };
                let volume = f32::from(self.playback.volume);
                return self.voice_task(move |v| async move {
                    v.apply(&audio).await?;
                    v.set_volume(volume).await
                });
            }
            PlayerMsg::Failed(e) => tracing::warn!("AI DJ voice player failed to start: {e}"),
            PlayerMsg::Event(PlayerEvent::Ended | PlayerEvent::Error(_)) => {
                if let Break::Overlap { clip, .. } = std::mem::take(&mut self.dj.brk) {
                    // Done talking: turn the music back up.
                    let _ = std::fs::remove_file(clip);
                    self.duck(false, false);
                }
            }
            PlayerMsg::Event(PlayerEvent::Crashed) => {
                self.voice = None;
                if self.dj_overlapping() {
                    self.drop_break();
                }
            }
            PlayerMsg::Event(_) => {}
        }
        Task::none()
    }

    /// Plans the next set when the current song is the last one queued.
    pub(super) fn dj_maybe_plan(&mut self) -> Task<Message> {
        if !self.ai_dj_on() || self.dj.busy || self.queue.upcoming() > 0 {
            return Task::none();
        }
        if self.dj.backoff_until.is_some_and(|t| Instant::now() < t) {
            return self.fetch_autodj();
        }
        let Some(current) = self.queue.current().map(|i| i.song.clone()) else { return Task::none() };
        let Some(key) = self.dj.key.clone() else { return self.fetch_autodj() };
        let seed = self.dj.seed.get_or_insert_with(|| current.clone()).clone();
        let source =
            self.sources.get(&seed.server_id).or_else(|| self.sources.get(&current.server_id)).cloned();
        let Some(source) = source else { return Task::none() };

        self.roll_quota();
        let s = self.config.settings.ai_dj.clone();
        let talk = self.dj.sets_since_talk + 1 >= s.talk_every.max(1);
        let voice =
            talk && !self.dlna_casting() && self.dj.quota.can_voice(s.max_voiced_per_day, quota::unix_now());
        self.dj.quota.planned += 1;
        if voice {
            self.dj.quota.voiced += 1;
        }
        self.save_quota();
        self.dj.sets_since_talk += 1;
        self.dj.busy = true;
        // Where the break sits, at random among what fits: talking over the
        // end of the song needs enough of it left for Gemini to answer.
        let remaining = self.duration().saturating_sub(self.playback.position).as_secs_f32();
        let modes: Vec<BreakMode> = BreakMode::ALL
            .into_iter()
            .filter(|m| match m {
                BreakMode::Between => true,
                BreakMode::Intro => voice && self.voice.is_some(),
                BreakMode::Outro | BreakMode::Across => voice && self.voice.is_some() && remaining > 30.0,
            })
            .collect();
        let mut mode = *modes.choose(&mut rand::rng()).unwrap_or(&BreakMode::Between);
        // Testing hook: LISTEN_AI_DJ_MODES=outro,intro,across,between (in turn).
        if let Ok(list) = std::env::var("LISTEN_AI_DJ_MODES") {
            let forced: Vec<BreakMode> = list
                .split(',')
                .filter_map(|m| match m.trim() {
                    "between" => Some(BreakMode::Between),
                    "outro" => Some(BreakMode::Outro),
                    "intro" => Some(BreakMode::Intro),
                    "across" => Some(BreakMode::Across),
                    _ => None,
                })
                .collect();
            if let Some(m) = forced.get(self.dj.quota.planned as usize % forced.len().max(1))
                && modes.contains(m)
            {
                mode = *m;
            }
        }

        // Nothing that played this session, is queued, or was picked lately —
        // in any version.
        let exclude: HashSet<String> = self
            .queue
            .iter()
            .map(|i| song_key(&i.song))
            .chain(self.dj.history.iter().cloned())
            .chain(self.session_played.iter().cloned())
            .collect();
        let picks = self.dj.listener_picks.clone();
        let lookup = self.flow_lookup();
        let flow_ctx = self.flow_ctx(&lookup);
        let mix_preview = self.dj_mix_preview(mode, voice);
        let http = self.http.clone();
        let persona = self.dj_persona();
        let played = self.dj.played.clone();
        let mood = self.dj.mood.clone();
        let seed_mood = self.dj.seed_mood.clone();
        let last_line = self.dj.last_line.clone();
        let first = std::mem::take(&mut self.dj.first);
        // Two-host shows: the co-host chimes in about every other break.
        let duo = persona.cohost.is_some() && rand::random_bool(0.5);
        let seed_context = self.dj.seed_context.clone();
        let context_len = self.config.settings.auto_dj.item_count.clamp(1, 10) as usize;
        let dir = self.dj_dir();
        let generation = self.dj.generation;
        tracing::info!(
            "AI DJ planning a set (seed “{}”, {} played since the break, voice: {voice}, break {})",
            seed.title,
            played.len(),
            mode.name()
        );
        Task::perform(
            async move {
                let gemini = Gemini::new(http, key)?;
                let (pool, similar) =
                    candidates(source, &seed, &current, &picks, &exclude, &lookup, &flow_ctx).await;
                // Nothing queued by the Auto-DJ to learn from: take what it would
                // have picked (the seed's similar songs, in order).
                let looked_up = if seed_context.is_empty() {
                    similar.into_iter().filter(|s| s.id != seed.id).take(context_len).collect()
                } else {
                    Vec::new()
                };
                let context = if seed_context.is_empty() { &looked_up } else { &seed_context };
                tracing::info!(
                    "AI DJ pool: {} candidates; seed context: {} songs{}",
                    pool.len(),
                    context.len(),
                    if seed_context.is_empty() { " (looked up)" } else { " (kept for the session)" }
                );
                if pool.is_empty() {
                    return Err(DjError::Api("no songs to choose from".into()));
                }
                // Measured tempo/key/energy for the prompt.
                let mut notes: std::collections::HashMap<String, String> = pool
                    .iter()
                    .chain([&seed, &current])
                    .chain(context.iter())
                    .chain(picks.iter())
                    .chain(played.iter().map(|p| &p.song))
                    .filter_map(|s| lookup(s).map(|f| (s.id.clone(), f.describe())))
                    .collect();
                // How each candidate would be mixed in, so the DJ knows its craft.
                if let Some(preview) = &mix_preview {
                    preview.annotate(&pool, &mut notes);
                }
                let req = PlanRequest {
                    persona,
                    seed: &seed,
                    seed_context: context,
                    listener_picks: &picks,
                    mood: mood.as_deref(),
                    seed_mood: seed_mood.as_deref(),
                    played: &played,
                    current: Some(&current),
                    last_line: last_line.as_deref(),
                    candidates: &pool,
                    notes: &notes,
                    set_min: s.set_min.max(1) as usize,
                    set_max: s.set_max.max(s.set_min).max(1) as usize,
                    first,
                    mode,
                    duo,
                    show: listen_dj::Show {
                        language: crate::i18n::english_name(crate::i18n::dj_language(&s.language)),
                        location: &s.location,
                        notes: &s.notes,
                    },
                };
                let (plan, _) = gemini.plan(&s.text_model, &req).await?;
                let songs = plan.picks.iter().map(|&i| pool[i].clone()).collect();
                let (mut clip, mut clip_secs, mut tts_error) = (None, 0.0, None);
                if voice && !plan.turns.is_empty() {
                    match speak(
                        &gemini,
                        &s.tts_model,
                        persona,
                        &plan.turns,
                        s.voice_gain_db,
                        s.place_pronunciation().as_slice(),
                    )
                    .await
                    {
                        Ok(pcm) => match dir.as_ref().map(|d| {
                            clip_secs = pcm.duration_secs();
                            save_clip(d, &pcm)
                        }) {
                            Some(Ok(path)) => clip = Some(path),
                            Some(Err(e)) => tts_error = Some(DjError::Api(e)),
                            None => {}
                        },
                        Err(e) => {
                            tracing::warn!("AI DJ speech failed: {e}");
                            tts_error = Some(e);
                        }
                    }
                }
                let line = if talk { plan.line() } else { String::new() };
                Ok(Planned {
                    songs,
                    mood: plan.mood,
                    line,
                    clip,
                    clip_secs,
                    seed_context: looked_up,
                    mode,
                    tts_error,
                })
            },
            move |r| Message::DjPlanned(generation, r),
        )
    }

    pub(super) fn dj_planned(&mut self, generation: u64, result: Result<Planned, DjError>) -> Task<Message> {
        if generation != self.dj.generation {
            if let Ok(Planned { clip: Some(clip), .. }) = result {
                let _ = std::fs::remove_file(clip);
            }
            return Task::none();
        }
        self.dj.busy = false;
        let planned = match result {
            Ok(p) => p,
            Err(e) => return self.dj_failed(e),
        };
        self.dj.failures = 0;
        if self.dj.seed_context.is_empty() {
            self.dj.seed_context = planned.seed_context.clone();
        }
        if let Some(e) = &planned.tts_error {
            if matches!(e, DjError::Quota { daily: true, .. }) {
                tracing::info!(
                    "AI DJ: the speech quota is used up; lines are shown as text (speech retried in an hour)"
                );
                self.dj.quota.mark_exhausted(quota::unix_now());
            }
            // The request failed, so it didn't use up a voiced break.
            self.dj.quota.voiced = self.dj.quota.voiced.saturating_sub(1);
            self.save_quota();
        }
        if planned.clip.is_some() && self.dj.quota.tts_exhausted {
            self.dj.quota.clear_exhausted();
            self.save_quota();
        }
        if !planned.mood.is_empty() {
            self.dj.mood = Some(planned.mood.clone());
            self.dj.seed_mood.get_or_insert_with(|| planned.mood.clone());
        }
        let count = planned.songs.len();
        for song in &planned.songs {
            self.dj.history.push_back(song_key(song));
        }
        while self.dj.history.len() > 300 {
            self.dj.history.pop_front();
        }
        let refs: Vec<ImageRef> = planned.songs.iter().filter_map(|s| s.image.clone()).collect();
        let first = planned.songs.first().cloned();
        self.queue.add_last(planned.songs, true);
        let before_uid = self.queue.iter().nth(self.queue.len().saturating_sub(count)).map(|i| i.uid);
        match before_uid {
            Some(before_uid) if !planned.line.is_empty() => {
                let mode = self.feasible_mode(planned.mode, planned.clip_secs, first.as_ref());
                tracing::info!(
                    "AI DJ queued {count} songs; break {} ({}, {:.1}s)",
                    if planned.clip.is_some() { "voiced" } else { "as text" },
                    mode.name(),
                    planned.clip_secs
                );
                self.dj.brk = Break::Armed {
                    before_uid,
                    clip: planned.clip,
                    line: planned.line,
                    mode,
                    clip_secs: planned.clip_secs,
                };
            }
            _ => {
                if let Some(clip) = planned.clip {
                    let _ = std::fs::remove_file(clip);
                }
                tracing::info!("AI DJ queued {count} songs (no break this time)");
            }
        }
        let images = self.request_images(refs.iter().map(Some));
        Task::batch([images, self.refresh_next()])
    }

    /// The planned mode, or the closest one that still fits now that the
    /// clip's length is known (Gemini may have been slow, the song short).
    fn feasible_mode(&self, mode: BreakMode, clip_secs: f32, first: Option<&Song>) -> BreakMode {
        if !mode.over_music() {
            return mode;
        }
        if self.voice.is_none() {
            return BreakMode::Between;
        }
        let remaining = self.duration().as_secs_f32() - self.playback.position.as_secs_f32();
        let fits = |m: BreakMode| remaining > lead_secs(m, clip_secs) + 2.0;
        // Never talk over the next singer: a voice over the new song's start must end before its vocals
        // (from the analysis; unknown intros are trusted).
        let intro = first
            .and_then(|s| self.analysis.as_ref()?.lookup(s))
            .and_then(|r| r.features().and_then(super::dj_mix::instrumental_intro_secs));
        let over_intro = |m: BreakMode| match (m, intro) {
            (BreakMode::Intro, Some(i)) => clip_secs + 0.5 <= i,
            // Across: a bit over half of the clip falls after the change.
            (BreakMode::Across, Some(i)) => clip_secs - lead_secs(BreakMode::Across, clip_secs) + 0.5 <= i,
            _ => true,
        };
        match mode {
            BreakMode::Outro if fits(BreakMode::Outro) => BreakMode::Outro,
            BreakMode::Outro | BreakMode::Across
                if fits(BreakMode::Across) && over_intro(BreakMode::Across) =>
            {
                BreakMode::Across
            }
            BreakMode::Across | BreakMode::Intro if over_intro(BreakMode::Intro) => BreakMode::Intro,
            BreakMode::Outro | BreakMode::Across | BreakMode::Intro if fits(BreakMode::Outro) => {
                BreakMode::Outro
            }
            _ => BreakMode::Between,
        }
    }

    fn dj_failed(&mut self, e: DjError) -> Task<Message> {
        self.dj.failures += 1;
        let wait = match &e {
            DjError::Quota { daily: true, .. } => Duration::from_secs(3600),
            DjError::Quota { retry_after: Some(d), .. } => *d + Duration::from_secs(5),
            _ => Duration::from_secs(30 * 2u64.pow(self.dj.failures.min(6))),
        };
        self.dj.backoff_until = Some(Instant::now() + wait.min(Duration::from_secs(3600)));
        tracing::warn!("AI DJ: {e}; Auto-DJ fills in, retrying in {}s", wait.as_secs());
        match e {
            DjError::Auth | DjError::NoKey => {
                self.toast_warning(
                    "AI DJ: the Gemini API key was rejected — check it in Settings → AI DJ.".into(),
                );
            }
            _ if self.dj.failures == 1 => {
                self.toast_warning(format!("AI DJ unavailable ({e}); similar songs fill in for now."));
            }
            _ => {}
        }
        self.dj.sets_since_talk = self.dj.sets_since_talk.saturating_sub(1);
        self.fetch_autodj()
    }

    // ---- settings -------------------------------------------------------

    pub(super) fn dj_setting(&mut self, change: Setting) -> Task<Message> {
        let s = &mut self.config.settings.ai_dj;
        match change {
            Setting::AiDj(on) => return self.dj_switch(on),
            Setting::AiDjPersona(id) => s.persona = id,
            Setting::AiDjTtsModel(m) => s.tts_model = m,
            Setting::AiDjSetMax(n) => {
                s.set_max = n.clamp(2, 8);
                s.set_min = s.set_min.min(s.set_max);
            }
            Setting::AiDjTalkEvery(n) => s.talk_every = n.clamp(1, 5),
            Setting::AiDjMaxVoiced(n) => s.max_voiced_per_day = n,
            Setting::AiDjVoiceGain(db) => s.voice_gain_db = db.clamp(-12.0, 12.0),
            Setting::AiDjLocation(text) => s.location = text,
            Setting::AiDjPronunciation(text) => s.location_pronunciation = text,
            Setting::AiDjNotes(text) => s.notes = text,
            Setting::AiDjLanguage(code) => s.language = code,
            Setting::AiDjKeyInput(text) => {
                self.dj.key_input = text;
                return Task::none();
            }
            Setting::AiDjSaveKey => {
                let key = self.dj.key_input.trim().to_string();
                self.dj.key_input.clear();
                if key.is_empty() {
                    if let Some(secrets) = &self.secrets {
                        secrets.delete(&ServerId(KEY_ENTRY.into()));
                    }
                    self.dj.key = None;
                    self.dj.status = Some("API key removed.".into());
                    if self.config.settings.ai_dj.enabled {
                        return self.dj_switch(false);
                    }
                    return Task::none();
                }
                if let (Some(secrets), false) = (&self.secrets, self.ephemeral) {
                    secrets.set(&ServerId(KEY_ENTRY.into()), &key);
                }
                self.dj.key = Some(key);
                // A new key may have its own (or a paid) quota.
                self.dj.quota.clear_exhausted();
                self.dj.backoff_until = None;
                self.dj.failures = 0;
                self.save_quota();
                self.dj.status = Some("API key saved. Checking it…".into());
                return self.dj_preview();
            }
            Setting::AiDjPreview => return self.dj_preview(),
            _ => return Task::none(),
        }
        self.save_config();
        Task::none()
    }

    /// Speaks the persona's sample line (cached per persona and model, so a
    /// preview costs one speech request only once) and plays it.
    fn dj_preview(&mut self) -> Task<Message> {
        let (Some(key), Some(dir)) = (self.dj.key.clone(), self.dj_dir()) else {
            self.dj.status = Some("Add a Gemini API key first.".into());
            return Task::none();
        };
        if self.dj.previewing {
            return Task::none();
        }
        let s = self.config.settings.ai_dj.clone();
        let persona = self.dj_persona();
        let path = dir.join(format!("preview-{}-{}.wav", persona.id, s.tts_model));
        let cached = path.exists();
        if !cached {
            self.roll_quota();
            self.dj.quota.voiced += 1;
            self.save_quota();
        }
        self.dj.previewing = true;
        let http = self.http.clone();
        let mpv = self.player_config.binary.clone();
        Task::perform(
            async move {
                if !cached {
                    let gemini = Gemini::new(http, key)?;
                    let pcm = speak(
                        &gemini,
                        &s.tts_model,
                        persona,
                        &sample_turns(persona),
                        s.voice_gain_db,
                        s.place_pronunciation().as_slice(),
                    )
                    .await?;
                    std::fs::create_dir_all(&dir).map_err(|e| DjError::Api(e.to_string()))?;
                    std::fs::write(&path, listen_dj::wav::encode(&pcm))
                        .map_err(|e| DjError::Api(e.to_string()))?;
                }
                let mut cmd = tokio::process::Command::new(mpv);
                listen_core::platform::no_window(cmd.as_std_mut());
                let _ = cmd
                    .args(["--no-config", "--no-video", "--really-quiet", "--audio-client-name=Listen AI DJ"])
                    .arg(&path)
                    .status()
                    .await;
                Ok(())
            },
            Message::DjPreviewDone,
        )
    }

    pub(super) fn dj_preview_done(&mut self, result: Result<(), DjError>) {
        self.dj.previewing = false;
        self.dj.status = Some(match result {
            Ok(()) => {
                // Speech works, so the quota isn't used up (anymore).
                if self.dj.quota.tts_exhausted {
                    self.dj.quota.clear_exhausted();
                    self.save_quota();
                }
                format!("{} works.", self.dj_persona().name)
            }
            Err(e) => {
                if matches!(e, DjError::Quota { daily: true, .. }) {
                    self.dj.quota.mark_exhausted(quota::unix_now());
                    self.save_quota();
                }
                format!("Didn't work: {e}")
            }
        });
    }

    /// "12 of 15 spoken breaks left today" for the settings.
    pub fn dj_quota_line(&self) -> String {
        let q = &self.dj.quota;
        let today = quota::pacific_day(SystemTime::now());
        let max = self.config.settings.ai_dj.max_voiced_per_day;
        let used = if q.day == today { q.voiced } else { 0 };
        if q.day == today && q.tts_exhausted && !q.can_voice(max, quota::unix_now()) {
            return "Google reported the speech quota as used up — lines show as text; trying again within the hour."
                .into();
        }
        // The quota day starts at 08:00 UTC (midnight on the fixed UTC-8 clock in `quota`).
        let reset = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(8, 0, 0)
            .map(|t| t.and_utc().with_timezone(&chrono::Local).format("%H:%M").to_string())
            .unwrap_or_default();
        format!("{} of {max} spoken breaks left today (resets {reset}).", max.saturating_sub(used))
    }
}

/// Seconds before the song's end the voice starts: over the outro it ends with the song; across the
/// change a bit under half of it falls before the end.
fn lead_secs(mode: BreakMode, clip_secs: f32) -> f32 {
    match mode {
        BreakMode::Outro => clip_secs + 0.3,
        BreakMode::Across => (clip_secs * 0.45).clamp(2.5, (clip_secs - 2.0).max(2.5)),
        BreakMode::Between | BreakMode::Intro => 0.0,
    }
}

/// Library songs around the seed, the listener's picks and the current song, minus anything played this
/// session or queued (by version key); one version per song (radio edits preferred), shuffled.
async fn candidates(
    source: std::sync::Arc<dyn listen_source::MusicSource>,
    seed: &Song,
    current: &Song,
    picks: &[Song],
    exclude: &HashSet<String>,
    lookup: &super::flow::FlowLookup,
    flow_ctx: &listen_mix::score::FlowCtx,
) -> (Vec<Song>, Vec<Song>) {
    let from_current = async {
        if current.id == seed.id {
            Vec::new()
        } else {
            source.similar_songs(current, 10).await.unwrap_or_default()
        }
    };
    // Mostly the seed's neighbours, some of the current song's for flow, a few random ones; the listener's
    // two latest picks pull the pool their way.
    let from_picks = async {
        let mut out = Vec::new();
        for pick in picks.iter().rev().take(2) {
            out.extend(source.similar_songs(pick, 8).await.unwrap_or_default());
        }
        out
    };
    let (a, b, c, d) =
        tokio::join!(source.similar_songs(seed, 30), from_current, source.random_songs(8, None), from_picks);
    let seed_similar = a.unwrap_or_default();
    let current_key = song_key(current);
    let usable = |s: &Song| {
        let key = song_key(s);
        !exclude.contains(&key) && key != current_key
    };
    // Picks' neighbours first so they survive the cut, then the seed's.
    let similar: Vec<Song> =
        d.into_iter().chain(seed_similar.iter().cloned()).chain(b).filter(usable).collect();
    let mut pool = dedupe_versions(similar);
    // With analysis: keep the similar songs that also flow from the current
    // one (rank fusion of the source's order and tempo/key/energy fit).
    if let Some(here) = lookup(current) {
        let annotated: Vec<(Song, Option<listen_core::analysis::FlowFeatures>)> =
            pool.iter().map(|s| (s.clone(), lookup(s))).collect();
        let flow = listen_mix::score::rank((current, Some(&here)), &annotated, flow_ctx);
        let mut fused: Vec<(usize, usize)> = flow.iter().enumerate().map(|(r, &i)| (i + r, i)).collect();
        fused.sort();
        pool = fused.into_iter().map(|(_, i)| annotated[i].0.clone()).collect();
    }
    pool.truncate(POOL - 6);
    let mut keys: HashSet<String> = pool.iter().map(song_key).collect();
    let random = dedupe_versions(c.unwrap_or_default().into_iter().filter(usable).collect());
    pool.extend(random.into_iter().filter(|s| keys.insert(song_key(s))).take(POOL - pool.len()));
    pool.shuffle(&mut rand::rng());
    (pool, seed_similar)
}

async fn speak(
    gemini: &Gemini,
    model: &str,
    persona: &Persona,
    turns: &[Turn],
    gain_db: f32,
    say: &[(&str, &str)],
) -> Result<listen_dj::wav::Pcm, DjError> {
    let (wav, _) = match gemini.speak(model, persona, turns, say).await {
        // A per-minute limit: wait as told (briefly) and try once more.
        Err(DjError::Quota { daily: false, retry_after }) => {
            tokio::time::sleep(retry_after.unwrap_or(Duration::from_secs(20)).min(Duration::from_secs(40)))
                .await;
            gemini.speak(model, persona, turns, say).await?
        }
        other => other?,
    };
    let pcm = listen_dj::wav::decode(&wav).map_err(DjError::Api)?;
    Ok(listen_dj::wav::prepare(pcm, VOICE_DBFS, gain_db, 0.35))
}

/// What the voice preview says: the host's sample, and the co-host's reply.
fn sample_turns(persona: &Persona) -> Vec<Turn> {
    let mut turns = vec![Turn {
        speaker: persona.host.into(),
        text: crate::i18n::tr(persona.sample).into(),
        style: String::new(),
    }];
    if let Some(co) = &persona.cohost {
        turns.push(Turn {
            speaker: co.name.into(),
            text: crate::i18n::tr(co.sample).into(),
            style: String::new(),
        });
    }
    turns
}

fn save_clip(dir: &std::path::Path, pcm: &listen_dj::wav::Pcm) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let path = dir.join(format!("break-{stamp}.wav"));
    std::fs::write(&path, listen_dj::wav::encode(pcm)).map_err(|e| e.to_string())?;
    Ok(path)
}
