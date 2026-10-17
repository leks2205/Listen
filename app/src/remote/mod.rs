//! Remote control server (Feishin's `main/features/core/remote`): HTTP and WebSocket on one port
//! (default 4333), optional basic auth, an embedded phone page and Feishin's message format
//! (`shared/types/remote-types.ts`). Clients only get what [`Shared`] holds, never server credentials.

pub mod ws;

use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, watch};

const PAGE: &str = include_str!("page.html");
const MANIFEST: &str = r##"{"name":"Listen Remote","short_name":"Listen","start_url":"/","display":"standalone","background_color":"#111111","theme_color":"#111111"}"##;
const PING_EVERY: Duration = Duration::from_secs(10);
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HEAD: usize = 16 * 1024;
const MAX_CLIENTS: usize = 32;
/// Close code of a deliberate stop (the client does not retry).
pub const CLOSE_DOWN: u16 = 4000;
/// Close code for "settings changed": the page reloads and reconnects.
pub const CLOSE_CHANGED: u16 = 4002;
const CLOSE_AUTH: u16 = 4004;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind: String,
    pub port: u16,
    /// Both empty = no authentication.
    pub username: String,
    pub password: String,
}

impl Config {
    fn auth_required(&self) -> bool {
        !self.username.is_empty() || !self.password.is_empty()
    }

    /// Whether an `Authorization: Basic ...` header carries the right login.
    pub fn authorized(&self, header: Option<&str>) -> bool {
        if !self.auth_required() {
            return true;
        }
        let Some(b64) = header.and_then(|h| h.split_once(' ')).map(|(_, v)| v.trim()) else { return false };
        use base64::Engine;
        let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(b64) else { return false };
        let Ok(text) = String::from_utf8(raw) else { return false };
        let (user, pass) = text.split_once(':').unwrap_or((&text, ""));
        // Both compared in full, whatever the first result.
        let u = constant_eq(user.as_bytes(), self.username.as_bytes());
        let p = constant_eq(pass.as_bytes(), self.password.as_bytes());
        u & p
    }
}

pub(crate) fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browse {
    Albums,
    Playlists,
    Tracks,
    Radio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayWhat {
    Album,
    Playlist,
    Track,
    Radio,
    TrackRadio,
}

/// What a client asked for; the app carries it out.
#[derive(Debug, Clone)]
pub enum Command {
    Play,
    Pause,
    Next,
    Previous,
    ToggleRepeat,
    ToggleShuffle,
    /// Seconds.
    Seek(f64),
    /// 0 to 100.
    Volume(u8),
    Favorite {
        id: String,
        favorite: bool,
    },
    Rating {
        id: String,
        rating: u8,
    },
    QueueJump(String),
    RemoveFromQueue(String),
    ReorderQueue {
        unique_id: String,
        target: String,
        top: bool,
    },
    ClearQueue,
    Browse {
        kind: Browse,
        search: Option<String>,
        start: usize,
        limit: usize,
        request_id: String,
        reply: Reply,
    },
    PlayItem {
        what: PlayWhat,
        id: String,
        play_type: Option<String>,
        request_id: Option<String>,
        reply: Reply,
    },
}

/// Where a client's answer goes.
#[derive(Debug, Clone)]
pub struct Reply(mpsc::UnboundedSender<String>);

impl Reply {
    pub fn send(&self, event: &str, data: Value) {
        let _ = self.0.send(json!({ "event": event, "data": data }).to_string());
    }

    pub fn list(&self, event: &str, request_id: &str, has_more: bool, items: Vec<Value>) {
        self.send(event, json!({ "hasMore": has_more, "items": items, "requestId": request_id }));
    }

    pub fn ack(&self, request_id: &str, error: Option<&str>) {
        self.send("operation-ack", json!({ "requestId": request_id, "error": error }));
    }

    pub fn error(&self, message: &str) {
        self.send("error", json!(message));
    }
}

#[derive(Default)]
struct Snapshot {
    /// Feishin's `SongState`: position, repeat, shuffle, song, status, volume.
    state: serde_json::Map<String, Value>,
    queue: Value,
    radio: Value,
    cover: Option<PathBuf>,
    /// Feishin's `accent-color`: `{ dark, light }` primary colours (CSS), once known.
    accent: Option<Value>,
}

/// What the app publishes and the server serves: the current state plus a broadcast of every change.
pub struct Shared {
    snap: Mutex<Snapshot>,
    tx: broadcast::Sender<Arc<str>>,
}

impl Default for Shared {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(256);
        let snap = Snapshot {
            queue: json!({ "currentUniqueId": null, "items": [] }),
            radio: json!({ "isActive": false }),
            ..Default::default()
        };
        Self { snap: Mutex::new(snap), tx }
    }
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("remote::Shared")
    }
}

