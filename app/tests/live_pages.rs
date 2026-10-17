//! Renders pages with real server data (headless). Skipped unless
//! `LISTEN_ND_URL`, `LISTEN_ND_USER`, `LISTEN_ND_PASS` are set.
//! The test fetches data itself and feeds the results to `update` as the
//! app's own tasks would.

use iced_test::simulator::Simulator;
use listen::app::{ArtistDetail, INTER, Listen, Message, PlaylistDetail, Route, SourceHandle};
use listen::{Args, FONTS};
use listen_core::model::ServerId;
use listen_source::{ListQuery, MusicSource};
use std::path::PathBuf;
use std::sync::Arc;

fn snapshot(state: &Listen, name: &str) {
    let settings = iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    };
    let theme = state.theme();
    let mut ui = Simulator::with_size(settings, iced::Size::new(1600.0, 950.0), state.view());
    let snap = ui.snapshot(&theme).expect("snapshot");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
            std::fs::remove_file(e.path()).unwrap();
        }
    }
    assert!(snap.matches_image(dir.join(name)).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn pages_with_real_data() {
    let (Ok(url), Ok(user), Ok(pass)) =
        (std::env::var("LISTEN_ND_URL"), std::env::var("LISTEN_ND_USER"), std::env::var("LISTEN_ND_PASS"))
    else {
        eprintln!("LISTEN_ND_* not set; skipping");
        return;
    };
    let source: Arc<dyn MusicSource> =
        Arc::new(listen_subsonic::Subsonic::new(ServerId("live".into()), &url, &user, &pass).unwrap());
    let args = Args { theme: Some("tokyoNight".into()), ephemeral: true, ..Args::default() };
    let (mut state, _) = Listen::boot(&args);
    let _ = state.update(Message::Connected(ServerId("live".into()), Ok(SourceHandle(source.clone()))));

    // Artists page
    let artists = source.artists(&ListQuery { limit: 500, ..Default::default() }).await.unwrap();
    let first_artist = artists.items.first().cloned().expect("an artist");
    let _ = state.update(Message::Navigate(Route::AlbumArtists));
    let _ = state.update(Message::ArtistsLoaded(Ok(artists)));
    snapshot(&state, "live-artists");

    // Artist page
    let albums = source.artist_albums(&first_artist.id).await.unwrap();
    let artist = source.artist(&first_artist.id).await.unwrap();
    let top = source.top_songs(&artist, 10).await.unwrap_or_default();
    let _ = state.update(Message::Navigate(Route::Artist(artist.id.clone())));
    let _ = state.update(Message::ArtistLoaded(Ok(ArtistDetail { artist, albums, top_songs: top })));
    snapshot(&state, "live-artist");

    // Playlists (sidebar) + search
    let playlists = source.playlists().await.unwrap();
    let _ = state.update(Message::PlaylistsLoaded(Ok(playlists.clone())));
    if let Some(p) = playlists.first() {
        let songs = source.playlist_songs(&p.id).await.unwrap();
        let _ = state.update(Message::Navigate(Route::Playlist(p.id.clone())));
        let _ = state.update(Message::PlaylistLoaded(Ok(PlaylistDetail { playlist: p.clone(), songs })));
        snapshot(&state, "live-playlist");
    }
    let query = first_artist.name.split_whitespace().next().unwrap_or("a").to_string();
    let _ = state.update(Message::SearchInput(query.clone()));
    let _ = state.update(Message::SearchSubmit); // bumps the search generation to 1
    let results = source.search(&query, 20).await.map_err(|e| e.to_string());
    let _ = state.update(Message::SearchLoaded(1, results));
    snapshot(&state, "live-search");

    // Genres + folders
    let genres = source.genres().await.map_err(|e| e.to_string());
    let _ = state.update(Message::Navigate(Route::Genres));
    let _ = state.update(Message::GenresLoaded(genres));
    snapshot(&state, "live-genres");
    let root = source.folder(None).await.map_err(|e| e.to_string());
    let _ = state.update(Message::Navigate(Route::Folder(None)));
    let _ = state.update(Message::Extra(listen::app::library_extra::ExtraMsg::FolderLoaded(None, root)));
    snapshot(&state, "live-folders");
}

/// Loads artwork like the app's image tasks would.
async fn feed_images(
    state: &mut Listen,
    source: &Arc<dyn MusicSource>,
    refs: Vec<listen_core::model::ImageRef>,
) {
    let http = reqwest::Client::builder().user_agent("Listen-test").build().unwrap();
    for r in refs {
        let Some(url) = source.image_url(&r, listen::app::ART_SIZE) else { continue };
        let bytes = listen::images::fetch(http.clone(), url, None).await;
        let _ = state.update(Message::ImageLoaded(state.image_key(&r), bytes));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn youtube_music_pages() {
    if std::env::var("LISTEN_YTM_LIVE").is_err() {
        eprintln!("LISTEN_YTM_LIVE not set; skipping");
        return;
    }
    let yt = listen_ytmusic::YouTubeMusic::connect(ServerId("ytm".into()), listen_ytmusic::GUEST_COOKIE)
        .await
        .expect("guest connect");
    let source: Arc<dyn MusicSource> = Arc::new(yt);
    let args = Args { theme: Some("tokyoNight".into()), ephemeral: true, ..Args::default() };
    let (mut state, _) = Listen::boot(&args);
    let _ = state.update(Message::Connected(ServerId("ytm".into()), Ok(SourceHandle(source.clone()))));

    let query = "Kai Rosenkranz Gothic".to_string();
    let _ = state.update(Message::SearchInput(query.clone()));
    let _ = state.update(Message::SearchSubmit);
    let results = source.search(&query, 20).await.expect("search");
    let mut refs: Vec<_> = results.albums.iter().filter_map(|a| a.image.clone()).collect();
    refs.extend(results.artists.iter().filter_map(|a| a.image.clone()));
    refs.extend(results.songs.iter().take(10).filter_map(|s| s.image.clone()));
    let album_id = results.albums.first().map(|a| a.id.clone()).expect("album");
    let _ = state.update(Message::SearchLoaded(1, Ok(results)));
    feed_images(&mut state, &source, refs).await;
    snapshot(&state, "ytm-search");

    let album = source.album(&album_id).await.expect("album");
    let image = album.image.clone();
    let _ = state.update(Message::Navigate(Route::Album(album_id)));
    let _ = state.update(Message::AlbumLoaded(Ok(album)));
    feed_images(&mut state, &source, image.clone().into_iter().collect()).await;
    if let Some(img) = image {
        let http = reqwest::Client::new();
        let bytes =
            listen::images::fetch(http, source.image_url(&img, listen::app::ART_SIZE).unwrap(), None).await;
        let blurred = bytes.and_then(|b| listen::images::blur(&b, 4.5));
        let _ = state.update(Message::BlurLoaded(state.image_key(&img), blurred));
    }
    snapshot(&state, "ytm-album");
}
