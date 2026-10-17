//! Navidrome's native login against a fake server: parallel list loads share
//! one login, and a rate-limited login (HTTP 429) pauses the native API
//! instead of logging in again on every load.

use listen_core::model::ServerId;
use listen_source::{AlbumQuery, MusicSource};
use listen_subsonic::Subsonic;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PING: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.58.0","openSubsonic":true}}"#;
const LIST: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","albumList2":{"album":[]}}}"#;

/// A fake Navidrome; `limited` makes `/auth/login` answer 429. Returns the
/// base URL and the number of login requests seen.
async fn fake(limited: bool) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let logins = Arc::new(AtomicUsize::new(0));
    let count = logins.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let count = count.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = req.lines().next().unwrap_or("").to_string();
                let (status, body, extra) = if line.contains("/auth/login") {
                    count.fetch_add(1, Ordering::SeqCst);
                    // Slow enough that a second load arrives while it runs.
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    if limited {
                        ("429 Too Many Requests", r#"{"error":"too many"}"#.to_string(), "")
                    } else {
                        ("200 OK", r#"{"token":"tok"}"#.to_string(), "")
                    }
                } else if line.contains("/api/album") {
                    ("200 OK", "[]".to_string(), "x-total-count: 0\r\n")
                } else if line.contains("/rest/ping") {
                    ("200 OK", PING.to_string(), "")
                } else {
                    ("200 OK", LIST.to_string(), "")
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (base, logins)
}

fn client(base: &str) -> Subsonic {
    Subsonic::new(ServerId("t".into()), base, "alice", "pw").unwrap()
}

#[tokio::test]
async fn parallel_loads_share_one_login() {
    let (base, logins) = fake(false).await;
    let s = client(&base);
    let q = AlbumQuery { limit: 10, ..Default::default() };
    let _ = tokio::join!(s.albums(&q), s.albums(&q), s.albums(&q));
    assert_eq!(logins.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_rate_limited_login_is_not_retried_at_once() {
    let (base, logins) = fake(true).await;
    let s = client(&base);
    let q = AlbumQuery { limit: 10, ..Default::default() };
    let _ = tokio::join!(s.albums(&q), s.albums(&q));
    let _ = s.albums(&q).await;
    assert_eq!(logins.load(Ordering::SeqCst), 1, "one login attempt, then Subsonic for a minute");
}
