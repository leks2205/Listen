//! A child `mpv` process controlled through its JSON IPC socket
//! (<https://mpv.io/manual/stable/#json-ipc>), the same approach Feishin uses.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc, oneshot};

#[derive(Debug, thiserror::Error)]
pub enum MpvError {
    #[error("could not start mpv ({0}); is the `mpv` package installed?")]
    Spawn(std::io::Error),
    #[error("could not connect to mpv's IPC socket: {0}")]
    Connect(std::io::Error),
    #[error("mpv IPC error: {0}")]
    Io(#[from] std::io::Error),
    #[error("mpv command failed: {0}")]
    Command(String),
    #[error("mpv exited")]
    Closed,
}

/// Raw events from mpv (property changes and player events).
#[derive(Debug, Clone, PartialEq)]
pub enum MpvEvent {
    Property {
        name: String,
        value: Value,
    },
    /// `end-file` with its reason (`eof`, `stop`, `quit`, `error`, `redirect`).
    EndFile {
        reason: String,
        error: Option<String>,
        playlist_entry_id: Option<i64>,
    },
    StartFile {
        playlist_entry_id: Option<i64>,
    },
    FileLoaded,
    /// `playback-restart`: a load or seek finished; audio runs (or is ready, when paused).
    PlaybackRestart,
    Idle,
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct MpvOptions {
    /// Path or name of the mpv binary.
    pub binary: PathBuf,
    /// Extra `--key=value` arguments (Feishin's "mpv extra parameters").
    pub extra_args: Vec<String>,
    /// Use `--ao=null` (tests, headless CI).
    pub null_audio: bool,
    /// mpv's own detailed log (`--log-file`), for diagnosing failed streams.
    pub log_file: Option<PathBuf>,
    /// Name of the audio stream (PipeWire node); the visualizer looks for `Listen`.
    pub client_name: String,
    /// mpv `--audio-buffer` in seconds (mpv's default is 0.2). Decks that must line up use the same value so
    /// their output and unpause latencies match.
    pub audio_buffer: Option<f64>,
    /// Decode video (off by default: Listen's decks are audio only).
    pub video: bool,
}

impl Default for MpvOptions {
    fn default() -> Self {
        Self {
            binary: "mpv".into(),
            extra_args: Vec::new(),
            null_audio: false,
            log_file: None,
            client_name: "Listen".into(),
            audio_buffer: None,
            video: false,
        }
    }
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, MpvError>>>>>;

/// Handle to a running mpv. Dropping it kills the process.
pub struct Mpv {
    /// `None` for an inert handle (tests): no process, every command fails.
    child: Option<Child>,
    writer: Arc<Mutex<Box<dyn tokio::io::AsyncWrite + Send + Unpin>>>,
    pending: Pending,
    next_id: AtomicU64,
    /// The IPC socket file, removed when the handle is dropped.
    socket: Option<PathBuf>,
}

/// Properties observed right after start-up.
pub const OBSERVED: &[&str] = &[
    "time-pos",
    "duration",
    "pause",
    "playlist-pos",
    "idle-active",
    "volume",
    "mute",
    "audio-codec-name",
    "metadata",
];

impl Mpv {
    pub async fn spawn(opts: &MpvOptions) -> Result<(Self, mpsc::UnboundedReceiver<MpvEvent>), MpvError> {
        remove_stale_sockets();
        let socket = ipc_path();
        let mut args = vec![
            "--idle=yes".to_string(),
            "--no-config".into(),
            "--load-scripts=no".into(),
            "--audio-display=no".into(),
            "--terminal=no".into(),
            // Names the PipeWire/Pulse stream so the visualizer can capture exactly this app.
            format!("--audio-client-name={}", opts.client_name),
            "--prefetch-playlist=yes".into(),
            // Listen resolves streams itself; mpv's yt-dlp hook would only retry failed URLs slowly.
            "--ytdl=no".into(),
            "--network-timeout=10".into(),
            "--stream-lavf-o=reconnect=1,reconnect_streamed=1,reconnect_delay_max=5".into(),
            format!("--input-ipc-server={}", socket.display()),
        ];
        args.push(if opts.video { "--vid=auto" } else { "--no-video" }.into());
        // Windows mpv registers its own media session (SMTC) with the stream's file
        // name; Discord and the volume flyout then show "mpv" instead of Listen's.
        if cfg!(windows) {
            args.push("--media-controls=no".into());
        }
        if opts.null_audio {
            args.push("--ao=null".into());
        }
        if let Some(buffer) = opts.audio_buffer {
            args.push(format!("--audio-buffer={buffer}"));
        }
        if let Some(log) = &opts.log_file {
            args.push(format!("--log-file={}", log.display()));
        }
        if cfg!(windows) && !opts.video {
            args.extend(windows_audio_defaults(&opts.extra_args));
        }
        args.extend(opts.extra_args.iter().cloned());

        let mut cmd = Command::new(&opts.binary);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        let child = cmd
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(MpvError::Spawn)?;

        let (reader, writer) = connect(&socket).await?;
        let pending: Pending = Arc::default();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        tokio::spawn(read_loop(reader, pending.clone(), events_tx));

        let mpv = Self {
            child: Some(child),
            writer: Arc::new(Mutex::new(writer)),
            pending,
            next_id: AtomicU64::new(1),
            socket: Some(socket),
        };
        for (i, name) in OBSERVED.iter().enumerate() {
            mpv.command(json!(["observe_property", i + 1, name])).await?;
        }
        Ok((mpv, events_rx))
    }

