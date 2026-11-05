//! Local listening history: recording plays, the Recently played / Statistics
//! / Mixes pages and the privacy controls. It stays on this computer unless merging is on
//! (`history_sync`).

use super::{Listen, Message, Route};
use iced::Task;
use listen_core::history::{self, Entry, MixKind, Picker, PlayCtx};
use listen_core::model::Song;
use listen_core::settings::HistoryRetention;

const RECENT_LIMIT: usize = 500;
const MIX_LIMIT: usize = 50;
const TOP: usize = 10;
const PRUNE_EVERY_SECS: i64 = 86_400;

#[derive(Debug, Clone)]
pub enum HistoryMsg {
    PlayRecent(usize),
    PlayMix(usize),
    /// Statistics year (`None` = all time).
    Year(Option<i32>),
    ConfirmClear(bool),
    Clear,
    Export(bool),
    /// Merge with the other devices now.
    Sync,
    Synced(Result<super::history_sync::Outcome, String>),
    SyncToken(String),
    SaveSyncToken,
}

#[derive(Debug, Default)]
pub struct HistoryView {
    pub recent: Vec<Song>,
    /// Index into `History::entries` of each row of `recent`.
    pub recent_idx: Vec<usize>,
    /// When expired entries were last dropped (unix seconds).
    pub last_prune: i64,
    pub mix: Vec<Song>,
    pub mix_counts: Vec<(MixKind, usize)>,
    pub stats: history::Stats,
    pub year: Option<i32>,
    pub confirm_clear: bool,
    /// "Don't record this session": never saved, off again on the next start.
    pub private: bool,
    pub sync: super::history_sync::SyncState,
    pub syncing: bool,
    pub sync_status: Option<super::history_sync::SyncStatus>,
    /// The token being typed, gone once saved.
    pub sync_token: String,
    pub sync_token_saved: bool,
}

impl Listen {
    pub(super) fn history_boot(&mut self) {
        self.history_view.year = Some(history::current_year(history::local_offset()));
        self.history_view.last_prune = history::now_secs();
        if let Some(days) = self.config.settings.history.retention.max_age_days()
            && let Err(e) = self.history.prune(Some(days), history::now_secs())
        {
            tracing::warn!("pruning the listening history: {e}");
        }
    }

    /// Play mode, picker and device for a history entry of the current song.
    pub(super) fn play_ctx(&self) -> PlayCtx {
        use super::play_mode::PlayMode as M;
        use listen_core::history::PlayMode;
        let mode = match self.play_mode() {
            M::Off => PlayMode::Off,
            M::Shuffle => PlayMode::Shuffle,
            M::AutoDj => PlayMode::AutoDj,
            M::AiDj => PlayMode::AiDj,
        };
        let picker = match self.queue.current() {
            Some(i) if i.bridge.is_some() => Picker::Bridge,
            Some(i) if i.why.is_some() => i.why.as_ref().map_or(Picker::User, |w| w.picker),
            Some(i) if i.ai => Picker::AiDj,
            Some(i) if i.auto => Picker::AutoDj,
            _ => Picker::User,
        };
        PlayCtx { mode, picker, device: listen_analysis::store::machine_id() }
    }

    /// The user leaves the current song for another one (Next, Previous, another song started by hand):
    /// a skip when it started playing but has not counted as played yet. Automatic handovers (a DJ mix,
    /// the end of the song, a failed stream) never call this; DJ breaks and bridges are not logged.
    pub(super) fn note_skip(&mut self) {
        if let Some(e) = self.pending_skip() {
            self.commit_skip(e);
        }
    }

    /// Moves in the queue with `step` and records the skip only when another song became current
    /// (Previous on the first song just restarts it).
    pub(super) fn step_skipping(&mut self, step: impl FnOnce(&mut listen_core::queue::Queue)) {
        let before = self.queue.current().map(|i| i.uid);
        let skip = self.pending_skip();
        step(&mut self.queue);
        if let Some(e) = skip
            && self.queue.current().map(|i| i.uid) != before
        {
            self.commit_skip(e);
        }
    }

    /// The skip entry for the current song, if leaving it now is a skip.
    fn pending_skip(&self) -> Option<Entry> {
        if !self.config.settings.history.enabled
            || self.history_view.private
            || self.playback.scrobbled
            || self.playback.last_pos.is_none()
            || self.restored.is_some()
            || self.radio.active.is_some()
            || self.bridge_playing()
            || self.dj_airing()
        {
            return None;
        }
        let song = &self.queue.current()?.song;
        let at = self.playback.position.as_secs_f32();
        let mut entry = Entry::skipped(song, history::now_secs(), at, self.play_ctx());
        if let Some(d) = self.playback.duration.filter(|d| !d.is_zero()) {
            entry.frac = Some((at / d.as_secs_f32()).clamp(0.0, 1.0));
        }
        Some(entry)
    }

