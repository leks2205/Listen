//! Volume levelling (item 2): every song is brought to the target loudness of the "Volume
//! levelling" setting with the gain from `listen_mix::loudness` (contract C5). The gain is the
//! `level_in` of an `alimiter` at the end of each deck's `af` chain, so boosts are limited.
//! A new song gets its gain at once; a changed setting or a late loudness value ramps at most
//! 1 dB per 50 ms (`ramp_toward` on the fade clock).
//!
//! mpv builds a deck's lavfi graph from the `af` string when audio starts and again after every
//! seek or new file, so a live `af-command` is lost then (and fails before the graph exists):
//! a value counts as sent only once mpv took it, and each `Restarted` of a deck sends it again.

use super::{Listen, Message, err};
use iced::Task;
use listen_core::model::Song;
use listen_core::model::SourceKind;
use listen_core::settings::{LEVELLING_LABEL, LEVELLING_TARGET, Levelling};
use listen_mix::loudness::{self, GainDb, GainSource, RgTag};
use listen_source::Urgency;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Smallest change worth an `af-command` (dB).
const SEND_EPS_DB: f32 = 0.05;

#[derive(Debug, Default, Clone, Copy)]
pub struct DeckLevel {
    /// The song the gain is for.
    uid: Option<u64>,
    /// The setting the target was computed with.
    computed_with: Option<Levelling>,
    /// `ytm` generation the target was computed with (a late YouTube loudness recomputes).
    generation: u64,
    pub target: Option<GainDb>,
    /// The gain on the deck now (dB; ramps toward `target`).
    pub applied_db: f32,
    /// Last value mpv took (or is being sent); `None` sends the applied gain again.
    sent_db: Option<f32>,
}

impl DeckLevel {
    /// Takes the deck's song and its gain. A new song gets its gain at once; a changed setting
    /// or source only moves the target (the gain ramps). Returns the new target when it is
    /// worth a log line.
    fn retarget(
        &mut self,
        uid: Option<u64>,
        gain: Option<GainDb>,
        setting: Levelling,
        generation: u64,
    ) -> Option<GainDb> {
        let new_song = uid != self.uid;
        if !new_song && self.computed_with == Some(setting) && self.generation == generation {
            return None;
        }
        let changed = gain.filter(|g| new_song || self.target.is_none_or(|t| (t.db - g.db).abs() > 0.01));
        self.uid = uid;
        self.computed_with = Some(setting);
        self.generation = generation;
        self.target = gain;
        if new_song && let Some(g) = gain {
            self.applied_db = g.db;
        }
        changed
    }

    /// Ramps toward the target over `dt` seconds; the dB value to send to mpv, if any.
    fn step(&mut self, dt: f32) -> Option<f32> {
        let target = self.target.map_or(0.0, |t| t.db);
        self.applied_db = loudness::ramp_toward(self.applied_db, target, dt);
        let stale = self.sent_db.is_none_or(|s| (s - self.applied_db).abs() > SEND_EPS_DB)
            || (self.applied_db == target && self.sent_db != Some(target));
        stale.then(|| {
            self.sent_db = Some(self.applied_db);
            self.applied_db
        })
    }

    /// mpv refused `db` (no graph yet): send again on the next step unless something newer went out.
    fn send_failed(&mut self, db: f32) {
        if self.sent_db == Some(db) {
            self.sent_db = None;
        }
    }

    /// mpv rebuilt the deck's graph from the `af` string (seek, new file): the live gain is gone.
    fn graph_rebuilt(&mut self) {
        self.sent_db = None;
    }
}

#[derive(Debug, Default)]
pub struct LevelState {
    pub decks: [DeckLevel; 2],
    /// YouTube's per-video `loudnessDb` for songs without an analysis record (video id → dB).
    pub ytm: HashMap<String, f32>,
    /// Videos asked for (once per run, answered or not).
    ytm_asked: HashSet<String>,
    generation: u64,
    last_tick: Option<Instant>,
    /// The levelling stage is in the decks' chains (stays until a switch-off has ramped to 0 dB).
    pub chain_on: bool,
}

impl LevelState {
    /// A YouTube loudness arrived: songs without a better source recompute (and ramp).
    pub fn add_ytm(&mut self, video_id: String, loudness_db: f32) {
        self.ytm.insert(video_id, loudness_db);
        self.generation += 1;
    }
}