    /// A handle without a process: lets app tests put "running" decks in
    /// place without mpv. Every command fails with [`MpvError::Closed`].
    #[doc(hidden)]
    pub fn inert() -> Self {
        Self {
            child: None,
            writer: Arc::new(Mutex::new(Box::new(tokio::io::sink()))),
            pending: Pending::default(),
            next_id: AtomicU64::new(1),
            socket: None,
        }
    }

    /// Sends a command array and waits for mpv's reply.
    pub async fn command(&self, command: Value) -> Result<Value, MpvError> {
        if self.child.is_none() {
            return Err(MpvError::Closed);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let mut line =
            serde_json::to_vec(&json!({ "command": command, "request_id": id })).expect("json serializes");
        line.push(b'\n');
        {
            let mut w = self.writer.lock().await;
            w.write_all(&line).await?;
            w.flush().await?;
        }
        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(MpvError::Closed),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(MpvError::Command("timed out".into()))
            }
        }
    }

    pub async fn set_property(&self, name: &str, value: Value) -> Result<(), MpvError> {
        self.command(json!(["set_property", name, value])).await.map(|_| ())
    }

    pub async fn get_property(&self, name: &str) -> Result<Value, MpvError> {
        self.command(json!(["get_property", name])).await
    }

    /// `loadfile` in `replace` or `append` mode; `options` become per-file options.
    pub async fn loadfile(
        &self,
        url: &str,
        append: bool,
        options: &[(String, String)],
    ) -> Result<(), MpvError> {
        let mode = if append { "append" } else { "replace" };
        let cmd = if options.is_empty() {
            json!(["loadfile", url, mode])
        } else {
            let opts: serde_json::Map<String, Value> =
                options.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
            // Named arguments, so it works whether or not mpv knows the `index`
            // parameter (added in 0.38; 0.37 rejects it as "invalid parameter").
            json!({ "name": "loadfile", "url": url, "flags": mode, "options": opts })
        };
        self.command(cmd).await.map(|_| ())
    }

    /// Whether the process is still alive.
    pub fn is_running(&mut self) -> bool {
        self.child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    pub async fn quit(mut self) {
        let _ = self.command(json!(["quit"])).await;
        if let Some(child) = self.child.as_mut() {
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        // mpv is killed with the handle and does not get to remove its socket.
        if let Some(socket) = &self.socket {
            let _ = std::fs::remove_file(socket);
        }
    }
}

/// Arguments that keep the audio-only mpv from opening a video window or output
/// on Windows; the ones the user's extra arguments already carry are left out.
fn windows_audio_defaults(extra: &[String]) -> Vec<String> {
    let has = |key: &str| {
        extra.iter().any(|a| {
            let a = a.trim().trim_start_matches('-');
            a == key || a.strip_prefix(key).is_some_and(|rest| rest.starts_with('='))
        })
    };
    let mut out = Vec::new();
    if !has("no-video") && !has("video") && !has("vid") {
        out.push("--no-video".to_string());
    }
    if !has("force-window") {
        out.push("--force-window=no".into());
    }
    if !has("vo") {
        out.push("--vo=null".into());
    }
    out
}

/// `CREATE_NO_WINDOW`: spawned helpers must not flash a console window on Windows.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(unix)]
fn socket_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// The process id in `listen-mpv-<pid>-<n>.sock`.
#[cfg(unix)]
fn socket_pid(name: &str) -> Option<u32> {
    name.strip_prefix("listen-mpv-")?.strip_suffix(".sock")?.split('-').next()?.parse().ok()
}

/// Deletes sockets of Listen processes that no longer run (crashes, kills), once per start.
/// (Windows named pipes vanish with their process.)
#[cfg(not(unix))]
fn remove_stale_sockets() {}

#[cfg(unix)]
fn remove_stale_sockets() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        let Ok(entries) = std::fs::read_dir(socket_dir()) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(socket_pid) else { continue };
            if pid != std::process::id() && !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    });
}

fn ipc_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    #[cfg(unix)]
    {
        socket_dir().join(format!("listen-mpv-{}-{n}.sock", std::process::id()))
    }
    #[cfg(windows)]
    {
        PathBuf::from(format!(r"\\.\pipe\listen-mpv-{}-{n}", std::process::id()))
    }
}

type Reader = Box<dyn tokio::io::AsyncRead + Send + Unpin>;
type Writer = Box<dyn tokio::io::AsyncWrite + Send + Unpin>;

