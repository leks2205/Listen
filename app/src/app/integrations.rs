//! Discord rich presence, the remote control server, scrobble options, song change notifications and
//! private mode (Feishin's `use-discord-rpc`, `main/features/core/remote`, `use-scrobble`).
//!
//! Private mode (`history_view.private`) stops scrobbling, Discord presence and local history.

use super::*;
use crate::{discord, notify, remote};
use listen_core::settings::{DiscordDisplay, DiscordLink, ScrobbleMode};
use listen_source::Capabilities;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// The keyring entry of the remote control password.
const REMOTE_SECRET: &str = "remote-control";
/// Songs sent to the phone's queue tab at most.
const REMOTE_QUEUE_MAX: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    RemotePort,
    RemoteBind,
    RemoteUser,
    RemotePassword,
    LastfmKey,
}

#[derive(Debug, Clone)]
pub enum IntegrationsMsg {
    /// Private mode on / off (the app menu's entry).
    TogglePrivate,
    SetPrivate(bool),
    /// Send the Discord presence again (every 15 s, like Feishin).
    DiscordRefresh,
    /// The Last.fm cover of an album (key, URL).
    DiscordCover(String, Option<String>),
    /// The debounce of a song change notification is over.
    NotifyFire(u64),
    /// A text field of the settings page changed / was submitted.
    Input(Field, String),
    Save(Field),
    /// A phone asked for something.
    Remote(remote::Command),
    RemoteStarted(u64, Result<Arc<remote::Running>, String>),
    /// Songs of a remote browse (kept so the phone can play one by id).
    RemoteTracks(Vec<Song>),
    /// Songs a phone asked to play (what to do with them, the answer to send).
    RemoteLoaded(Result<Vec<Song>, String>, String, Option<(String, remote::Reply)>),
    /// Stations fetched for a phone that asked to play one.
    RemoteStations(String, Result<Vec<listen_source::InternetRadio>, String>),
}

/// A change on the settings page.
#[derive(Debug, Clone)]
pub enum IntegrationSetting {
    DiscordListening(bool),
    DiscordPaused(bool),
    DiscordStateIcon(bool),
    DiscordDisplay(DiscordDisplay),
    DiscordLink(DiscordLink),
    DiscordServerImage(bool),
    ScrobbleMode(ScrobbleMode),
    ScrobblePercent(u8),
    ScrobbleSecs(u32),
    Notify(bool),
    RemoteEnabled(bool),
}

/// Text fields being edited (saved on Enter).
#[derive(Debug, Default)]
pub struct Inputs {
    pub port: Option<String>,
    pub bind: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub lastfm: Option<String>,
}

#[derive(Debug, PartialEq)]
struct DiscordKey {
    /// The queue entry, or the radio marker.
    what: Option<u64>,
    playing: bool,
    start: i64,
    length: i64,
    sig: u64,
    covers: usize,
}

struct RemoteRuntime {
    shared: Arc<remote::Shared>,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<remote::Command>,
    cmd_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<remote::Command>>>,
    running: Option<(remote::Config, Arc<remote::Running>)>,
    /// The server being started (generation, config).
    starting: Option<(u64, remote::Config)>,
    generation: u64,
    /// Why the server is not running (shown in the settings).
    error: Option<String>,
    password: String,
    song_key: Option<(Option<u64>, bool, Option<u8>, bool)>,
    queue_key: Option<(u64, Option<u64>)>,
    /// The cover file of the playing entry, looked up once per entry.
    cover: Option<(u64, Option<PathBuf>)>,
    /// Songs of the phone's last track list.
    tracks: HashMap<String, Song>,
}

pub struct State {
    pub inputs: Inputs,
    discord_key: Option<DiscordKey>,
    discord_published: Option<discord::Presence>,
    covers: HashMap<String, Option<String>>,
    asking: HashSet<String>,
    notify: notify::Debounce,
    remote: RemoteRuntime,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("integrations::State")
    }
}

impl Default for State {
    fn default() -> Self {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            inputs: Inputs::default(),
            discord_key: None,
            discord_published: None,
            covers: HashMap::new(),
            asking: HashSet::new(),
            notify: notify::Debounce::default(),
            remote: RemoteRuntime {
                shared: Arc::new(remote::Shared::default()),
                cmd_tx,
                cmd_rx: Arc::new(tokio::sync::Mutex::new(cmd_rx)),
                running: None,
                starting: None,
                generation: 0,
                error: None,
                password: String::new(),
                song_key: None,
                queue_key: None,
                cover: None,
                tracks: HashMap::new(),
            },
        }
    }
}

impl State {
    /// Send the Discord presence again on the next message.
    pub fn discord_dirty(&mut self) {
        self.discord_key = None;
        self.discord_published = None;
    }

