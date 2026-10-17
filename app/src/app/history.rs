//! Local listening history: recording plays, the Recently played / Statistics
//! / Mixes pages and the privacy controls. Everything stays on this computer.

use super::{Listen, Message, Route};
use iced::Task;
use listen_core::history::{self, Entry, MixKind};
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

    /// The current song counts as played.
    pub(super) fn record_play(&mut self) {
        if !self.config.settings.history.enabled || self.history_view.private {
            return;
        }
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else { return };
        let now = history::now_secs();
        if now - self.history_view.last_prune >= PRUNE_EVERY_SECS {
            self.history_view.last_prune = now;
            if let Err(e) = self.history.prune(self.config.settings.history.retention.max_age_days(), now) {
                tracing::warn!("pruning the listening history: {e}");
            }
        }
        if let Err(e) = self.history.add(Entry::new(&song, now)) {
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
        v.mix_counts = MixKind::ALL
            .into_iter()
            .map(|k| (k, history::mix(entries, k, now, tz, MIX_LIMIT).len()))
            .collect();
        v.mix = match &self.route {
            Route::Mix(kind) => {
                history::mix(entries, *kind, now, tz, MIX_LIMIT).iter().map(Entry::to_song).collect()
            }
            _ => Vec::new(),
        };
    }

    /// Removes the single play at `index` of the history.
    pub(super) fn history_remove_entry(&mut self, index: usize) {
        match self.history.remove(index) {
            Ok(()) => self.toast_success("Removed from the history.".into()),
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
}
