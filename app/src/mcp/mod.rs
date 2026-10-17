//! MCP server: lets an AI assistant on this computer (or the LAN, when allowed) control Listen.
//! Streamable HTTP, JSON responses only: one JSON-RPC 2.0 message per `POST /mcp`, each with
//! `Authorization: Bearer <token>`. `initialize`, `ping` and `tools/list` are answered here;
//! `tools/call` goes to the app as a [`Call`].

pub mod tools;

use crate::remote::{constant_eq, read_head, respond};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};

/// The protocol revision this server speaks (older clients get theirs echoed).
pub const PROTOCOL: &str = "2025-06-18";
const SUPPORTED: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_BODY: usize = 1024 * 1024;
const MAX_CLIENTS: usize = 16;
const BODY_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a tool call may take in the app (searches, playlist edits).
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// `127.0.0.1`, or `0.0.0.0` when the local network is allowed.
    pub bind: String,
    pub port: u16,
    pub token: String,
}

impl Config {
    /// Whether an `Authorization` header carries the token.
    pub fn authorized(&self, header: Option<&str>) -> bool {
        let Some(given) = header.and_then(|h| h.trim().strip_prefix("Bearer ")) else { return false };
        !self.token.is_empty() && constant_eq(given.trim().as_bytes(), self.token.as_bytes())
    }
}

/// A tool call for the app to carry out.
#[derive(Debug, Clone)]
pub struct Call {
    pub tool: String,
    pub args: Value,
    pub reply: Reply,
}

/// What a tool call returns: a JSON value, or why it failed.
pub type Outcome = Result<Value, String>;

/// Where the app's answer to a [`Call`] goes (once).
#[derive(Debug, Clone)]
pub struct Reply(Arc<Mutex<Option<oneshot::Sender<Outcome>>>>);

impl Reply {
    pub fn new() -> (Self, oneshot::Receiver<Result<Value, String>>) {
        let (tx, rx) = oneshot::channel();
        (Self(Arc::new(Mutex::new(Some(tx)))), rx)
    }

    pub fn send(&self, result: Result<Value, String>) {
        if let Some(tx) = self.0.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(result);
        }
    }

    pub fn ok(&self, value: Value) {
        self.send(Ok(value));
    }

    pub fn err(&self, message: impl Into<String>) {
        self.send(Err(message.into()));
    }
}

/// A running server; dropping it stops it.
pub struct Running {
    pub addr: SocketAddr,
    stop: watch::Sender<bool>,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mcp::Running({})", self.addr)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

struct Ctx {
    cfg: Config,
    calls: mpsc::UnboundedSender<Call>,
}

/// Starts listening. Errors are the message to show (port in use, bad address).
pub async fn start(cfg: Config, calls: mpsc::UnboundedSender<Call>) -> Result<Running, String> {
    let ip: std::net::IpAddr =
        cfg.bind.trim().parse().map_err(|_| format!("“{}” is not an IP address", cfg.bind))?;
    let listener = TcpListener::bind(SocketAddr::new(ip, cfg.port)).await.map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let ctx = Arc::new(Ctx { cfg, calls });
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
                            tracing::debug!("mcp: {e}");
                        }
                    });
                }
            }
        }
    });
    tracing::info!("MCP server listening on {addr}");
    Ok(Running { addr, stop: stop_tx })
}

