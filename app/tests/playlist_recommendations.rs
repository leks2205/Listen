//! The "Recommended" strip under a playlist's rows: the shared source implementation and what
//! the page does with it (cache, the "+", the refresh button), against a fake server.

use async_trait::async_trait;
use iced::Task;
use listen::app::playlists::PlaylistPageMsg;
use listen::app::{Listen, Message, Route, SourceHandle};
use listen::{Args, demo};
use listen_core::config::ServerConfig;
use listen_core::model::{
    Album, Artist, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
};
use listen_source::{
    AlbumQuery, Capabilities, ListQuery, MusicSource, Result, SongQuery, SourceError, StreamSource,
};
use std::sync::{Arc, Mutex};

#[derive(Default, Debug)]
struct Calls {
    similar: Vec<(String, usize)>,
    added: Vec<(String, Vec<String>)>,
}

struct Fake {
    caps: Capabilities,
    calls: Arc<Mutex<Calls>>,
}

/// `s0`..`s11`; the playlist has the first six.
fn songs(n: usize) -> Vec<Song> {
    demo::album_songs(0)
        .into_iter()
        .cycle()
        .take(n)
        .enumerate()
        .map(|(i, mut s)| {
            s.id = format!("s{i}");
            s.title = format!("Song {i}");
            s.server_id = ServerId("srv".into());
            s.source = SourceKind::Subsonic;
            s
        })
        .collect()
}

#[async_trait]
impl MusicSource for Fake {
    fn kind(&self) -> SourceKind {
        SourceKind::Subsonic
    }
    fn capabilities(&self) -> Capabilities {
        self.caps
    }
    async fn ping(&self) -> Result<()> {
        Ok(())
    }
    async fn albums(&self, _: &AlbumQuery) -> Result<Page<Album>> {
        Err(SourceError::Unsupported)
    }
    async fn album(&self, _: &str) -> Result<Album> {
        Err(SourceError::Unsupported)
    }
    async fn artists(&self, _: &ListQuery) -> Result<Page<Artist>> {
        Err(SourceError::Unsupported)
    }
    async fn artist(&self, _: &str) -> Result<Artist> {
        Err(SourceError::Unsupported)
    }
    async fn artist_albums(&self, _: &str) -> Result<Vec<Album>> {
        Ok(vec![])
    }
    async fn top_songs(&self, _: &Artist, _: usize) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn songs(&self, _: &SongQuery) -> Result<Page<Song>> {
        Err(SourceError::Unsupported)
    }
    async fn random_songs(&self, _: usize, _: Option<&str>) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn search(&self, _: &str, _: usize) -> Result<SearchResults> {
        Ok(SearchResults::default())
    }
    async fn playlists(&self) -> Result<Vec<Playlist>> {
        let mut p = demo::navidrome_playlist().playlist;
        p.id = "pl".into();
        p.name = "Mix".into();
        p.server_id = ServerId("srv".into());
        Ok(vec![p])
    }
    async fn playlist_songs(&self, _: &str) -> Result<Vec<Song>> {
        Ok(songs(6))
    }
    async fn set_favorite(&self, _: &[String], _: bool) -> Result<()> {
        Ok(())
    }
    async fn stream(&self, _: &Song) -> Result<StreamSource> {
        Err(SourceError::Unsupported)
    }
    fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
        None
    }
    async fn similar_songs(&self, song: &Song, count: usize) -> Result<Vec<Song>> {
        self.calls.lock().unwrap().similar.push((song.id.clone(), count));
        // The server answers with the seed itself and songs the playlist already has.
        Ok(songs(12))
    }
    async fn add_to_playlist(&self, id: &str, ids: &[String]) -> Result<()> {
        self.calls.lock().unwrap().added.push((id.to_string(), ids.to_vec()));
        Ok(())
    }
}

fn fake(caps: Capabilities) -> (Arc<Fake>, Arc<Mutex<Calls>>) {
    let calls = Arc::new(Mutex::new(Calls::default()));
    (Arc::new(Fake { caps, calls: calls.clone() }), calls)
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new().unwrap().block_on(f)
}