    /// The remote server runs on this address (for the settings page).
    pub fn remote_addr(&self) -> Option<std::net::SocketAddr> {
        self.remote.running.as_ref().map(|(_, r)| r.addr)
    }

    pub fn remote_error(&self) -> Option<&str> {
        self.remote.error.as_deref()
    }

    pub fn remote_password_set(&self) -> bool {
        !self.remote.password.is_empty()
    }

    /// The presence last sent to Discord (`None` = cleared).
    pub fn discord_presence(&self) -> Option<&discord::Presence> {
        self.discord_published.as_ref()
    }

    /// The next unhandled phone command (tests; the app gets them through the subscription).
    pub fn poll_remote_command(&self) -> Option<remote::Command> {
        self.remote.cmd_rx.try_lock().ok()?.try_recv().ok()
    }

    /// A cover may have arrived: look for the playing entry's file again.
    pub fn cover_dirty(&mut self) {
        if self.remote.cover.as_ref().is_some_and(|(_, c)| c.is_none()) {
            self.remote.cover = None;
        }
    }
}

#[derive(Clone)]
struct CommandSource(Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<remote::Command>>>);

impl Hash for CommandSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

fn settings_sig(d: &listen_core::settings::DiscordSettings, client_id: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (d.show_as_listening, d.show_paused, d.state_icon, d.display, d.link, d.server_image, &d.lastfm_key)
        .hash(&mut h);
    client_id.hash(&mut h);
    h.finish()
}

impl Listen {
    /// Private mode: no scrobbles, no Discord presence, no local history.
    pub fn private_mode(&self) -> bool {
        self.history_view.private
    }

    pub fn set_private_mode(&mut self, on: bool) {
        if self.history_view.private == on {
            return;
        }
        self.history_view.private = on;
        self.integ.discord_dirty();
        self.toast_info(if on {
            "Private mode is on: nothing is scrobbled, shown on Discord or saved in the history.".into()
        } else {
            "Private mode is off.".into()
        });
    }

    pub(super) fn integrations_boot(&mut self) -> Task<Message> {
        if self.demo {
            return Task::none();
        }
        self.integ.remote.password =
            self.secrets.as_ref().and_then(|s| s.get(&ServerId(REMOTE_SECRET.into()))).unwrap_or_default();
        self.remote_apply()
    }

    pub(super) fn integration_setting(&mut self, change: IntegrationSetting) -> Task<Message> {
        let s = &mut self.config.settings;
        let ig = &mut s.integrations;
        let mut remote = false;
        match change {
            IntegrationSetting::DiscordListening(v) => ig.discord.show_as_listening = v,
            IntegrationSetting::DiscordPaused(v) => ig.discord.show_paused = v,
            IntegrationSetting::DiscordStateIcon(v) => ig.discord.state_icon = v,
            IntegrationSetting::DiscordDisplay(v) => ig.discord.display = v,
            IntegrationSetting::DiscordLink(v) => ig.discord.link = v,
            IntegrationSetting::DiscordServerImage(v) => ig.discord.server_image = v,
            IntegrationSetting::ScrobbleMode(v) => ig.scrobble_mode = v,
            IntegrationSetting::ScrobblePercent(v) => s.playback.scrobble_at_percent = v.clamp(25, 90),
            IntegrationSetting::ScrobbleSecs(v) => s.playback.scrobble_at_secs = v.min(1200),
            IntegrationSetting::Notify(v) => ig.notify_song_change = v,
            IntegrationSetting::RemoteEnabled(v) => {
                ig.remote.enabled = v;
                remote = true;
            }
        }
        self.save_config();
        if remote { self.remote_apply() } else { Task::none() }
    }

