//! The app side of the MCP server (`crate::mcp`): starts/stops it with the settings and
//! carries out tool calls. Tools that ask a server run as tasks and come back as
//! [`McpMsg::Answer`], so their items are remembered before the client sees the refs.

use super::menu::{MenuAction, MenuTarget};
use super::play_mode::PlayMode;
use super::{Listen, Message};
use crate::mcp::{self, Call, Reply};
use iced::{Subscription, Task};
use listen_core::model::{Album, Artist, Playlist, ServerId, Song};
use listen_core::queue::Repeat;
use listen_source::{Capabilities, ItemKind, MusicSource};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

/// The keyring entry of the access token.
const TOKEN_SECRET: &str = "mcp-token";

/// Something a tool returned, kept so later calls can use its ref.
#[derive(Debug, Clone)]
pub enum Item {
    Song(Song),
    Album(Album),
    Artist(Artist),
    Playlist(Playlist),
}

impl Item {
    pub fn reference(&self) -> String {
        match self {
            Item::Song(s) => song_ref(s),
            Item::Album(a) => format!("album:{}|{}", a.server_id.0, a.id),
            Item::Artist(a) => format!("artist:{}|{}", a.server_id.0, a.id),
            Item::Playlist(p) => format!("playlist:{}|{}", p.server_id.0, p.id),
        }
    }
}

fn song_ref(s: &Song) -> String {
    format!("song:{}|{}", s.server_id.0, s.id)
}

fn secs(d: Duration) -> f64 {
    (d.as_secs_f64() * 10.0).round() / 10.0
}

pub fn song_json(s: &Song) -> Value {
    json!({
        "ref": song_ref(s),
        "title": s.title,
        "artist": s.artist_line(),
        "album": s.album,
        "album_ref": s.album_id.as_ref().map(|id| format!("album:{}|{id}", s.server_id.0)),
        "artist_refs": s.artists.iter().filter_map(|a| a.id.as_ref().map(|id| format!("artist:{}|{id}", s.server_id.0))).collect::<Vec<_>>(),
        "duration": secs(s.duration),
        "year": s.year,
        "favorite": s.favorite,
        "rating": s.rating,
    })
}

fn album_json(a: &Album) -> Value {
    json!({
        "ref": Item::Album(a.clone()).reference(),
        "name": a.name,
        "artist": a.artist_line(),
        "year": a.year,
        "songs": a.song_count,
        "favorite": a.favorite,
    })
}

fn artist_json(a: &Artist) -> Value {
    json!({
        "ref": Item::Artist(a.clone()).reference(),
        "name": a.name,
        "albums": a.album_count,
        "favorite": a.favorite,
    })
}

fn playlist_json(p: &Playlist) -> Value {
    json!({
        "ref": Item::Playlist(p.clone()).reference(),
        "name": p.name,
        "songs": p.song_count,
        "owner": p.owner,
        "editable": p.editable,
    })
}

#[derive(Debug, Clone)]
pub enum McpMsg {
    Call(Call),
    Started(u64, Result<Arc<mcp::Running>, String>),
    /// A task's result: remember `items`, then answer.
    Answer {
        reply: Reply,
        items: Vec<Item>,
        result: Result<Value, String>,
        playlists_changed: bool,
    },
    Enabled(bool),
    AllowLan(bool),
    PortInput(String),
    PortSave,
    CopyToken,
    CopySetup,
    Regenerate,
}

struct Runtime {
    calls_tx: tokio::sync::mpsc::UnboundedSender<Call>,
    calls_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Call>>>,
    running: Option<(mcp::Config, Arc<mcp::Running>)>,
    starting: Option<(u64, mcp::Config)>,
    generation: u64,
}

pub struct McpState {
    runtime: Runtime,
    /// Why the server is not running (shown in the settings).
    pub error: Option<String>,
    token: String,
    /// The port being typed.
    pub port_input: Option<String>,
    /// Items returned by tools, by ref.
    items: HashMap<String, Item>,
}

impl std::fmt::Debug for McpState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mcp::State")
    }
}

impl Default for McpState {
    fn default() -> Self {
        let (calls_tx, calls_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            runtime: Runtime {
                calls_tx,
                calls_rx: Arc::new(tokio::sync::Mutex::new(calls_rx)),
                running: None,
                starting: None,
                generation: 0,
            },
            error: None,
            token: String::new(),
            port_input: None,
            items: HashMap::new(),
        }
    }
}

impl McpState {
    /// Where the server listens, when it runs.
    pub fn addr(&self) -> Option<std::net::SocketAddr> {
        self.runtime.running.as_ref().map(|(_, r)| r.addr)
    }

    pub fn has_token(&self) -> bool {
        !self.token.is_empty()
    }

    /// A sender for calls, as the server uses (tests drive the app with it).
    pub fn sender(&self) -> tokio::sync::mpsc::UnboundedSender<Call> {
        self.runtime.calls_tx.clone()
    }

    /// The next call the app has not handled yet (tests; the running app gets
    /// them through the subscription).
    pub fn poll_call(&self) -> Option<Call> {
        self.runtime.calls_rx.try_lock().ok()?.try_recv().ok()
    }
}

#[derive(Clone)]
struct CallSource(Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Call>>>);

impl Hash for CallSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

