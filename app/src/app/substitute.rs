//! Plays a YouTube Music song from one of the user's own servers when one has the same recording.
//! The queue keeps the YouTube Music song (likes, playlists, radio, video stay); only the audio
//! stream is swapped, where it is resolved. Matching is `listen_core::substitute`; looking up and
//! remembering the answer is here.

use super::{Listen, Message, Sources, stream_track};
use iced::Task;
use iced::futures::future::join_all;
use listen_core::model::{ServerId, Song, SourceKind};
use listen_core::substitute::{best_match, queries, score, time_shift};
use listen_player::Track;
use listen_source::MusicSource;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// How long starting a song waits for a lookup that is not done yet (then YouTube Music plays).
const WAIT: Duration = Duration::from_millis(1500);
/// One server's answer to a search is dropped after this.
const SERVER_TIMEOUT: Duration = Duration::from_secs(8);
const SEARCH_LIMIT: usize = 20;
/// A lookup in which every server failed is repeated after this.
const RETRY_AFTER: Duration = Duration::from_secs(120);
const MAX_ENTRIES: usize = 4000;
/// Songs after the current one whose lookup is started ahead of time.
const WARM_AHEAD: usize = 4;

/// What a lookup found.
#[derive(Debug, Clone)]
struct Found {
    song: Option<Song>,
    /// No server answered: not a real "none".
    failed: bool,
}

struct Lookup {
    rx: watch::Receiver<Option<Found>>,
    started: Instant,
    /// How many own servers were asked (a newly connected server asks again).
    servers: usize,
}

#[derive(Default)]
struct State {
    lookups: HashMap<String, Lookup>,
    /// Songs the user (or a failing server) sent back to YouTube Music.
    denied: HashSet<String>,
    /// The own copy a song's stream was last taken from.
    used: HashMap<String, Song>,
}

struct Shared {
    enabled: AtomicBool,
    state: Mutex<State>,
}

/// Shared by the UI and the stream-resolving tasks; cheap to clone.
#[derive(Clone)]
pub struct Substitutes(Arc<Shared>);

impl Default for Substitutes {
    fn default() -> Self {
        Self(Arc::new(Shared { enabled: AtomicBool::new(true), state: Mutex::default() }))
    }
}

impl std::fmt::Debug for Substitutes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Substitutes")
    }
}

/// The own copy of a YouTube Music song, for the player bar.
pub struct Alternative {
    pub song: Song,
    /// It is the one playing (else the YouTube Music version is, by choice).
    pub active: bool,
}

/// Own servers, in a fixed order (Navidrome first, then Jellyfin, then local folders).
fn own_sources(sources: &Sources) -> Vec<(ServerId, Arc<dyn MusicSource>)> {
    let rank = |k: SourceKind| match k {
        SourceKind::Subsonic => 0,
        SourceKind::Jellyfin => 1,
        SourceKind::Local => 2,
        SourceKind::YouTubeMusic => 3,
    };
    let mut own: Vec<_> = sources
        .iter()
        .filter(|(_, s)| s.kind() != SourceKind::YouTubeMusic)
        .map(|(id, s)| (id.clone(), s.clone()))
        .collect();
    own.sort_by(|a, b| (rank(a.1.kind()), &a.0.0).cmp(&(rank(b.1.kind()), &b.0.0)));
    own
}

/// The best own copy on one server; `Err` when no search got an answer.
async fn search_server(source: Arc<dyn MusicSource>, wanted: &Song) -> Result<Option<Song>, ()> {
    let mut answered = false;
    for q in queries(wanted) {
        match tokio::time::timeout(SERVER_TIMEOUT, source.search(&q, SEARCH_LIMIT)).await {
            Ok(Ok(found)) => {
                answered = true;
                if let Some(song) = best_match(wanted, &found.songs) {
                    return Ok(Some(song.clone()));
                }
            }
            Ok(Err(e)) => tracing::debug!("own-copy search for “{}”: {e}", wanted.title),
            Err(_) => tracing::debug!("own-copy search for “{}” timed out", wanted.title),
        }
    }
    if answered { Ok(None) } else { Err(()) }
}