    pub(super) fn on_integrations(&mut self, msg: IntegrationsMsg) -> Task<Message> {
        match msg {
            IntegrationsMsg::TogglePrivate => {
                let on = !self.private_mode();
                self.set_private_mode(on);
            }
            IntegrationsMsg::SetPrivate(on) => self.set_private_mode(on),
            IntegrationsMsg::DiscordRefresh => self.integ.discord_dirty(),
            IntegrationsMsg::DiscordCover(key, url) => {
                self.integ.asking.remove(&key);
                self.integ.covers.insert(key, url);
            }
            IntegrationsMsg::NotifyFire(uid) => return self.notify_fire(uid),
            IntegrationsMsg::Input(field, text) => {
                let i = &mut self.integ.inputs;
                match field {
                    Field::RemotePort => i.port = Some(text),
                    Field::RemoteBind => i.bind = Some(text),
                    Field::RemoteUser => i.user = Some(text),
                    Field::RemotePassword => i.password = Some(text),
                    Field::LastfmKey => i.lastfm = Some(text),
                }
            }
            IntegrationsMsg::Save(field) => return self.save_field(field),
            IntegrationsMsg::Remote(cmd) => return self.on_remote_command(cmd),
            IntegrationsMsg::RemoteStarted(generation, result) => {
                let r = &mut self.integ.remote;
                if r.starting.as_ref().map(|(g, _)| *g) != Some(generation) {
                    return Task::none(); // an older start, replaced since
                }
                let (_, cfg) = r.starting.take().expect("checked above");
                match result {
                    Ok(running) => {
                        r.error = None;
                        r.running = Some((cfg, running));
                        r.song_key = None;
                        r.queue_key = None;
                    }
                    Err(e) => {
                        let port = cfg.port;
                        r.error = Some(e.clone());
                        self.toast_error(format!(
                            "The remote control server could not start on port {port}: {e}"
                        ));
                    }
                }
            }
            IntegrationsMsg::RemoteTracks(songs) => {
                let map = &mut self.integ.remote.tracks;
                if map.len() > 2000 {
                    map.clear();
                }
                map.extend(songs.into_iter().map(|s| (s.id.clone(), s)));
            }
            IntegrationsMsg::RemoteLoaded(result, play_type, answer) => {
                let outcome = match result {
                    Ok(songs) if songs.is_empty() => Err("Nothing to play".to_string()),
                    Ok(songs) => Ok(songs),
                    Err(e) => Err(e),
                };
                if let Some((id, reply)) = &answer {
                    reply.ack(id, outcome.as_ref().err().map(String::as_str));
                }
                if let Ok(songs) = outcome {
                    return self.remote_enqueue(songs, &play_type);
                }
            }
            IntegrationsMsg::RemoteStations(id, result) => {
                if let Ok(stations) = result {
                    self.radio.stations = stations;
                    self.radio.loaded = true;
                    return self.handle(Message::Radio(radio::RadioMsg::Play(id)));
                }
            }
        }
        Task::none()
    }

    fn save_field(&mut self, field: Field) -> Task<Message> {
        let i = &mut self.integ.inputs;
        match field {
            Field::RemotePort => {
                let Some(text) = i.port.take() else { return Task::none() };
                match text.trim().parse::<u16>() {
                    Ok(port) if port > 0 => {
                        self.config.settings.integrations.remote.port = port;
                        self.save_config();
                        return self.remote_apply();
                    }
                    _ => self.toast_error("The port must be a number from 1 to 65535.".into()),
                }
            }
            Field::RemoteBind => {
                let Some(text) = i.bind.take() else { return Task::none() };
                if text.trim().parse::<std::net::IpAddr>().is_ok() {
                    self.config.settings.integrations.remote.bind = text.trim().to_string();
                    self.save_config();
                    return self.remote_apply();
                }
                self.toast_error("The address must be an IP address such as 0.0.0.0 or 127.0.0.1.".into());
            }
            Field::RemoteUser => {
                let Some(text) = i.user.take() else { return Task::none() };
                self.config.settings.integrations.remote.username = text.trim().to_string();
                self.save_config();
                return self.remote_apply();
            }
            Field::RemotePassword => {
                let Some(text) = i.password.take() else { return Task::none() };
                if let Some(secrets) = self.secrets.as_ref().filter(|_| !self.ephemeral) {
                    let id = ServerId(REMOTE_SECRET.into());
                    if text.is_empty() {
                        secrets.delete(&id);
                    } else {
                        secrets.set(&id, &text);
                    }
                }
                self.integ.remote.password = text;
                return self.remote_apply();
            }
            Field::LastfmKey => {
                let Some(text) = i.lastfm.take() else { return Task::none() };
                self.config.settings.integrations.discord.lastfm_key = text.trim().to_string();
                self.integ.covers.clear();
                self.save_config();
            }
        }
        Task::none()
    }

    // ---- Discord ------------------------------------------------------------------

