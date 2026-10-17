//! Offline downloads: songs, albums and playlists are fetched one at a time
//! into [`listen_offline::Store`] (Navidrome: the server's download endpoint;
//! YouTube Music: its audio fetch, under the limits in `listen_offline`), and
//! played from the local copy whenever there is one.

use super::{Listen, Message};
use iced::Task;
use listen_core::model::{ServerId, Song, SourceKind};
use listen_offline::{Refused, Store};
use listen_source::{Capabilities, MusicSource};
use std::collections::VecDeque;
use std::time::Duration;

/// Pause between two YouTube Music downloads (no request bursts).
const YTM_PAUSE: Duration = Duration::from_secs(4);

#[derive(Debug, Default)]
pub struct OfflineState {
    queue: VecDeque<Song>,
    /// The song being downloaded.
    pub active: Option<Song>,
    /// Songs finished or failed since the queue was last empty.
    done: usize,
    total: usize,
    batch_failed: usize,
    /// Songs that could not be downloaded: (title, reason).
    pub failed: Vec<(String, String)>,
    last_was_ytm: bool,
    /// The active download was removed meanwhile: delete it once it lands.
    cancel_active: bool,
    /// What is downloaded (sorted) and its size on disk; see `Listen::offline_refresh`.
    pub songs: Vec<Song>,
    pub sizes: Vec<u64>,
}

impl OfflineState {
    pub fn busy(&self) -> bool {
        self.active.is_some() || !self.queue.is_empty()
    }

    /// "Downloading “Title” (2 of 5)…", while something downloads.
    pub fn status(&self) -> Option<String> {
        let song = self.active.as_ref()?;
        Some(format!("Downloading “{}” ({} of {})…", song.title, (self.done + 1).min(self.total), self.total))
    }

    pub fn queued(&self, song: &Song) -> bool {
        self.active.as_ref().is_some_and(|a| same(a, song)) || self.queue.iter().any(|q| same(q, song))
    }
}

fn same(a: &Song, b: &Song) -> bool {
    a.server_id == b.server_id && a.id == b.id
}

#[derive(Debug, Clone)]
pub enum OfflineMsg {
    Download(Vec<Song>),
    Finished(Box<Song>, Result<(), String>),
    Remove(ServerId, String),
    RemoveAll,
    /// Play the downloads from this position of the (sorted) list.
    Play(usize),
    ClearFailed,
}

/// Downloaded songs in a stable order (artist, album, track) with their sizes.
fn sorted_entries(store: &Store) -> (Vec<Song>, Vec<u64>) {
    let mut entries = store.entries();
    entries.sort_by_cached_key(|e| {
        let s = &e.song;
        (
            s.artist_line().to_lowercase(),
            s.album.clone().unwrap_or_default().to_lowercase(),
            s.disc_number,
            s.track_number,
            s.title.to_lowercase(),
        )
    });
    entries.into_iter().map(|e| (e.song, e.bytes)).unzip()
}

pub fn format_bytes(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= 1024.0 * MB {
        format!("{:.1} GB", b / (1024.0 * MB))
    } else if bytes == 0 {
        "0 MB".to_string()
    } else {
        format!("{:.0} MB", (b / MB).ceil())
    }
}

impl Listen {
    pub(super) fn offline_boot(&mut self) {
        let dir = match (&self.paths, self.ephemeral) {
            (Some(p), false) => p.data_dir.join("offline"),
            _ => std::env::temp_dir().join(format!("listen-offline-{}", std::process::id())),
        };
        match Store::open(&dir) {
            Ok(store) => {
                let store = listen_offline::init(store);
                let expired = store.expire_unverified();
                self.offline_refresh();
                if expired > 0 {
                    self.toast_warning(format!(
                        "Removed {expired} offline YouTube Music songs: the account was not verified online for 30 days."
                    ));
                }
            }
            Err(e) => tracing::warn!("offline store unavailable ({}): {e}", dir.display()),
        }
    }

    /// Drops queued downloads (of one server, or all) and marks a matching active one for deletion.
    pub(super) fn offline_cancel(&mut self, server: Option<&ServerId>) {
        let matches = |s: &Song| server.is_none_or(|id| &s.server_id == id);
        let before = self.offline.queue.len();
        self.offline.queue.retain(|s| !matches(s));
        self.offline.total = self.offline.total.saturating_sub(before - self.offline.queue.len());
        if self.offline.active.as_ref().is_some_and(matches) {
            self.offline.cancel_active = true;
        }
    }

