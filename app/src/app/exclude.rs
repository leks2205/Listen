//! Snooze, Not interested and Hide in this playlist (listen-core `exclude`): the song menus set them,
//! every automatic pool (Auto-DJ, AI DJ, bridges, the history mixes) leaves the songs out, playlist
//! playback leaves out the songs hidden in that playlist, and Settings → Hidden songs undoes them.
//! Songs picked by hand always play.

use super::{Listen, Message};
use iced::Task;
use listen_core::exclude::{Exclusions, FILE_NAME};
use listen_core::history::now_secs;
use listen_core::model::{Song, SourceKind};
use listen_core::versions::song_key;
use std::collections::HashSet;
use std::path::PathBuf;

/// How long cleared marks are kept so the clear reaches every device.
const KEEP_CLEARED_DAYS: i64 = 90;

#[derive(Debug, Clone)]
pub enum ExcludeMsg {
    /// Keep the songs out of automatic picks for 30 days.
    Snooze(Vec<Song>),
    /// Keep the songs out of automatic picks for good (and dislike them on YouTube Music).
    NotInterested(Vec<Song>),
    /// Hide (`true`) or show again the songs in the playlist with this id.
    HideIn(String, Vec<Song>, bool),
    /// Settings → Hidden songs: end the snooze of this song key.
    Unsnooze(String),
    /// Settings → Hidden songs: take "Not interested" back for this song key.
    Restore(String),
    /// Settings → Hidden songs: show this song key again in the playlist.
    Unhide(String, String),
    /// The YouTube Music dislike (or taking it back) finished.
    Rated(Result<(), String>),
    /// listen-server answered with the merged state of every device.
    Synced(Result<Exclusions, String>),
}

#[derive(Debug, Default)]
pub struct ExcludeState {
    pub set: Exclusions,
    /// `None` for demo / ephemeral sessions: nothing is written.
    pub path: Option<PathBuf>,
    pub syncing: bool,
    /// Title and artist per song key, for Settings → Hidden songs (from the songs marked this run and
    /// the history).
    pub names: std::collections::HashMap<String, (String, String)>,
}

impl ExcludeState {
    pub fn load(dir: Option<&std::path::Path>) -> Self {
        let path = dir.map(|d| d.join(FILE_NAME));
        let mut set = path.as_deref().map(Exclusions::load).unwrap_or_default();
        set.prune(now_secs(), KEEP_CLEARED_DAYS);
        Self { set, path, syncing: false, names: Default::default() }
    }
}

fn artist(song: &Song) -> String {
    song.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
}

impl Listen {
    /// May this song enter the queue through an automatic pick now?
    #[cfg(test)]
    pub(super) fn auto_allows(&self, song: &Song) -> bool {
        self.exclusions.set.allows(song, listen_core::exclude::Ctx::Auto { now: now_secs() })
    }

    /// Song keys every automatic pool leaves out now: snoozed, "Not interested", and worn out (skipped
    /// again and again lately, `memory.rs`).
    pub(super) fn auto_blocked(&self) -> HashSet<String> {
        let mut keys: HashSet<String> = self.exclusions.set.blocked_keys(now_secs()).cloned().collect();
        keys.extend(self.worn_out());
        keys
    }

    /// Playlist playback: the songs of playlist `id` minus the ones hidden in it, with `start` moved to
    /// the same song. A hidden song the user started by hand (`keep`) still plays.
    pub(super) fn without_hidden(
        &self,
        id: &str,
        songs: Vec<Song>,
        start: usize,
        keep: bool,
    ) -> (Vec<Song>, usize) {
        let hidden: HashSet<&String> = self.exclusions.set.hidden_keys(id).collect();
        if hidden.is_empty() {
            return (songs, start);
        }
        let mut out = Vec::with_capacity(songs.len());
        let mut new_start = 0;
        for (i, s) in songs.into_iter().enumerate() {
            if i == start {
                new_start = out.len();
            }
            if (keep && i == start) || !hidden.contains(&song_key(&s)) {
                out.push(s);
            }
        }
        let new_start = new_start.min(out.len().saturating_sub(1));
        (out, new_start)
    }

    pub(super) fn exclude_save(&mut self) {
        if let Some(path) = &self.exclusions.path
            && let Err(e) = self.exclusions.set.save(path)
        {
            tracing::warn!("saving {FILE_NAME}: {e}");
        }
    }

    fn exclude_remember(&mut self, songs: &[Song]) {
        for s in songs {
            self.exclusions.names.insert(song_key(s), (s.title.clone(), artist(s)));
        }
    }

    /// Upcoming songs a DJ added that are now left out go from the queue (never the current one, never
    /// the user's own).
    fn exclude_prune_queue(&mut self) {
        let blocked = self.auto_blocked();
        let current = self.queue.current().map(|i| i.uid);
        let drop: Vec<u64> = self
            .queue
            .iter()
            .filter(|i| i.auto && Some(i.uid) != current && blocked.contains(&song_key(&i.song)))
            .map(|i| i.uid)
            .collect();
        for uid in &drop {
            self.queue.remove(*uid);
        }
        if !drop.is_empty() {
            tracing::info!("dropped {} DJ picks that are snoozed or not wanted", drop.len());
        }
    }