fn app() -> (Listen, Arc<Mutex<Calls>>) {
    let (mut s, _) =
        Listen::boot(&Args { ephemeral: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let id = ServerId("srv".into());
    s.config.servers = vec![ServerConfig {
        id: id.clone(),
        name: "srv".into(),
        kind: SourceKind::Subsonic,
        url: "https://x.example".into(),
        username: "me".into(),
    }];
    let (source, calls) = fake(Capabilities::PLAYLIST_RECOMMENDATIONS | Capabilities::SIMILAR_SONGS);
    let _ = s.update(Message::Connected(id, Ok(SourceHandle(source))));
    (s, calls)
}

/// Runs a task and returns the messages it produced.
fn run(task: Task<Message>) -> Vec<Message> {
    use iced::futures::StreamExt;
    let Some(mut stream) = iced_runtime::task::into_stream(task) else { return vec![] };
    tokio::runtime::Runtime::new().unwrap().block_on(async move {
        let mut out = Vec::new();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(m) = action {
                out.push(m);
            }
        }
        out
    })
}

/// Sends `m`, then feeds the answers back (three rounds).
fn settle(s: &mut Listen, m: Message) {
    let mut pending = run(s.update(m));
    for _ in 0..3 {
        let next: Vec<Message> = pending.drain(..).flat_map(|m| run(s.update(m))).collect();
        pending = next;
    }
}

#[test]
fn the_shared_implementation_leaves_out_the_playlist_and_repeats_nothing() {
    let (source, _) = fake(Capabilities::PLAYLIST_RECOMMENDATIONS | Capabilities::SIMILAR_SONGS);
    let have = songs(6);
    let got = block_on(source.playlist_recommendations(&have, 5)).unwrap();
    assert_eq!(got.len(), 5);
    let ids: Vec<&str> = got.iter().map(|s| s.id.as_str()).collect();
    for id in &ids {
        assert!(!have.iter().any(|h| h.id == *id), "{id} is already in the playlist");
    }
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "no song twice");
}

#[test]
fn sources_without_similarity_have_no_recommendations() {
    let (source, calls) = fake(Capabilities::SIMILAR_SONGS);
    assert!(block_on(source.playlist_recommendations(&songs(6), 5)).is_err());
    assert!(calls.lock().unwrap().similar.is_empty(), "asked a source that cannot");
}

#[test]
fn the_playlist_page_shows_the_strip_and_the_plus_adds_one_song() {
    let (mut s, calls) = app();
    settle(&mut s, Message::Navigate(Route::Playlist("pl".into())));
    let strip = &s.playlist_page.recommended;
    assert_eq!(strip.songs.len(), 5);
    assert!(strip.error.is_none());
    assert!(!calls.lock().unwrap().similar.is_empty(), "asked the source");

    // The strip is kept for the playlist: leaving and coming back costs no request.
    let asked = calls.lock().unwrap().similar.len();
    settle(&mut s, Message::Navigate(Route::Home));
    settle(&mut s, Message::Navigate(Route::Playlist("pl".into())));
    assert_eq!(calls.lock().unwrap().similar.len(), asked, "the strip is cached");

    // The "+" adds exactly that song and drops its row.
    let first = s.playlist_page.recommended.songs[0].clone();
    settle(&mut s, Message::PlaylistPage(PlaylistPageMsg::RecommendAdd(0)));
    assert_eq!(s.playlist_page.recommended.songs.len(), 4);
    let added = calls.lock().unwrap().added.clone();
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!(added[0].0, "pl");
    assert_eq!(added[0].1, std::slice::from_ref(&first.id));

    // The refresh button asks for another set.
    let asked = calls.lock().unwrap().similar.len();
    settle(&mut s, Message::PlaylistPage(PlaylistPageMsg::Recommend(true)));
    assert!(calls.lock().unwrap().similar.len() > asked, "refresh asks again");
    assert_eq!(s.playlist_page.recommended.songs.len(), 5);
}