    /// Re-reads the downloads list after the store changed.
    pub(super) fn offline_refresh(&mut self) {
        if let Some(store) = self.offline_store() {
            (self.offline.songs, self.offline.sizes) = sorted_entries(store);
        }
    }

    pub fn offline_store(&self) -> Option<&'static Store> {
        listen_offline::global()
    }

    /// Whether a song can be downloaded (its server is connected and supports it).
    pub fn can_download(&self, song: &Song) -> bool {
        self.sources.get(&song.server_id).is_some_and(|s| s.capabilities().contains(Capabilities::DOWNLOAD))
    }

    pub fn downloaded(&self, song: &Song) -> bool {
        self.offline_store().is_some_and(|s| s.contains(&song.server_id, &song.id))
    }

    pub(super) fn on_offline(&mut self, msg: OfflineMsg) -> Task<Message> {
        let Some(store) = self.offline_store() else { return Task::none() };
        let task = self.on_offline_inner(store, msg);
        self.offline_refresh();
        task
    }

    fn on_offline_inner(&mut self, store: &'static Store, msg: OfflineMsg) -> Task<Message> {
        match msg {
            OfflineMsg::Download(songs) => return self.offline_enqueue(store, songs),
            OfflineMsg::Finished(song, result) => {
                self.offline.active = None;
                if std::mem::take(&mut self.offline.cancel_active) {
                    store.remove(&song.server_id, &song.id);
                    if !self.offline.busy() {
                        self.offline.done = 0;
                        self.offline.total = 0;
                        self.offline.batch_failed = 0;
                    }
                    return self.offline_pump(store);
                }
                self.offline.done += 1;
                self.offline.last_was_ytm = song.source == SourceKind::YouTubeMusic;
                match result {
                    Ok(()) => tracing::info!("downloaded “{}” [{}]", song.title, song.id),
                    Err(e) => {
                        tracing::warn!("download of “{}” failed: {e}", song.title);
                        self.offline.batch_failed += 1;
                        self.offline.failed.push((song.title.clone(), e));
                    }
                }
                if !self.offline.busy() {
                    let failed = self.offline.batch_failed;
                    let ok = self.offline.done.saturating_sub(failed);
                    self.toast_info(match (ok, failed) {
                        (n, 0) => format!("Downloaded {n} for offline play."),
                        (0, f) => format!("{f} downloads failed (see Downloads)."),
                        (n, f) => format!("Downloaded {n} for offline play, {f} failed (see Downloads)."),
                    });
                    self.offline.done = 0;
                    self.offline.total = 0;
                    self.offline.batch_failed = 0;
                }
                return self.offline_pump(store);
            }
            OfflineMsg::Remove(server, id) => {
                self.offline.queue.retain(|s| !(s.server_id == server && s.id == id));
                if self.offline.active.as_ref().is_some_and(|s| s.server_id == server && s.id == id) {
                    self.offline.cancel_active = true;
                }
                store.remove(&server, &id);
            }
            OfflineMsg::RemoveAll => {
                self.offline_cancel(None);
                store.remove_all();
            }
            OfflineMsg::ClearFailed => self.offline.failed.clear(),
            OfflineMsg::Play(index) => {
                let songs = self.offline.songs.clone();
                return self.play_songs(songs, index);
            }
        }
        Task::none()
    }

    fn offline_enqueue(&mut self, store: &'static Store, songs: Vec<Song>) -> Task<Message> {
        let mut added = 0;
        let mut refused: Option<Refused> = None;
        let mut unsupported = false;
        for song in songs {
            if store.contains(&song.server_id, &song.id) || self.offline.queued(&song) {
                continue;
            }
            if !self.can_download(&song) {
                unsupported = true;
                continue;
            }
            // Queued YouTube Music songs count against the cap too.
            let queued_ytm = self
                .offline
                .queue
                .iter()
                .chain(self.offline.active.iter())
                .filter(|s| s.source == SourceKind::YouTubeMusic)
                .count();
            if song.source == SourceKind::YouTubeMusic {
                let stored = store.entries().iter().filter(|e| e.song.source == song.source).count();
                if let Err(r) = store.check_add(song.source, 0) {
                    refused = Some(r);
                    continue;
                }
                if stored + queued_ytm >= listen_offline::YTM_MAX_TRACKS {
                    refused = Some(Refused::TooManySongs);
                    continue;
                }
            }
            self.offline.queue.push_back(song);
            self.offline.total += 1;
            added += 1;
        }
        self.toast_info(match (added, refused, unsupported) {
            (_, Some(r), _) => format!("{r}. Remove some downloads first."),
            (0, None, true) => "This server does not support downloads (or is not connected).".into(),
            (0, None, false) => "Already downloaded.".into(),
            (1, None, _) => "Downloading 1 song…".into(),
            (n, None, _) => format!("Downloading {n} songs…"),
        });
        self.offline_pump(store)
    }

    fn offline_pump(&mut self, store: &'static Store) -> Task<Message> {
        if self.offline.active.is_some() {
            return Task::none();
        }
        while let Some(song) = self.offline.queue.pop_front() {
            let Some(source) = self.sources.get(&song.server_id).cloned() else {
                self.offline.done += 1;
                self.offline.batch_failed += 1;
                self.offline.failed.push((song.title.clone(), "the server is not connected".into()));
                continue;
            };
            let pause = song.source == SourceKind::YouTubeMusic && self.offline.last_was_ytm;
            self.offline.active = Some(song.clone());
            let http = self.http.clone();
            let target = song.clone();
            return Task::perform(
                async move {
                    if pause {
                        tokio::time::sleep(YTM_PAUSE).await;
                    }
                    download(&*source, &target, store, &http).await
                },
                move |r| Message::Offline(OfflineMsg::Finished(Box::new(song.clone()), r)),
            );
        }
        if self.offline.total > 0 {
            self.offline.done = 0;
            self.offline.total = 0;
            self.offline.batch_failed = 0;
        }
        Task::none()
    }
}