    /// The cover URL Discord can fetch for the playing song, if any; the second
    /// value is a Last.fm lookup to start.
    fn discord_cover(&mut self) -> (Option<String>, Task<Message>) {
        let d = &self.config.settings.integrations.discord;
        let Some(item) = self.queue.current() else { return (None, Task::none()) };
        let song = &item.song;
        match &song.image {
            // YouTube Music thumbnails are public URLs.
            Some(ImageRef::Url(u)) if u.starts_with("https://") => return (Some(u.clone()), Task::none()),
            Some(image) if d.server_image && song.source != SourceKind::YouTubeMusic => {
                let public = self
                    .config
                    .servers
                    .iter()
                    .find(|s| s.id == song.server_id)
                    .is_some_and(|s| discord::is_public_url(&s.url));
                if public
                    && let Some(url) = self
                        .sources
                        .get(&song.server_id)
                        .and_then(|s| s.image_url(image, ART_SIZE))
                        .filter(|u| u.starts_with("https://"))
                {
                    return (Some(url), Task::none());
                }
            }
            _ => {}
        }
        let key = d.lastfm_key.trim();
        let (Some(album), Some(artist)) =
            (song.album.as_deref(), song.album_artists.first().or(song.artists.first()))
        else {
            return (None, Task::none());
        };
        if key.is_empty() {
            return (None, Task::none());
        }
        let cache = format!("{}\u{1f}{album}", artist.name);
        if let Some(found) = self.integ.covers.get(&cache) {
            return (found.clone(), Task::none());
        }
        if !self.integ.asking.insert(cache.clone()) {
            return (None, Task::none());
        }
        let (key, artist, album) = (key.to_string(), artist.name.clone(), album.to_string());
        let task = Task::perform(async move { lastfm_cover(&key, &artist, &album).await }, move |url| {
            Message::Integrations(IntegrationsMsg::DiscordCover(cache.clone(), url))
        });
        (None, task)
    }

