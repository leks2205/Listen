//! Favourites of albums and artists against a fake Jellyfin: the request
//! path and method of `FavoriteItems`.

use listen_core::model::ServerId;
use listen_jellyfin::Jellyfin;
use listen_source::{Capabilities, ItemKind, MusicSource};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
            log.lock().unwrap().push(req.lines().next().unwrap_or("").to_string());
            let body = if req.contains("AuthenticateByName") {
                r#"{"AccessToken":"tok","User":{"Id":"u1"}}"#
            } else {
                r#"{"Items":[],"TotalRecordCount":0}"#
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

#[tokio::test]
async fn albums_and_artists_use_favorite_items() {
    let (base, seen) = fake_server().await;
    let jf = Jellyfin::connect(ServerId("t".into()), &base, "alice", "pw").await.unwrap();
    assert!(jf.capabilities().contains(Capabilities::FAVORITE_ITEMS));
    jf.set_favorite_items(ItemKind::Album, &["al1".into()], true).await.unwrap();
    jf.set_favorite_items(ItemKind::Artist, &["ar1".into()], false).await.unwrap();
    assert!(jf.favorite_albums().await.unwrap().is_empty());
    assert!(jf.favorite_artists().await.unwrap().is_empty());
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|l| l.starts_with("POST /Users/u1/FavoriteItems/al1")), "{seen:?}");
    assert!(seen.iter().any(|l| l.starts_with("DELETE /Users/u1/FavoriteItems/ar1")), "{seen:?}");
    assert!(seen.iter().any(|l| l.contains("Filters=IsFavorite") && l.contains("MusicAlbum")), "{seen:?}");
    assert!(
        seen.iter().any(|l| l.contains("Artists/AlbumArtists") && l.contains("isFavorite=true")),
        "{seen:?}"
    );
}
