//! Handoff between devices: each device tells listen-server what it plays (`/v1/now`), shows what the
//! others play, and takes the music over — song, position and queue — when Play is pressed here or a
//! new audio output turns up here (headphones connected) while another device plays or just stopped.
//! The device that lost the music pauses. Wire format and rules: `docs/handoff.md`.

use super::history_sync;
use super::{Listen, Message};
use iced::Task;
use listen_core::config::ServerConfig;
use listen_core::model::Song;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// How often the state is checked (and sent when it changed).
pub const TICK: Duration = Duration::from_secs(2);
/// While playing, the state goes out at least this often, so the others know it still plays.
const HEARTBEAT: Duration = Duration::from_secs(20);
/// A playing device not heard from for this long is taken as gone (app closed, no network).
const LIVE_MS: u64 = 75_000;
/// Play pressed here continues another device that stopped at most this long ago.
const PLAY_WINDOW_MS: u64 = 10 * 60_000;
/// A new audio output here takes over a device that stopped at most this long ago (its headphones
/// went away, so it paused).
const DEVICE_WINDOW_MS: u64 = 3 * 60_000;
/// Songs before and after the current one that go along with the queue.
const QUEUE_BEFORE: usize = 20;
const QUEUE_AFTER: usize = 150;
/// The long poll's wait (the server holds at most 30 s).
const WAIT_S: u64 = 25;

/// Another device as listen-server reports it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Remote {
    pub device: String,
    pub name: String,
    pub kind: String,
    pub playing: bool,
    pub position_s: f64,
    /// `{"server_key", "song"}`.
    pub song: Option<Value>,
    pub seq: u64,
    pub at_ms: u64,
    pub played_ms: u64,
    pub took_from: Option<String>,
    pub took_seq: u64,
    pub started_seq: u64,
    pub queue_len: usize,
}

impl Remote {
    /// The song's title and artists, for the strip over the player bar.
    pub fn title(&self) -> Option<(String, String)> {
        let song = self.song.as_ref()?.get("song")?;
        let title = song.get("title")?.as_str()?.to_string();
        let artists = song
            .get("artists")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.get("name")?.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        Some((title, artists))
    }

    pub fn label(&self) -> &str {
        if self.name.is_empty() { &self.device } else { &self.name }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Listing {
    pub seq: u64,
    pub now_ms: u64,
    pub devices: Vec<Remote>,
}

/// What made this device take over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Play was pressed here.
    Play,
    /// A new audio output appeared here.
    Output,
    /// "Play here" on the strip.
    Button,
}

#[derive(Debug, Clone)]
pub enum HandoffMsg {
    Tick,
    Polled(Result<Listing, String>),
    Published(Result<(), String>),
    Outputs(Vec<String>),
    /// The device's full record (with the queue) for a takeover.
    Fetched(Trigger, Result<Value, String>),
    PlayHere(String),
}

/// What was last sent: playing, the song (uid), the queue fingerprint.
type Sig = (bool, Option<u64>, u64);

#[derive(Debug, Default)]
pub struct Handoff {
    http: Option<reqwest::Client>,
    /// The last answer and when it came (to place the server clock).
    pub listing: Option<(Listing, Instant)>,
    polling: bool,
    retry_at: Option<Instant>,
    publishing: bool,
    sent: Option<(Sig, Instant, f64)>,
    queue_sent: Option<u64>,
    /// Set by a takeover; goes out with the next state.
    took_from: Option<String>,
    /// The newest takeover from this device already acted on.
    handled_took: u64,
    outputs: Option<HashSet<String>>,
    checking_outputs: bool,
    fetching: bool,
    /// The next Play is a plain play (a takeover that could not happen).
    bypass: bool,
    ticks: u32,
}

/// This computer's name for the others: the host name up to its first dot.
pub fn device_name() -> String {
    let name = |h: String| h.trim().split('.').next().map(str::to_string).filter(|h| !h.is_empty());
    let read = |p: &str| std::fs::read_to_string(p).ok().and_then(name);
    std::env::var("COMPUTERNAME")
        .ok()
        .and_then(name)
        .or_else(|| read("/etc/hostname"))
        .or_else(|| read("/proc/sys/kernel/hostname"))
        .or_else(|| std::env::var("HOSTNAME").ok().and_then(name))
        .unwrap_or_else(|| "Computer".into())
}

/// The server's clock now, from the last answer.
fn server_now(listing: &Listing, got: Instant) -> u64 {
    listing.now_ms + got.elapsed().as_millis() as u64
}

fn live(d: &Remote, now_ms: u64) -> bool {
    d.playing && now_ms.saturating_sub(d.at_ms) < LIVE_MS
}

/// The other devices that play right now, the most recent first.
pub fn playing_elsewhere<'a>(listing: &'a Listing, now_ms: u64, me: &str) -> Vec<&'a Remote> {
    listing.devices.iter().filter(|d| d.device != me && d.song.is_some() && live(d, now_ms)).collect()
}

