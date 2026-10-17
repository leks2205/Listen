//! Share links and item details against a fake Navidrome: checks the exact
//! `createShare` / `getSong` requests and what is made of the answers.

use listen_core::model::ServerId;
use listen_source::{Capabilities, ItemKind, MusicSource};
use listen_subsonic::Subsonic;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SHARE: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","shares":{"share":[
  {"id":"abc123","url":"https://music.example.com/share/abc123","description":"for you","expires":"2030-01-01T00:00:00Z"}]}}}"#;
const SONG: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","song":{
  "id":"s1","title":"Song One","path":"A/B/01.flac","size":1000,"suffix":"flac","bitRate":900,"samplingRate":44100,"bitDepth":16,"channelCount":2,"playCount":4}}}"#;
const FAIL: &str = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":0,"message":"Sharing is disabled"}}}"#;

async fn fake_server(share_body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(line.clone());
            let body = if line.contains("/rest/createShare") || line.contains("/rest/getShares") {
                share_body
            } else if line.contains("/rest/getSong") {
                SONG
            } else {
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    (base, seen)
}

fn params(line: &str) -> Vec<(String, String)> {
    let query = line.split('?').nth(1).and_then(|q| q.split(' ').next()).unwrap_or("");
    url::form_urlencoded::parse(query.as_bytes()).into_owned().collect()
}

fn values(line: &str, key: &str) -> Vec<String> {
    params(line).into_iter().filter(|(k, _)| k == key).map(|(_, v)| v).collect()
}

#[tokio::test]
async fn create_share_sends_ids_description_and_expiry() {
    let (base, seen) = fake_server(SHARE).await;
    let nd = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    assert!(nd.capabilities().contains(Capabilities::SHARES));
    let at = UNIX_EPOCH + Duration::from_secs(1_900_000_000);
    let share = nd
        .create_share(ItemKind::Album, &["al1".into(), "al2".into()], Some("for you"), Some(at))
        .await
        .unwrap();
    assert_eq!(share.url, "https://music.example.com/share/abc123");
    assert_eq!(share.description.as_deref(), Some("for you"));
    let line = seen.lock().unwrap()[0].clone();
    assert!(line.starts_with("GET /rest/createShare?"), "{line}");
    assert_eq!(values(&line, "id"), ["al1", "al2"]);
    assert_eq!(values(&line, "description"), ["for you"]);
    assert_eq!(values(&line, "expires"), ["1900000000000"]);
}

#[tokio::test]
async fn no_expiry_means_the_server_default() {
    let (base, seen) = fake_server(SHARE).await;
    let nd = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    nd.create_share(ItemKind::Song, &["s1".into()], None, None).await.unwrap();
    let line = seen.lock().unwrap()[0].clone();
    assert!(values(&line, "expires").is_empty() && values(&line, "description").is_empty());
}

#[tokio::test]
async fn a_failing_server_reports_why() {
    let (base, _) = fake_server(FAIL).await;
    let nd = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    let err = nd.create_share(ItemKind::Song, &["s1".into()], None, None).await.unwrap_err();
    assert!(err.to_string().contains("Sharing is disabled"), "{err}");
}

#[tokio::test]
async fn item_details_ask_get_song() {
    let (base, seen) = fake_server(SHARE).await;
    let nd = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    let d = nd.item_details(ItemKind::Song, "s1").await.unwrap();
    assert_eq!((d.path.as_deref(), d.bit_depth, d.play_count), (Some("A/B/01.flac"), Some(16), Some(4)));
    let line = seen.lock().unwrap()[0].clone();
    assert!(line.starts_with("GET /rest/getSong?") && values(&line, "id") == ["s1"], "{line}");
}
