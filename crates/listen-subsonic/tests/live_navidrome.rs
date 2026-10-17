//! Live test against a real Navidrome/OpenSubsonic server. Skipped unless
//! `LISTEN_ND_URL`, `LISTEN_ND_USER` and `LISTEN_ND_PASS` are set.
//! Expects at least one album with at least one song.

use listen_core::model::ServerId;
use listen_source::{AlbumQuery, AlbumSort, ListQuery, MusicSource, SongQuery, SourceError};
use listen_subsonic::Subsonic;

fn client() -> Option<Subsonic> {
    let url = std::env::var("LISTEN_ND_URL").ok()?;
    let user = std::env::var("LISTEN_ND_USER").ok()?;
    let pass = std::env::var("LISTEN_ND_PASS").ok()?;
    Some(Subsonic::new(ServerId("live".into()), &url, &user, &pass).expect("valid url"))
}

#[tokio::test]
async fn browse_and_stream() {
    let Some(nd) = client() else {
        eprintln!("LISTEN_ND_* not set; skipping");
        return;
    };
    nd.ping().await.expect("ping");

    let albums = nd
        .albums(&AlbumQuery { sort: AlbumSort::Name, limit: 50, ..Default::default() })
        .await
        .expect("album list");
    assert!(!albums.items.is_empty(), "server has albums");

    let album = nd.album(&albums.items[0].id).await.expect("album detail");
    assert!(!album.songs.is_empty(), "album has songs");
    let song = &album.songs[0];

    // The stream URL must actually serve audio.
    let stream = nd.stream(song).await.unwrap();
    let resp = reqwest::get(&stream.url).await.expect("stream request");
    assert!(resp.status().is_success(), "stream status {}", resp.status());
    let ctype = resp.headers().get("content-type").map(|v| v.to_str().unwrap_or("").to_string());
    assert!(ctype.as_deref().is_some_and(|c| c.starts_with("audio/")), "content-type {ctype:?}");

    let artists = nd.artists(&ListQuery { limit: 10, ..Default::default() }).await.expect("artists");
    assert!(artists.total.unwrap_or(0) > 0);

    let songs = nd.songs(&SongQuery { limit: 10, ..Default::default() }).await.expect("songs");
    assert!(!songs.items.is_empty());

    let first_word = song.title.split_whitespace().next().unwrap_or(&song.title).to_string();
    let found = nd.search(&first_word, 10).await.expect("search");
    assert!(found.songs.iter().any(|s| s.id == song.id), "search finds {first_word}");

    if let Some(image) = &album.image {
        let url = nd.image_url(image, 300).unwrap();
        let resp = reqwest::get(&url).await.expect("cover request");
        assert!(resp.status().is_success());
    }

    nd.set_favorite(std::slice::from_ref(&song.id), true).await.expect("star");
    nd.set_favorite(std::slice::from_ref(&song.id), false).await.expect("unstar");
}

#[tokio::test]
async fn wrong_password_is_auth_error() {
    let (Ok(url), Ok(user)) = (std::env::var("LISTEN_ND_URL"), std::env::var("LISTEN_ND_USER")) else {
        return;
    };
    let nd = Subsonic::new(ServerId("bad".into()), &url, &user, "definitely-wrong").unwrap();
    assert!(matches!(nd.ping().await, Err(SourceError::Auth(_))));
}