/// A random token of 32 hex digits.
fn new_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    (0..16).map(|_| format!("{:02x}", rng.random::<u8>())).collect()
}

fn answer(reply: Reply, items: Vec<Item>, result: Result<Value, String>, playlists_changed: bool) -> Message {
    Message::Mcp(McpMsg::Answer { reply, items, result, playlists_changed })
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn arg_refs(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn arg_u64s(args: &Value, key: &str) -> Vec<u64> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

/// `kind:server|id` → (kind, server, id).
fn split_ref(r: &str) -> Option<(&str, ServerId, &str)> {
    let (kind, rest) = r.split_once(':')?;
    let (server, id) = rest.split_once('|')?;
    Some((kind, ServerId(server.to_string()), id))
}

impl Listen {
    // ---- the server ------------------------------------------------------------

    pub(super) fn mcp_boot(&mut self) -> Task<Message> {
        if self.demo {
            return Task::none();
        }
        self.mcp.token =
            self.secrets.as_ref().and_then(|s| s.get(&ServerId(TOKEN_SECRET.into()))).unwrap_or_default();
        self.mcp_apply()
    }

    /// The token, made and stored on first use.
    fn mcp_token(&mut self) -> String {
        if self.mcp.token.is_empty() {
            self.mcp.token = new_token();
            if let Some(s) = self.secrets.as_ref().filter(|_| !self.ephemeral) {
                s.set(&ServerId(TOKEN_SECRET.into()), &self.mcp.token);
            }
        }
        self.mcp.token.clone()
    }

    fn mcp_config(&mut self) -> Option<mcp::Config> {
        let m = self.config.settings.integrations.mcp.clone();
        if !m.enabled || self.demo {
            return None;
        }
        let token = self.mcp_token();
        Some(mcp::Config {
            bind: if m.allow_lan { "0.0.0.0" } else { "127.0.0.1" }.into(),
            port: m.port,
            token,
        })
    }

    /// Starts, restarts or stops the server to match the settings.
    pub(super) fn mcp_apply(&mut self) -> Task<Message> {
        let want = self.mcp_config();
        let r = &mut self.mcp.runtime;
        let have = r.starting.as_ref().map(|(_, c)| c).or(r.running.as_ref().map(|(c, _)| c));
        if have == want.as_ref() {
            return Task::none();
        }
        let restarting = r.running.take().is_some();
        r.starting = None;
        self.mcp.error = None;
        let Some(cfg) = want else { return Task::none() };
        r.generation += 1;
        let generation = r.generation;
        r.starting = Some((generation, cfg.clone()));
        let calls = r.calls_tx.clone();
        Task::perform(
            async move {
                if restarting {
                    tokio::time::sleep(Duration::from_millis(150)).await; // the old listener lets go of the port
                }
                mcp::start(cfg, calls).await.map(Arc::new)
            },
            move |result| Message::Mcp(McpMsg::Started(generation, result)),
        )
    }

    pub(super) fn mcp_subscription(&self) -> Subscription<Message> {
        if self.mcp.runtime.running.is_none() {
            return Subscription::none();
        }
        Subscription::run_with(CallSource(self.mcp.runtime.calls_rx.clone()), |source| {
            let rx = source.0.clone();
            iced::stream::channel(32, async move |mut out: iced::futures::channel::mpsc::Sender<Message>| {
                use iced::futures::SinkExt;
                while let Some(call) = rx.lock().await.recv().await {
                    if out.send(Message::Mcp(McpMsg::Call(call))).await.is_err() {
                        break;
                    }
                }
                std::future::pending::<()>().await;
            })
        })
    }

    /// The command a client runs to add Listen (Claude Code's syntax; other
    /// clients take the same URL and header).
    pub fn mcp_setup_command(&self) -> String {
        let port = self.config.settings.integrations.mcp.port;
        let token = if self.mcp.token.is_empty() { "<token>" } else { self.mcp.token.as_str() };
        format!(
            "claude mcp add --transport http listen http://127.0.0.1:{port}/mcp --header \"Authorization: Bearer {token}\""
        )
    }

    pub(super) fn on_mcp(&mut self, msg: McpMsg) -> Task<Message> {
        match msg {
            McpMsg::Call(call) => return self.mcp_call(call),
            McpMsg::Started(generation, result) => {
                let r = &mut self.mcp.runtime;
                if r.starting.as_ref().map(|(g, _)| *g) != Some(generation) {
                    return Task::none();
                }
                let (_, cfg) = r.starting.take().expect("checked above");
                match result {
                    Ok(running) => r.running = Some((cfg, running)),
                    Err(e) => {
                        let port = cfg.port;
                        self.mcp.error = Some(format!("The MCP server could not start on port {port}: {e}"));
                    }
                }
            }
            McpMsg::Answer { reply, items, result, playlists_changed } => {
                for item in items {
                    self.mcp.items.insert(item.reference(), item);
                }
                reply.send(result);
                if playlists_changed {
                    return self.load_playlists();
                }
            }
            McpMsg::Enabled(on) => {
                self.config.settings.integrations.mcp.enabled = on;
                self.save_config();
                return self.mcp_apply();
            }
            McpMsg::AllowLan(on) => {
                self.config.settings.integrations.mcp.allow_lan = on;
                self.save_config();
                return self.mcp_apply();
            }
            McpMsg::PortInput(text) => self.mcp.port_input = Some(text),
            McpMsg::PortSave => {
                let Some(text) = self.mcp.port_input.take() else { return Task::none() };
                match text.trim().parse::<u16>() {
                    Ok(port) if port >= 1024 => {
                        self.config.settings.integrations.mcp.port = port;
                        self.save_config();
                        return self.mcp_apply();
                    }
                    _ => self.toast_error("Choose a port from 1024 to 65535.".into()),
                }
            }
            McpMsg::CopyToken => {
                let token = self.mcp_token();
                self.toast_success("Token copied.".into());
                return iced::clipboard::write(token);
            }
            McpMsg::CopySetup => {
                self.mcp_token();
                self.toast_success("Command copied.".into());
                return iced::clipboard::write(self.mcp_setup_command());
            }
            McpMsg::Regenerate => {
                self.mcp.token.clear();
                self.mcp_token();
                self.toast_info("New token made. Update it in your MCP client.".into());
                return self.mcp_apply();
            }
        }
        Task::none()
    }

    // ---- tools ----------------------------------------------------------------------

    fn remember(&mut self, items: impl IntoIterator<Item = Item>) {
        for item in items {
            self.mcp.items.insert(item.reference(), item);
        }
    }

    fn source_for(&self, server: Option<&str>) -> Result<(ServerId, Arc<dyn MusicSource>), String> {
        let id = match server {
            Some(s) if !s.is_empty() => ServerId(s.to_string()),
            _ => self.config.active_server.clone().ok_or("No server is active.")?,
        };
        let source =
            self.sources.get(&id).cloned().ok_or_else(|| format!("Server {} is not connected.", id.0))?;
        Ok((id, source))
    }

    fn item(&self, r: &str) -> Result<Item, String> {
        self.mcp.items.get(r).cloned().ok_or_else(|| {
            format!("Unknown ref “{r}”. Use a ref returned by search, get_album, get_playlist or get_queue.")
        })
    }

    /// The refs as menu targets, one per kind, in the order the kinds first appear.
    fn targets(&self, refs: &[String]) -> Result<Vec<MenuTarget>, String> {
        let mut songs = Vec::new();
        let (mut albums, mut artists, mut playlists) = (Vec::new(), Vec::new(), Vec::new());
        let mut order: Vec<u8> = Vec::new();
        for r in refs {
            let kind = match self.item(r)? {
                Item::Song(s) => {
                    songs.push(s);
                    0
                }
                Item::Album(a) => {
                    albums.push(a);
                    1
                }
                Item::Artist(a) => {
                    artists.push(a);
                    2
                }
                Item::Playlist(p) => {
                    playlists.push(p);
                    3
                }
            };
            if !order.contains(&kind) {
                order.push(kind);
            }
        }
        Ok(order
            .into_iter()
            .map(|k| match k {
                0 => MenuTarget::songs(&songs),
                1 => MenuTarget::Albums(albums.clone()),
                2 => MenuTarget::Artists(artists.clone()),
                _ => MenuTarget::Playlists(playlists.clone()),
            })
            .collect())
    }

    fn now_playing_json(&self) -> Value {
        let current = self.queue.current().map(|i| i.song.clone());
        let server = current.as_ref().and_then(|s| self.config.servers.iter().find(|c| c.id == s.server_id));
        json!({
            "song": current.as_ref().map(song_json),
            "playing": self.playback.playing,
            "position": secs(self.playback.position),
            "duration": self.playback.duration.or(current.as_ref().map(|s| s.duration)).map(secs),
            "volume": self.playback.volume,
            "muted": self.playback.muted,
            "play_mode": match self.play_mode() {
                PlayMode::Off => "off",
                PlayMode::Shuffle => "shuffle",
                PlayMode::AutoDj => "auto_dj",
                PlayMode::AiDj => "ai_dj",
            },
            "repeat": match self.queue.repeat {
                Repeat::Off => "off",
                Repeat::All => "all",
                Repeat::One => "one",
            },
            "speed": self.audio.speed,
            "server": server.map(|c| json!({ "id": c.id.0, "name": c.name, "kind": format!("{:?}", c.kind) })),
        })
    }

    fn ai_dj_json(&self) -> Value {
        let d = &self.config.settings.ai_dj;
        let persona = self.dj_persona();
        json!({
            "enabled": d.enabled,
            "key_stored": self.dj.has_key(),
            "persona": persona.id,
            "voice": persona.voice,
            "cohost_voice": persona.cohost.map(|c| c.voice),
            "tts_model": d.tts_model,
            "text_model": d.text_model,
            "language": d.language,
            "language_spoken": crate::i18n::english_name(crate::i18n::dj_language(&d.language)),
            "location": d.location,
            "pronunciation": d.location_pronunciation,
            "details": d.notes,
            "set_min": d.set_min,
            "set_max": d.set_max,
            "talk_every": d.talk_every,
            "max_voiced_per_day": d.max_voiced_per_day,
            "voice_gain_db": d.voice_gain_db,
        })
    }

    /// Runs one tool call; the answer goes to `call.reply` now or from a task.
    fn mcp_call(&mut self, call: Call) -> Task<Message> {
        let Call { tool, args, reply } = call;
        match self.mcp_tool(&tool, &args, reply.clone()) {
            Ok(Some(task)) => task,
            Ok(None) => Task::none(),
            Err(e) => {
                reply.err(e);
                Task::none()
            }
        }
    }

    /// `Ok(None)`: answered already; `Ok(Some(task))`: the task answers.
    fn mcp_tool(&mut self, tool: &str, args: &Value, reply: Reply) -> Result<Option<Task<Message>>, String> {
        let done = |reply: &Reply, task: Task<Message>, value: Value| {
            reply.ok(value);
            Ok(Some(task))
        };
        let msg = |s: &mut Self, m: Message| s.update(m);
        match tool {
            // ---- playback
            "get_now_playing" => {
                let now = self.now_playing_json();
                if let Some(song) = self.queue.current().map(|i| i.song.clone()) {
                    self.remember([Item::Song(song)]);
                }
                reply.ok(now);
                Ok(None)
            }
            "play" | "pause" | "toggle_play" => {
                let want = match tool {
                    "play" => Some(true),
                    "pause" => Some(false),
                    _ => None,
                };
                if self.queue.current().is_none() {
                    return Err("The queue is empty.".into());
                }
                let task = if want.is_none_or(|w| w != self.playback.playing) {
                    msg(self, Message::TogglePlay)
                } else {
                    Task::none()
                };
                done(&reply, task, json!({ "playing": want.unwrap_or(!self.playback.playing) }))
            }
            "next" => {
                let t = msg(self, Message::Next);
                done(&reply, t, json!({ "ok": true }))
            }
            "previous" => {
                let t = msg(self, Message::Previous);
                done(&reply, t, json!({ "ok": true }))
            }
            "seek" => {
                let s = args.get("seconds").and_then(Value::as_f64).ok_or("seconds is missing")?;
                let t = msg(self, Message::Seek(s.max(0.0) as f32));
                done(&reply, t, json!({ "position": s }))
            }
            "set_volume" => {
                let v =
                    args.get("volume").and_then(Value::as_f64).ok_or("volume is missing")?.clamp(0.0, 100.0);
                let t = msg(self, Message::Volume(v as f32));
                done(&reply, t, json!({ "volume": v.round() }))
            }
            "set_play_mode" => {
                let mode = match arg_str(args, "mode").unwrap_or("") {
                    "off" => PlayMode::Off,
                    "shuffle" => PlayMode::Shuffle,
                    "auto_dj" => PlayMode::AutoDj,
                    "ai_dj" if self.dj.has_key() => PlayMode::AiDj,
                    "ai_dj" => return Err("The AI DJ needs a Gemini key first (set_gemini_key).".into()),
                    other => return Err(format!("unknown mode “{other}”")),
                };
                let t = msg(self, Message::SetPlayMode(mode));
                done(&reply, t, json!({ "mode": arg_str(args, "mode") }))
            }
            "set_repeat" => {
                self.queue.repeat = match arg_str(args, "repeat").unwrap_or("") {
                    "off" => Repeat::Off,
                    "all" => Repeat::All,
                    "one" => Repeat::One,
                    other => return Err(format!("unknown repeat mode “{other}”")),
                };
                let t = self.refresh_next();
                done(&reply, t, json!({ "repeat": arg_str(args, "repeat") }))
            }
            "set_speed" => {
                let v = args.get("speed").and_then(Value::as_f64).ok_or("speed is missing")?;
                let t = msg(self, Message::Audio(super::audio::AudioMsg::SetSpeed(v as f32)));
                done(&reply, t, json!({ "speed": self.audio.speed }))
            }
            "sleep_timer" => {
                use super::sleep_timer::SleepMsg;
                let m = match (args.get("minutes").and_then(Value::as_u64), arg_str(args, "at")) {
                    (_, Some("cancel")) => SleepMsg::Cancel,
                    (_, Some("end_of_song")) => SleepMsg::EndOfSong,
                    (_, Some("end_of_album")) => SleepMsg::EndOfAlbum,
                    (Some(m), _) if m > 0 => SleepMsg::Minutes(m.min(24 * 60) as u32),
                    _ => return Err("give minutes, or at = end_of_song, end_of_album or cancel".into()),
                };
                let t = msg(self, Message::Fullscreen(super::fullscreen_state::FullscreenMsg::Sleep(m)));
                done(&reply, t, json!({ "ok": true }))
            }
            // ---- queue
            "get_queue" => {
                let current = self.queue.current().map(|i| i.uid);
                let items: Vec<Value> = self
                    .queue
                    .iter()
                    .map(|i| {
                        let mut v = song_json(&i.song);
                        v["uid"] = json!(i.uid);
                        v["current"] = json!(Some(i.uid) == current);
                        v["auto"] = json!(i.auto);
                        v
                    })
                    .collect();
                let songs: Vec<Item> = self.queue.iter().map(|i| Item::Song(i.song.clone())).collect();
                self.remember(songs);
                reply.ok(json!({ "entries": items }));
                Ok(None)
            }
            "play_songs" | "add_to_queue" => {
                let refs = arg_refs(args, "refs");
                if refs.is_empty() {
                    return Err("refs is empty".into());
                }
                let targets = self.targets(&refs)?;
                let shuffle = args.get("shuffle").and_then(Value::as_bool).unwrap_or(false);
                let how = if tool == "add_to_queue" {
                    if args.get("next").and_then(Value::as_bool).unwrap_or(false) { "next" } else { "last" }
                } else {
                    arg_str(args, "how").unwrap_or("now")
                };
                let mut tasks = Vec::new();
                for (i, target) in targets.into_iter().enumerate() {
                    let action = match (how, shuffle, i) {
                        ("now", false, 0) => MenuAction::Play,
                        ("now", true, 0) => MenuAction::Shuffle,
                        ("next", false, _) => MenuAction::PlayNext,
                        ("next", true, _) => MenuAction::PlayNextShuffled,
                        (_, true, _) => MenuAction::AddToQueueShuffled,
                        _ => MenuAction::AddToQueue,
                    };
                    tasks.push(self.without_queue_confirm(|s| s.apply_to_target(action, target)));
                }
                done(&reply, Task::batch(tasks), json!({ "ok": true, "how": how, "items": refs.len() }))
            }
            "play_queue_entry" => {
                let uid = args.get("uid").and_then(Value::as_u64).ok_or("uid is missing")?;
                if !self.queue.iter().any(|i| i.uid == uid) {
                    return Err(format!("no queue entry {uid}"));
                }
                let t = msg(self, Message::PlayQueueItem(uid));
                done(&reply, t, json!({ "ok": true }))
            }
            "remove_from_queue" => {
                let playing = self.queue.current().map(|i| i.uid);
                let uids: Vec<u64> =
                    arg_u64s(args, "uids").into_iter().filter(|u| Some(*u) != playing).collect();
                for uid in &uids {
                    self.queue.remove(*uid);
                }
                let t = self.refresh_next();
                done(&reply, t, json!({ "removed": uids.len() }))
            }
            "move_in_queue" => {
                let uids = arg_u64s(args, "uids");
                let at = args.get("position").and_then(Value::as_u64).ok_or("position is missing")? as usize;
                self.queue.move_uids(&uids, at.min(self.queue.len()));
                let t = self.refresh_next();
                done(&reply, t, json!({ "ok": true }))
            }
            "shuffle_queue" => {
                self.queue.shuffle_all();
                let t = self.refresh_next();
                done(&reply, t, json!({ "ok": true }))
            }
            "clear_queue" => {
                let t = msg(self, Message::Queue(super::queue_ui::QueueMsg::ClearConfirmed));
                done(&reply, t, json!({ "ok": true }))
            }
            // ---- library
            "list_servers" => {
                let active = self.config.active_server.clone();
                let servers: Vec<Value> = self
                    .config
                    .servers
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id.0, "name": c.name, "kind": format!("{:?}", c.kind),
                            "active": Some(&c.id) == active.as_ref(),
                            "connected": self.sources.contains_key(&c.id),
                        })
                    })
                    .collect();
                reply.ok(json!({ "servers": servers }));
                Ok(None)
            }
            "switch_server" => {
                let id = ServerId(arg_str(args, "server").ok_or("server is missing")?.to_string());
                if !self.config.servers.iter().any(|c| c.id == id) {
                    return Err(format!("no server {}", id.0));
                }
                let t = msg(self, Message::SwitchServer(id.clone()));
                done(&reply, t, json!({ "active": id.0 }))
            }
            "search" => {
                let query = arg_str(args, "query").ok_or("query is missing")?.to_string();
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20).clamp(1, 100) as usize;
                let (_, source) = self.source_for(arg_str(args, "server"))?;
                Ok(Some(Task::perform(
                    async move { source.search(&query, limit).await.map_err(|e| e.to_string()) },
                    move |r| {
                        let mut items = Vec::new();
                        let result = r.map(|res| {
                            let v = json!({
                                "songs": res.songs.iter().map(song_json).collect::<Vec<_>>(),
                                "albums": res.albums.iter().map(album_json).collect::<Vec<_>>(),
                                "artists": res.artists.iter().map(artist_json).collect::<Vec<_>>(),
                                "playlists": res.playlists.iter().map(playlist_json).collect::<Vec<_>>(),
                            });
                            items.extend(res.songs.into_iter().map(Item::Song));
                            items.extend(res.albums.into_iter().map(Item::Album));
                            items.extend(res.artists.into_iter().map(Item::Artist));
                            items.extend(res.playlists.into_iter().map(Item::Playlist));
                            v
                        });
                        answer(reply.clone(), items, result, false)
                    },
                )))
            }
            "get_album" => {
                let r = arg_str(args, "ref").ok_or("ref is missing")?;
                let (kind, server, id) = split_ref(r).ok_or("not a ref")?;
                if kind != "album" {
                    return Err("not an album ref".into());
                }
                let (_, source) = self.source_for(Some(&server.0))?;
                let id = id.to_string();
                Ok(Some(Task::perform(
                    async move { source.album(&id).await.map_err(|e| e.to_string()) },
                    move |r| {
                        let mut items = Vec::new();
                        let result = r.map(|a| {
                            let mut v = album_json(&a);
                            v["songs"] = json!(a.songs.iter().map(song_json).collect::<Vec<_>>());
                            items.extend(a.songs.iter().cloned().map(Item::Song));
                            items.push(Item::Album(a));
                            v
                        });
                        answer(reply.clone(), items, result, false)
                    },
                )))
            }
            "get_artist" => {
                let r = arg_str(args, "ref").ok_or("ref is missing")?;
                let (kind, server, id) = split_ref(r).ok_or("not a ref")?;
                if kind != "artist" {
                    return Err("not an artist ref".into());
                }
                let (_, source) = self.source_for(Some(&server.0))?;
                let id = id.to_string();
                Ok(Some(Task::perform(
                    async move {
                        let artist = source.artist(&id).await.map_err(|e| e.to_string())?;
                        let albums = source.artist_albums(&id).await.unwrap_or_default();
                        let top = source.top_songs(&artist, 10).await.unwrap_or_default();
                        Ok::<_, String>((artist, albums, top))
                    },
                    move |r| {
                        let mut items = Vec::new();
                        let result = r.map(|(artist, albums, top)| {
                            let mut v = artist_json(&artist);
                            v["biography"] = json!(artist.biography);
                            v["albums"] = json!(albums.iter().map(album_json).collect::<Vec<_>>());
                            v["top_songs"] = json!(top.iter().map(song_json).collect::<Vec<_>>());
                            items.push(Item::Artist(artist));
                            items.extend(albums.into_iter().map(Item::Album));
                            items.extend(top.into_iter().map(Item::Song));
                            v
                        });
                        answer(reply.clone(), items, result, false)
                    },
                )))
            }
            "list_playlists" => {
                let (_, source) = self.source_for(arg_str(args, "server"))?;
                Ok(Some(Task::perform(
                    async move { source.playlists().await.map_err(|e| e.to_string()) },
                    move |r| {
                        let mut items = Vec::new();
                        let result = r.map(|all| {
                            let v = json!({ "playlists": all.iter().map(playlist_json).collect::<Vec<_>>() });
                            items.extend(all.into_iter().map(Item::Playlist));
                            v
                        });
                        answer(reply.clone(), items, result, false)
                    },
                )))
            }
            "get_playlist" => {
                let r = arg_str(args, "ref").ok_or("ref is missing")?;
                let (kind, server, id) = split_ref(r).ok_or("not a ref")?;
                if kind != "playlist" {
                    return Err("not a playlist ref".into());
                }
                let (_, source) = self.source_for(Some(&server.0))?;
                let meta = self.mcp.items.get(r).cloned();
                let id = id.to_string();
                Ok(Some(Task::perform(
                    async move { source.playlist_songs(&id).await.map_err(|e| e.to_string()) },
                    move |res| {
                        let mut items = Vec::new();
                        let result = res.map(|songs| {
                            let mut v = match &meta {
                                Some(Item::Playlist(p)) => playlist_json(p),
                                _ => json!({}),
                            };
                            v["songs"] = json!(
                                songs
                                    .iter()
                                    .enumerate()
                                    .map(|(i, s)| {
                                        let mut j = song_json(s);
                                        j["position"] = json!(i);
                                        j
                                    })
                                    .collect::<Vec<_>>()
                            );
                            items.extend(songs.into_iter().map(Item::Song));
                            v
                        });
                        answer(reply.clone(), items, result, false)
                    },
                )))
            }
            "start_radio" => {
                let r = arg_str(args, "ref").ok_or("ref is missing")?.to_string();
                let target = self.targets(std::slice::from_ref(&r))?.remove(0);
                let task = match target {
                    t @ MenuTarget::Songs { .. } => {
                        self.without_queue_confirm(|s| s.apply_to_target(MenuAction::StartRadio, t))
                    }
                    t => self.menu_extra(MenuAction::Radio(super::menu::PlayHow::Now), t),
                };
                done(&reply, task, json!({ "ok": true }))
            }
            "get_lyrics" => {
                let Some(l) = &self.lyrics else {
                    reply.ok(json!({ "lyrics": null }));
                    return Ok(None);
                };
                let lines: Vec<Value> = l
                    .lines
                    .iter()
                    .map(|line| json!({ "at": line.start.map(secs), "text": line.text, "translation": line.translation }))
                    .collect();
                reply.ok(json!({ "provider": l.provider, "synced": l.synced, "lines": lines }));
                Ok(None)
            }
            // ---- playlists
            "create_playlist" => {
                let name = arg_str(args, "name").ok_or("name is missing")?.trim().to_string();
                if name.is_empty() {
                    return Err("the name is empty".into());
                }
                let (_, source) = self.source_for(arg_str(args, "server"))?;
                if !source.capabilities().contains(Capabilities::PLAYLIST_CREATE) {
                    return Err("This server can't create playlists.".into());
                }
                let mut ids = Vec::new();
                for r in arg_refs(args, "refs") {
                    match self.item(&r)? {
                        Item::Song(s) => ids.push(s.id),
                        _ => {
                            return Err(
                                "create_playlist takes song refs; add albums with add_to_playlist".into()
                            );
                        }
                    }
                }
                Ok(Some(Task::perform(
                    async move { source.create_playlist(&name, &ids).await.map_err(|e| e.to_string()) },
                    move |r| {
                        let mut items = Vec::new();
                        let result = r.map(|p| {
                            let v = playlist_json(&p);
                            items.push(Item::Playlist(p));
                            v
                        });
                        answer(reply.clone(), items, result, true)
                    },
                )))
            }
            "add_to_playlist" => {
                let Item::Playlist(p) = self.item(arg_str(args, "playlist").ok_or("playlist is missing")?)?
                else {
                    return Err("playlist is not a playlist ref".into());
                };
                let source = self
                    .sources
                    .get(&p.server_id)
                    .cloned()
                    .ok_or("the playlist's server is not connected")?;
                if !source.capabilities().contains(Capabilities::PLAYLIST_ADD) {
                    return Err("This server can't add to playlists.".into());
                }
                let refs = arg_refs(args, "refs");
                let targets = self.targets(&refs)?;
                let (mut ids, mut others) = (Vec::new(), Vec::new());
                for t in targets {
                    match t {
                        MenuTarget::Songs { songs, .. } => {
                            if songs.iter().any(|s| s.server_id != p.server_id) {
                                return Err("songs from another server can't go into this playlist".into());
                            }
                            ids.extend(songs.into_iter().map(|s| s.id));
                        }
                        t => others.push(t),
                    }
                }
                // Albums and artists: their songs are fetched and added like the menu does.
                let follow: Vec<_> = others
                    .into_iter()
                    .map(|t| self.apply_to_target(MenuAction::AddToPlaylist(p.id.clone(), p.name.clone()), t))
                    .collect();
                let id = p.id.clone();
                let n = ids.len();
                let add = Task::perform(
                    async move {
                        if ids.is_empty() {
                            return Ok(());
                        }
                        source.add_to_playlist(&id, &ids).await.map_err(|e| e.to_string())
                    },
                    move |r| answer(reply.clone(), vec![], r.map(|()| json!({ "added_songs": n })), true),
                );
                Ok(Some(Task::batch(follow.into_iter().chain([add]))))
            }
            "remove_from_playlist" | "rename_playlist" | "delete_playlist" => {
                let Item::Playlist(p) = self.item(arg_str(args, "playlist").ok_or("playlist is missing")?)?
                else {
                    return Err("playlist is not a playlist ref".into());
                };
                let source = self
                    .sources
                    .get(&p.server_id)
                    .cloned()
                    .ok_or("the playlist's server is not connected")?;
                let caps = source.capabilities();
                let id = p.id.clone();
                let task: Task<Result<Value, String>> = match tool {
                    "remove_from_playlist" => {
                        if !caps.contains(Capabilities::PLAYLIST_REMOVE) {
                            return Err("This server can't remove songs from playlists.".into());
                        }
                        let positions: Vec<usize> =
                            arg_u64s(args, "positions").into_iter().map(|p| p as usize).collect();
                        Task::future(async move {
                            source.remove_from_playlist(&id, &positions).await.map_err(|e| e.to_string())?;
                            Ok(json!({ "removed": positions.len() }))
                        })
                    }
                    "rename_playlist" => {
                        if !caps.contains(Capabilities::PLAYLIST_RENAME) {
                            return Err("This server can't rename playlists.".into());
                        }
                        let name = arg_str(args, "name").ok_or("name is missing")?.trim().to_string();
                        Task::future(async move {
                            source.rename_playlist(&id, &name).await.map_err(|e| e.to_string())?;
                            Ok(json!({ "name": name }))
                        })
                    }
                    _ => {
                        if !caps.contains(Capabilities::PLAYLIST_DELETE) || !p.editable {
                            return Err("This playlist can't be deleted.".into());
                        }
                        Task::future(async move {
                            source.delete_playlist(&id).await.map_err(|e| e.to_string())?;
                            Ok(json!({ "deleted": true }))
                        })
                    }
                };
                Ok(Some(task.map(move |r| answer(reply.clone(), vec![], r, true))))
            }
            // ---- favorites
            "set_favorite" | "set_rating" => {
                let targets = self.targets(&arg_refs(args, "refs"))?;
                let mut tasks = Vec::new();
                if tool == "set_favorite" {
                    let fav = args.get("favorite").and_then(Value::as_bool).ok_or("favorite is missing")?;
                    for t in targets {
                        tasks.push(self.menu_extra(MenuAction::Favorite(fav), t));
                    }
                } else {
                    let stars =
                        args.get("stars").and_then(Value::as_u64).ok_or("stars is missing")?.min(5) as u8;
                    for t in targets {
                        tasks.push(match t {
                            MenuTarget::Songs { songs, .. } => Task::batch(
                                songs.iter().map(|s| self.rate_song(s, stars)).collect::<Vec<_>>(),
                            ),
                            MenuTarget::Albums(v) => Task::batch(
                                v.iter()
                                    .map(|a| {
                                        self.set_item_rating(ItemKind::Album, &a.server_id, &a.id, stars)
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                            MenuTarget::Artists(v) => Task::batch(
                                v.iter()
                                    .map(|a| {
                                        self.set_item_rating(ItemKind::Artist, &a.server_id, &a.id, stars)
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                            _ => return Err("playlists can't be rated".into()),
                        });
                    }
                }
                done(&reply, Task::batch(tasks), json!({ "ok": true }))
            }
            // ---- AI DJ
            "get_ai_dj" => {
                reply.ok(self.ai_dj_json());
                Ok(None)
            }
            "list_personas" => {
                let list: Vec<Value> = listen_dj::PERSONAS
                    .iter()
                    .map(|p| {
                        json!({
                            "id": p.id, "name": p.name, "description": p.description, "voice": p.voice,
                            "cohost": p.cohost.map(|c| json!({ "name": c.name, "voice": c.voice })),
                        })
                    })
                    .collect();
                reply.ok(json!({ "personas": list }));
                Ok(None)
            }
            "set_ai_dj" => {
                use super::Setting as S;
                let mut tasks = Vec::new();
                if let Some(id) = arg_str(args, "persona") {
                    if !listen_dj::PERSONAS.iter().any(|p| p.id == id) {
                        return Err(format!("unknown persona “{id}” (see list_personas)"));
                    }
                    tasks.push(msg(self, Message::Setting(S::AiDjPersona(id.into()))));
                }
                if let Some(m) = arg_str(args, "tts_model") {
                    if !listen_dj::TTS_MODELS.contains(&m) {
                        return Err(format!("unknown voice model “{m}”"));
                    }
                    tasks.push(msg(self, Message::Setting(S::AiDjTtsModel(m.into()))));
                }
                if let Some(code) = arg_str(args, "language") {
                    if !code.is_empty() && !crate::i18n::LANGUAGES.iter().any(|(c, _)| *c == code) {
                        return Err(format!("unsupported language “{code}”"));
                    }
                    tasks.push(msg(self, Message::Setting(S::AiDjLanguage(code.into()))));
                }
                if let Some(v) = arg_str(args, "location") {
                    tasks.push(msg(self, Message::Setting(S::AiDjLocation(v.into()))));
                }
                if let Some(v) = arg_str(args, "pronunciation") {
                    tasks.push(msg(self, Message::Setting(S::AiDjPronunciation(v.into()))));
                }
                if let Some(v) = arg_str(args, "details") {
                    tasks.push(msg(self, Message::Setting(S::AiDjNotes(v.into()))));
                }
                if let Some(v) = args.get("set_max").and_then(Value::as_u64) {
                    tasks.push(msg(self, Message::Setting(S::AiDjSetMax(v as u32))));
                }
                if let Some(v) = args.get("talk_every").and_then(Value::as_u64) {
                    tasks.push(msg(self, Message::Setting(S::AiDjTalkEvery(v as u32))));
                }
                if let Some(v) = args.get("voice_gain_db").and_then(Value::as_f64) {
                    tasks.push(msg(self, Message::Setting(S::AiDjVoiceGain(v as f32))));
                }
                if let Some(v) = args.get("max_voiced_per_day").and_then(Value::as_u64) {
                    tasks.push(msg(self, Message::Setting(S::AiDjMaxVoiced(v as u32))));
                }
                if let Some(on) = args.get("enabled").and_then(Value::as_bool) {
                    if on && !self.dj.has_key() {
                        return Err("The AI DJ needs a Gemini key first (set_gemini_key).".into());
                    }
                    tasks.push(msg(self, Message::Setting(S::AiDj(on))));
                }
                let now = self.ai_dj_json();
                done(&reply, Task::batch(tasks), now)
            }
            "set_gemini_key" => {
                use super::Setting as S;
                let key = arg_str(args, "key").ok_or("key is missing")?.trim().to_string();
                let removed = key.is_empty();
                let input = msg(self, Message::Setting(S::AiDjKeyInput(key)));
                let save = msg(self, Message::Setting(S::AiDjSaveKey));
                done(
                    &reply,
                    Task::batch([input, save]),
                    json!({ "stored": !removed && self.dj.has_key(), "removed": removed }),
                )
            }
            // ---- settings
            "get_settings" => {
                let text =
                    listen_core::settings::export_json(&self.config.settings, env!("CARGO_PKG_VERSION"));
                let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                reply.ok(v.get("settings").cloned().unwrap_or(v));
                Ok(None)
            }
            "set_settings" => {
                let partial =
                    args.get("settings").filter(|v| v.is_object()).ok_or("settings must be an object")?;
                let report = listen_core::settings::import_json(&self.config.settings, &partial.to_string())?;
                let tab = self.config.settings.settings_tab;
                self.config.settings =
                    listen_core::settings::Settings { settings_tab: tab, ..report.settings };
                self.after_settings_replaced();
                let tasks = Task::batch([self.apply_audio(), self.remote_apply(), self.mcp_apply()]);
                done(&reply, tasks, json!({ "applied": report.applied, "refused": report.issues }))
            }
            "get_analysis_status" => {
                let a = &self.config.settings.analysis;
                let status = self.analysis_status.as_ref().map(|st| {
                    json!({
                        "songs_analysed": st.songs,
                        "waiting": st.queued,
                        "current": st.current,
                        "error": st.error,
                        "library": st.library.map(|(n, done)| json!({ "songs": n, "listed": done })),
                    })
                });
                reply.ok(json!({
                    "enabled": a.enabled,
                    "youtube_music": a.youtube_music && !a.shared_dir.trim().is_empty(),
                    "dj_transitions": self.config.settings.mixing.enabled,
                    "status": status,
                }));
                Ok(None)
            }
            other => Err(format!("unknown tool: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_split_into_kind_server_and_id() {
        let (kind, server, id) = split_ref("album:nd-1|al-9|x").unwrap();
        assert_eq!((kind, server.0.as_str(), id), ("album", "nd-1", "al-9|x"));
        assert!(split_ref("nonsense").is_none());
        assert_eq!(new_token().len(), 32);
        assert_ne!(new_token(), new_token());
    }
}
