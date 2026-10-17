//! Live test against a real Jellyfin server. Skipped unless `LISTEN_JF_URL`,
//! `LISTEN_JF_USER` and `LISTEN_JF_PASS` are set. Expects at least one album.

use listen_core::model::ServerId;
use listen_jellyfin::Jellyfin;
use listen_source::{AlbumQuery, AlbumSort, ListQuery, MusicSource, SongQuery, SourceError};

fn env() -> Option<(String, String, String)> {
    Some((
        std::env::var("LISTEN_JF_URL").ok()?,
        std::env::var("LISTEN_JF_USER").ok()?,
        std::env::var("LISTEN_JF_PASS").ok()?,
    ))
}

#[tokio::test]
async fn browse_and_stream() {
    let Some((url, user, pass)) = env() else {
        eprintln!("LISTEN_JF_* not set; skipping");
        return;
    };
    let jf = Jellyfin::connect(ServerId("live".into()), &url, &user, &pass).await.expect("sign in");
    jf.ping().await.expect("ping");

    let albums = jf
        .albums(&AlbumQuery { sort: AlbumSort::Name, limit: 50, ..Default::default() })
        .await
        .expect("albums");
    assert!(!albums.items.is_empty(), "server has albums");
    let album = jf.album(&albums.items[0].id).await.expect("album detail");
    assert!(!album.songs.is_empty(), "album has songs");
    let song = &album.songs[0];
    assert!(!song.duration.is_zero());

    let stream = jf.stream(song).await.unwrap();
    let resp = reqwest::get(&stream.url).await.expect("stream request");
    assert!(resp.status().is_success(), "stream status {}", resp.status());

    let artists = jf.artists(&ListQuery { limit: 10, ..Default::default() }).await.expect("artists");
    assert!(!artists.items.is_empty());
    let artist_albums = jf.artist_albums(&artists.items[0].id).await.expect("artist albums");
    assert!(!artist_albums.is_empty());

    let songs = jf.songs(&SongQuery { limit: 10, ..Default::default() }).await.expect("songs");
    assert!(!songs.items.is_empty());

    let word = song.title.split_whitespace().next().unwrap_or(&song.title).to_string();
    let found = jf.search(&word, 10).await.expect("search");
    assert!(found.songs.iter().any(|s| s.id == song.id), "search finds {word}");

    if let Some(image) = &album.image {
        let resp = reqwest::get(jf.image_url(image, 300).unwrap()).await.expect("cover");
        println!("cover status {}", resp.status());
    }

    jf.set_favorite(std::slice::from_ref(&song.id), true).await.expect("favorite");
    let favs = jf.songs(&SongQuery { favorites_only: true, limit: 50, ..Default::default() }).await.unwrap();
    assert!(favs.items.iter().any(|s| s.id == song.id));
    jf.set_favorite(std::slice::from_ref(&song.id), false).await.expect("unfavorite");

    let genres = jf.genres().await.expect("genres");
    println!("genres: {:?}", genres.iter().map(|g| &g.name).collect::<Vec<_>>());
    let mix = jf.similar_songs(song, 5).await.expect("instant mix");
    println!("instant mix: {} songs", mix.len());
    jf.scrobble(song, listen_source::ScrobbleEvent::NowPlaying).await.expect("now playing");
}

#[tokio::test]
async fn wrong_password_is_auth_error() {
    let Some((url, user, _)) = env() else { return };
    let r = Jellyfin::connect(ServerId("bad".into()), &url, &user, "definitely-wrong").await;
    assert!(matches!(r, Err(SourceError::Auth(_))));
}