impl Shared {
    fn broadcast(&self, event: &str, data: &Value) {
        let _ = self.tx.send(json!({ "event": event, "data": data }).to_string().into());
    }

    /// Sets a `SongState` field and tells the clients (as event `event`) when it changed.
    fn set(&self, key: &str, event: &str, data: Value) {
        {
            let mut s = self.snap.lock().expect("remote state");
            if s.state.get(key) == Some(&data) {
                return;
            }
            s.state.insert(key.into(), data.clone());
        }
        self.broadcast(event, &data);
    }

    pub fn status(&self, status: &str) {
        self.set("status", "playback", json!(status));
    }

    pub fn volume(&self, volume: u8) {
        self.set("volume", "volume", json!(volume));
    }

    pub fn repeat(&self, repeat: &str) {
        self.set("repeat", "repeat", json!(repeat));
    }

    pub fn shuffle(&self, shuffle: bool) {
        self.set("shuffle", "shuffle", json!(shuffle));
    }

    pub fn position(&self, secs: f64) {
        self.set("position", "position", json!(secs));
    }

    /// The playing song in Feishin's `QueueSong` shape (`null` = none).
    pub fn song(&self, song: Value, cover: Option<PathBuf>) {
        {
            let mut s = self.snap.lock().expect("remote state");
            s.cover = cover;
            if s.state.get("song").unwrap_or(&Value::Null) == &song {
                return;
            }
            s.state.insert("song".into(), song.clone());
        }
        self.broadcast("song", &song);
    }

    pub fn favorite(&self, id: &str, favorite: bool) {
        self.patch_song(id, "userFavorite", json!(favorite));
        self.broadcast("favorite", &json!({ "id": id, "favorite": favorite }));
    }

    pub fn rating(&self, id: &str, rating: u8) {
        self.patch_song(id, "userRating", json!(rating));
        self.broadcast("rating", &json!({ "id": id, "rating": rating }));
    }

    fn patch_song(&self, id: &str, key: &str, value: Value) {
        let mut s = self.snap.lock().expect("remote state");
        if let Some(Value::Object(song)) = s.state.get_mut("song")
            && song.get("id").and_then(Value::as_str) == Some(id)
        {
            song.insert(key.into(), value);
        }
    }

    pub fn queue(&self, current: Option<&str>, items: Vec<Value>) {
        let data = json!({ "currentUniqueId": current, "items": items });
        {
            let mut s = self.snap.lock().expect("remote state");
            if s.queue == data {
                return;
            }
            s.queue = data.clone();
        }
        self.broadcast("queue-state", &data);
    }

    pub fn radio(&self, status: Value) {
        {
            let mut s = self.snap.lock().expect("remote state");
            if s.radio == status {
                return;
            }
            s.radio = status.clone();
        }
        self.broadcast("radio-status", &status);
    }

    /// The desktop's primary colour (CSS) for the phone page's dark and light theme; sent on change.
    pub fn accent(&self, dark: &str, light: &str) {
        let data = json!({ "dark": dark, "light": light });
        {
            let mut s = self.snap.lock().expect("remote state");
            if s.accent.as_ref() == Some(&data) {
                return;
            }
            s.accent = Some(data.clone());
        }
        self.broadcast("accent-color", &data);
    }

    /// The messages a client gets once it is signed in (never before: they hold the queue).
    fn initial(&self) -> Vec<String> {
        let s = self.snap.lock().expect("remote state");
        let mut msgs = vec![
            ("state", Value::Object(s.state.clone())),
            ("queue-state", s.queue.clone()),
            ("radio-status", s.radio.clone()),
            ("confirm-queue-changes-setting", json!(false)),
        ];
        if let Some(accent) = &s.accent {
            msgs.push(("accent-color", accent.clone()));
        }
        msgs.into_iter().map(|(event, data)| json!({ "event": event, "data": data }).to_string()).collect()
    }