impl Listen {
    /// The song on `deck` right now: the queue's current one on the active deck, the mix's
    /// songs during a blend.
    fn levelling_song(&self, deck: usize) -> Option<(u64, Song)> {
        let uid = self
            .mix
            .deck_uid(deck)
            .or_else(|| (deck == self.decks.active).then(|| self.queue.current().map(|i| i.uid)).flatten())?;
        self.queue.iter().find(|i| i.uid == uid).map(|i| (uid, i.song.clone()))
    }

    /// The C5 gain of a song under the current setting.
    pub fn levelling_gain(&self, song: &Song) -> GainDb {
        let p = &self.config.settings.playback;
        let record = self.analysis.as_ref().and_then(|a| a.lookup(song));
        let features = record.as_deref().and_then(|r| r.features());
        let ytm = self.level.ytm.get(&song.id).copied();
        let album = p.replay_gain == listen_core::settings::ReplayGain::Album;
        loudness::track_gain(features, ytm, RgTag::from_info(&song.gain, album), p.levelling, true)
    }

    /// Gain (linear) the levelling stage of `deck` is built with.
    pub(super) fn levelling_level_in(&self, deck: usize) -> f32 {
        loudness::db_to_factor(self.level.decks[deck.min(1)].applied_db)
    }

    /// The decks' chains carry the levelling stage (and mpv's own ReplayGain is off).
    pub(super) fn levelling_in_chain(&self) -> bool {
        self.level.chain_on || self.config.settings.playback.levelling != Levelling::Off
    }

    /// Whether the levelling stage still has work: a deck's song changed or a gain ramps.
    pub(super) fn levelling_active(&self) -> bool {
        let setting = self.config.settings.playback.levelling;
        if !self.level.chain_on {
            return setting != Levelling::Off;
        }
        (0..2).any(|deck| {
            let d = &self.level.decks[deck];
            if self.decks.d[deck].player.is_none() {
                return false;
            }
            let uid = self.levelling_song(deck).map(|(u, _)| u);
            uid != d.uid
                || d.computed_with != Some(setting)
                || d.generation != self.level.generation
                || d.target.is_some_and(|t| (t.db - d.applied_db).abs() > 1e-3)
                || d.sent_db.is_none_or(|s| (s - d.applied_db).abs() > SEND_EPS_DB)
        })
    }

    /// One step: targets for new songs (applied at once), ramps for the rest, `af-command`s
    /// for what changed. Switching levelling on adds the stage at 0 dB; switching it off ramps
    /// to 0 dB, then takes the stage out of the chain.
    pub(super) fn levelling_tick(&mut self) -> Task<Message> {
        let now = Instant::now();
        let dt = self.level.last_tick.map_or(0.03, |t| now.duration_since(t).as_secs_f32().min(0.1));
        self.level.last_tick = Some(now);
        let setting = self.config.settings.playback.levelling;
        if setting != Levelling::Off && !self.level.chain_on {
            // The chains were built with the stage at 0 dB. Switched on mid-song, the songs
            // playing ramp from there instead of jumping; at a song's start the gain applies
            // at once.
            self.level.chain_on = true;
            let mid_song = self.est_position().as_secs_f32() > 1.0;
            for deck in 0..2 {
                let uid = self.levelling_song(deck).map(|(u, _)| u).filter(|_| mid_song);
                self.level.decks[deck] = DeckLevel { uid, sent_db: Some(0.0), ..DeckLevel::default() };
            }
            return Task::none();
        }
        if !self.level.chain_on || self.mock_playback() {
            return Task::none();
        }
        let mut tasks = vec![self.levelling_fetch_ytm()];
        let mut all_unity = true;
        for deck in 0..2 {
            let Some(player) = self.decks.d[deck].player.clone() else { continue };
            // The AI DJ's voice clip plays without the chain.
            if deck == self.decks.active && self.dj_airing() {
                continue;
            }
            let song = self.levelling_song(deck);
            let gain = song.as_ref().map(|(_, s)| self.levelling_gain(s));
            let generation = self.level.generation;
            let d = &mut self.level.decks[deck];
            if let Some(g) = d.retarget(song.as_ref().map(|(u, _)| *u), gain, setting, generation)
                && let Some((_, s)) = &song
            {
                let lufs = g.lufs.map_or("?".into(), |l| format!("{l:.1} LUFS"));
                tracing::info!(
                    "levelling: {} {:+.1} dB from {} ({lufs}, target {setting})",
                    s.title,
                    g.db,
                    g.source.as_str()
                );
            }
            let send = d.step(dt);
            if d.applied_db.abs() > 1e-3 {
                all_unity = false;
            }
            if let Some(db) = send {
                let arg = format!("{:.6}", loudness::db_to_factor(db));
                let deck = deck as u8;
                tasks.push(Task::perform(
                    async move { player.af_command(LEVELLING_LABEL, "level_in", &arg, LEVELLING_TARGET).await },
                    move |r| Message::LevelSent(deck, db, r.map_err(err)),
                ));
            }
        }
        if setting == Levelling::Off && all_unity {
            self.level.chain_on = false;
            for d in &mut self.level.decks {
                *d = DeckLevel::default();
            }
            tasks.push(self.apply_audio());
        }
        Task::batch(tasks)
    }