async fn search_all(own: Vec<(ServerId, Arc<dyn MusicSource>)>, wanted: Song) -> Found {
    let answers = join_all(own.into_iter().map(|(_, s)| search_server(s, &wanted))).await;
    let failed = answers.iter().all(Result::is_err);
    // The best-scoring copy; the earlier server wins a tie.
    let mut best: Option<(i64, Song)> = None;
    for song in answers.into_iter().flatten().flatten() {
        let s = score(&wanted, &song).unwrap_or(0);
        if best.as_ref().is_none_or(|(b, _)| s > *b) {
            best = Some((s, song));
        }
    }
    let song = best.map(|(_, s)| s);
    match &song {
        Some(s) => tracing::info!(
            "own copy of “{}” [{}]: “{}” on {}",
            wanted.title,
            wanted.id,
            s.title,
            s.server_id.0
        ),
        None => tracing::debug!("no own copy of “{}” [{}]", wanted.title, wanted.id),
    }
    Found { song, failed }
}

impl Substitutes {
    pub fn enabled(&self) -> bool {
        self.0.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, on: bool) {
        self.0.enabled.store(on, Ordering::Relaxed);
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn applies(&self, song: &Song) -> bool {
        self.enabled() && song.source == SourceKind::YouTubeMusic && song.server_id.0 != "demo"
    }

    /// The own copy once a lookup found one (not for songs sent back to YouTube Music).
    pub fn known(&self, song: &Song) -> Option<Song> {
        if !self.applies(song) {
            return None;
        }
        let state = self.state();
        if state.denied.contains(&song.id) {
            return None;
        }
        state.lookups.get(&song.id)?.rx.borrow().as_ref()?.song.clone()
    }

    /// The own copy the song's stream came from.
    pub fn used(&self, song: &Song) -> Option<Song> {
        self.applies(song).then(|| self.state().used.get(&song.id).cloned()).flatten()
    }

    /// What the player bar offers: the own copy that plays, or the one the user turned down.
    pub fn alternative(&self, song: &Song) -> Option<Alternative> {
        if !self.applies(song) {
            return None;
        }
        let state = self.state();
        if let Some(own) = state.used.get(&song.id) {
            return Some(Alternative { song: own.clone(), active: true });
        }
        if state.denied.contains(&song.id) {
            let found = state.lookups.get(&song.id)?.rx.borrow().as_ref()?.song.clone()?;
            return Some(Alternative { song: found, active: false });
        }
        None
    }

    pub fn denied(&self, song: &Song) -> bool {
        self.state().denied.contains(&song.id)
    }

    /// Sends the song back to YouTube Music (or allows its own copy again).
    pub fn set_denied(&self, song: &Song, denied: bool) {
        let mut state = self.state();
        if denied {
            state.denied.insert(song.id.clone());
            state.used.remove(&song.id);
        } else {
            state.denied.remove(&song.id);
        }
    }

    /// Whether a lookup for this song is yet to be started.
    fn wants_lookup(&self, song: &Song, servers: usize) -> bool {
        self.applies(song)
            && servers > 0
            && self.state().lookups.get(&song.id).is_none_or(|l| stale(l, servers))
    }

    /// The own copy of `song`: from the remembered answer, or by asking the servers (once per song,
    /// the search keeps running when `wait` runs out). `None` = play YouTube Music.
    async fn resolve(&self, sources: &Sources, song: &Song, wait: Option<Duration>) -> Option<Song> {
        if !self.applies(song) || self.denied(song) {
            return None;
        }
        let own = own_sources(sources);
        if own.is_empty() {
            return None;
        }
        let mut rx = {
            let mut state = self.state();
            match state.lookups.get(&song.id) {
                Some(l) if !stale(l, own.len()) => l.rx.clone(),
                _ => {
                    if state.lookups.len() >= MAX_ENTRIES {
                        state.lookups.clear();
                    }
                    let (tx, rx) = watch::channel(None);
                    let servers = own.len();
                    state
                        .lookups
                        .insert(song.id.clone(), Lookup { rx: rx.clone(), started: Instant::now(), servers });
                    let wanted = song.clone();
                    tokio::spawn(async move {
                        let _ = tx.send(Some(search_all(own, wanted).await));
                    });
                    rx
                }
            }
        };
        let done = rx.wait_for(|v| v.is_some());
        let found = match wait {
            Some(d) => tokio::time::timeout(d, done).await.ok()?.ok()?.clone(),
            None => done.await.ok()?.clone(),
        };
        found?.song.filter(|_| !self.denied(song))
    }

    /// Records which copy a song plays from (`None`: YouTube Music).
    pub(super) fn mark(&self, song: &Song, own: Option<Song>) {
        let mut state = self.state();
        match own {
            Some(own) => {
                state.used.insert(song.id.clone(), own);
            }
            None => {
                state.used.remove(&song.id);
            }
        }
    }
}

/// A finished lookup that should be repeated: every server failed a while ago, or the servers changed.
fn stale(l: &Lookup, servers: usize) -> bool {
    let Some(found) = l.rx.borrow().clone() else { return false };
    l.servers != servers || (found.failed && l.started.elapsed() > RETRY_AFTER)
}

/// The stream to play for a queue song: the own copy of a YouTube Music song when there is one,
/// else the song's own source. An own copy that cannot be opened is dropped for the session.
pub async fn stream_for(sources: &Sources, subs: &Substitutes, song: &Song) -> Result<Track, String> {
    stream_for_within(sources, subs, song, WAIT).await
}

async fn stream_for_within(
    sources: &Sources,
    subs: &Substitutes,
    song: &Song,
    wait: Duration,
) -> Result<Track, String> {
    // A downloaded copy of the song is what the user asked for.
    if subs.applies(song)
        && super::offline::local_track(song).is_none()
        && let Some(own) = subs.resolve(sources, song, Some(wait)).await
    {
        match stream_track(sources, &own).await {
            Ok(track) => {
                subs.mark(song, Some(own));
                return Ok(track);
            }
            Err(e) => {
                tracing::warn!("own copy of “{}” does not play ({e}); using YouTube Music", song.title);
                subs.set_denied(song, true);
            }
        }
    }
    subs.mark(song, None);
    stream_track(sources, song).await
}

impl Listen {
    /// The song to analyse and mix with: its own copy where one is known.
    pub(super) fn playable(&self, song: &Song) -> Song {
        self.substitutes.known(song).unwrap_or_else(|| song.clone())
    }