    fn cover(&self) -> Option<PathBuf> {
        self.snap.lock().expect("remote state").cover.clone()
    }
}

/// A running server; dropping it stops it.
pub struct Running {
    pub addr: SocketAddr,
    stop: watch::Sender<Option<u16>>,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "remote::Running({})", self.addr)
    }
}

impl Running {
    /// Closes every client with `code` and stops listening.
    pub fn shutdown(&self, code: u16) {
        let _ = self.stop.send(Some(code));
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.shutdown(CLOSE_DOWN);
    }
}

struct Ctx {
    cfg: Config,
    shared: Arc<Shared>,
    cmds: mpsc::UnboundedSender<Command>,
    stop: watch::Receiver<Option<u16>>,
}

/// Starts listening. Errors are the message to show (port in use, bad address).
pub async fn start(
    cfg: Config,
    shared: Arc<Shared>,
    cmds: mpsc::UnboundedSender<Command>,
) -> Result<Running, String> {
    let ip: std::net::IpAddr =
        cfg.bind.trim().parse().map_err(|_| format!("“{}” is not an IP address", cfg.bind))?;
    let listener = TcpListener::bind(SocketAddr::new(ip, cfg.port)).await.map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let (stop_tx, mut stop_rx) = watch::channel(None);
    let ctx = Arc::new(Ctx { cfg, shared, cmds, stop: stop_rx.clone() });
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_CLIENTS));
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = stop_rx.changed() => break,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let Ok(permit) = slots.clone().try_acquire_owned() else { continue };
                    let ctx = ctx.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        if let Err(e) = connection(stream, ctx).await {
                            tracing::debug!("remote: {e}");
                        }
                    });
                }
            }
        }
    });
    tracing::info!("remote control listening on {addr}");
    Ok(Running { addr, stop: stop_tx })
}

pub(crate) struct Head {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

pub(crate) async fn read_head(stream: &mut TcpStream) -> Result<(Head, Vec<u8>), String> {
    let mut buf = Vec::new();
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let text = String::from_utf8_lossy(&buf[..end]).into_owned();
            let rest = buf[end + 4..].to_vec();
            let mut lines = text.split("\r\n");
            let mut first = lines.next().unwrap_or("").split(' ');
            let method = first.next().unwrap_or("").to_string();
            let path = first.next().unwrap_or("/").to_string();
            let headers = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect();
            return Ok((Head { method, path, headers }, rest));
        }
        if buf.len() > MAX_HEAD {
            return Err("request head too large".into());
        }
        let mut chunk = [0u8; 2048];
        let n = stream.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("closed".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

pub(crate) async fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    extra: &[(&str, &str)],
    body: &[u8],
    head_only: bool,
) -> std::io::Result<()> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\
         X-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\n",
        body.len()
    );
    for (k, v) in extra {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    stream.write_all(out.as_bytes()).await?;
    if !head_only {
        stream.write_all(body).await?;
    }
    stream.flush().await
}

fn image_type(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xff, 0xd8, ..] => "image/jpeg",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        _ => "application/octet-stream",
    }
}