    /// Asks YouTube for the loudness of the songs on the decks and the next one when they are
    /// YouTube Music songs without an analysis record.
    fn levelling_fetch_ytm(&mut self) -> Task<Message> {
        let mut songs: Vec<Song> = (0..2).filter_map(|d| self.levelling_song(d).map(|(_, s)| s)).collect();
        songs.extend(self.queue.peek_next().map(|i| i.song.clone()));
        let mut tasks = Vec::new();
        for song in songs {
            if song.source != SourceKind::YouTubeMusic
                || self.level.ytm_asked.contains(&song.id)
                || self
                    .analysis
                    .as_ref()
                    .and_then(|a| a.lookup(&song))
                    .is_some_and(|r| r.features().is_some())
            {
                continue;
            }
            self.level.ytm_asked.insert(song.id.clone());
            let id = song.id.clone();
            tasks.push(Task::perform(
                async move {
                    let r = listen_ytmusic::loudness::fetch_loudness_db(&id, Urgency::Now).await;
                    (id, r)
                },
                |(id, r)| Message::YtmLoudness(id, r),
            ));
        }
        Task::batch(tasks)
    }

    /// mpv's answer to a level `af-command`.
    pub(super) fn on_level_sent(&mut self, deck: u8, db: f32, r: Result<(), String>) {
        if let Err(e) = r {
            // Normal right after a load, before mpv has built the graph; the next tick retries.
            tracing::debug!("levelling: deck {deck} did not take {db:+.2} dB yet: {e}");
            self.level.decks[usize::from(deck).min(1)].send_failed(db);
        }
    }

    /// A deck (re)started playback: its graph was rebuilt with the gain of the `af` string.
    pub(super) fn levelling_on_restart(&mut self, deck: u8) {
        if self.level.chain_on {
            self.level.decks[usize::from(deck).min(1)].graph_rebuilt();
        }
    }

    pub(super) fn on_ytm_loudness(&mut self, id: String, r: Result<Option<f32>, String>) {
        match r {
            Ok(Some(db)) => {
                tracing::debug!("levelling: YouTube loudness of {id}: {db:+.2} dB");
                self.level.add_ytm(id, db);
            }
            Ok(None) => tracing::debug!("levelling: YouTube has no loudness for {id}"),
            Err(e) => tracing::debug!("levelling: YouTube loudness of {id} failed: {e}"),
        }
    }

    /// Get info: the gain levelling applies to `song` and where it came from.
    pub fn levelling_describe(&self, song: &Song) -> Option<String> {
        let g = self.levelling_gain(song);
        (g.source != GainSource::Off).then(|| match g.lufs {
            Some(l) => format!("{:+.1} dB ({l:.1} LUFS, {})", g.db, crate::i18n::tr(source_label(g.source))),
            None => format!("{:+.1} dB ({})", g.db, crate::i18n::tr("loudness unknown")),
        })
    }
}

