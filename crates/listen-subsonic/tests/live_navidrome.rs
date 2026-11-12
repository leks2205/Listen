//! Live test against a real Navidrome/OpenSubsonic server: `#[ignore]`d, run with
//! `cargo test -p listen-subsonic --test live_navidrome -- --ignored` and `LISTEN_ND_URL`,
//! `LISTEN_ND_USER`, `LISTEN_ND_PASS` set. Expects at least one album with at least one song.
//! It flips the star of one song and puts it back: use a test account.

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
#[ignore = "live: needs a Navidrome server in LISTEN_ND_*; run with --ignored"]
async fn browse_and_stream() {
    let Some(nd) = client() else {
        eprintln!("SKIPPED browse_and_stream: LISTEN_ND_* not set");
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

    // Flip the star and put it back: the server may be someone's real library.
    let was = song.favorite;
    nd.set_favorite(std::slice::from_ref(&song.id), !was).await.expect("flip star");
    nd.set_favorite(std::slice::from_ref(&song.id), was).await.expect("restore star");
}

#[tokio::test]
#[ignore = "live: needs a Navidrome server in LISTEN_ND_*; run with --ignored"]
async fn wrong_password_is_auth_error() {
    let (Ok(url), Ok(user)) = (std::env::var("LISTEN_ND_URL"), std::env::var("LISTEN_ND_USER")) else {
        eprintln!("SKIPPED wrong_password_is_auth_error: LISTEN_ND_* not set");
        return;
    };
    let nd = Subsonic::new(ServerId("bad".into()), &url, &user, "definitely-wrong").unwrap();
    assert!(matches!(nd.ping().await, Err(SourceError::Auth(_))));
}

/// Every test in this file talks to a real server (maybe someone's real library), so each is
/// `#[ignore = "live: …"]`: it runs only with `-- --ignored` and the variables set, never in a plain `cargo test`.
#[test]
fn live_tests_run_only_when_asked() {
    let src = include_str!("live_navidrome.rs");
    let marker = concat!("#[tokio", "::test]");
    for test in src.split(marker).skip(1) {
        let head = &test[..test.find("async fn").expect("a test function")];
        let name = test[test.find("async fn").unwrap()..].split('(').next().unwrap_or_default();
        assert!(head.contains("#[ignore = \"live:"), "{name} must be #[ignore = \"live: …\"]");
    }
}