async fn connection(mut stream: TcpStream, ctx: Arc<Ctx>) -> Result<(), String> {
    let (head, rest) = tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut stream))
        .await
        .map_err(|_| "request timed out".to_string())??;
    let authorization = head.header("authorization").map(str::to_string);
    let authed = ctx.cfg.authorized(authorization.as_deref());
    let is_ws = head.header("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    if is_ws {
        let Some(key) = head.header("sec-websocket-key") else {
            respond(&mut stream, "400 Bad Request", "text/plain", &[], b"Bad Request", false)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(());
        };
        let reply = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\r\n",
            ws::accept_key(key)
        );
        stream.write_all(reply.as_bytes()).await.map_err(|e| e.to_string())?;
        return socket(stream, ctx, authed, rest).await;
    }
    let result = if !authed {
        respond(
            &mut stream,
            "401 Unauthorized",
            "text/plain",
            &[("WWW-Authenticate", "Basic realm=\"Listen remote\"")],
            b"Authorization required",
            false,
        )
        .await
    } else {
        let head_only = head.method == "HEAD";
        if head.method != "GET" && !head_only {
            respond(&mut stream, "405 Method Not Allowed", "text/plain", &[], b"Method Not Allowed", false)
                .await
        } else {
            let path = head.path.split(['?', '#']).next().unwrap_or("/");
            match path {
                "/credentials" => {
                    respond(
                        &mut stream,
                        "200 OK",
                        "text/plain",
                        &[],
                        authorization.as_deref().unwrap_or("").as_bytes(),
                        head_only,
                    )
                    .await
                }
                "/manifest.json" => {
                    respond(&mut stream, "200 OK", "application/json", &[], MANIFEST.as_bytes(), head_only)
                        .await
                }
                "/favicon.ico" => {
                    respond(&mut stream, "204 No Content", "image/x-icon", &[], b"", head_only).await
                }
                "/cover" => match ctx.shared.cover() {
                    Some(file) => match tokio::fs::read(&file).await {
                        Ok(bytes) => {
                            respond(&mut stream, "200 OK", image_type(&bytes), &[], &bytes, head_only).await
                        }
                        Err(_) => {
                            respond(&mut stream, "404 Not Found", "text/plain", &[], b"No cover", head_only)
                                .await
                        }
                    },
                    None => {
                        respond(&mut stream, "404 Not Found", "text/plain", &[], b"No cover", head_only).await
                    }
                },
                _ => {
                    // Every other path is the page (its router lives in the URL fragment).
                    respond(
                        &mut stream,
                        "200 OK",
                        "text/html; charset=UTF-8",
                        &[("Content-Security-Policy", "default-src 'self' 'unsafe-inline' data: ws: wss:")],
                        PAGE.as_bytes(),
                        head_only,
                    )
                    .await
                }
            }
        }
    };
    result.map_err(|e| e.to_string())
}

/// Reads a number that may arrive as JSON number or string.
fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// One client message to a command (`None` = unknown or malformed).
fn parse_client(msg: &Value, reply: &Reply) -> Option<Command> {
    let text = |k: &str| msg.get(k).and_then(Value::as_str).map(str::to_string);
    let request_id = text("requestId");
    let browse = |kind| {
        Some(Command::Browse {
            kind,
            search: text("searchTerm").filter(|s| !s.is_empty()),
            start: msg.get("startIndex").and_then(num).map_or(0, |n| n.max(0.0) as usize),
            limit: msg.get("limit").and_then(num).map_or(50, |n| (n.max(1.0) as usize).min(500)),
            request_id: request_id.clone()?,
            reply: reply.clone(),
        })
    };
    let play = |what| {
        Some(Command::PlayItem {
            what,
            id: text("id")?,
            play_type: text("playType"),
            request_id: request_id.clone(),
            reply: reply.clone(),
        })
    };
    Some(match msg.get("event")?.as_str()? {
        "play" => Command::Play,
        "pause" => Command::Pause,
        "next" => Command::Next,
        "previous" => Command::Previous,
        "repeat" => Command::ToggleRepeat,
        "shuffle" => Command::ToggleShuffle,
        "position" => Command::Seek(msg.get("position").and_then(num)?.max(0.0)),
        "volume" => Command::Volume(msg.get("volume").and_then(num)?.clamp(0.0, 100.0).round() as u8),
        "favorite" => Command::Favorite { id: text("id")?, favorite: msg.get("favorite")?.as_bool()? },
        "rating" => Command::Rating {
            id: text("id")?,
            rating: msg.get("rating").and_then(num)?.clamp(0.0, 5.0) as u8,
        },
        "queue-jump" => Command::QueueJump(text("uniqueId")?),
        "remove-from-queue" => Command::RemoveFromQueue(text("uniqueId")?),
        "reorder-queue" => Command::ReorderQueue {
            unique_id: text("uniqueId")?,
            target: text("targetUniqueId")?,
            top: text("edge")? == "top",
        },
        "clear-queue" => {
            if let Some(id) = &request_id {
                reply.ack(id, None);
            }
            Command::ClearQueue
        }
        "albums-request" => browse(Browse::Albums)?,
        "playlists-request" => browse(Browse::Playlists)?,
        "tracks-request" => browse(Browse::Tracks)?,
        "radio-request" => browse(Browse::Radio)?,
        "play-album" => play(PlayWhat::Album)?,
        "play-playlist" => play(PlayWhat::Playlist)?,
        "play-track" => play(PlayWhat::Track)?,
        "play-radio" => play(PlayWhat::Radio)?,
        "play-track-radio" => play(PlayWhat::TrackRadio)?,
        _ => return None,
    })
}

