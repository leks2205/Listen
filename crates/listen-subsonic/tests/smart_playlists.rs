//! Smart-playlist calls against a fake Navidrome: login, create, update, read, list.

use listen_core::model::ServerId;
use listen_source::nsp::{Nsp, query::Query};
use listen_source::{Capabilities, MusicSource, SmartPlaylistDef, SourceError};
use listen_subsonic::Subsonic;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    auth: Option<String>,
    body: Value,
}

struct Fake {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

/// A Navidrome that answers the routes the smart-playlist calls use. `navidrome`: whether
/// the ping says so; `reject_first_token`: the first token is refused once (a 401).
async fn fake(navidrome: bool, reject_first_token: bool) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::<Seen>::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        let mut logins = 0;
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = Vec::new();
            let (head, body) = loop {
                let mut chunk = [0u8; 4096];
                let n = sock.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    break (String::new(), Vec::new());
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..end]).to_string();
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap_or(0))
                        })
                        .unwrap_or(0usize);
                    while buf.len() < end + 4 + len {
                        let n = sock.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    break (head, buf[end + 4..].to_vec());
                }
            };
            let mut first = head.lines().next().unwrap_or("").split(' ');
            let (method, target) =
                (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
            let path = target.split('?').next().unwrap_or("").to_string();
            let auth = head.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("x-nd-authorization").then(|| v.trim().to_string())
            });
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            log.lock().unwrap().push(Seen {
                method: method.clone(),
                path: path.clone(),
                auth: auth.clone(),
                body,
            });
            let (status, reply) = match (method.as_str(), path.as_str()) {
                (_, "/rest/ping") => (
                    200,
                    json!({"subsonic-response": {"status": "ok", "version": "1.16.1",
                        "type": if navidrome { "navidrome" } else { "gonic" }, "serverVersion": "0.55.1"}}),
                ),
                ("POST", "/auth/login") => {
                    logins += 1;
                    (200, json!({"token": format!("tok{logins}")}))
                }
                _ if reject_first_token && auth.as_deref() == Some("Bearer tok1") => (401, json!({})),
                ("POST", "/api/playlist") => (200, json!({"id": "new-pl"})),
                ("PUT", "/api/playlist/pl-9") => (200, json!({"id": "pl-9"})),
                ("GET", "/api/playlist/pl-9") => (
                    200,
                    json!({"id": "pl-9", "name": "Fresh", "comment": "c", "public": true, "sync": true,
                        "rules": {"any": [{"is": {"loved": true}}], "sort": "-year", "limit": 5}}),
                ),
                ("GET", "/api/playlist/plain") => {
                    (200, json!({"id": "plain", "name": "Plain", "rules": null}))
                }
                ("GET", "/api/playlist") => {
                    (200, json!([{"id": "a", "rules": null}, {"id": "b", "rules": {"all": []}}, {"id": "c"}]))
                }
                _ => (404, json!({})),
            };
            let text = reply.to_string();
            let out = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                text.len()
            );
            let _ = sock.write_all(out.as_bytes()).await;
        }
    });
    Fake { url, seen }
}

fn def() -> SmartPlaylistDef {
    let mut q = Query::default();
    let fields = listen_source::nsp::query::field_options([]);
    let year = fields.iter().find(|f| f.key == "year").unwrap();
    q.set_field(&[], 0, year);
    q.set_op(&[], 0, "gt");
    q.set_a(&[], 0, "1990".into());
    q.limit = "25".into();
    SmartPlaylistDef {
        name: "90s".into(),
        comment: "after 1990".into(),
        public: true,
        sync: false,
        rules: q.to_nsp(&fields).unwrap(),
    }
}

async fn connect(fake: &Fake) -> Subsonic {
    let s = Subsonic::new(ServerId("t".into()), &fake.url, "alice", "pw").unwrap();
    s.ping().await.unwrap();
    s
}

#[tokio::test]
async fn create_posts_the_rules_with_the_native_token() {
    let f = fake(true, false).await;
    let s = connect(&f).await;
    assert!(s.capabilities().contains(Capabilities::SMART_PLAYLISTS));
    let p = s.create_smart_playlist(&def()).await.unwrap();
    assert_eq!((p.id.as_str(), p.name.as_str(), p.public, p.editable), ("new-pl", "90s", true, true));
    let seen = f.seen.lock().unwrap().clone();
    let login = seen.iter().find(|r| r.path == "/auth/login").unwrap();
    assert_eq!(login.body, json!({"username": "alice", "password": "pw"}));
    let post = seen.iter().find(|r| r.method == "POST" && r.path == "/api/playlist").unwrap();
    assert_eq!(post.auth.as_deref(), Some("Bearer tok1"));
    assert_eq!(post.body["name"], "90s");
    assert_eq!(post.body["comment"], "after 1990");
    assert_eq!((post.body["public"].clone(), post.body["sync"].clone()), (json!(true), json!(false)));
    assert_eq!(post.body["rules"], json!({"all": [{"gt": {"year": 1990}}], "sort": "+album", "limit": 25}));
}

#[tokio::test]
async fn update_puts_to_the_playlist_and_a_stale_token_logs_in_again() {
    let f = fake(true, true).await;
    let s = connect(&f).await;
    s.update_smart_playlist("pl-9", &def()).await.unwrap();
    let seen = f.seen.lock().unwrap().clone();
    let puts: Vec<&Seen> = seen.iter().filter(|r| r.method == "PUT").collect();
    // The first token was refused, a second login got a new one.
    assert_eq!(puts.len(), 2);
    assert_eq!(puts[1].auth.as_deref(), Some("Bearer tok2"));
    assert_eq!(puts[1].path, "/api/playlist/pl-9");
    assert_eq!(seen.iter().filter(|r| r.path == "/auth/login").count(), 2);
    assert_eq!(puts[1].body["rules"]["all"][0], json!({"gt": {"year": 1990}}));
}

#[tokio::test]
async fn reads_rules_and_lists_the_smart_ones() {
    let f = fake(true, false).await;
    let s = connect(&f).await;
    let d = s.smart_playlist("pl-9").await.unwrap().unwrap();
    assert_eq!((d.name.as_str(), d.comment.as_str(), d.public, d.sync), ("Fresh", "c", true, true));
    assert_eq!(d.rules.limit, Some(5));
    assert_eq!(Query::from_nsp(&d.rules, &[]).sort[0].field, "year");
    assert!(s.smart_playlist("plain").await.unwrap().is_none());
    assert_eq!(s.smart_playlist_ids().await.unwrap(), ["b"]);
    let _: Nsp = d.rules;
}

#[tokio::test]
async fn plain_subsonic_has_no_smart_playlists() {
    let f = fake(false, false).await;
    let s = connect(&f).await;
    assert!(!s.capabilities().contains(Capabilities::SMART_PLAYLISTS));
    assert!(matches!(s.create_smart_playlist(&def()).await, Err(SourceError::Unsupported)));
    assert!(s.smart_playlist_ids().await.unwrap().is_empty());
    assert!(f.seen.lock().unwrap().iter().all(|r| r.path != "/auth/login"));
}