/// mpv creates the socket asynchronously after start; retry for a while.
async fn connect(path: &std::path::Path) -> Result<(Reader, Writer), MpvError> {
    let mut last_err = None;
    // Up to 15 s: a cold mpv start on a busy machine can take a while.
    for _ in 0..300 {
        #[cfg(unix)]
        match tokio::net::UnixStream::connect(path).await {
            Ok(stream) => {
                let (r, w) = stream.into_split();
                return Ok((Box::new(r), Box::new(w)));
            }
            Err(e) => last_err = Some(e),
        }
        #[cfg(windows)]
        match tokio::net::windows::named_pipe::ClientOptions::new().open(path) {
            Ok(pipe) => {
                let (r, w) = tokio::io::split(pipe);
                return Ok((Box::new(r), Box::new(w)));
            }
            Err(e) => last_err = Some(e),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(MpvError::Connect(last_err.unwrap_or_else(|| std::io::Error::other("timeout"))))
}

async fn read_loop(reader: Reader, pending: Pending, events: mpsc::UnboundedSender<MpvEvent>) {
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        // Bytes, not `lines()`: one non-UTF-8 line (a tag) must not end the loop.
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            tracing::warn!("unparseable mpv message: {line}");
            continue;
        };
        if let Some(id) = msg.get("request_id").and_then(Value::as_u64) {
            if let Some(tx) = pending.lock().await.remove(&id) {
                let result = match msg["error"].as_str() {
                    Some("success") => Ok(msg["data"].clone()),
                    Some(err) => Err(MpvError::Command(err.to_string())),
                    None => Ok(Value::Null),
                };
                let _ = tx.send(result);
            }
            continue;
        }
        if let Some(event) = parse_event(&msg)
            && events.send(event).is_err()
        {
            break;
        }
    }
    // Wake everyone still waiting.
    for (_, tx) in pending.lock().await.drain() {
        let _ = tx.send(Err(MpvError::Closed));
    }
    let _ = events.send(MpvEvent::Shutdown);
}

pub fn parse_event(msg: &Value) -> Option<MpvEvent> {
    let entry = || msg.get("playlist_entry_id").and_then(Value::as_i64);
    Some(match msg.get("event")?.as_str()? {
        "property-change" => MpvEvent::Property {
            name: msg["name"].as_str()?.to_string(),
            value: msg.get("data").cloned().unwrap_or(Value::Null),
        },
        "end-file" => MpvEvent::EndFile {
            reason: msg["reason"].as_str().unwrap_or("unknown").to_string(),
            error: msg["file_error"].as_str().map(String::from),
            playlist_entry_id: entry(),
        },
        "start-file" => MpvEvent::StartFile { playlist_entry_id: entry() },
        "file-loaded" => MpvEvent::FileLoaded,
        "playback-restart" => MpvEvent::PlaybackRestart,
        "idle" => MpvEvent::Idle,
        "shutdown" => MpvEvent::Shutdown,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn socket_names_carry_the_pid() {
        assert_eq!(socket_pid("listen-mpv-1234-7.sock"), Some(1234));
        assert_eq!(socket_pid("other.sock"), None);
    }

    #[test]
    fn windows_defaults_skip_what_the_user_set() {
        let none = windows_audio_defaults(&[]);
        assert_eq!(none, ["--no-video", "--force-window=no", "--vo=null"]);
        let user = ["--vo=gpu".to_string(), "force-window=yes".to_string()];
        assert_eq!(windows_audio_defaults(&user), ["--no-video"]);
        let all = ["--no-video".into(), "--force-window=no".into(), "--vo=null".into()];
        assert!(windows_audio_defaults(&all).is_empty());
    }

    #[tokio::test]
    async fn reader_survives_invalid_utf8_and_split_lines() {
        let (mut tx, rx) = tokio::io::duplex(64);
        let (events, mut out) = mpsc::unbounded_channel();
        tokio::spawn(read_loop(Box::new(rx), Pending::default(), events));
        tx.write_all(b"{\"event\":\"idle\"}\r\n{\"event\":\"x\",\"t\":\"\xff\"}\n{\"even").await.unwrap();
        tx.write_all(b"t\":\"file-loaded\"}\n").await.unwrap();
        drop(tx);
        assert_eq!(out.recv().await, Some(MpvEvent::Idle));
        assert_eq!(out.recv().await, Some(MpvEvent::FileLoaded));
        assert_eq!(out.recv().await, Some(MpvEvent::Shutdown));
    }

    #[test]
    fn parses_events() {
        let e = parse_event(&json!({"event": "property-change", "id": 1, "name": "time-pos", "data": 1.5}));
        assert_eq!(e, Some(MpvEvent::Property { name: "time-pos".into(), value: json!(1.5) }));
        let e = parse_event(&json!({"event": "end-file", "reason": "eof", "playlist_entry_id": 3}));
        assert_eq!(
            e,
            Some(MpvEvent::EndFile { reason: "eof".into(), error: None, playlist_entry_id: Some(3) })
        );
        assert_eq!(parse_event(&json!({"event": "playback-restart"})), Some(MpvEvent::PlaybackRestart));
        assert_eq!(parse_event(&json!({"event": "audio-reconfig"})), None);
        assert_eq!(parse_event(&json!({"request_id": 1})), None);
    }
}
