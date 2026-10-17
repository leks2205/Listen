//! Favourites and ratings of albums and artists against a fake Navidrome:
//! checks the exact parameters of `star`, `unstar` and `setRating`.

use listen_core::model::ServerId;
use listen_source::{Capabilities, ItemKind, MusicSource};
use listen_subsonic::Subsonic;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const OK: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#;
const STARRED: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","starred2":{
  "artist":[{"id":"ar1","name":"Artist One","albumCount":2,"starred":"2024-01-01T00:00:00Z","userRating":4}],
  "album":[{"id":"al1","name":"Album One","artist":"Artist One","artistId":"ar1","songCount":9,"starred":"2024-01-01T00:00:00Z","userRating":5}],
  "song":[]}}}"#;
const FOLDERS: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","musicFolders":{"musicFolder":[{"id":1,"name":"Music"}]}}}"#;
const INDEXES: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","indexes":{"index":[{"name":"A","artist":[{"id":"f1","name":"Abba"},{"id":"f2","name":"Air"}]}]}}}"#;

async fn fake_server() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = req.lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(line.clone());
            let body = if line.contains("/rest/getStarred2") {
                STARRED
            } else if line.contains("/rest/getMusicFolders") {
                FOLDERS
            } else if line.contains("/rest/getIndexes") {
                INDEXES
            } else {
                OK
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

fn client(base: &str) -> Subsonic {
    Subsonic::new(ServerId("t".into()), base, "alice", "pw").unwrap()
}

fn params(line: &str) -> Vec<(String, String)> {
    let query = line.split('?').nth(1).and_then(|q| q.split(' ').next()).unwrap_or("");
    url::form_urlencoded::parse(query.as_bytes()).into_owned().collect()
}

fn has(line: &str, k: &str, v: &str) -> bool {
    params(line).iter().any(|(pk, pv)| pk == k && pv == v)
}

#[tokio::test]
async fn albums_artists_and_songs_are_starred_by_their_own_parameter() {
    let (base, seen) = fake_server().await;
    let nd = client(&base);
    nd.set_favorite_items(ItemKind::Album, &["al1".into(), "al2".into()], true).await.unwrap();
    nd.set_favorite_items(ItemKind::Artist, &["ar1".into()], false).await.unwrap();
    nd.set_favorite_items(ItemKind::Song, &["s1".into()], true).await.unwrap();
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /rest/star?"), "{}", seen[0]);
    assert!(has(&seen[0], "albumId", "al1") && has(&seen[0], "albumId", "al2"));
    assert!(!params(&seen[0]).iter().any(|(k, _)| k == "id" || k == "artistId"));
    assert!(seen[1].starts_with("GET /rest/unstar?") && has(&seen[1], "artistId", "ar1"));
    assert!(seen[2].starts_with("GET /rest/star?") && has(&seen[2], "id", "s1"));
}

#[tokio::test]
async fn every_kind_is_rated_with_set_rating() {
    let (base, seen) = fake_server().await;
    let nd = client(&base);
    nd.set_item_rating(ItemKind::Album, "al1", 4).await.unwrap();
    nd.set_item_rating(ItemKind::Artist, "ar1", 0).await.unwrap();
    nd.set_item_rating(ItemKind::Song, "s1", 9).await.unwrap();
    let seen = seen.lock().unwrap();
    for line in seen.iter() {
        assert!(line.starts_with("GET /rest/setRating?"), "{line}");
    }
    assert!(has(&seen[0], "id", "al1") && has(&seen[0], "rating", "4"));
    assert!(has(&seen[1], "id", "ar1") && has(&seen[1], "rating", "0"));
    // The rating is clamped to five stars.
    assert!(has(&seen[2], "rating", "5"));
}

#[tokio::test]
async fn favourites_come_from_get_starred2() {
    let (base, _) = fake_server().await;
    let nd = client(&base);
    let albums = nd.favorite_albums().await.unwrap();
    assert_eq!(albums.len(), 1);
    assert!(albums[0].favorite && albums[0].rating == Some(5));
    let artists = nd.favorite_artists().await.unwrap();
    assert_eq!(artists.len(), 1);
    assert!(artists[0].favorite);
    assert_eq!(artists[0].rating, Some(4));
}

#[tokio::test]
async fn the_folder_tree_starts_with_the_top_level_folders() {
    let (base, _) = fake_server().await;
    let nd = client(&base);
    assert!(nd.capabilities().contains(Capabilities::FOLDER_TREE | Capabilities::FAVORITE_ITEMS));
    let tree = nd.folder_tree().await.unwrap();
    assert_eq!(tree.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Abba", "Air"]);
    assert!(tree.iter().all(|n| n.children.is_empty()));
}