    /// Starts the lookups for the current and the next songs (a no-op for the ones already asked).
    pub(super) fn substitute_warm(&self) -> Task<Message> {
        if !self.substitutes.enabled() {
            return Task::none();
        }
        let servers = own_sources(&self.sources).len();
        let position = self.queue.current_position().unwrap_or(0);
        let tasks: Vec<Task<Message>> = self
            .queue
            .iter()
            .skip(position)
            .take(WARM_AHEAD + 1)
            .map(|i| &i.song)
            .filter(|s| self.substitutes.wants_lookup(s, servers))
            .map(|song| {
                let (subs, sources, song) = (self.substitutes.clone(), self.sources.clone(), song.clone());
                Task::perform(
                    async move {
                        subs.resolve(&sources, &song, None).await;
                        song.id
                    },
                    Message::Substituted,
                )
            })
            .collect();
        Task::batch(tasks)
    }

    /// A lookup finished: the next song's slot may need its own copy.
    pub(super) fn on_substituted(&mut self, id: &str) -> Task<Message> {
        let next = self.queue.peek_next().map(|i| i.song.clone());
        match next {
            Some(song) if song.id == id && self.substitutes.known(&song).is_some() => self.refresh_next(),
            _ => Task::none(),
        }
    }

    /// The player bar's button: back to the YouTube Music version of the playing song, or to its own copy.
    pub(super) fn substitute_toggle(&mut self) -> Task<Message> {
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else { return Task::none() };
        let Some(alt) = self.substitutes.alternative(&song) else { return Task::none() };
        // The same moment in the other file.
        let shift = self.substitute_shift(&song, &alt.song);
        let mut pos = self.playback.position.as_secs_f64();
        pos = if alt.active { pos + shift } else { pos - shift }.max(0.0);
        self.substitutes.set_denied(&song, alt.active);
        self.stop_first = true;
        let task = self.start_current();
        if pos > 3.0 {
            self.resume_at = Some((self.play_token, Duration::from_secs_f64(pos)));
        }
        task
    }

    /// `ytm time = own time + shift` from the two analyses; 0 when either is missing.
    pub(super) fn substitute_shift(&self, song: &Song, own: &Song) -> f64 {
        let Some(handle) = self.analysis.as_ref() else { return 0.0 };
        let (Some(a), Some(b)) = (handle.lookup(song), handle.lookup(own)) else { return 0.0 };
        match (a.features(), b.features()) {
            (Some(a), Some(b)) => time_shift(a, b).unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// Name of a connected server for the "Playing from ..." hint.
    pub fn server_name(&self, id: &ServerId) -> String {
        self.config.servers.iter().find(|s| &s.id == id).map_or_else(|| id.0.clone(), |s| s.name.clone())
    }
}

#[cfg(test)]
mod tests;
