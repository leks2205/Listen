//! Resolves a real YouTube Music stream with yt-dlp. Opt-in: `LISTEN_YTM_LIVE=1`
//! (network + yt-dlp + a JS runtime required).

use listen_ytmusic::ytdlp::YtDlp;

#[tokio::test]
async fn resolves_audio_url() {
    if std::env::var("LISTEN_YTM_LIVE").is_err() {
        eprintln!("LISTEN_YTM_LIVE not set; skipping");
        return;
    }
    let ytdlp = YtDlp::new();
    println!("yt-dlp: {:?}", ytdlp.check().await);
    let r = ytdlp.resolve("dQw4w9WgXcQ").await.expect("stream url");
    assert!(r.url.starts_with("https://"), "{}", r.url);
    assert!(r.expires_at.is_some());
    // Second call is served from the cache.
    let again = ytdlp.resolve("dQw4w9WgXcQ").await.unwrap();
    assert_eq!(again.url, r.url);
}

/// Checks that ytmapi-rs still understands YouTube Music's responses. Uses a
/// consent-only cookie (EU visitors are otherwise redirected to a consent page).
#[tokio::test]
async fn innertube_parsing_still_works() {
    use listen_core::model::ServerId;
    use listen_source::MusicSource;
    if std::env::var("LISTEN_YTM_LIVE").is_err() {
        return;
    }
    let cookie = std::env::var("LISTEN_YTM_COOKIE").unwrap_or_else(|_| "SOCS=CAI; SAPISID=anonymous;".into());
    let ytm = listen_ytmusic::YouTubeMusic::connect(ServerId("t".into()), &cookie).await.expect("connect");
    let found = ytm.search("Kai Rosenkranz Gothic", 10).await.expect("search");
    println!(
        "search: {} songs, {} albums, {} artists",
        found.songs.len(),
        found.albums.len(),
        found.artists.len()
    );
    assert!(!found.songs.is_empty());
    let album = ytm.album(&found.albums[0].id).await.expect("album");
    assert!(!album.songs.is_empty());
    let radio = ytm.similar_songs(&found.songs[0], 10).await.expect("radio");
    println!("ok: album '{}' ({} tracks), radio {}", album.name, album.songs.len(), radio.len());
}

/// The home feed parser against the real `FEmusic_home` response (guest feed).
#[tokio::test]
async fn home_feed_parses() {
    use listen_core::model::ServerId;
    use listen_source::MusicSource;
    if std::env::var("LISTEN_YTM_LIVE").is_err() {
        return;
    }
    let yt = listen_ytmusic::YouTubeMusic::connect(ServerId("t".into()), "").await.expect("guest");
    let shelves = yt.home().await.expect("home feed");
    for s in &shelves {
        println!(
            "{}: {} albums, {} playlists, {} songs",
            s.title,
            s.albums.len(),
            s.playlists.len(),
            s.songs.len()
        );
    }
    assert!(shelves.len() >= 2, "home has shelves");
    let items: usize = shelves.iter().map(|s| s.albums.len() + s.playlists.len() + s.songs.len()).sum();
    assert!(items >= 10, "home shelves have items");
    if let Some(song) = shelves.iter().flat_map(|s| &s.songs).next() {
        assert!(!song.id.is_empty() && !song.title.is_empty() && !song.artists.is_empty(), "{song:?}");
    }
    if let Some(p) = shelves.iter().flat_map(|s| &s.playlists).next() {
        let songs = yt.playlist_songs(&p.id).await.expect("home playlist opens");
        println!("{}: {} songs", p.name, songs.len());
    }
}