    fn commit_skip(&mut self, entry: Entry) {
        tracing::info!("skipped “{}” at {:.0} s", entry.title, entry.at_s.unwrap_or(0.0));
        // Counted once: a second skip path for the same song does nothing.
        self.playback.scrobbled = true;
        if let Err(e) = self.history.add(entry) {
            tracing::warn!("saving the listening history: {e}");
        }
        if matches!(self.route, Route::Stats) {
            self.refresh_history_views();
        }
    }

    /// The current song counts as played.
    pub(super) fn record_play(&mut self) {
        // A bridge only played a few bars.
        if !self.config.settings.history.enabled || self.history_view.private || self.bridge_playing() {
            return;
        }
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else { return };
        let ctx = self.play_ctx();
        let now = history::now_secs();
        if now - self.history_view.last_prune >= PRUNE_EVERY_SECS {
            self.history_view.last_prune = now;
            if let Err(e) = self.history.prune(self.config.settings.history.retention.max_age_days(), now) {
                tracing::warn!("pruning the listening history: {e}");
            }
        }
        if let Err(e) = self.history.add(Entry { ctx: Some(ctx), ..Entry::new(&song, now) }) {
            tracing::warn!("saving the listening history: {e}");
        }
        if matches!(self.route, Route::Recent | Route::Stats | Route::Mixes | Route::Mix(_)) {
            self.refresh_history_views();
        }
    }

    /// Recomputes what the history pages show.
    pub(super) fn refresh_history_views(&mut self) {
        let tz = history::local_offset();
        let now = history::now_secs();
        let entries = &self.history.entries;
        // The mixes are automatic picks: snoozed and "Not interested" songs stay out.
        let blocked = self.auto_blocked();
        let mix = |kind| {
            let mut m = history::mix(entries, kind, now, tz, MIX_LIMIT + blocked.len());
            m.retain(|e| !blocked.contains(&listen_core::versions::song_key(&e.to_song())));
            m.truncate(MIX_LIMIT);
            m
        };
        let v = &mut self.history_view;
        let recent = self.history.recent(RECENT_LIMIT);
        v.recent_idx = recent.iter().map(|(i, _)| *i).collect();
        v.recent = recent.into_iter().map(|(_, e)| e.to_song()).collect();
        v.stats = history::stats(entries, v.year, tz, TOP);
        if let Some(y) = v.year
            && !v.stats.years.contains(&y)
            && let Some(&newest) = v.stats.years.first()
        {
            v.year = Some(newest);
            v.stats = history::stats(entries, v.year, tz, TOP);
        }
        v.mix_counts = MixKind::ALL.into_iter().map(|k| (k, mix(k).len())).collect();
        v.mix = match &self.route {
            Route::Mix(kind) => mix(*kind).iter().map(Entry::to_song).collect(),
            _ => Vec::new(),
        };
    }

    /// Removes the single play at `index` of the history.
    pub(super) fn history_remove_entry(&mut self, index: usize) {
        let entry = self.history.entries.get(index).cloned();
        match self.history.remove(index) {
            Ok(()) => {
                if let Some(entry) = &entry {
                    self.history_sync_removed(entry);
                }
                self.toast_success("Removed from the history.".into());
            }
            Err(e) => self.toast_error(format!("Could not update the history: {e}")),
        }
        self.refresh_history_views();
    }

    pub(super) fn on_history(&mut self, msg: HistoryMsg) -> Task<Message> {
        match msg {
            HistoryMsg::PlayRecent(i) => return self.play_picked(self.history_view.recent.clone(), i),
            HistoryMsg::PlayMix(i) => return self.play_picked(self.history_view.mix.clone(), i),
            HistoryMsg::Year(year) => {
                self.history_view.year = year;
                self.refresh_history_views();
            }
            HistoryMsg::ConfirmClear(v) => self.history_view.confirm_clear = v,
            HistoryMsg::Clear => {
                self.history_view.confirm_clear = false;
                match self.history.clear() {
                    Ok(()) => self.toast_success("Listening history deleted.".into()),
                    Err(e) => self.toast_error(format!("Could not delete the history: {e}")),
                }
                self.refresh_history_views();
            }
            HistoryMsg::Sync => return Task::batch([self.history_sync(), self.exclude_sync()]),
            HistoryMsg::Synced(result) => self.history_synced(result),
            HistoryMsg::SyncToken(token) => self.history_view.sync_token = token,
            HistoryMsg::SaveSyncToken => return self.history_sync_save_token(),
            HistoryMsg::Export(csv) => {
                let dir = listen_core::platform::home_dir()
                    .map(|h| h.join("Downloads"))
                    .filter(|d| d.is_dir())
                    .or_else(|| self.paths.as_ref().map(|p| p.data_dir.clone()));
                self.toast_info(match dir {
                    None => "No folder to export to.".into(),
                    Some(dir) => match self.export_history(&dir, csv) {
                        Ok(path) => format!("History exported to {}.", path.display()),
                        Err(e) => format!("Could not export the history: {e}"),
                    },
                });
            }
        }
        Task::none()
    }