    fn sync_discord(&mut self) -> Task<Message> {
        let w = &self.config.settings.window;
        let d = &self.config.settings.integrations.discord;
        let want = w.discord_rpc && !self.demo && !self.private_mode();
        let radio = self.radio.active.clone().filter(|_| want);
        let current = self.queue.current().filter(|_| want && radio.is_none());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let start = now - self.playback.position.as_secs() as i64;
        let key = DiscordKey {
            what: if radio.is_some() { Some(u64::MAX) } else { current.map(|i| i.uid) },
            playing: self.playback.playing,
            // Rounded to 5 s so small clock drift doesn't cause updates.
            start: if radio.is_some() { 0 } else { start - start % 5 },
            length: self.duration().as_secs() as i64,
            sig: settings_sig(d, &w.discord_client_id) ^ u64::from(want),
            covers: self.integ.covers.len(),
        };
        if self.integ.discord_key.as_ref() == Some(&key) {
            return Task::none();
        }
        self.integ.discord_key = Some(key);
        let has_current = current.is_some();
        let (image, lookup) = if has_current { self.discord_cover() } else { (None, Task::none()) };
        let w = &self.config.settings.window;
        let d = &self.config.settings.integrations.discord;
        let input = match (radio, self.queue.current().filter(|_| has_current)) {
            (Some(station), _) => Some(discord::Input {
                track: discord::Track {
                    title: station.title.clone().unwrap_or_else(|| station.station.name.clone()),
                    artists: vec![station.station.name.clone()],
                    album: Some(station.station.name.clone()),
                    album_artists: vec![],
                    mbz: Default::default(),
                },
                playing: self.playback.playing,
                position_secs: 0.0,
                duration_secs: 0.0,
                now,
                image: None,
            }),
            (None, Some(item)) => Some(discord::Input {
                track: discord::Track {
                    title: item.song.title.clone(),
                    artists: item.song.artists.iter().map(|a| a.name.clone()).collect(),
                    album: item.song.album.clone(),
                    album_artists: item.song.album_artists.iter().map(|a| a.name.clone()).collect(),
                    mbz: item.song.mbz.clone(),
                },
                playing: self.playback.playing,
                position_secs: self.playback.position.as_secs_f64(),
                duration_secs: self.duration().as_secs_f64(),
                now,
                image,
            }),
            _ => None,
        };
        let presence = input.and_then(|i| discord::build(d, &i));
        if presence == self.integ.discord_published {
            return lookup;
        }
        self.integ.discord_published = presence.clone();
        let (client, id) = (self.discord.clone(), w.discord_client_id.clone());
        Task::batch([
            lookup,
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || client.apply(&id, presence)).await.map_err(err)?
                },
                Message::DiscordDone,
            ),
        ])
    }

    // ---- notifications -----------------------------------------------------------

    fn sync_notifications(&mut self) -> Task<Message> {
        let current = self.queue.current().map(|i| i.uid);
        let Some(uid) = self.integ.notify.changed(current) else { return Task::none() };
        if !self.config.settings.integrations.notify_song_change || self.demo {
            return Task::none();
        }
        Task::perform(tokio::time::sleep(notify::DEBOUNCE), move |()| {
            Message::Integrations(IntegrationsMsg::NotifyFire(uid))
        })
    }

    fn notify_fire(&mut self, uid: u64) -> Task<Message> {
        if !self.config.settings.integrations.notify_song_change
            || !self.playback.playing
            || !self.integ.notify.fire(uid)
        {
            return Task::none();
        }
        let Some(item) = self.queue.current().filter(|i| i.uid == uid) else { return Task::none() };
        let icon = item
            .song
            .image
            .as_ref()
            .and_then(|i| self.images.disk_path(&self.image_key(i)))
            .filter(|p| p.exists());
        let content = notify::content(&item.song, icon);
        Task::future(async move {
            if let Err(e) = notify::send(&content).await {
                tracing::info!("notification: {e}");
            }
        })
        .discard()
    }

    // ---- the remote control server ------------------------------------------------

    fn remote_config(&self) -> Option<remote::Config> {
        let r = &self.config.settings.integrations.remote;
        let password = &self.integ.remote.password;
        // Without a user name and password the page is only offered on this computer.
        let bind =
            if r.username.trim().is_empty() || password.is_empty() { "127.0.0.1" } else { r.bind.as_str() };
        (r.enabled && !self.demo).then(|| remote::Config {
            bind: bind.into(),
            port: r.port,
            username: r.username.clone(),
            password: self.integ.remote.password.clone(),
        })
    }

    /// Starts, restarts or stops the server to match the settings.
    pub(super) fn remote_apply(&mut self) -> Task<Message> {
        let want = self.remote_config();
        let r = &mut self.integ.remote;
        let have = r.starting.as_ref().map(|(_, c)| c).or(r.running.as_ref().map(|(c, _)| c));
        if have == want.as_ref() {
            return Task::none();
        }
        let restarting = r.running.is_some();
        // The clients reload and reconnect on "settings changed".
        if let Some((_, running)) = r.running.take() {
            running.shutdown(remote::CLOSE_CHANGED);
        }
        r.starting = None;
        r.error = None;
        let Some(cfg) = want else { return Task::none() };
        r.generation += 1;
        let generation = r.generation;
        r.starting = Some((generation, cfg.clone()));
        let (shared, cmds) = (r.shared.clone(), r.cmd_tx.clone());
        Task::perform(
            async move {
                if restarting {
                    tokio::time::sleep(Duration::from_millis(150)).await; // the old listener lets go of the port
                }
                remote::start(cfg, shared, cmds).await.map(Arc::new)
            },
            move |result| Message::Integrations(IntegrationsMsg::RemoteStarted(generation, result)),
        )
    }

    pub(super) fn remote_subscription(&self) -> Subscription<Message> {
        if self.integ.remote.running.is_none() {
            return Subscription::none();
        }
        Subscription::run_with(CommandSource(self.integ.remote.cmd_rx.clone()), |source| {
            let rx = source.0.clone();
            iced::stream::channel(32, async move |mut out: iced::futures::channel::mpsc::Sender<Message>| {
                use iced::futures::SinkExt;
                loop {
                    let cmd = rx.lock().await.recv().await;
                    match cmd {
                        Some(cmd) => {
                            if out.send(Message::Integrations(IntegrationsMsg::Remote(cmd))).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                std::future::pending::<()>().await;
            })
        })
    }

    pub(super) fn discord_subscription(&self) -> Subscription<Message> {
        if self.config.settings.window.discord_rpc && !self.demo && !self.private_mode() {
            iced::time::every(Duration::from_secs(discord::REFRESH_SECS))
                .map(|_| Message::Integrations(IntegrationsMsg::DiscordRefresh))
        } else {
            Subscription::none()
        }
    }

    fn current_song_is(&self, id: &str) -> Option<Song> {
        self.queue.current().map(|i| i.song.clone()).filter(|s| s.id == id)
    }

    fn on_remote_command(&mut self, cmd: remote::Command) -> Task<Message> {
        use remote::Command as C;
        match cmd {
            C::Play => {
                if !self.playback.playing {
                    return self.handle(Message::TogglePlay);
                }
            }
            C::Pause => {
                if self.playback.playing {
                    return self.handle(Message::TogglePlay);
                }
            }
            C::Next => return self.handle(Message::Next),
            C::Previous => return self.handle(Message::Previous),
            C::ToggleRepeat => return self.handle(Message::CycleRepeat),
            C::ToggleShuffle => return self.handle(Message::ToggleShuffle),
            C::Seek(secs) => return self.handle(Message::Seek(secs as f32)),
            C::Volume(v) => return self.handle(Message::Volume(f32::from(v))),
            C::Favorite { id, favorite } => {
                if let Some(song) = self.current_song_is(&id)
                    && song.favorite != favorite
                {
                    return self.set_song_favorite(&song, favorite);
                }
            }
            C::Rating { id, rating } => {
                if let Some(song) = self.current_song_is(&id) {
                    return self.rate_song(&song, rating);
                }
            }
            C::QueueJump(uid) => {
                if let Ok(uid) = uid.parse() {
                    return self.handle(Message::PlayQueueItem(uid));
                }
            }
            C::RemoveFromQueue(uid) => {
                if let Ok(uid) = uid.parse::<u64>()
                    && self.queue.current().map(|i| i.uid) != Some(uid)
                {
                    self.queue.remove(uid);
                    return self.refresh_next();
                }
            }
            C::ReorderQueue { unique_id, target, top } => {
                if let (Ok(uid), Ok(target)) = (unique_id.parse::<u64>(), target.parse::<u64>())
                    && let Some(at) = (0..self.queue.len())
                        .find(|&i| self.queue.item_at(i).is_some_and(|it| it.uid == target))
                {
                    self.queue.move_uids(&[uid], if top { at } else { at + 1 });
                    return self.refresh_next();
                }
            }
            C::ClearQueue => return self.handle(Message::Queue(queue_ui::QueueMsg::ClearConfirmed)),
            C::Browse { kind, search, start, limit, request_id, reply } => {
                return self.remote_browse(kind, search, start, limit, request_id, reply);
            }
            C::PlayItem { what, id, play_type, request_id, reply } => {
                return self.remote_play(
                    what,
                    id,
                    play_type.unwrap_or_else(|| "now".into()),
                    request_id,
                    reply,
                );
            }
        }
        Task::none()
    }

    fn remote_browse(
        &mut self,
        kind: remote::Browse,
        search: Option<String>,
        start: usize,
        limit: usize,
        request_id: String,
        reply: remote::Reply,
    ) -> Task<Message> {
        let Some(source) = self.source.clone() else {
            reply.error("No server is selected");
            return Task::none();
        };
        Task::perform(
            async move {
                match kind {
                    remote::Browse::Albums => {
                        let q = AlbumQuery { search, start, limit, ..Default::default() };
                        match source.albums(&q).await {
                            Ok(page) => {
                                let more = page.has_more();
                                let items = page
                                    .items
                                    .iter()
                                    .map(|a| {
                                        json!({
                                            "id": a.id, "name": a.name,
                                            "albumArtistName": listen_core::model::join_names(&a.album_artists),
                                            "duration": a.duration.as_millis() as u64, "imageUrl": null,
                                            "releaseYear": a.year, "songCount": a.song_count,
                                        })
                                    })
                                    .collect();
                                reply.list("albums-response", &request_id, more, items);
                            }
                            Err(e) => reply.error(&e.to_string()),
                        }
                        Vec::new()
                    }
                    remote::Browse::Playlists => {
                        match source.playlists().await {
                            Ok(all) => {
                                let needle = search.map(|s| s.to_lowercase());
                                let matching: Vec<_> = all
                                    .iter()
                                    .filter(|p| {
                                        needle.as_ref().is_none_or(|n| p.name.to_lowercase().contains(n))
                                    })
                                    .collect();
                                let more = matching.len() > start + limit;
                                let items = matching
                                    .into_iter()
                                    .skip(start)
                                    .take(limit)
                                    .map(|p| {
                                        json!({
                                            "id": p.id, "name": p.name, "duration": p.duration.as_millis() as u64,
                                            "imageUrl": null, "songCount": p.song_count,
                                        })
                                    })
                                    .collect();
                                reply.list("playlists-response", &request_id, more, items);
                            }
                            Err(e) => reply.error(&e.to_string()),
                        }
                        Vec::new()
                    }
                    remote::Browse::Tracks => {
                        let q = SongQuery { search, start, limit, ..Default::default() };
                        match source.songs(&q).await {
                            Ok(page) => {
                                let more = page.has_more();
                                let items = page
                                    .items
                                    .iter()
                                    .map(|s| {
                                        json!({
                                            "id": s.id, "name": s.title, "album": s.album,
                                            "artistName": s.artist_line(),
                                            "duration": s.duration.as_millis() as u64, "imageUrl": null,
                                        })
                                    })
                                    .collect();
                                reply.list("tracks-response", &request_id, more, items);
                                return page.items;
                            }
                            Err(e) => reply.error(&e.to_string()),
                        }
                        Vec::new()
                    }
                    remote::Browse::Radio => {
                        match source.internet_radios().await {
                            Ok(all) => {
                                let more = all.len() > start + limit;
                                let items = all
                                    .iter()
                                    .skip(start)
                                    .take(limit)
                                    .map(|r| {
                                        json!({ "id": r.id, "name": r.name, "homepageUrl": r.home_page_url, "imageUrl": null })
                                    })
                                    .collect();
                                reply.list("radio-response", &request_id, more, items);
                            }
                            Err(e) => reply.error(&e.to_string()),
                        }
                        Vec::new()
                    }
                }
            },
            |songs| Message::Integrations(IntegrationsMsg::RemoteTracks(songs)),
        )
    }

    fn remote_play(
        &mut self,
        what: remote::PlayWhat,
        id: String,
        play_type: String,
        request_id: Option<String>,
        reply: remote::Reply,
    ) -> Task<Message> {
        use remote::PlayWhat as P;
        let answer = request_id.map(|r| (r, reply.clone()));
        let Some(source) = self.source.clone() else {
            if let Some((r, reply)) = &answer {
                reply.ack(r, Some("No server is selected"));
            } else {
                reply.error("No server is selected");
            }
            return Task::none();
        };
        match what {
            P::Radio => {
                let known = self.radio.stations.iter().any(|s| s.id == id);
                if known {
                    return self.handle(Message::Radio(radio::RadioMsg::Play(id)));
                }
                Task::perform(async move { source.internet_radios().await.map_err(err) }, move |r| {
                    Message::Integrations(IntegrationsMsg::RemoteStations(id.clone(), r))
                })
            }
            P::Album | P::Playlist => Task::perform(
                async move {
                    if what == P::Album {
                        source.album(&id).await.map(|a| a.songs).map_err(err)
                    } else {
                        source.playlist_songs(&id).await.map_err(err)
                    }
                },
                move |r| {
                    Message::Integrations(IntegrationsMsg::RemoteLoaded(r, play_type.clone(), answer.clone()))
                },
            ),
            P::Track | P::TrackRadio => {
                let song = self.integ.remote.tracks.get(&id).cloned().or_else(|| self.current_song_is(&id));
                let Some(song) = song else {
                    if let Some((r, reply)) = &answer {
                        reply.ack(r, Some("Open the track from a list first"));
                    }
                    return Task::none();
                };
                Task::perform(
                    async move {
                        if what == P::Track {
                            return Ok(vec![song]);
                        }
                        let mut songs = vec![song.clone()];
                        songs.extend(source.similar_songs(&song, 50).await.map_err(err)?);
                        Ok(songs)
                    },
                    move |r| {
                        Message::Integrations(IntegrationsMsg::RemoteLoaded(
                            r,
                            play_type.clone(),
                            answer.clone(),
                        ))
                    },
                )
            }
        }
    }

    /// Feishin's play types: now / next / last, each also shuffled.
    fn remote_enqueue(&mut self, mut songs: Vec<Song>, play_type: &str) -> Task<Message> {
        use rand::seq::SliceRandom;
        if play_type.to_lowercase().contains("shuffle") {
            songs.shuffle(&mut rand::rng());
        }
        if play_type.starts_with("next") {
            self.queue.enqueue_next(songs);
            self.refresh_next()
        } else if play_type.starts_with("last") {
            self.queue.enqueue(songs);
            self.refresh_next()
        } else {
            self.play_songs(songs, 0)
        }
    }

    /// Publishes the player to the phones when something changed.
    fn sync_remote(&mut self) {
        if self.integ.remote.running.is_none() {
            return;
        }
        let item = self.queue.current();
        let radio = self.radio.active.as_ref();
        let uid = item.map(|i| i.uid);
        if self.integ.remote.cover.as_ref().is_none_or(|(u, _)| Some(*u) != uid)
            && let Some(item) = item
        {
            let path = item
                .song
                .image
                .as_ref()
                .and_then(|i| self.images.disk_path(&self.image_key(i)))
                .filter(|p| p.exists());
            self.integ.remote.cover = Some((item.uid, path));
        }
        let shared = self.integ.remote.shared.clone();
        let (dark, light) = remote_accent(&self.config.settings.appearance);
        shared.accent(&dark, &light);
        let status = match (item, radio) {
            (None, None) => "stopped",
            _ if self.playback.playing => "playing",
            _ => "paused",
        };
        shared.status(status);
        shared.volume(self.playback.volume);
        shared.shuffle(self.play_mode() == super::play_mode::PlayMode::Shuffle);
        shared.repeat(match self.queue.repeat {
            listen_core::queue::Repeat::Off => "none",
            listen_core::queue::Repeat::One => "one",
            listen_core::queue::Repeat::All => "all",
        });
        shared.position(self.playback.position.as_secs_f64().floor());
        let rating_ok = |s: &Song| {
            s.source != SourceKind::YouTubeMusic
                && self
                    .sources
                    .get(&s.server_id)
                    .is_some_and(|x| x.capabilities().contains(Capabilities::RATINGS))
        };
        let cover = self.integ.remote.cover.as_ref().and_then(|(_, p)| p.clone());
        let song_key = item.map(|i| (Some(i.uid), i.song.favorite, i.song.rating, cover.is_some()));
        if self.integ.remote.song_key != song_key.or(Some((None, false, None, false))) {
            self.integ.remote.song_key = song_key.or(Some((None, false, None, false)));
            let song = item.map_or(Value::Null, |i| remote_song(i, rating_ok(&i.song), cover.is_some()));
            shared.song(song, cover);
        }
        let key = (self.queue.signature(), uid);
        if self.integ.remote.queue_key != Some(key) {
            self.integ.remote.queue_key = Some(key);
            let items: Vec<Value> = (0..self.queue.len().min(REMOTE_QUEUE_MAX))
                .filter_map(|i| self.queue.item_at(i))
                .map(|it| {
                    json!({
                        "album": it.song.album, "artistName": it.song.artist_line(),
                        "duration": it.song.duration.as_millis() as u64, "id": it.song.id,
                        "imageUrl": null, "name": it.song.title, "uniqueId": it.uid.to_string(),
                    })
                })
                .collect();
            shared.queue(uid.map(|u| u.to_string()).as_deref(), items);
        }
        shared.radio(match radio {
            Some(r) => json!({ "isActive": true, "imageUrl": null, "stationName": r.station.name }),
            None => json!({ "isActive": false }),
        });
    }

    /// Everything the integrations publish after a message: Discord, the
    /// phones and the song change notification.
    pub(super) fn sync_integrations(&mut self) -> Task<Message> {
        self.sync_remote();
        Task::batch([self.sync_discord(), self.sync_notifications()])
    }
}

/// The primary colour of the default dark and light themes for the phone
/// (Feishin `resolveThemePrimaryColor`), as `#rrggbb`.
fn remote_accent(a: &listen_core::settings::Appearance) -> (String, String) {
    let shade = crate::style::ShadeChoice((!a.use_theme_primary_shade).then_some(a.primary_shade));
    let colour = |theme: &str| {
        let p = listen_core::theme::resolve(theme, a.accent, a.use_theme_accent);
        let c = crate::style::Tokens::build(&p, shade, &Default::default()).primary_shade;
        let [r, g, b, _] = c.into_rgba8();
        format!("#{r:02x}{g:02x}{b:02x}")
    };
    (colour("defaultDark"), colour("defaultLight"))
}

/// The playing song in Feishin's `QueueSong` shape (what the phone page reads).
fn remote_song(item: &listen_core::queue::QueueItem, rating_ok: bool, cover: bool) -> Value {
    let s = &item.song;
    let artist = |r: &listen_core::model::ArtistRef| json!({ "id": r.id, "name": r.name });
    json!({
        "id": s.id,
        "_uniqueId": item.uid.to_string(),
        "_serverId": s.server_id.0,
        "name": s.title,
        "album": s.album,
        "albumId": s.album_id,
        "artistName": s.artist_line(),
        "artists": s.artists.iter().map(artist).collect::<Vec<_>>(),
        "albumArtists": s.album_artists.iter().map(artist).collect::<Vec<_>>(),
        "duration": s.duration.as_millis() as u64,
        // The cover is served by the remote itself; no music server address or token leaves Listen.
        "imageUrl": cover.then_some("/cover"),
        "userFavorite": s.favorite,
        "userRating": rating_ok.then(|| s.rating.unwrap_or(0)),
        "bitRate": s.bit_rate,
        "trackNumber": s.track_number,
        "discNumber": s.disc_number,
        "year": s.year,
        "genres": s.genres,
    })
}

/// The album cover Last.fm knows (`album.getinfo`), for Discord.
async fn lastfm_cover(key: &str, artist: &str, album: &str) -> Option<String> {
    let url = url::Url::parse_with_params(
        "https://ws.audioscrobbler.com/2.0/",
        [
            ("method", "album.getinfo"),
            ("api_key", key),
            ("artist", artist),
            ("album", album),
            ("format", "json"),
        ],
    )
    .ok()?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(8)).build().ok()?;
    let text = client.get(url).send().await.ok()?.text().await.ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let image = v["album"]["image"].as_array()?.get(3)?["#text"].as_str()?.to_string();
    (image.starts_with("https://")).then_some(image)
}

#[cfg(test)]
mod accent_tests {
    use super::*;
    use listen_core::theme::Rgb;

    #[test]
    fn the_phone_gets_the_accent_as_hex_for_both_themes() {
        let mut a = listen_core::settings::Appearance {
            use_theme_accent: false,
            accent: Some(Rgb(200, 30, 30)),
            use_theme_primary_shade: false,
            primary_shade: 6,
            ..Default::default()
        };
        let (dark, light) = remote_accent(&a);
        for c in [&dark, &light] {
            assert!(c.len() == 7 && c.starts_with('#'), "{c}");
        }
        // The accent drives it: another accent gives another colour.
        a.accent = Some(Rgb(30, 30, 200));
        assert_ne!(remote_accent(&a).0, dark);
        // The shade setting moves it too.
        let before = remote_accent(&a);
        a.primary_shade = 2;
        assert_ne!(remote_accent(&a), before);
    }
}