/// Fetches one song into the store.
async fn download(
    source: &dyn MusicSource,
    song: &Song,
    store: &Store,
    http: &reqwest::Client,
) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| e.to_string();
    if let Some(url) = source.download_url(song) {
        let name = listen_offline::file_name(song, song.codec.as_deref().unwrap_or("audio"));
        let part = store.part_path(&name);
        let result = fetch_url(http, &url, &part).await;
        if let Err(e) = result {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
        return store.commit(song, &name).map_err(|e| fail(&e));
    }
    let path = source
        .local_copy(song)
        .await
        .map_err(|e| fail(&e))?
        .ok_or_else(|| "no downloadable audio for this song".to_string())?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("audio").to_string();
    let name = listen_offline::file_name(song, &ext);
    let part = store.part_path(&name);
    let (src, dst) = (path, part.clone());
    let copied = tokio::task::spawn_blocking(move || std::fs::copy(src, dst).map(|_| ())).await;
    let size = match copied {
        Ok(Ok(())) => std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0),
        Ok(Err(e)) => {
            let _ = std::fs::remove_file(&part);
            return Err(fail(&e));
        }
        Err(e) => return Err(fail(&e)),
    };
    if let Err(refused) = store.check_add(song.source, size) {
        let _ = std::fs::remove_file(&part);
        return Err(refused.to_string());
    }
    store.commit(song, &name).map_err(|e| fail(&e))
}

/// Streams `url` into `part`; a server's error page is not audio.
async fn fetch_url(http: &reqwest::Client, url: &str, part: &std::path::Path) -> Result<(), String> {
    use std::io::Write;
    let mut resp = http.get(url).send().await.map_err(|e| e.without_url().to_string())?;
    if !resp.status().is_success() {
        return Err(format!("the server answered HTTP {}", resp.status()));
    }
    let kind = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    // Subsonic answers errors with HTTP 200 and a JSON/XML body.
    if kind.starts_with("text/") || kind.contains("json") || kind.contains("xml") {
        return Err("the server did not return an audio file".into());
    }
    let expected = resp.content_length();
    let mut file = std::fs::File::create(part).map_err(|e| e.to_string())?;
    let mut got = 0u64;
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.without_url().to_string())? {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        got += chunk.len() as u64;
    }
    file.flush().map_err(|e| e.to_string())?;
    match expected {
        Some(n) if n != got => Err(format!("incomplete download ({got} of {n} bytes)")),
        _ => Ok(()),
    }
}

/// The downloaded file of a song, if any (`stream_track` plays it).
pub fn local_track(song: &Song) -> Option<std::path::PathBuf> {
    listen_offline::global()?.lookup(&song.server_id, &song.id)
}