    fn export_history(&self, dir: &std::path::Path, csv: bool) -> std::io::Result<std::path::PathBuf> {
        let (text, ext) =
            if csv { (self.history.export_csv(), "csv") } else { (self.history.export_json(), "json") };
        let path = dir.join(format!("listen-history-{}.{ext}", history::now_secs()));
        std::fs::write(&path, text)?;
        Ok(path)
    }

    pub(super) fn set_history_retention(&mut self, r: HistoryRetention) {
        self.config.settings.history.retention = r;
        if let Err(e) = self.history.prune(r.max_age_days(), history::now_secs()) {
            tracing::warn!("pruning the listening history: {e}");
        }
        self.refresh_history_views();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Args, app::Listen};
    use listen_core::model::{ServerId, SourceKind};
    use listen_core::settings::HistoryRetention;

    const DAY: i64 = 86_400;

    fn rig() -> Listen {
        let (mut s, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
        s.history.entries.clear();
        s.queue.set(vec![dummy()], 0);
        s
    }

    fn dummy() -> Song {
        Song {
            mbz: Default::default(),
            extra: Default::default(),
            id: "1".into(),
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            title: "T".into(),
            artists: vec![],
            album_artists: vec![],
            album: None,
            album_id: None,
            image: None,
            duration: std::time::Duration::from_secs(200),
            track_number: None,
            disc_number: None,
            year: None,
            genres: vec![],
            codec: None,
            bit_rate: None,
            sample_rate: None,
            bit_depth: None,
            bpm: None,
            favorite: false,
            rating: None,
            play_count: None,
            gain: Default::default(),
            explicit: false,
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: None,
            comment: None,
        }
    }

    #[test]
    fn records_a_play_unless_off_or_private() {
        let mut s = rig();
        s.record_play();
        assert_eq!(s.history.entries.len(), 1);
        s.history_view.private = true;
        s.record_play();
        assert_eq!(s.history.entries.len(), 1);
        s.history_view.private = false;
        s.config.settings.history.enabled = false;
        s.record_play();
        assert_eq!(s.history.entries.len(), 1);
    }

    #[test]
    fn retention_change_prunes() {
        let mut s = rig();
        let mut old = s.queue.current().map(|i| Entry::new(&i.song, 0)).unwrap();
        old.ts = history::now_secs() - 400 * DAY;
        s.history.entries.push(old);
        s.record_play();
        assert_eq!(s.history.entries.len(), 2);
        s.set_history_retention(HistoryRetention::OneYear);
        assert_eq!(s.history.entries.len(), 1);
    }

    #[test]
    fn recording_drops_expired_entries_after_a_day() {
        let mut s = rig();
        s.config.settings.history.retention = HistoryRetention::OneYear;
        let mut old = s.queue.current().map(|i| Entry::new(&i.song, 0)).unwrap();
        old.ts = history::now_secs() - 400 * DAY;
        s.history.entries.push(old);
        s.history_view.last_prune = history::now_secs() - 2 * DAY;
        s.record_play();
        assert_eq!(s.history.entries.len(), 1);
    }

    #[test]
    fn removes_one_play_only_and_year_falls_back() {
        let mut s = rig();
        s.record_play();
        s.record_play();
        s.record_play();
        s.refresh_history_views();
        assert_eq!(s.history_view.recent_idx, vec![2, 1, 0]);
        s.history_remove_entry(s.history_view.recent_idx[0]);
        assert_eq!(s.history.entries.len(), 2);
        s.history.entries.iter_mut().for_each(|e| e.ts = 1_600_000_000);
        s.history_view.year = Some(2099);
        s.refresh_history_views();
        assert_eq!(s.history_view.year, Some(2020));
        assert_eq!(s.history_view.stats.plays, 2);
    }

    #[test]
    fn export_writes_a_file() {
        let mut s = rig();
        s.record_play();
        let dir = std::env::temp_dir().join(format!("listen-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let json = s.export_history(&dir, false).unwrap();
        let csv = s.export_history(&dir, true).unwrap();
        assert!(std::fs::read_to_string(json).unwrap().contains("\"song_id\""));
        assert!(std::fs::read_to_string(csv).unwrap().starts_with("timestamp,"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two songs queued, the first one playing `secs` seconds in (position events arrived).
    fn playing_at(secs: u64) -> Listen {
        let mut s = rig();
        let mut b = dummy();
        b.id = "2".into();
        s.queue.set(vec![dummy(), b], 0);
        s.playback.playing = true;
        for p in 0..=secs {
            s.playback.track_played(std::time::Duration::from_secs(p));
        }
        s.playback.position = std::time::Duration::from_secs(secs);
        s
    }

    #[test]
    fn next_before_the_scrobble_point_records_one_skip() {
        use listen_core::history::{PlayMode, PlayStatus};
        let mut s = playing_at(10);
        s.config.settings.ai_dj.enabled = false;
        s.config.settings.auto_dj.enabled = true;
        if let Some(i) = s.queue.current_mut() {
            i.auto = true;
        }
        let _ = s.update(Message::Next);
        assert_eq!(s.history.entries.len(), 1);
        let e = &s.history.entries[0];
        assert_eq!((e.status, e.song_id.as_str()), (PlayStatus::Skipped, "1"));
        assert!((e.at_s.unwrap() - 10.0).abs() < 0.5);
        assert!((e.frac.unwrap() - 0.05).abs() < 0.01);
        let ctx = e.ctx.as_ref().unwrap();
        assert_eq!((ctx.mode, ctx.picker), (PlayMode::AutoDj, Picker::AutoDj));
        assert!(!ctx.device.is_empty());
        assert!(s.history.recent(10).is_empty(), "a skip is not a play");
    }

    #[test]
    fn plays_handovers_and_unstarted_songs_are_not_skips() {
        use listen_core::history::PlayStatus;
        // Played past the scrobble point, then Next: one play, no skip.
        let mut s = playing_at(150);
        s.record_play();
        s.playback.scrobbled = true;
        let _ = s.update(Message::Next);
        assert_eq!(s.history.entries.iter().map(|e| e.status).collect::<Vec<_>>(), [PlayStatus::Played]);
        assert_eq!(s.history.entries[0].ctx.as_ref().map(|c| c.picker), Some(Picker::User));

        // The song ends on its own (automatic handover): nothing.
        let mut s = playing_at(10);
        s.queue.advance();
        let _ = s.start_current();
        assert!(s.history.entries.is_empty());

        // A song that never started (no position yet), or a restored queue: nothing.
        let mut s = rig();
        let _ = s.update(Message::Next);
        assert!(s.history.entries.is_empty());
        let mut s = playing_at(10);
        s.restored = Some(std::time::Duration::from_secs(10));
        s.note_skip();
        assert!(s.history.entries.is_empty());

        // History off or private: nothing; two skip paths for one song: one skip.
        let mut s = playing_at(10);
        s.history_view.private = true;
        s.note_skip();
        assert!(s.history.entries.is_empty());
        let mut s = playing_at(10);
        s.note_skip();
        s.note_skip();
        assert_eq!(s.history.entries.len(), 1);
    }

    #[test]
    fn previous_and_picking_another_song_skip_too() {
        // Previous on the first song only restarts it: no skip.
        let mut s = playing_at(5);
        let _ = s.update(Message::Previous);
        assert!(s.history.entries.is_empty());
        let mut s = playing_at(5);
        s.queue.advance();
        let _ = s.update(Message::Previous);
        assert_eq!(s.history.entries.len(), 1);
        assert_eq!(s.history.entries[0].song_id, "2");
        // More than 10 s in, Previous restarts the song: no skip.
        let mut s = playing_at(12);
        let _ = s.update(Message::Previous);
        assert!(s.history.entries.is_empty());
        let mut s = playing_at(5);
        let uid = s.queue.peek_next().map(|i| i.uid).unwrap();
        let _ = s.update(Message::PlayQueueItem(uid));
        assert_eq!(s.history.entries.len(), 1);
    }

    #[test]
    fn a_song_that_ended_on_its_own_is_no_skip_later() {
        use crate::playback::PlayerMsg;
        use listen_player::PlayerEvent;
        // Ends below the scrobble point (seeked to the end), then the user starts another song.
        let mut s = playing_at(10);
        let _ = s.update(Message::Deck(0, PlayerMsg::Event(PlayerEvent::Ended)));
        let uid = s.queue.peek_next().map(|i| i.uid).unwrap();
        let _ = s.update(Message::PlayQueueItem(uid));
        assert!(s.history.entries.is_empty(), "{:?}", s.history.entries);
        // Same after Stop.
        let mut s = playing_at(10);
        let _ = s.update(Message::Stop);
        let uid = s.queue.peek_next().map(|i| i.uid).unwrap();
        let _ = s.update(Message::PlayQueueItem(uid));
        assert!(s.history.entries.is_empty(), "{:?}", s.history.entries);
    }
}