/// Get info: where a gain came from, in English (translated by the caller).
fn source_label(source: GainSource) -> &'static str {
    match source {
        GainSource::Analysis => "from analysis",
        GainSource::YouTube => "from YouTube",
        GainSource::ReplayGain => "from ReplayGain",
        GainSource::Off | GainSource::Unknown => "loudness unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bug 123: Get info names the gain's source in German too.
    #[test]
    fn levelling_sources_are_german_in_the_german_ui() {
        let de: Vec<_> =
            [GainSource::Analysis, GainSource::YouTube, GainSource::ReplayGain, GainSource::Unknown]
                .into_iter()
                .map(|s| crate::i18n::translate_in("de", source_label(s)).unwrap_or_default())
                .collect();
        assert_eq!(de, ["aus der Analyse", "von YouTube", "aus ReplayGain", "Lautheit unbekannt"]);
    }
    use listen_core::model::GainInfo;

    fn g(db: f32) -> Option<GainDb> {
        Some(GainDb { db, source: GainSource::Analysis, lufs: Some(-14.0 - db) })
    }

    #[test]
    fn new_song_snaps_setting_change_ramps() {
        let mut d = DeckLevel::default();
        assert!(d.retarget(Some(1), g(-6.0), Levelling::Normal, 0).is_some());
        assert_eq!(d.step(0.03), Some(-6.0), "a new song starts at its gain");
        assert_eq!(d.step(0.03), None, "nothing to send while steady");
        // Switched to Loud (+3 dB): ramps, never more than 1 dB per 50 ms.
        assert!(d.retarget(Some(1), g(-3.0), Levelling::Loud, 0).is_some());
        let mut last = -6.0;
        let mut steps = 0;
        while let Some(db) = d.step(0.05) {
            assert!(db - last <= 1.0 + 1e-4, "{last} -> {db}");
            last = db;
            steps += 1;
        }
        assert_eq!(last, -3.0);
        assert_eq!(steps, 3);
        // Switched off: ramps to 0 dB.
        d.retarget(Some(1), Some(GainDb::UNITY), Levelling::Off, 0);
        while d.step(0.03).is_some() {}
        assert_eq!(d.applied_db, 0.0);
        // The next song under a new source generation snaps again.
        d.retarget(Some(2), g(4.0), Levelling::Normal, 1);
        assert_eq!(d.step(0.03), Some(4.0));
    }

    #[test]
    fn refused_or_lost_gain_is_sent_again() {
        let mut d = DeckLevel::default();
        d.retarget(Some(1), g(-6.0), Levelling::Normal, 0);
        assert_eq!(d.step(0.03), Some(-6.0));
        // Sent before mpv had a graph: refused, so the next step sends it again.
        d.send_failed(-6.0);
        assert_eq!(d.step(0.03), Some(-6.0));
        assert_eq!(d.step(0.03), None);
        // A refusal of an older value does not resend the newer one.
        d.send_failed(-5.0);
        assert_eq!(d.step(0.03), None);
        // A seek rebuilt the graph from the chain string: the gain goes out again.
        d.graph_rebuilt();
        assert_eq!(d.step(0.03), Some(-6.0));
        assert_eq!(d.step(0.03), None);
    }

    #[test]
    fn late_youtube_loudness_ramps() {
        let mut d = DeckLevel::default();
        let unknown = Some(GainDb { db: 0.0, source: GainSource::Unknown, lufs: None });
        d.retarget(Some(7), unknown, Levelling::Normal, 0);
        assert_eq!(d.step(0.03), Some(0.0));
        assert!(d.retarget(Some(7), g(-2.0), Levelling::Normal, 1).is_some(), "generation bump");
        let db = d.step(0.03).expect("ramp step");
        assert!((db + 0.6).abs() < 1e-4, "20 dB/s × 30 ms: {db}");
    }

    #[test]
    fn gain_sources_and_chain() {
        let (mut s, _) = Listen::boot(&crate::Args { demo: true, ephemeral: true, ..crate::Args::default() });
        let mut song = crate::demo::album_songs(0).remove(0);
        song.gain = GainInfo { track_gain: Some(-4.0), track_peak: Some(0.9), ..GainInfo::default() };
        let gain = s.levelling_gain(&song);
        // ReplayGain −4 dB ⇒ −14 LUFS (RG 2 reference −18) ⇒ 0 dB to Normal.
        assert_eq!(gain.source, GainSource::ReplayGain);
        assert!(gain.db.abs() < 1e-4, "{gain:?}");
        assert_eq!(s.levelling_describe(&song).as_deref(), Some("+0.0 dB (-14.0 LUFS, from replaygain)"));
        s.level.add_ytm(song.id.clone(), -3.0);
        song.source = SourceKind::YouTubeMusic;
        let gain = s.levelling_gain(&song);
        assert_eq!(gain.source, GainSource::YouTube, "YouTube before ReplayGain");
        assert!((gain.db - 3.0).abs() < 1e-4);

        s.config.settings.playback.replay_gain = listen_core::settings::ReplayGain::Track;
        let a = s.audio_settings_for(0);
        assert!(
            a.audio_filters.ends_with(&listen_core::settings::levelling_filter(1.0)),
            "{}",
            a.audio_filters
        );
        assert_eq!(a.replay_gain, "no", "levelling replaces mpv's ReplayGain");
        s.config.settings.playback.levelling = Levelling::Off;
        let a = s.audio_settings_for(0);
        assert!(!a.audio_filters.contains("alimiter"), "{}", a.audio_filters);
        assert_eq!(a.replay_gain, "track");
        assert_eq!(s.levelling_describe(&song), None);
    }
}
