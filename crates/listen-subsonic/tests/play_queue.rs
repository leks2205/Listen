//! Saving and restoring the play queue (`savePlayQueue` / `getPlayQueue`)
//! against a fake server: the exact request and what is made of the answer.

use listen_core::model::ServerId;
use listen_source::{Capabilities, MusicSource, PlayQueue};
use listen_subsonic::Subsonic;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const QUEUE: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","playQueue":{
  "current":"s2","position":42000,"changedBy":"Feishin","entry":[
  {"id":"s1","title":"One","isDir":false,"duration":100},
  {"id":"s2","title":"Two","isDir":false,"duration":200}]}}}"#;
const EMPTY: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#;

async fn fake_server(body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
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
            log.lock().unwrap().push(line);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    (base, seen)
}

fn values(line: &str, key: &str) -> Vec<String> {
    let query = line.split('?').nth(1).and_then(|q| q.split(' ').next()).unwrap_or("");
    url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .filter(|(k, _)| k == key)
        .map(|(_, v)| v)
        .collect()
}

#[tokio::test]
async fn the_queue_is_restored_with_its_current_song_and_position() {
    let (base, seen) = fake_server(QUEUE).await;
    let nd = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    assert!(nd.capabilities().contains(Capabilities::PLAY_QUEUE_SYNC));
    let q = nd.play_queue().await.unwrap().expect("a saved queue");
    assert_eq!(q.songs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["s1", "s2"]);
    assert_eq!(q.current.as_deref(), Some("s2"));
    assert_eq!(q.position, Duration::from_secs(42));
    assert_eq!(q.changed_by.as_deref(), Some("Feishin"));
    assert!(seen.lock().unwrap()[0].starts_with("GET /rest/getPlayQueue?"));
}

#[tokio::test]
async fn no_saved_queue_is_none() {
    let (base, _) = fake_server(EMPTY).await;
    let nd = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    assert!(nd.play_queue().await.unwrap().is_none());
}

#[tokio::test]
async fn saving_sends_every_id_the_current_one_and_the_position_in_ms() {
    // Songs as the server describes them: the restore answer gives two.
    let (base, seen) = fake_server(QUEUE).await;
    let nd = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    let songs = nd.play_queue().await.unwrap().unwrap().songs;
    let queue = PlayQueue {
        songs,
        current: Some("s1".into()),
        position: Duration::from_millis(1500),
        changed_by: None,
    };
    nd.save_play_queue(&queue).await.unwrap();
    let line = seen.lock().unwrap()[1].clone();
    assert!(line.starts_with("GET /rest/savePlayQueue?"), "{line}");
    assert_eq!(values(&line, "id"), ["s1", "s2"]);
    assert_eq!(values(&line, "current"), ["s1"]);
    assert_eq!(values(&line, "position"), ["1500"]);
}