    /// Dislikes (or takes the dislike back of) the YouTube Music songs among `songs`.
    fn exclude_rate(&self, songs: &[Song], disliked: bool) -> Task<Message> {
        let mut tasks = Vec::new();
        let mut by_server: std::collections::HashMap<_, Vec<String>> = Default::default();
        for s in songs.iter().filter(|s| s.source == SourceKind::YouTubeMusic) {
            by_server.entry(s.server_id.clone()).or_default().push(s.id.clone());
        }
        for (server, ids) in by_server {
            let Some(source) = self.sources.get(&server).cloned() else { continue };
            tasks.push(Task::perform(
                async move { source.set_disliked(&ids, disliked).await.map_err(|e| e.to_string()) },
                |r| Message::Exclude(ExcludeMsg::Rated(r)),
            ));
        }
        Task::batch(tasks)
    }

    fn exclude_find(&self, key: &str) -> Option<Song> {
        self.history.entries.iter().rev().find(|e| song_key(&e.to_song()) == key).map(|e| e.to_song())
    }

    pub(super) fn on_exclude(&mut self, msg: ExcludeMsg) -> Task<Message> {
        let now = now_secs();
        let task = match msg {
            ExcludeMsg::Snooze(songs) => {
                self.exclude_remember(&songs);
                for s in &songs {
                    self.exclusions.set.snooze(s, now);
                }
                self.exclude_prune_queue();
                self.toast_success(if songs.len() == 1 {
                    "Snoozed for 30 days: the DJs leave it out.".into()
                } else {
                    "Snoozed for 30 days: the DJs leave them out.".into()
                });
                Task::none()
            }
            ExcludeMsg::NotInterested(songs) => {
                self.exclude_remember(&songs);
                for s in &songs {
                    self.exclusions.set.set_not_interested(&song_key(s), true, now);
                }
                self.exclude_prune_queue();
                self.toast_success("The DJs won't pick it again. Undo in Settings → Hidden songs.".into());
                if self.config.settings.history.dislike_not_interested {
                    self.exclude_rate(&songs, true)
                } else {
                    Task::none()
                }
            }
            ExcludeMsg::HideIn(id, songs, on) => {
                self.exclude_remember(&songs);
                for s in &songs {
                    self.exclusions.set.set_hidden_in(&id, &song_key(s), on, now);
                }
                self.toast_success(if on {
                    "Hidden in this playlist: it is skipped when the playlist plays.".into()
                } else {
                    "Shown in this playlist again.".into()
                });
                Task::none()
            }
            ExcludeMsg::Unsnooze(key) => {
                self.exclusions.set.unsnooze(&key, now);
                Task::none()
            }
            ExcludeMsg::Restore(key) => {
                self.exclusions.set.set_not_interested(&key, false, now);
                let song = self.exclude_find(&key);
                match song.filter(|_| self.config.settings.history.dislike_not_interested) {
                    Some(s) => self.exclude_rate(&[s], false),
                    None => Task::none(),
                }
            }
            ExcludeMsg::Unhide(id, key) => {
                self.exclusions.set.set_hidden_in(&id, &key, false, now);
                Task::none()
            }
            ExcludeMsg::Synced(result) => {
                self.exclusions.syncing = false;
                match result {
                    Ok(theirs) => {
                        if self.exclusions.set.merge(&theirs) {
                            tracing::info!("merged the hidden songs of the other devices");
                            self.exclude_save();
                            self.exclude_prune_queue();
                            self.refresh_history_views();
                        }
                    }
                    Err(e) => tracing::warn!("merging the hidden songs: {e}"),
                }
                return Task::none();
            }
            ExcludeMsg::Rated(Ok(())) => return Task::none(),
            ExcludeMsg::Rated(Err(e)) => {
                tracing::warn!("YouTube Music dislike: {e}");
                return Task::none();
            }
        };
        self.exclude_save();
        Task::batch([task, self.exclude_sync()])
    }

