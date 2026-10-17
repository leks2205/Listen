//! The remote control server: HTTP routes, basic-auth, and the WebSocket
//! message protocol (Feishin's `remote-types.ts`), with a real client.

use futures_util::{SinkExt, StreamExt};
use listen::remote::{self, Browse, Command, Config, PlayWhat, Shared};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as Ws;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

struct Server {
    addr: std::net::SocketAddr,
    shared: Arc<Shared>,
    cmds: mpsc::UnboundedReceiver<Command>,
    running: remote::Running,
}

async fn server(username: &str, password: &str) -> Server {
    let shared = Arc::new(Shared::default());
    let (tx, cmds) = mpsc::unbounded_channel();
    let cfg =
        Config { bind: "127.0.0.1".into(), port: 0, username: username.into(), password: password.into() };
    let running = remote::start(cfg, shared.clone(), tx).await.expect("starts");
    Server { addr: running.addr, shared, cmds, running }
}

fn basic(user: &str, pass: &str) -> String {
    use base64::Engine;
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}")))
}

/// One HTTP request; returns (status line, headers + body text).
async fn http(addr: std::net::SocketAddr, path: &str, auth: Option<&str>) -> (String, String) {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    let mut req = format!("GET {path} HTTP/1.1\r\nHost: x\r\n");
    if let Some(a) = auth {
        req.push_str(&format!("Authorization: {a}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).await.unwrap();
    let text = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    (head.lines().next().unwrap_or("").to_string(), format!("{head}\n{body}"))
}

type Client = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(addr: std::net::SocketAddr, auth: Option<&str>) -> Client {
    let mut req = format!("ws://{addr}/").into_client_request().unwrap();
    if let Some(a) = auth {
        req.headers_mut().insert("Authorization", a.parse().unwrap());
    }
    tokio_tungstenite::connect_async(req).await.expect("handshake").0
}

async fn next(c: &mut Client) -> Option<Ws> {
    tokio::time::timeout(Duration::from_secs(3), c.next()).await.ok().flatten().and_then(Result::ok)
}

async fn event(c: &mut Client) -> (String, Value) {
    loop {
        match next(c).await.expect("a message") {
            Ws::Text(t) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                return (v["event"].as_str().unwrap().to_string(), v["data"].clone());
            }
            Ws::Ping(_) | Ws::Pong(_) => continue,
            other => panic!("unexpected {other:?}"),
        }
    }
}

async fn initial(c: &mut Client) -> Vec<String> {
    let mut names = Vec::new();
    for _ in 0..4 {
        names.push(event(c).await.0);
    }
    names
}

async fn command(s: &mut Server) -> Command {
    tokio::time::timeout(Duration::from_secs(3), s.cmds.recv()).await.expect("a command").expect("open")
}

#[tokio::test]
async fn open_server_sends_the_state_and_follows_changes() {
    let s = server("", "").await;
    s.shared.status("playing");
    s.shared.volume(40);
    s.shared.song(json!({ "id": "s1", "name": "Song", "duration": 1000 }), None);
    s.shared.queue(Some("7"), vec![json!({ "uniqueId": "7", "name": "Song" })]);
    let mut c = connect(s.addr, None).await;
    // Feishin's order: state, queue, radio, confirm-queue-changes.
    let (e, d) = event(&mut c).await;
    assert_eq!(e, "state");
    assert_eq!(
        (d["status"].as_str(), d["volume"].as_u64(), d["song"]["name"].as_str()),
        (Some("playing"), Some(40), Some("Song"))
    );
    let (e, d) = event(&mut c).await;
    assert_eq!((e.as_str(), d["currentUniqueId"].as_str()), ("queue-state", Some("7")));
    assert_eq!(event(&mut c).await, ("radio-status".to_string(), json!({ "isActive": false })));
    assert_eq!(event(&mut c).await, ("confirm-queue-changes-setting".to_string(), json!(false)));

    s.shared.volume(55);
    assert_eq!(event(&mut c).await, ("volume".to_string(), json!(55)));
    s.shared.volume(55); // unchanged: nothing is sent
    s.shared.shuffle(true);
    assert_eq!(event(&mut c).await, ("shuffle".to_string(), json!(true)));
    s.shared.repeat("all");
    assert_eq!(event(&mut c).await, ("repeat".to_string(), json!("all")));
    s.shared.status("paused");
    assert_eq!(event(&mut c).await, ("playback".to_string(), json!("paused")));
    s.shared.position(12.0);
    assert_eq!(event(&mut c).await, ("position".to_string(), json!(12.0)));
    s.shared.favorite("s1", true);
    assert_eq!(event(&mut c).await, ("favorite".to_string(), json!({ "id": "s1", "favorite": true })));
    s.shared.rating("s1", 4);
    assert_eq!(event(&mut c).await, ("rating".to_string(), json!({ "id": "s1", "rating": 4 })));
    s.shared.song(Value::Null, None);
    assert_eq!(event(&mut c).await, ("song".to_string(), Value::Null));
    s.shared.radio(json!({ "isActive": true, "imageUrl": null, "stationName": "FM" }));
    assert_eq!(event(&mut c).await.0, "radio-status");
    // A second client gets the state as it is now.
    let mut c2 = connect(s.addr, None).await;
    let (_, d) = event(&mut c2).await;
    assert_eq!(
        (d["status"].as_str(), d["shuffle"].as_bool(), d["repeat"].as_str()),
        (Some("paused"), Some(true), Some("all"))
    );
}

#[tokio::test]
async fn client_messages_become_commands() {
    let mut s = server("", "").await;
    let mut c = connect(s.addr, None).await;
    initial(&mut c).await;
    for (msg, check) in [
        (json!({ "event": "play" }), "Play"),
        (json!({ "event": "pause" }), "Pause"),
        (json!({ "event": "next" }), "Next"),
        (json!({ "event": "previous" }), "Previous"),
        (json!({ "event": "shuffle" }), "ToggleShuffle"),
        (json!({ "event": "repeat" }), "ToggleRepeat"),
        (json!({ "event": "position", "position": 93.5 }), "Seek(93.5)"),
        (json!({ "event": "volume", "volume": 250 }), "Volume(100)"),
        (json!({ "event": "volume", "volume": "-3" }), "Volume(0)"),
        (json!({ "event": "queue-jump", "uniqueId": "9" }), "QueueJump(\"9\")"),
        (json!({ "event": "remove-from-queue", "uniqueId": "4" }), "RemoveFromQueue(\"4\")"),
    ] {
        c.send(Ws::text(msg.to_string())).await.unwrap();
        assert_eq!(format!("{:?}", command(&mut s).await), check);
    }
    c.send(Ws::text(json!({ "event": "favorite", "id": "s1", "favorite": true }).to_string())).await.unwrap();
    assert!(matches!(command(&mut s).await, Command::Favorite { id, favorite: true } if id == "s1"));
    c.send(Ws::text(json!({ "event": "rating", "id": "s1", "rating": 9 }).to_string())).await.unwrap();
    assert!(matches!(command(&mut s).await, Command::Rating { rating: 5, .. }));
    c.send(Ws::text(
        json!({ "event": "reorder-queue", "uniqueId": "1", "targetUniqueId": "2", "edge": "bottom" })
            .to_string(),
    ))
    .await
    .unwrap();
    assert!(matches!(command(&mut s).await, Command::ReorderQueue { top: false, .. }));
    // Malformed and unknown messages are ignored, and the connection stays up.
    for junk in ["not json", "{}", "{\"event\":\"warp\"}", "{\"event\":\"position\"}"] {
        c.send(Ws::text(junk)).await.unwrap();
    }
    c.send(Ws::text(json!({ "event": "next" }).to_string())).await.unwrap();
    assert!(matches!(command(&mut s).await, Command::Next));
}

#[tokio::test]
async fn clearing_the_queue_is_acknowledged_and_browsing_answers_the_asker_only() {
    let mut s = server("", "").await;
    let mut a = connect(s.addr, None).await;
    let mut b = connect(s.addr, None).await;
    initial(&mut a).await;
    initial(&mut b).await;
    a.send(Ws::text(json!({ "event": "clear-queue", "requestId": "r1" }).to_string())).await.unwrap();
    assert_eq!(
        event(&mut a).await,
        ("operation-ack".to_string(), json!({ "requestId": "r1", "error": null }))
    );
    assert!(matches!(command(&mut s).await, Command::ClearQueue));

    a.send(Ws::text(
        json!({ "event": "albums-request", "requestId": "q1", "searchTerm": "abbey", "startIndex": 10, "limit": 25 })
            .to_string(),
    ))
    .await
    .unwrap();
    let Command::Browse { kind, search, start, limit, request_id, reply } = command(&mut s).await else {
        panic!("browse")
    };
    assert_eq!(
        (kind, search.as_deref(), start, limit, request_id.as_str()),
        (Browse::Albums, Some("abbey"), 10, 25, "q1")
    );
    reply.list("albums-response", &request_id, true, vec![json!({ "id": "a1", "name": "Abbey Road" })]);
    let (e, d) = event(&mut a).await;
    assert_eq!(e, "albums-response");
    assert_eq!(
        (d["requestId"].as_str(), d["hasMore"].as_bool(), d["items"][0]["name"].as_str()),
        (Some("q1"), Some(true), Some("Abbey Road"))
    );
    // The other client got nothing.
    assert!(tokio::time::timeout(Duration::from_millis(300), b.next()).await.is_err());

    b.send(Ws::text(
        json!({ "event": "play-album", "id": "a1", "playType": "next", "requestId": "p1" }).to_string(),
    ))
    .await
    .unwrap();
    let Command::PlayItem { what, id, play_type, request_id, reply } = command(&mut s).await else {
        panic!("play")
    };
    assert_eq!(
        (what, id.as_str(), play_type.as_deref(), request_id.as_deref()),
        (PlayWhat::Album, "a1", Some("next"), Some("p1"))
    );
    reply.ack("p1", Some("No server"));
    assert_eq!(
        event(&mut b).await,
        ("operation-ack".to_string(), json!({ "requestId": "p1", "error": "No server" }))
    );
}

#[tokio::test]
async fn http_routes_and_basic_auth() {
    let s = server("alex", "s3cret").await;
    let good = basic("alex", "s3cret");
    let (status, body) = http(s.addr, "/", None).await;
    assert!(status.contains("401"), "{status}");
    assert!(body.contains("WWW-Authenticate: Basic"), "{body}");
    assert!(!body.contains("s3cret"));
    for wrong in [
        basic("alex", "wrong"),
        basic("other", "s3cret"),
        basic("", ""),
        "Basic !!!".to_string(),
        "Bearer x".to_string(),
    ] {
        assert!(http(s.addr, "/", Some(&wrong)).await.0.contains("401"), "{wrong}");
    }
    let (status, body) = http(s.addr, "/", Some(&good)).await;
    assert!(status.contains("200"), "{status}");
    assert!(body.contains("<title>Listen Remote</title>") && body.contains("new WebSocket"));
    // The page needs nothing from the internet.
    assert!(!body.contains("https://") && !body.contains("http://cdn"));
    let (_, body) = http(s.addr, "/credentials", Some(&good)).await;
    assert!(body.ends_with(&good), "the page signs its socket in with the header it was served with");
    assert!(http(s.addr, "/manifest.json", Some(&good)).await.1.contains("\"Listen Remote\""));
    assert!(http(s.addr, "/credentials", None).await.0.contains("401"));
    assert!(http(s.addr, "/cover", None).await.0.contains("401"));
    assert!(http(s.addr, "/cover", Some(&good)).await.0.contains("404"), "no cover yet");
    // Any other path is the page (single page app).
    assert!(http(s.addr, "/library?x=1", Some(&good)).await.1.contains("Listen Remote"));
}

#[tokio::test]
async fn cover_is_served_from_the_published_file_only() {
    let s = server("", "").await;
    let dir = std::env::temp_dir().join(format!("listen-remote-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("cover.png");
    let png = [0x89u8, b'P', b'N', b'G', 1, 2, 3];
    std::fs::write(&file, png).unwrap();
    s.shared.song(json!({ "id": "s1", "imageUrl": "/cover" }), Some(file.clone()));
    let mut conn = tokio::net::TcpStream::connect(s.addr).await.unwrap();
    conn.write_all(b"GET /cover HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
    let mut out = Vec::new();
    conn.read_to_end(&mut out).await.unwrap();
    assert!(out.starts_with(b"HTTP/1.1 200"));
    assert!(String::from_utf8_lossy(&out).contains("Content-Type: image/png"));
    assert!(out.ends_with(&png));
    // Path tricks do not reach other files.
    assert!(http(s.addr, "/cover/../../etc/passwd", None).await.1.contains("Listen Remote"));
    // The phone can also ask for the cover over the socket (Feishin's `proxy`).
    let mut c = connect(s.addr, None).await;
    initial(&mut c).await;
    c.send(Ws::text(json!({ "event": "proxy" }).to_string())).await.unwrap();
    let (e, d) = event(&mut c).await;
    assert_eq!(e, "proxy");
    use base64::Engine;
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(d.as_str().unwrap()).unwrap(), png);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn websocket_authentication() {
    let mut s = server("alex", "s3cret").await;
    s.shared.queue(Some("1"), vec![json!({ "uniqueId": "1", "name": "Private queue" })]);
    let good = basic("alex", "s3cret");

    // A header at the upgrade signs in at once.
    let mut c = connect(s.addr, Some(&good)).await;
    assert_eq!(initial(&mut c).await[0], "state");

    // Without one the socket gets nothing, and commands are ignored, until it authenticates.
    let mut c = connect(s.addr, None).await;
    s.shared.volume(30);
    c.send(Ws::text(json!({ "event": "next" }).to_string())).await.unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(300), c.next()).await.is_err(), "nothing before auth");
    assert!(s.cmds.try_recv().is_err(), "no command before auth");
    c.send(Ws::text(json!({ "event": "authenticate", "header": good }).to_string())).await.unwrap();
    let (e, d) = event(&mut c).await;
    assert_eq!((e.as_str(), d["volume"].as_u64()), ("state", Some(30)));
    event(&mut c).await;
    c.send(Ws::text(json!({ "event": "next" }).to_string())).await.unwrap();
    assert!(matches!(command(&mut s).await, Command::Next));

    // Wrong credentials: closed with 4004, and the queue never leaked.
    let mut bad = connect(s.addr, None).await;
    bad.send(Ws::text(json!({ "event": "authenticate", "header": basic("alex", "nope") }).to_string()))
        .await
        .unwrap();
    match next(&mut bad).await {
        Some(Ws::Close(Some(frame))) => assert_eq!(u16::from(frame.code), 4004),
        other => panic!("expected a close, got {other:?}"),
    }
}

#[tokio::test]
async fn shutdown_closes_clients_with_the_given_code() {
    let s = server("", "").await;
    let mut c = connect(s.addr, None).await;
    initial(&mut c).await;
    s.running.shutdown(remote::CLOSE_CHANGED);
    loop {
        match next(&mut c).await {
            Some(Ws::Close(Some(frame))) => {
                assert_eq!(u16::from(frame.code), 4002);
                break;
            }
            Some(_) => continue,
            None => panic!("closed without a code"),
        }
    }
    // The port is free again.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(tokio::net::TcpStream::connect(s.addr).await.is_err());
}

#[tokio::test]
async fn bad_addresses_and_busy_ports_are_reported() {
    let s = server("", "").await;
    let (tx, _rx) = mpsc::unbounded_channel();
    let busy = Config {
        bind: "127.0.0.1".into(),
        port: s.addr.port(),
        username: String::new(),
        password: String::new(),
    };
    let err = remote::start(busy, s.shared.clone(), tx.clone()).await.expect_err("port in use");
    assert!(err.to_lowercase().contains("in use"), "{err}");
    let bad = Config { bind: "not an ip".into(), port: 0, username: String::new(), password: String::new() };
    assert!(remote::start(bad, s.shared.clone(), tx).await.expect_err("bad address").contains("not an IP"));
}

#[tokio::test]
async fn accent_colour_is_sent_after_the_state_and_follows_changes() {
    let s = server("", "").await;
    s.shared.accent("#112233", "#445566");
    let mut c = connect(s.addr, None).await;
    // Feishin sends `accent-color` last, only once it is known.
    assert_eq!(
        initial(&mut c).await,
        ["state", "queue-state", "radio-status", "confirm-queue-changes-setting"]
    );
    assert_eq!(
        event(&mut c).await,
        ("accent-color".to_string(), json!({ "dark": "#112233", "light": "#445566" }))
    );
    s.shared.accent("#112233", "#445566"); // unchanged: nothing is sent
    s.shared.accent("#aabbcc", "#445566");
    assert_eq!(
        event(&mut c).await,
        ("accent-color".to_string(), json!({ "dark": "#aabbcc", "light": "#445566" }))
    );
}