async fn socket(
    mut stream: TcpStream,
    ctx: Arc<Ctx>,
    mut authed: bool,
    mut buf: Vec<u8>,
) -> Result<(), String> {
    let mut events = ctx.shared.tx.subscribe();
    let (reply_tx, mut replies) = mpsc::unbounded_channel::<String>();
    let reply = Reply(reply_tx);
    let mut stop = ctx.stop.clone();
    let mut asm = ws::Assembler::default();
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    let mut alive = true;
    let auth_deadline = tokio::time::sleep(AUTH_TIMEOUT);
    tokio::pin!(auth_deadline);
    let mut chunk = vec![0u8; 8192];

    async fn send(stream: &mut TcpStream, bytes: &[u8]) -> Result<(), String> {
        stream.write_all(bytes).await.map_err(|e| e.to_string())
    }

    if authed {
        for m in ctx.shared.initial() {
            send(&mut stream, &ws::text(&m)).await?;
        }
    }
    loop {
        // Frames that arrived with the handshake or in one read.
        loop {
            let frame = match ws::parse(&mut buf) {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(_) => {
                    let _ = send(&mut stream, &ws::close(1002, "protocol error")).await;
                    return Ok(());
                }
            };
            let event = match asm.push(frame) {
                Ok(Some(e)) => e,
                Ok(None) => continue,
                Err(_) => {
                    let _ = send(&mut stream, &ws::close(1002, "protocol error")).await;
                    return Ok(());
                }
            };
            match event {
                ws::Event::Ping(p) => send(&mut stream, &ws::encode(ws::Opcode::Pong, &p)).await?,
                ws::Event::Pong => alive = true,
                ws::Event::Close => {
                    let _ = send(&mut stream, &ws::close(1000, "")).await;
                    return Ok(());
                }
                ws::Event::Message(text) => {
                    let Ok(msg) = serde_json::from_str::<Value>(&text) else { continue };
                    let event = msg.get("event").and_then(Value::as_str).unwrap_or("");
                    if !authed {
                        if event != "authenticate" {
                            continue;
                        }
                        let header = msg.get("header").and_then(Value::as_str);
                        if ctx.cfg.authorized(header) {
                            authed = true;
                            for m in ctx.shared.initial() {
                                send(&mut stream, &ws::text(&m)).await?;
                            }
                        } else {
                            let _ = send(&mut stream, &ws::close(CLOSE_AUTH, "Invalid credentials")).await;
                            return Ok(());
                        }
                        continue;
                    }
                    if event == "proxy" {
                        if let Some(file) = ctx.shared.cover() {
                            match tokio::fs::read(&file).await {
                                Ok(bytes) => {
                                    use base64::Engine;
                                    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                                    reply.send("proxy", json!(data));
                                }
                                Err(e) => reply.error(&e.to_string()),
                            }
                        }
                        continue;
                    }
                    if let Some(cmd) = parse_client(&msg, &reply) {
                        let _ = ctx.cmds.send(cmd);
                    }
                }
            }
        }
        tokio::select! {
            _ = stop.changed() => {
                let code = (*stop.borrow()).unwrap_or(CLOSE_DOWN);
                let _ = send(&mut stream, &ws::close(code, "")).await;
                return Ok(());
            }
            _ = &mut auth_deadline, if !authed => {
                let _ = send(&mut stream, &ws::close(CLOSE_AUTH, "Authentication timed out")).await;
                return Ok(());
            }
            _ = ping.tick() => {
                if !alive {
                    return Err("client missed a heartbeat".into());
                }
                alive = false;
                send(&mut stream, &ws::encode(ws::Opcode::Ping, b"")).await?;
            }
            Some(m) = replies.recv() => {
                if authed {
                    send(&mut stream, &ws::text(&m)).await?;
                }
            }
            event = events.recv() => match event {
                Ok(m) => {
                    if authed {
                        send(&mut stream, &ws::text(&m)).await?;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if authed {
                        for m in ctx.shared.initial() {
                            send(&mut stream, &ws::text(&m)).await?;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            read = stream.read(&mut chunk) => {
                let n = read.map_err(|e| e.to_string())?;
                if n == 0 {
                    return Ok(());
                }
                buf.extend_from_slice(&chunk[..n]);
            }
        }
    }
}
