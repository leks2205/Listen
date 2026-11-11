//! Live test against a real Jellyfin server: `#[ignore]`d, run with
//! `cargo test -p listen-jellyfin --test live_jellyfin -- --ignored` and `LISTEN_JF_URL`,
//! `LISTEN_JF_USER`, `LISTEN_JF_PASS` set. Expects at least one album. It flips the favourite
//! of one song and puts it back, and reports one "now playing": use a test account.

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
#[ignore = "live server: needs LISTEN_JF_*, run with --ignored"]
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

    // Flip the favourite and put it back: the server may be someone's real library.
    let was = song.favorite;
    jf.set_favorite(std::slice::from_ref(&song.id), !was).await.expect("flip favourite");
    let favs = jf.songs(&SongQuery { favorites_only: true, limit: 500, ..Default::default() }).await;
    jf.set_favorite(std::slice::from_ref(&song.id), was).await.expect("restore favourite");
    assert_eq!(favs.unwrap().items.iter().any(|s| s.id == song.id), !was);

    let genres = jf.genres().await.expect("genres");
    println!("genres: {:?}", genres.iter().map(|g| &g.name).collect::<Vec<_>>());
    let mix = jf.similar_songs(song, 5).await.expect("instant mix");
    println!("instant mix: {} songs", mix.len());
    jf.scrobble(song, listen_source::ScrobbleEvent::NowPlaying).await.expect("now playing");
}

#[tokio::test]
#[ignore = "live server: needs LISTEN_JF_*, run with --ignored"]
async fn wrong_password_is_auth_error() {
    let Some((url, user, _)) = env() else { return };
    let r = Jellyfin::connect(ServerId("bad".into()), &url, &user, "definitely-wrong").await;
    assert!(matches!(r, Err(SourceError::Auth(_))));
}

/// Every test in this file talks to a real server (maybe someone's real library), so each is
/// `#[ignore]`: it runs only with `-- --ignored` and the variables set, never in a plain `cargo test`.
#[test]
fn live_tests_run_only_when_asked() {
    let src = include_str!("live_jellyfin.rs");
    let marker = concat!("#[tokio", "::test]");
    for test in src.split(marker).skip(1) {
        let head = &test[..test.find("async fn").expect("a test function")];
        let name = test[test.find("async fn").unwrap()..].split('(').next().unwrap_or_default();
        assert!(head.contains("#[ignore"), "{name} must be #[ignore]");
    }
}
