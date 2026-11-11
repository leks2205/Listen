//! The plain Subsonic lists (a server without Navidrome's native API) against a
//! fake server: every filter the list offers applies, alone or together, or the
//! list fails; none is silently dropped.

use listen_core::model::ServerId;
use listen_source::{AlbumQuery, AlbumSort, MusicSource, SongQuery, SortOrder, SourceError};
use listen_subsonic::Subsonic;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How many albums the fake's alphabetical list holds ("Album 000" ...).
const ALBUMS: usize = 150;

fn ok(key: &str, payload: Value) -> String {
    let mut root = json!({"status": "ok", "version": "1.16.1", "type": "gonic", "openSubsonic": false});
    if !key.is_empty() {
        root[key] = payload;
    }
    json!({ "subsonic-response": root }).to_string()
}

fn song(id: &str, title: &str, genre: &str, starred: bool) -> Value {
    let mut s =
        json!({"id": id, "title": title, "artist": "Band", "album": "LP", "genre": genre, "isDir": false});
    if starred {
        s["starred"] = json!("2024-01-01T00:00:00Z");
    }
    s
}

fn album(id: String, name: String, genre: &str) -> Value {
    json!({"id": id, "name": name, "artist": "Band", "genre": genre})
}

fn params(line: &str) -> Vec<(String, String)> {
    let query = line.split('?').nth(1).and_then(|q| q.split(' ').next()).unwrap_or("");
    url::form_urlencoded::parse(query.as_bytes()).into_owned().collect()
}

fn param(p: &[(String, String)], k: &str) -> Option<String> {
    p.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.clone())
}

fn answer(line: &str) -> String {
    let p = params(line);
    let num = |k: &str| param(&p, k).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    if line.contains("/rest/getStarred2") {
        let songs = vec![song("s1", "Starred Rock", "Rock", true), song("s2", "Starred Jazz", "Jazz", true)];
        ok("starred2", json!({"song": songs, "album": [], "artist": []}))
    } else if line.contains("/rest/getSongsByGenre") {
        let all = vec![song("r1", "Red", "Rock", false), song("r2", "Blue", "Rock", false)];
        let page: Vec<Value> = all.into_iter().skip(num("offset")).take(num("count")).collect();
        ok("songsByGenre", json!({ "song": page }))
    } else if line.contains("/rest/getAlbumList2") {
        let all: Vec<Value> = match param(&p, "type").as_deref() {
            Some("byGenre") => vec![
                album("g1".into(), "Rock One".into(), "Rock"),
                album("g2".into(), "Rock Two".into(), "Rock"),
            ],
            _ => (0..ALBUMS).map(|i| album(format!("a{i:03}"), format!("Album {i:03}"), "Pop")).collect(),
        };
        let page: Vec<Value> = all.into_iter().skip(num("offset")).take(num("size")).collect();
        ok("albumList2", json!({ "album": page }))
    } else if line.contains("/rest/search3") {
        ok("searchResult3", json!({"song": [], "album": [], "artist": []}))
    } else {
        ok("", Value::Null)
    }
}

async fn fake() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = vec![0u8; 16384];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = req.lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(line.clone());
            let body = answer(&line);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    (base, seen)
}

async fn client() -> (Subsonic, Arc<Mutex<Vec<String>>>) {
    let (base, seen) = fake().await;
    let s = Subsonic::new(ServerId("t".into()), &base, "alice", "pw").unwrap();
    s.ping().await.unwrap();
    (s, seen)
}

fn ids<T>(items: &[T], id: impl Fn(&T) -> &str) -> Vec<String> {
    items.iter().map(|i| id(i).to_string()).collect()
}

#[tokio::test]
async fn plain_subsonic_songs_honour_favourite_and_genre_together() {
    let (s, _) = client().await;
    let q = SongQuery { favorite: Some(true), genres: vec!["Rock".into()], limit: 50, ..Default::default() };
    let page = s.songs(&q).await.unwrap();
    assert_eq!(ids(&page.items, |s| &s.id), ["s1"]);
    // `favorites_only` is the same filter.
    let q = SongQuery { favorites_only: true, genres: vec!["Rock".into()], limit: 50, ..Default::default() };
    assert_eq!(ids(&s.songs(&q).await.unwrap().items, |s| &s.id), ["s1"]);
    let q = SongQuery { favorite: Some(true), search: Some("jazz".into()), limit: 50, ..Default::default() };
    assert_eq!(ids(&s.songs(&q).await.unwrap().items, |s| &s.id), ["s2"]);
}

#[tokio::test]
async fn plain_subsonic_search_keeps_the_genre() {
    let (s, seen) = client().await;
    let q = SongQuery {
        genres: vec!["Rock".into()],
        search: Some("red".into()),
        limit: 50,
        ..Default::default()
    };
    assert_eq!(ids(&s.songs(&q).await.unwrap().items, |s| &s.id), ["r1"]);
    let q = AlbumQuery {
        genres: vec!["Rock".into()],
        search: Some("two".into()),
        limit: 50,
        ..Default::default()
    };
    assert_eq!(ids(&s.albums(&q).await.unwrap().items, |a| &a.id), ["g2"]);
    let lines = seen.lock().unwrap().clone();
    assert!(!lines.iter().any(|l| l.contains("/rest/search3")), "search3 drops the genre: {lines:?}");
}

#[tokio::test]
async fn not_favourited_fails_on_plain_subsonic_instead_of_listing_everything() {
    let (s, _) = client().await;
    let q = SongQuery { favorite: Some(false), limit: 50, ..Default::default() };
    assert!(matches!(s.songs(&q).await, Err(SourceError::Unsupported)));
    let q = AlbumQuery { favorite: Some(false), limit: 50, ..Default::default() };
    assert!(matches!(s.albums(&q).await, Err(SourceError::Unsupported)));
    // The plain lists still work.
    let q =
        AlbumQuery { sort: AlbumSort::Name, order: SortOrder::Ascending, limit: 10, ..Default::default() };
    assert_eq!(s.albums(&q).await.unwrap().items.len(), 10);
}