/// The device to take the music from: one that plays now, else one that played more recently than
/// this device and stopped within the trigger's window.
pub fn candidate<'a>(listing: &'a Listing, now_ms: u64, me: &str, trigger: Trigger) -> Option<&'a Remote> {
    let mine = listing.devices.iter().find(|d| d.device == me).map_or(0, |d| d.played_ms);
    let window = match trigger {
        Trigger::Play | Trigger::Button => PLAY_WINDOW_MS,
        Trigger::Output => DEVICE_WINDOW_MS,
    };
    let others = || listing.devices.iter().filter(|d| d.device != me && d.song.is_some());
    if let Some(d) = others().filter(|d| live(d, now_ms)).max_by_key(|d| d.at_ms) {
        return Some(d);
    }
    others()
        .filter(|d| d.played_ms > mine && now_ms.saturating_sub(d.played_ms) < window)
        .max_by_key(|d| d.played_ms)
}

/// A song as it travels: the server named the same way on every device (`docs/history-sync.md`).
pub fn wire_song(song: &Song, servers: &[ServerConfig]) -> Value {
    let key = match servers.iter().find(|s| s.id == song.server_id) {
        Some(s) if s.kind == song.source => history_sync::key_of(s),
        _ if song.source == listen_core::model::SourceKind::YouTubeMusic => {
            history_sync::source_name(song.source)
        }
        _ => format!("{}:{}", history_sync::source_name(song.source), song.server_id.0),
    };
    json!({ "server_key": key, "song": song })
}

/// A song from another device with this computer's server, or `None` when it has no such server.
pub fn local_song(item: &Value, servers: &[ServerConfig]) -> Option<Song> {
    let mut song: Song = serde_json::from_value(item.get("song")?.clone()).ok()?;
    let key = item.get("server_key").and_then(Value::as_str).unwrap_or_default();
    let server = history_sync::local_server(key, song.source, servers)?;
    song.server_id = server.id.clone();
    Some(song)
}

/// The queue to play from a device's record: the songs this computer can play and where to start.
pub fn queue_of(record: &Value, servers: &[ServerConfig]) -> Option<(Vec<Song>, usize)> {
    let mut songs = Vec::new();
    let mut start = None;
    if let Some(q) = record.get("queue") {
        let index = q.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        for (i, item) in q.get("items").and_then(Value::as_array).into_iter().flatten().enumerate() {
            if let Some(song) = local_song(item, servers) {
                if i == index {
                    start = Some(songs.len());
                }
                songs.push(song);
            }
        }
    }
    if start.is_none() {
        // No queue sent, or its current song does not play here: the song alone.
        let song = local_song(record.get("song")?, servers)?;
        songs = vec![song];
        start = Some(0);
    }
    Some((songs, start?))
}

/// Where the other device is in its song now.
pub fn position_of(record: &Value) -> f64 {
    let pos = record.get("position_s").and_then(Value::as_f64).unwrap_or(0.0);
    let playing = record.get("playing").and_then(Value::as_bool).unwrap_or(false);
    let now = record.get("now_ms").and_then(Value::as_u64).unwrap_or(0);
    let at = record.get("at_ms").and_then(Value::as_u64).unwrap_or(0);
    if playing && now > at { pos + (now - at) as f64 / 1000.0 } else { pos }
}

fn failed(e: reqwest::Error) -> String {
    e.without_url().to_string()
}

