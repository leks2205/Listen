//! Quick Connect against a fake Jellyfin (initiate, poll, token, sign-in) and the
//! "prefer instant mix" radio routes.

use listen_core::model::{ServerId, Song};
use listen_jellyfin::quick_connect::{self, Initiated};
use listen_jellyfin::{Jellyfin, Options, TOKEN_PREFIX};
use listen_source::MusicSource;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Fake {
    base: String,
    seen: Arc<Mutex<Vec<String>>>,
}

/// Approves the code on the second poll; `/Similar` answers empty.
async fn fake_server() -> Fake {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let polls = Arc::new(Mutex::new(0));
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = req.lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(line.clone());
            let (status, body) = if line.starts_with("POST /QuickConnect/Initiate") {
                (200, r#"{"Code":"123456","Secret":"s3cret","Authenticated":false}"#.to_string())
            } else if line.starts_with("GET /QuickConnect/Connect?secret=s3cret") {
                let mut p = polls.lock().unwrap();
                *p += 1;
                (200, format!(r#"{{"Authenticated":{}}}"#, *p >= 2))
            } else if line.starts_with("POST /Users/AuthenticateWithQuickConnect") {
                assert!(req.contains(r#""Secret":"s3cret""#));
                (200, r#"{"AccessToken":"tok","User":{"Id":"u1","Name":"Alice"}}"#.to_string())
            } else if line.starts_with("GET /Users/Me") {
                assert!(req.contains(r#"Token="tok""#));
                (200, r#"{"Id":"u1","Name":"Alice"}"#.to_string())
            } else if line.contains("/Similar") {
                (200, r#"{"Items":[],"TotalRecordCount":0}"#.to_string())
            } else if line.contains("/InstantMix") {
                (
                    200,
                    r#"{"Items":[{"Id":"m1","Name":"Mixed","Type":"Audio"}],"TotalRecordCount":1}"#
                        .to_string(),
                )
            } else {
                (404, "{}".to_string())
            };
            let resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    Fake { base, seen }
}

#[tokio::test]
async fn quick_connect_flow_ends_in_a_token_sign_in() {
    let fake = fake_server().await;
    let options = Options::default();
    let device = "listen-jf-1";
    let Initiated { code, secret } = quick_connect::initiate(&fake.base, device, options).await.unwrap();
    assert_eq!(code, "123456");
    assert!(!quick_connect::is_approved(&fake.base, device, &secret, options).await.unwrap());
    assert!(quick_connect::is_approved(&fake.base, device, &secret, options).await.unwrap());
    let session = quick_connect::authenticate(&fake.base, device, &secret, options).await.unwrap();
    assert_eq!(
        (session.token.as_str(), session.user_id.as_str(), session.user_name.as_str()),
        ("tok", "u1", "Alice")
    );

    // The stored secret is the token; connecting asks /Users/Me instead of a password login.
    let secret = format!("{TOKEN_PREFIX}{}", session.token);
    Jellyfin::connect(ServerId("jf-1".into()), &fake.base, &session.user_name, &secret).await.unwrap();
    let seen = fake.seen.lock().unwrap().clone();
    assert!(seen.iter().any(|l| l.starts_with("GET /Users/Me")));
    assert!(!seen.iter().any(|l| l.contains("AuthenticateByName")));
}

#[tokio::test]
async fn a_server_without_quick_connect_says_so() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let _ = sock.read(&mut buf).await;
        let _ = sock
            .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    });
    let err = quick_connect::initiate(&base, "d", Options::default()).await.unwrap_err();
    assert!(err.to_string().contains("not enabled"), "{err}");
}

#[test]
fn approval_url_points_at_the_web_client() {
    assert_eq!(
        quick_connect::approval_url("https://jf.example.com/").as_deref(),
        Some("https://jf.example.com/web/#/quickconnect")
    );
    assert_eq!(quick_connect::approval_url("jf.example.com"), None);
}

fn song() -> Song {
    serde_json::from_value(serde_json::json!({
        "id": "a1", "server_id": "jf-1", "source": "jellyfin", "title": "T", "artists": [],
        "album_artists": [], "album": null, "album_id": null, "image": null, "duration": 1,
        "track_number": null, "disc_number": null, "year": null, "genres": [], "codec": null,
        "bit_rate": null, "sample_rate": null, "bit_depth": null, "bpm": null, "favorite": false,
        "rating": null, "play_count": null, "gain": {}, "explicit": false
    }))
    .unwrap()
}

#[tokio::test]
async fn radio_asks_similar_first_unless_instant_mix_is_preferred() {
    let fake = fake_server().await;
    let sid = ServerId("jf-1".into());
    let secret = format!("{TOKEN_PREFIX}tok");
    let default =
        Jellyfin::connect_with(sid.clone(), &fake.base, "Alice", &secret, Options::default()).await.unwrap();
    let songs = default.similar_songs(&song(), 10).await.unwrap();
    assert_eq!(songs.len(), 1, "falls back to InstantMix when /Similar is empty");
    let log = fake.seen.lock().unwrap().clone();
    let similar = log.iter().position(|l| l.contains("/Items/a1/Similar")).expect("similar asked");
    let mix = log.iter().position(|l| l.contains("/Items/a1/InstantMix")).expect("mix asked");
    assert!(similar < mix);

    fake.seen.lock().unwrap().clear();
    let preferred = Jellyfin::connect_with(
        sid,
        &fake.base,
        "Alice",
        &secret,
        Options { prefer_instant_mix: true, ..Options::default() },
    )
    .await
    .unwrap();
    preferred.similar_songs(&song(), 10).await.unwrap();
    assert!(!fake.seen.lock().unwrap().iter().any(|l| l.contains("/Similar")));
}
