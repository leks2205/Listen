//! Navidrome's native genre filter against a fake server: a genre name the
//! server's id map lacks never turns into an unfiltered list, and a genre
//! added by a scan is found again (reload on a miss, or the list's Refresh).

use listen_core::model::ServerId;
use listen_source::{AlbumQuery, MusicSource};
use listen_subsonic::Subsonic;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PING: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.58.0","openSubsonic":true}}"#;

#[derive(Default)]
struct Seen {
    /// Request lines of `/api/album`.
    albums: Mutex<Vec<String>>,
    /// How often `/api/genre` was read.
    genre_loads: AtomicUsize,
    /// After the "scan", the server also knows Jazz.
    scanned: AtomicBool,
}

async fn fake() -> (String, Arc<Seen>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let seen = Arc::new(Seen::default());
    let shared = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let seen = shared.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = req.lines().next().unwrap_or("").to_string();
                let (body, extra) = if line.contains("/auth/login") {
                    (r#"{"token":"tok"}"#.to_string(), "")
                } else if line.contains("/api/genre") {
                    seen.genre_loads.fetch_add(1, Ordering::SeqCst);
                    if seen.scanned.load(Ordering::SeqCst) {
                        (r#"[{"id":"g1","name":"Rock"},{"id":"g2","name":"Jazz"}]"#.to_string(), "")
                    } else {
                        (r#"[{"id":"g1","name":"Rock"}]"#.to_string(), "")
                    }
                } else if line.contains("/api/album") {
                    seen.albums.lock().unwrap().push(line.clone());
                    ("[]".to_string(), "x-total-count: 0\r\n")
                } else {
                    (PING.to_string(), "")
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (base, seen)
}

fn genre(name: &str) -> AlbumQuery {
    AlbumQuery { genres: vec![name.into()], limit: 10, ..Default::default() }
}

#[tokio::test]
async fn genre_filter_never_widens_to_the_whole_library() {
    let (base, seen) = fake().await;
    let s = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    let page = s.albums(&genre("Jazz")).await.unwrap();
    assert!(page.items.is_empty());
    assert_eq!(page.total, Some(0));
    let albums = seen.albums.lock().unwrap().clone();
    assert!(albums.iter().all(|l| l.contains("genre_id=")), "no unfiltered list: {albums:?}");
    // A known and an unknown genre together match nothing either (not just the known one).
    let both = AlbumQuery { genres: vec!["Rock".into(), "Jazz".into()], limit: 10, ..Default::default() };
    assert!(s.albums(&both).await.unwrap().items.is_empty());
    let albums = seen.albums.lock().unwrap().clone();
    assert!(albums.iter().all(|l| l.contains("genre_id=")), "no narrowed list: {albums:?}");
}

#[tokio::test]
async fn refresh_reloads_genre_ids() {
    let (base, seen) = fake().await;
    let s = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    s.albums(&genre("Rock")).await.unwrap();
    s.albums(&genre("Rock")).await.unwrap();
    assert_eq!(seen.genre_loads.load(Ordering::SeqCst), 1, "the map is cached");
    s.refresh().await;
    s.albums(&genre("Rock")).await.unwrap();
    assert_eq!(seen.genre_loads.load(Ordering::SeqCst), 2, "Refresh drops the map");
    // A genre added by a scan is looked up again without a Refresh.
    seen.scanned.store(true, Ordering::SeqCst);
    s.albums(&genre("Jazz")).await.unwrap();
    assert_eq!(seen.genre_loads.load(Ordering::SeqCst), 3);
    let last = seen.albums.lock().unwrap().last().cloned().unwrap();
    assert!(last.contains("genre_id=g2"), "{last}");
}