async fn read_body(stream: &mut TcpStream, mut buf: Vec<u8>, len: usize) -> Result<Vec<u8>, String> {
    while buf.len() < len {
        let mut chunk = [0u8; 8192];
        let n = tokio::time::timeout(BODY_TIMEOUT, stream.read(&mut chunk))
            .await
            .map_err(|_| "body timed out".to_string())?
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("closed".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    buf.truncate(len);
    Ok(buf)
}

/// Browsers may only reach the server from pages on this computer (DNS rebinding).
fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else { return true };
    let host = origin.split("://").nth(1).unwrap_or(origin);
    let host = host.split(['/', ':']).next().unwrap_or("");
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

async fn connection(mut stream: TcpStream, ctx: Arc<Ctx>) -> Result<(), String> {
    let (head, rest) = read_head(&mut stream).await?;
    let json_type = "application/json";
    let path = head.path.split('?').next().unwrap_or("");
    let send = async |stream: &mut TcpStream, status: &str, body: &[u8]| {
        respond(stream, status, json_type, &[], body, false).await.map_err(|e| e.to_string())
    };
    if path != "/mcp" {
        return send(&mut stream, "404 Not Found", br#"{"error":"not found"}"#).await;
    }
    if !origin_allowed(head.header("Origin")) {
        return send(&mut stream, "403 Forbidden", br#"{"error":"origin not allowed"}"#).await;
    }
    if !ctx.cfg.authorized(head.header("Authorization")) {
        return respond(
            &mut stream,
            "401 Unauthorized",
            json_type,
            &[("WWW-Authenticate", "Bearer")],
            br#"{"error":"missing or wrong token"}"#,
            false,
        )
        .await
        .map_err(|e| e.to_string());
    }
    match head.method.as_str() {
        "POST" => {}
        // No server-sent stream and no sessions: GET and DELETE are not offered.
        _ => {
            return respond(
                &mut stream,
                "405 Method Not Allowed",
                json_type,
                &[("Allow", "POST")],
                br#"{"error":"use POST"}"#,
                false,
            )
            .await
            .map_err(|e| e.to_string());
        }
    }
    let len: usize = head.header("Content-Length").and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return send(&mut stream, "413 Payload Too Large", br#"{"error":"too large"}"#).await;
    }
    let body = read_body(&mut stream, rest, len).await?;
    let message: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            let answer = error(Value::Null, -32700, &format!("parse error: {e}"));
            return send(&mut stream, "200 OK", answer.to_string().as_bytes()).await;
        }
    };
    match handle(&message, &ctx.calls).await {
        Some(answer) => send(&mut stream, "200 OK", answer.to_string().as_bytes()).await,
        // A notification or a response: accepted, nothing to say.
        None => {
            respond(&mut stream, "202 Accepted", json_type, &[], b"", false).await.map_err(|e| e.to_string())
        }
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Answers one JSON-RPC message (`None` for notifications).
pub async fn handle(message: &Value, calls: &mpsc::UnboundedSender<Call>) -> Option<Value> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // A response to something we never asked, or garbage.
        return message.get("id").map(|id| error(id.clone(), -32600, "invalid request"));
    };
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL);
            let version = if SUPPORTED.contains(&asked) { asked } else { PROTOCOL };
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "listen", "title": "Listen", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": tools::INSTRUCTIONS,
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({ "tools": tools::definitions() })),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error(id, -32602, "missing tool name"));
            };
            if !tools::exists(name) {
                return Some(error(id, -32602, &format!("unknown tool: {name}")));
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            tracing::info!("mcp: {name}");
            let (reply, answer) = Reply::new();
            if calls.send(Call { tool: name.to_string(), args, reply }).is_err() {
                return Some(error(id, -32603, "Listen is shutting down"));
            }
            let outcome = match tokio::time::timeout(CALL_TIMEOUT, answer).await {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(_)) => Err("the call was dropped".to_string()),
                Err(_) => Err("timed out".to_string()),
            };
            let (text, is_error) = match outcome {
                Ok(value) => (serde_json::to_string_pretty(&value).unwrap_or_default(), false),
                Err(e) => (e, true),
            };
            result(id, json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        _ => error(id, -32601, &format!("method not found: {method}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calls() -> (mpsc::UnboundedSender<Call>, mpsc::UnboundedReceiver<Call>) {
        mpsc::unbounded_channel()
    }

    #[tokio::test]
    async fn initialize_ping_and_list() {
        let (tx, _rx) = calls();
        let init =
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}});
        let r = handle(&init, &tx).await.unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "listen");
        let r =
            handle(&json!({"jsonrpc":"2.0","id":"x","method":"initialize","params":{}}), &tx).await.unwrap();
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL);
        assert!(handle(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}), &tx).await.is_none());
        let r = handle(&json!({"jsonrpc":"2.0","id":2,"method":"ping"}), &tx).await.unwrap();
        assert_eq!(r["result"], json!({}));
        let r = handle(&json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}), &tx).await.unwrap();
        let names: Vec<&str> =
            r["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        for want in
            ["get_now_playing", "play_songs", "add_to_playlist", "set_ai_dj", "set_settings", "search"]
        {
            assert!(names.contains(&want), "{want} missing");
        }
        let r = handle(&json!({"jsonrpc":"2.0","id":4,"method":"nope"}), &tx).await.unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn calls_go_to_the_app_and_unknown_tools_are_refused() {
        let (tx, mut rx) = calls();
        let r = handle(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nope"}}), &tx)
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], -32602);
        let app = tokio::spawn(async move {
            let call = rx.recv().await.unwrap();
            assert_eq!(call.tool, "set_volume");
            assert_eq!(call.args["volume"], 40);
            call.reply.ok(json!({ "volume": 40 }));
            let call = rx.recv().await.unwrap();
            call.reply.err("no song is playing");
        });
        let call = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"set_volume","arguments":{"volume":40}}});
        let r = handle(&call, &tx).await.unwrap();
        assert_eq!(r["result"]["isError"], false);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("40"));
        let call = json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"next"}});
        let r = handle(&call, &tx).await.unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(r["result"]["content"][0]["text"], "no song is playing");
        app.await.unwrap();
    }

    #[test]
    fn token_and_origin_checks() {
        let cfg = Config { bind: "127.0.0.1".into(), port: 0, token: "abc".into() };
        assert!(cfg.authorized(Some("Bearer abc")));
        assert!(!cfg.authorized(Some("Bearer abd")));
        assert!(!cfg.authorized(Some("Basic abc")));
        assert!(!cfg.authorized(None));
        let empty = Config { token: String::new(), ..cfg };
        assert!(!empty.authorized(Some("Bearer ")));
        assert!(origin_allowed(None));
        assert!(origin_allowed(Some("http://localhost:3000")));
        assert!(origin_allowed(Some("http://127.0.0.1")));
        assert!(!origin_allowed(Some("https://evil.example")));
    }

    #[tokio::test]
    async fn the_server_wants_the_token() {
        use tokio::io::AsyncWriteExt;
        let (tx, _rx) = calls();
        let cfg = Config { bind: "127.0.0.1".into(), port: 0, token: "sesame".into() };
        let running = start(cfg, tx).await.unwrap();
        let post = async |auth: &str| {
            let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
            let mut s = TcpStream::connect(running.addr).await.unwrap();
            let req = format!(
                "POST /mcp HTTP/1.1\r\nHost: x\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            s.write_all(req.as_bytes()).await.unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        };
        assert!(post("").await.starts_with("HTTP/1.1 401"));
        assert!(post("Authorization: Bearer wrong\r\n").await.starts_with("HTTP/1.1 401"));
        let ok = post("Authorization: Bearer sesame\r\n").await;
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(ok.contains(r#""result":{}"#), "{ok}");
    }
}