async fn get_json<T: serde::de::DeserializeOwned>(
    http: reqwest::Client,
    url: String,
    token: String,
) -> Result<T, String> {
    let response = http.get(url).bearer_auth(token).send().await.map_err(failed)?;
    let text = response.error_for_status().map_err(failed)?.text().await.map_err(failed)?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

impl Listen {
    fn handoff_base(&self) -> Option<(String, String)> {
        let h = &self.config.settings.history;
        let base = h.sync_server.trim();
        if !h.handoff || base.is_empty() || self.ephemeral || self.demo || self.mock_playback() {
            return None;
        }
        let token = self.history_sync_token()?;
        Some((format!("{}/", base.trim_end_matches('/')), token))
    }

    /// A song plays here. (A fresh mpv reports "not paused" while idle with an empty queue.)
    pub(super) fn playing_here(&self) -> bool {
        self.playback.playing && self.radio.active.is_none() && self.queue.current().is_some()
    }

    pub(super) fn handoff_on(&self) -> bool {
        self.handoff_base().is_some()
    }

    fn handoff_http(&mut self) -> reqwest::Client {
        self.handoff
            .http
            .get_or_insert_with(|| {
                reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(8))
                    .timeout(Duration::from_secs(WAIT_S + 15))
                    .build()
                    .unwrap_or_default()
            })
            .clone()
    }

    pub(super) fn on_handoff(&mut self, msg: HandoffMsg) -> Task<Message> {
        match msg {
            HandoffMsg::Tick => self.handoff_tick(),
            HandoffMsg::Polled(result) => self.handoff_polled(result),
            HandoffMsg::Published(result) => {
                self.handoff.publishing = false;
                if let Err(e) = result {
                    tracing::debug!("handoff: sending the state failed: {e}");
                    self.handoff.sent = None;
                }
                Task::none()
            }
            HandoffMsg::Outputs(names) => self.handoff_outputs(names),
            HandoffMsg::Fetched(trigger, result) => self.handoff_take(trigger, result),
            HandoffMsg::PlayHere(device) => self.handoff_fetch(Trigger::Button, device),
        }
    }

    fn handoff_tick(&mut self) -> Task<Message> {
        if !self.handoff_on() {
            return Task::none();
        }
        self.handoff.ticks = self.handoff.ticks.wrapping_add(1);
        let mut tasks = vec![self.handoff_poll(), self.handoff_publish(false)];
        if self.handoff.ticks.is_multiple_of(2)
            && !self.handoff.checking_outputs
            && let Some(player) = self.decks.active_player()
        {
            self.handoff.checking_outputs = true;
            tasks.push(Task::perform(
                async move { player.audio_devices().await.map(|l| l.into_iter().map(|(n, _)| n).collect()) },
                |r: Result<Vec<String>, _>| Message::Handoff(HandoffMsg::Outputs(r.unwrap_or_default())),
            ));
        }
        Task::batch(tasks)
    }

    fn handoff_poll(&mut self) -> Task<Message> {
        if self.handoff.polling || self.handoff.retry_at.is_some_and(|t| Instant::now() < t) {
            return Task::none();
        }
        let Some((base, token)) = self.handoff_base() else { return Task::none() };
        let after = self.handoff.listing.as_ref().map_or(0, |(l, _)| l.seq);
        self.handoff.polling = true;
        let url = format!("{base}v1/now?after={after}&wait={WAIT_S}");
        let http = self.handoff_http();
        Task::perform(get_json::<Listing>(http, url, token), |r| Message::Handoff(HandoffMsg::Polled(r)))
    }

    fn handoff_polled(&mut self, result: Result<Listing, String>) -> Task<Message> {
        self.handoff.polling = false;
        let listing = match result {
            Ok(l) => l,
            Err(e) => {
                tracing::debug!("handoff: {e}");
                self.handoff.retry_at = Some(Instant::now() + Duration::from_secs(15));
                return Task::none();
            }
        };
        self.handoff.retry_at = None;
        let me = listen_analysis::store::machine_id();
        let started = listing.devices.iter().find(|d| d.device == me).map_or(0, |d| d.started_seq);
        // Another device took the music from this one: stop here.
        let taken = listing
            .devices
            .iter()
            .filter(|d| d.took_from.as_deref() == Some(me.as_str()) && d.took_seq > started)
            .filter(|d| d.took_seq > self.handoff.handled_took)
            .max_by_key(|d| d.took_seq)
            .map(|d| (d.took_seq, d.label().to_string()));
        self.handoff.listing = Some((listing, Instant::now()));
        let mut tasks = vec![self.handoff_poll()];
        if let Some((seq, name)) = taken {
            self.handoff.handled_took = seq;
            if self.playing_here() {
                tracing::info!("handoff: {name} took over; pausing here");
                self.toast_info(crate::i18n::trf("Now playing on {}.", &[&name]));
                tasks.push(Task::done(Message::TogglePlay));
            }
        }
        Task::batch(tasks)
    }

    /// Sends this device's state when it changed (or as a heartbeat while playing).
    fn handoff_publish(&mut self, force: bool) -> Task<Message> {
        if self.handoff.publishing {
            return Task::none();
        }
        let Some((base, token)) = self.handoff_base() else { return Task::none() };
        let current = if self.radio.active.is_some() { None } else { self.queue.current() };
        let playing = self.playing_here();
        let pos = self.playback.position.as_secs_f64();
        let queue_sig = self.queue.signature();
        let sig: Sig = (playing, current.map(|i| i.uid), queue_sig);
        let due = match &self.handoff.sent {
            None => true,
            Some((s, at, sent_pos)) => {
                let expected = if playing { sent_pos + at.elapsed().as_secs_f64() } else { *sent_pos };
                *s != sig
                    || (playing && at.elapsed() >= HEARTBEAT)
                    || (pos - expected).abs() > 3.0
                    || self.handoff.took_from.is_some()
            }
        };
        if !due && !force {
            return Task::none();
        }
        let servers = &self.config.servers;
        let mut body = json!({
            "name": device_name(),
            "kind": "desktop",
            "playing": playing,
            "position_s": pos,
            "song": current.filter(|i| i.bridge.is_none()).map(|i| wire_song(&i.song, servers)),
        });
        if self.handoff.queue_sent != Some(queue_sig) && self.radio.active.is_none() {
            let at = self.queue.current_position().unwrap_or(0);
            let from = at.saturating_sub(QUEUE_BEFORE);
            let items: Vec<(usize, Value)> = (from..(at + QUEUE_AFTER + 1).min(self.queue.len()))
                .filter_map(|p| self.queue.item_at(p).map(|i| (p, i)))
                .filter(|(_, i)| i.bridge.is_none())
                .map(|(p, i)| (p, wire_song(&i.song, servers)))
                .collect();
            let index = items.iter().position(|(p, _)| *p == at).unwrap_or(0);
            body["queue"] =
                json!({ "index": index, "items": items.into_iter().map(|(_, v)| v).collect::<Vec<_>>() });
        }
        if let Some(from) = self.handoff.took_from.take() {
            body["took_from"] = json!(from);
        }
        self.handoff.sent = Some((sig, Instant::now(), pos));
        self.handoff.queue_sent = Some(queue_sig);
        self.handoff.publishing = true;
        let url = format!("{base}v1/now/{}", listen_analysis::store::machine_id());
        let http = self.handoff_http();
        Task::perform(
            async move {
                let r = http.put(url).bearer_auth(token).json(&body).send().await.map_err(failed)?;
                r.error_for_status().map(|_| ()).map_err(failed)
            },
            |r| Message::Handoff(HandoffMsg::Published(r)),
        )
    }

    /// Play pressed while nothing plays here: take over another device when one qualifies.
    pub(super) fn handoff_on_play(&mut self) -> Option<Task<Message>> {
        if std::mem::take(&mut self.handoff.bypass) || self.playing_here() || !self.handoff_on() {
            return None;
        }
        let device = self.handoff_candidate(Trigger::Play)?;
        Some(self.handoff_fetch(Trigger::Play, device))
    }

    fn handoff_candidate(&self, trigger: Trigger) -> Option<String> {
        let (listing, got) = self.handoff.listing.as_ref()?;
        // An answer older than a minute (no connection) is not trusted.
        if got.elapsed() > Duration::from_secs(WAIT_S + 35) {
            return None;
        }
        let me = listen_analysis::store::machine_id();
        candidate(listing, server_now(listing, *got), &me, trigger).map(|d| d.device.clone())
    }

    fn handoff_outputs(&mut self, names: Vec<String>) -> Task<Message> {
        self.handoff.checking_outputs = false;
        if names.is_empty() {
            return Task::none();
        }
        let now: HashSet<String> = names.into_iter().filter(|n| n != "auto").collect();
        let added: Vec<String> = match &self.handoff.outputs {
            Some(before) => now.difference(before).cloned().collect(),
            None => Vec::new(),
        };
        self.handoff.outputs = Some(now);
        if added.is_empty() || self.playing_here() {
            return Task::none();
        }
        let Some(device) = self.handoff_candidate(Trigger::Output) else { return Task::none() };
        tracing::info!("handoff: new audio output {added:?}; taking over from {device}");
        self.handoff_fetch(Trigger::Output, device)
    }

    fn handoff_fetch(&mut self, trigger: Trigger, device: String) -> Task<Message> {
        if self.handoff.fetching {
            return Task::none();
        }
        let Some((base, token)) = self.handoff_base() else { return Task::none() };
        self.handoff.fetching = true;
        let http = self.handoff_http();
        let url = format!("{base}v1/now/{device}");
        Task::perform(get_json::<Value>(http, url, token), move |r| {
            Message::Handoff(HandoffMsg::Fetched(trigger, r))
        })
    }

    fn handoff_take(&mut self, trigger: Trigger, result: Result<Value, String>) -> Task<Message> {
        self.handoff.fetching = false;
        let fallback = |s: &mut Self| {
            if trigger == Trigger::Play {
                s.handoff.bypass = true;
                Task::done(Message::TogglePlay)
            } else {
                Task::none()
            }
        };
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("handoff: fetching the other device failed: {e}");
                return fallback(self);
            }
        };
        let name = record.get("name").and_then(Value::as_str).unwrap_or("another device").to_string();
        let device = record.get("device").and_then(Value::as_str).unwrap_or_default().to_string();
        let Some((songs, start)) = queue_of(&record, &self.config.servers) else {
            let title = record.pointer("/song/song/title").and_then(Value::as_str).unwrap_or("?");
            self.toast_error(crate::i18n::trf("“{}” from {} can't play here.", &[&title, &name]));
            return fallback(self);
        };
        let duration = songs[start].duration.as_secs_f64();
        let mut at = position_of(&record);
        if duration > 0.0 {
            at = at.min(duration - 2.0).max(0.0);
        }
        tracing::info!("handoff ({trigger:?}): continuing “{}” from {name} at {at:.1} s", songs[start].title);
        self.toast_info(crate::i18n::trf("Continuing from {}.", &[&name]));
        self.queue.set(songs, start);
        let task = self.start_current();
        if at > 1.0 {
            self.resume_at = Some((self.play_token, Duration::from_secs_f64(at)));
        }
        if !device.is_empty() {
            self.handoff.took_from = Some(device);
        }
        // Playing from here on; the state goes out with the takeover mark.
        self.playback.playing = true;
        self.handoff.sent = None;
        Task::batch([task, self.handoff_publish(true)])
    }

    /// The other devices playing now (for the strip over the player bar).
    pub fn handoff_elsewhere(&self) -> Vec<&Remote> {
        if !self.handoff_on() {
            return Vec::new();
        }
        let Some((listing, got)) = &self.handoff.listing else { return Vec::new() };
        if got.elapsed() > Duration::from_secs(WAIT_S + 35) {
            return Vec::new();
        }
        playing_elsewhere(listing, server_now(listing, *got), &listen_analysis::store::machine_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::{ServerId, SourceKind};

    fn remote(device: &str, playing: bool, at_ms: u64, played_ms: u64) -> Remote {
        Remote {
            device: device.into(),
            playing,
            at_ms,
            played_ms,
            song: Some(json!({"server_key": "youtubemusic", "song": {"title": "T"}})),
            ..Default::default()
        }
    }

    fn listing(devices: Vec<Remote>) -> Listing {
        Listing { seq: 1, now_ms: 0, devices }
    }

    #[test]
    fn a_playing_device_is_taken_over_by_any_trigger() {
        let now = 1_000_000;
        let l = listing(vec![
            remote("me", false, now - 5000, now - 5000),
            remote("phone", true, now - 10_000, now - 10_000),
        ]);
        for t in [Trigger::Play, Trigger::Output] {
            assert_eq!(candidate(&l, now, "me", t).unwrap().device, "phone");
        }
    }

    #[test]
    fn a_silent_playing_device_counts_as_gone() {
        let now = 10_000_000;
        let l = listing(vec![remote("phone", true, now - LIVE_MS - 1, now - LIVE_MS - 1)]);
        // It still played recently enough for Play, not for a new output.
        assert!(candidate(&l, now, "me", Trigger::Play).is_some());
        let old = listing(vec![remote("phone", true, now - PLAY_WINDOW_MS - 1, now - PLAY_WINDOW_MS - 1)]);
        assert!(candidate(&old, now, "me", Trigger::Play).is_none());
        assert!(playing_elsewhere(&l, now, "me").is_empty());
    }

    #[test]
    fn a_stopped_device_counts_only_when_newer_than_this_one_and_within_the_window() {
        let now = 10_000_000;
        let paused = remote("phone", false, now - 60_000, now - 120_000);
        let l = listing(vec![paused.clone(), remote("me", false, now - 300_000, now - 300_000)]);
        assert_eq!(candidate(&l, now, "me", Trigger::Play).unwrap().device, "phone");
        assert_eq!(candidate(&l, now, "me", Trigger::Output).unwrap().device, "phone");
        // This device played after the phone stopped: Play continues here instead.
        let l = listing(vec![paused.clone(), remote("me", false, now - 30_000, now - 30_000)]);
        assert!(candidate(&l, now, "me", Trigger::Play).is_none());
        // Stopped five minutes ago: Play still continues it, a new output no longer does.
        let five = remote("phone", false, now - 300_000, now - 300_000);
        let l = listing(vec![five]);
        assert!(candidate(&l, now, "me", Trigger::Play).is_some());
        assert!(candidate(&l, now, "me", Trigger::Output).is_none());
    }

    #[test]
    fn this_device_and_songless_devices_never_count() {
        let now = 1_000_000;
        let mut radio = remote("radio", true, now, now);
        radio.song = None;
        let l = listing(vec![remote("me", true, now, now), radio]);
        assert!(candidate(&l, now, "me", Trigger::Play).is_none());
    }

    fn server(id: &str, kind: SourceKind, url: &str) -> ServerConfig {
        ServerConfig { id: ServerId(id.into()), name: id.into(), kind, url: url.into(), username: "u".into() }
    }

    fn song(id: &str, server: &str, source: SourceKind) -> Value {
        let mut s = crate::demo::album_songs(0)[0].clone();
        s.id = id.into();
        s.server_id = ServerId(server.into());
        s.source = source;
        s.title = id.into();
        serde_json::to_value(s).unwrap()
    }

    #[test]
    fn a_queue_comes_over_with_this_computers_servers() {
        let here = [
            server("nav-here", SourceKind::Subsonic, "https://music.example.org"),
            server("yt-here", SourceKind::YouTubeMusic, ""),
        ];
        let record = json!({
            "queue": {"index": 2, "items": [
                {"server_key": "local:phone-folder", "song": song("a", "phone-folder", SourceKind::Local)},
                {"server_key": "subsonic:music.example.org", "song": song("b", "nav-phone", SourceKind::Subsonic)},
                {"server_key": "youtubemusic", "song": song("c", "yt-phone", SourceKind::YouTubeMusic)},
                {"server_key": "subsonic:music.example.org", "song": song("d", "nav-phone", SourceKind::Subsonic)},
            ]},
            "song": {"server_key": "youtubemusic", "song": song("c", "yt-phone", SourceKind::YouTubeMusic)},
        });
        let (songs, start) = queue_of(&record, &here).unwrap();
        assert_eq!(songs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["b", "c", "d"]);
        assert_eq!(start, 1);
        assert_eq!(songs[0].server_id.0, "nav-here");
        assert_eq!(songs[1].server_id.0, "yt-here");
        // Sent back out, the same key arrives.
        assert_eq!(wire_song(&songs[0], &here)["server_key"], "subsonic:music.example.org");
    }

    #[test]
    fn a_current_song_that_cannot_play_here_gives_nothing() {
        let here = [server("nav-here", SourceKind::Subsonic, "https://music.example.org")];
        let record = json!({
            "queue": {"index": 0, "items": [{"server_key": "local:x", "song": song("a", "x", SourceKind::Local)}]},
            "song": {"server_key": "local:x", "song": song("a", "x", SourceKind::Local)},
        });
        assert!(queue_of(&record, &here).is_none());
    }

    #[test]
    fn the_position_runs_on_while_the_other_device_plays() {
        let r = json!({"position_s": 30.0, "playing": true, "at_ms": 1000, "now_ms": 6000});
        assert!((position_of(&r) - 35.0).abs() < 1e-9);
        let paused = json!({"position_s": 30.0, "playing": false, "at_ms": 1000, "now_ms": 6000});
        assert!((position_of(&paused) - 30.0).abs() < 1e-9);
    }
}
