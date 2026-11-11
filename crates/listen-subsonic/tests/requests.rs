//! How requests go out, against a fake server: a long id list is posted as a
//! form where the server lists OpenSubsonic `formPost` (proxies cap the request
//! line), and an error page with a non-2xx status is reported by its status.

use listen_core::model::{ServerId, Song, SourceKind};
use listen_source::{MusicSource, PlayQueue, SourceError};
use listen_subsonic::Subsonic;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const EXTENSIONS: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","openSubsonic":true,"openSubsonicExtensions":[{"name":"formPost","versions":[1]}]}}"#;

/// One request as the fake saw it.
#[derive(Debug, Clone)]
struct Seen {
    line: String,
    body: String,
}

/// Reads the head and the `Content-Length` body.
async fn read_request(sock: &mut tokio::net::TcpStream) -> (String, String) {
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 8192];
    loop {
        let n = sock.read(&mut chunk).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf).to_string();
        if let Some(end) = text.find("\r\n\r\n") {
            let len = text[..end]
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().to_string())
                })
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            if buf.len() >= end + 4 + len {
                return (text[..end].to_string(), text[end + 4..].to_string());
            }
        }
    }
    (String::from_utf8_lossy(&buf).to_string(), String::new())
}

/// A fake server; `open` = OpenSubsonic with `formPost`, `status` answers every request.
async fn fake(open: bool, status: &'static str) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let (head, body) = read_request(&mut sock).await;
            let line = head.lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(Seen { line: line.clone(), body });
            let (ctype, text) = if status != "200 OK" {
                ("text/html", "<html><body>Request-URI Too Large</body></html>".to_string())
            } else if open && line.contains("getOpenSubsonicExtensions") {
                ("application/json", EXTENSIONS.to_string())
            } else {
                let open = if open { "true" } else { "false" };
                (
                    "application/json",
                    format!(
                        r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1","openSubsonic":{open}}}}}"#
                    ),
                )
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    (base, seen)
}

fn big_queue() -> PlayQueue {
    let songs = (0..1000)
        .map(|i| Song::new(format!("{i:022}"), ServerId("t".into()), SourceKind::Subsonic, "Song"))
        .collect();
    PlayQueue {
        songs,
        current: Some(format!("{:022}", 5)),
        position: Duration::from_secs(3),
        changed_by: None,
    }
}

fn saves(seen: &Arc<Mutex<Vec<Seen>>>) -> Vec<Seen> {
    seen.lock().unwrap().iter().filter(|s| s.line.contains("savePlayQueue")).cloned().collect()
}

#[tokio::test]
async fn a_non_json_error_status_is_reported_with_its_http_code() {
    let (base, _) = fake(false, "414 URI Too Long").await;
    let s = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    match s.ping().await {
        Err(SourceError::Server { code: 414, message }) => assert_eq!(message, "request too large"),
        other => panic!("expected HTTP 414, got {other:?}"),
    }
    let (base, _) = fake(false, "502 Bad Gateway").await;
    let s = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    assert!(matches!(s.ping().await, Err(SourceError::Server { code: 502, .. })));
}

#[tokio::test]
async fn long_id_lists_are_posted_as_a_form_when_the_server_allows_it() {
    let (base, seen) = fake(true, "200 OK").await;
    let s = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    s.ping().await.unwrap();
    s.save_play_queue(&big_queue()).await.unwrap();
    let saves = saves(&seen);
    assert_eq!(saves.len(), 1);
    let save = &saves[0];
    assert!(save.line.starts_with("POST /rest/savePlayQueue "), "{}", save.line);
    assert!(!save.line.contains('?'), "nothing in the URL (ids, token): {}", save.line);
    let form: Vec<(String, String)> =
        url::form_urlencoded::parse(save.body.as_bytes()).into_owned().collect();
    assert_eq!(form.iter().filter(|(k, _)| k == "id").count(), 1000);
    assert!(form.iter().any(|(k, _)| k == "t"), "auth travels in the body");
    // Short requests stay GETs.
    let pings: Vec<Seen> =
        seen.lock().unwrap().iter().filter(|s| s.line.contains("/rest/ping")).cloned().collect();
    assert!(pings.iter().all(|p| p.line.starts_with("GET ")));
}

#[tokio::test]
async fn long_id_lists_stay_get_without_form_post() {
    let (base, seen) = fake(false, "200 OK").await;
    let s = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    s.ping().await.unwrap();
    s.save_play_queue(&big_queue()).await.unwrap();
    let saves = saves(&seen);
    assert!(saves[0].line.starts_with("GET /rest/savePlayQueue?"), "{}", saves[0].line);
}