    /// Sends the state to listen-server and merges what comes back (with "Merge with other devices"
    /// on, the same address and token as the history). A server without `/v1/exclusions` only logs.
    pub(super) fn exclude_sync(&mut self) -> Task<Message> {
        if !self.config.settings.history.sync || self.exclusions.syncing || self.exclusions.path.is_none() {
            return Task::none();
        }
        let Some(server) = self.listen_server() else { return Task::none() };
        let (url, token) = (server.url("v1/exclusions"), server.token);
        let http = self.http.clone();
        let body = self.exclusions.set.clone();
        self.exclusions.syncing = true;
        Task::perform(
            async move {
                let response = http
                    .post(url)
                    .timeout(super::history_sync::SYNC_TIMEOUT)
                    .bearer_auth(token)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| e.without_url().to_string())?;
                let response = response.error_for_status().map_err(|e| e.without_url().to_string())?;
                response.json::<Exclusions>().await.map_err(|e| e.without_url().to_string())
            },
            |r| Message::Exclude(ExcludeMsg::Synced(r)),
        )
    }

    /// One row of Settings → Hidden songs: title and artist of a song key.
    pub fn exclude_name(&self, key: &str) -> (String, String) {
        if let Some(n) = self.exclusions.names.get(key) {
            return n.clone();
        }
        match self.exclude_find(key) {
            Some(s) => (s.title.clone(), artist(&s)),
            None => (key.split('|').next().unwrap_or(key).to_string(), String::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Args;
    use crate::demo;

    fn rig() -> Listen {
        let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
        s.config.settings.history.dislike_not_interested = false;
        s
    }

    #[test]
    fn snooze_and_not_interested_keep_songs_out_of_dj_picks_only() {
        let mut s = rig();
        let songs = demo::album_songs(0);
        let (a, b) = (songs[0].clone(), songs[1].clone());
        let _ = s.on_exclude(ExcludeMsg::Snooze(vec![a.clone()]));
        let _ = s.on_exclude(ExcludeMsg::NotInterested(vec![b.clone()]));
        assert!(!s.auto_allows(&a) && !s.auto_allows(&b));
        assert!(s.auto_allows(&songs[2]));
        assert_eq!(s.auto_blocked().len(), 2);
        // Undo from Settings → Hidden songs.
        let _ = s.on_exclude(ExcludeMsg::Unsnooze(song_key(&a)));
        let _ = s.on_exclude(ExcludeMsg::Restore(song_key(&b)));
        assert!(s.auto_allows(&a) && s.auto_allows(&b));
    }

    #[test]
    fn auto_dj_fill_and_queue_leave_snoozed_songs_out() {
        let mut s = rig();
        let songs = demo::album_songs(3);
        s.queue.set(demo::album_songs(0)[..1].to_vec(), 0);
        let _ = s.on_exclude(ExcludeMsg::Snooze(vec![songs[0].clone()]));
        let _ = s.update(Message::AutoDjLoaded(Ok(songs.clone())));
        let keys: Vec<String> = s.queue.iter().map(|i| song_key(&i.song)).collect();
        assert!(!keys.contains(&song_key(&songs[0])), "{keys:?}");
        assert!(keys.len() > 1, "the other songs were added");
        // A DJ pick already queued goes when it gets snoozed; the user's own songs stay.
        let picked = s.queue.iter().find(|i| i.auto).map(|i| i.song.clone()).unwrap();
        let first = s.queue.iter().next().map(|i| (i.uid, i.song.clone())).unwrap();
        let _ = s.on_exclude(ExcludeMsg::NotInterested(vec![picked.clone(), first.1.clone()]));
        assert!(!s.queue.iter().any(|i| song_key(&i.song) == song_key(&picked)));
        assert_eq!(s.queue.current().map(|i| i.uid), Some(first.0), "the current song stays");
    }

    #[test]
    fn the_other_devices_state_merges_in() {
        let mut s = rig();
        let songs = demo::album_songs(0);
        let mut theirs = Exclusions::default();
        theirs.set_not_interested(&song_key(&songs[0]), true, now_secs());
        theirs.set_hidden_in("p1", &song_key(&songs[1]), true, now_secs());
        let _ = s.on_exclude(ExcludeMsg::Synced(Ok(theirs)));
        assert!(!s.auto_allows(&songs[0]));
        assert_eq!(s.without_hidden("p1", songs.clone(), 0, false).0.len(), songs.len() - 1);
        // A failed sync changes nothing and allows the next one.
        let _ = s.on_exclude(ExcludeMsg::Synced(Err("offline".into())));
        assert!(!s.exclusions.syncing && !s.auto_allows(&songs[0]));
    }

    #[test]
    fn hidden_songs_skip_only_in_their_playlist() {
        let mut s = rig();
        let songs = demo::album_songs(0);
        let _ = s.on_exclude(ExcludeMsg::HideIn("p1".into(), vec![songs[1].clone()], true));
        let (out, start) = s.without_hidden("p1", songs.clone(), 2, false);
        assert_eq!(out.len(), songs.len() - 1);
        assert_eq!(out[start].id, songs[2].id);
        // Started by hand: it plays.
        let (out, start) = s.without_hidden("p1", songs.clone(), 1, true);
        assert_eq!((out.len(), out[start].id.as_str()), (songs.len(), songs[1].id.as_str()));
        // Another playlist and the DJs are not affected.
        assert_eq!(s.without_hidden("p2", songs.clone(), 0, false).0.len(), songs.len());
        assert!(s.auto_allows(&songs[1]));
        let _ = s.on_exclude(ExcludeMsg::Unhide("p1".into(), song_key(&songs[1])));
        assert_eq!(s.without_hidden("p1", songs.clone(), 0, false).0.len(), songs.len());
    }
}
