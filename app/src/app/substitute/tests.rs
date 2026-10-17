use super::*;
use listen_core::model::{Album, Artist, ImageRef, Page, Playlist, SearchResults};
use listen_source::{AlbumQuery, Capabilities, ListQuery, StreamSource};
use std::sync::atomic::AtomicUsize;

/// A server with a fixed library: `search` answers after `delay`, `stream` gives `Kind://id`.
struct Fake {
    kind: SourceKind,
    library: Vec<Song>,
    delay: Duration,
    searches: AtomicUsize,
    broken: bool,
}

impl Fake {
    fn new(kind: SourceKind, library: Vec<Song>) -> Fake {
        Fake { kind, library, delay: Duration::ZERO, searches: 0.into(), broken: false }
    }
}

#[async_trait::async_trait]
impl MusicSource for Fake {
    fn kind(&self) -> SourceKind {
        self.kind
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::empty()
    }
    async fn ping(&self) -> listen_source::Result<()> {
        Ok(())
    }
    async fn albums(&self, _: &AlbumQuery) -> listen_source::Result<Page<Album>> {
        unimplemented!()
    }
    async fn album(&self, _: &str) -> listen_source::Result<Album> {
        unimplemented!()
    }
    async fn artists(&self, _: &ListQuery) -> listen_source::Result<Page<Artist>> {
        unimplemented!()
    }
    async fn artist(&self, _: &str) -> listen_source::Result<Artist> {
        unimplemented!()
    }
    async fn artist_albums(&self, _: &str) -> listen_source::Result<Vec<Album>> {
        unimplemented!()
    }
    async fn top_songs(&self, _: &Artist, _: usize) -> listen_source::Result<Vec<Song>> {
        unimplemented!()
    }
    async fn songs(&self, _: &listen_source::SongQuery) -> listen_source::Result<Page<Song>> {
        unimplemented!()
    }
    async fn random_songs(&self, _: usize, _: Option<&str>) -> listen_source::Result<Vec<Song>> {
        unimplemented!()
    }
    async fn search(&self, text: &str, _: usize) -> listen_source::Result<SearchResults> {
        self.searches.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        let text = text.to_lowercase();
        let songs = self.library.iter().filter(|s| s.title.to_lowercase().contains(&text)).cloned().collect();
        Ok(SearchResults { songs, albums: vec![], artists: vec![], playlists: vec![] })
    }
    async fn playlists(&self) -> listen_source::Result<Vec<Playlist>> {
        unimplemented!()
    }
    async fn playlist_songs(&self, _: &str) -> listen_source::Result<Vec<Song>> {
        unimplemented!()
    }
    async fn set_favorite(&self, _: &[String], _: bool) -> listen_source::Result<()> {
        unimplemented!()
    }
    async fn stream(&self, song: &Song) -> listen_source::Result<StreamSource> {
        if self.broken {
            return Err(listen_source::SourceError::Network("down".into()));
        }
        Ok(StreamSource::direct(format!("{:?}://{}", self.kind, song.id)))
    }
    fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
        None
    }
}

fn song(kind: SourceKind, server: &str, id: &str, title: &str, secs: u64) -> Song {
    let mut song = crate::demo::album_songs(0)[0].clone();
    song.id = id.into();
    song.server_id = ServerId(server.into());
    song.source = kind;
    song.title = title.into();
    song.artists = vec![listen_core::model::ArtistRef { id: None, name: "Some Artist".into() }];
    song.album_artists = vec![];
    song.album = None;
    song.duration = Duration::from_secs(secs);
    song
}

fn ytm(id: &str, title: &str) -> Song {
    song(SourceKind::YouTubeMusic, "yt", id, title, 200)
}

fn own_copy(title: &str) -> Song {
    song(SourceKind::Subsonic, "nd", "nd-1", title, 201)
}

fn setup(own: Fake) -> (Sources, Arc<Fake>) {
    let own = Arc::new(own);
    let mut sources = Sources::new();
    sources.insert(ServerId("yt".into()), Arc::new(Fake::new(SourceKind::YouTubeMusic, vec![])));
    sources.insert(ServerId("nd".into()), own.clone());
    (sources, own)
}

#[tokio::test]
async fn an_own_copy_is_streamed_and_remembered() {
    let (sources, own) = setup(Fake::new(SourceKind::Subsonic, vec![own_copy("Africa")]));
    let subs = Substitutes::default();
    let yt = ytm("v1", "Africa (Official Video)");
    let track = stream_for(&sources, &subs, &yt).await.unwrap();
    assert_eq!(track.url, "Subsonic://nd-1");
    assert_eq!(subs.used(&yt).unwrap().id, "nd-1");
    assert!(subs.alternative(&yt).unwrap().active);
    // The second time nothing is searched.
    let before = own.searches.load(Ordering::SeqCst);
    stream_for(&sources, &subs, &yt).await.unwrap();
    assert_eq!(own.searches.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn no_own_copy_plays_youtube_and_the_answer_is_cached() {
    let (sources, own) = setup(Fake::new(SourceKind::Subsonic, vec![own_copy("Rosanna")]));
    let subs = Substitutes::default();
    let yt = ytm("v2", "Africa");
    let track = stream_for(&sources, &subs, &yt).await.unwrap();
    assert_eq!(track.url, "YouTubeMusic://v2");
    assert!(subs.used(&yt).is_none() && subs.alternative(&yt).is_none());
    let before = own.searches.load(Ordering::SeqCst);
    assert!(before > 0);
    stream_for(&sources, &subs, &yt).await.unwrap();
    assert_eq!(own.searches.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn switched_off_streams_youtube_without_searching() {
    let (sources, own) = setup(Fake::new(SourceKind::Subsonic, vec![own_copy("Africa")]));
    let subs = Substitutes::default();
    subs.set_enabled(false);
    let track = stream_for(&sources, &subs, &ytm("v1", "Africa")).await.unwrap();
    assert_eq!(track.url, "YouTubeMusic://v1");
    assert_eq!(own.searches.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn other_sources_are_left_alone() {
    let (sources, own) = setup(Fake::new(SourceKind::Subsonic, vec![own_copy("Africa")]));
    let subs = Substitutes::default();
    let nd = song(SourceKind::Subsonic, "nd", "nd-9", "Africa", 200);
    let track = stream_for(&sources, &subs, &nd).await.unwrap();
    assert_eq!(track.url, "Subsonic://nd-9");
    assert_eq!(own.searches.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_slow_server_never_holds_playback_up() {
    let mut fake = Fake::new(SourceKind::Subsonic, vec![own_copy("Africa")]);
    fake.delay = Duration::from_millis(400);
    let (sources, _) = setup(fake);
    let subs = Substitutes::default();
    let yt = ytm("v1", "Africa");
    let started = Instant::now();
    let track = stream_for_within(&sources, &subs, &yt, Duration::from_millis(50)).await.unwrap();
    assert_eq!(track.url, "YouTubeMusic://v1");
    assert!(started.elapsed() < Duration::from_millis(300));
    // The search went on: the next lookup has the answer.
    let found = subs.resolve(&sources, &yt, Some(Duration::from_secs(3))).await;
    assert_eq!(found.unwrap().id, "nd-1");
    assert_eq!(subs.known(&yt).unwrap().id, "nd-1");
    assert!(subs.alternative(&yt).is_none(), "YouTube Music played, so nothing is substituted");
}

#[tokio::test]
async fn an_own_copy_that_does_not_play_falls_back() {
    let mut fake = Fake::new(SourceKind::Subsonic, vec![own_copy("Africa")]);
    fake.broken = true;
    let (sources, _) = setup(fake);
    let subs = Substitutes::default();
    let yt = ytm("v1", "Africa");
    let track = stream_for(&sources, &subs, &yt).await.unwrap();
    assert_eq!(track.url, "YouTubeMusic://v1");
    assert!(subs.denied(&yt));
    assert!(subs.known(&yt).is_none());
}

#[tokio::test]
async fn the_user_can_go_back_and_forth() {
    let (sources, _) = setup(Fake::new(SourceKind::Subsonic, vec![own_copy("Africa")]));
    let subs = Substitutes::default();
    let yt = ytm("v1", "Africa");
    stream_for(&sources, &subs, &yt).await.unwrap();
    subs.set_denied(&yt, true);
    let alt = subs.alternative(&yt).unwrap();
    assert!(!alt.active && alt.song.id == "nd-1");
    assert_eq!(stream_for(&sources, &subs, &yt).await.unwrap().url, "YouTubeMusic://v1");
    subs.set_denied(&yt, false);
    assert_eq!(stream_for(&sources, &subs, &yt).await.unwrap().url, "Subsonic://nd-1");
}

#[tokio::test]
async fn no_own_server_means_no_lookup() {
    let mut sources = Sources::new();
    sources.insert(ServerId("yt".into()), Arc::new(Fake::new(SourceKind::YouTubeMusic, vec![])));
    let subs = Substitutes::default();
    let yt = ytm("v1", "Africa");
    assert_eq!(stream_for(&sources, &subs, &yt).await.unwrap().url, "YouTubeMusic://v1");
    assert!(!subs.wants_lookup(&yt, 0));
}

#[tokio::test]
async fn a_server_connected_later_is_asked() {
    let subs = Substitutes::default();
    let yt = ytm("v1", "Africa");
    let mut sources = Sources::new();
    sources.insert(ServerId("yt".into()), Arc::new(Fake::new(SourceKind::YouTubeMusic, vec![])));
    sources.insert(ServerId("jf".into()), Arc::new(Fake::new(SourceKind::Jellyfin, vec![])));
    assert!(subs.resolve(&sources, &yt, None).await.is_none());
    sources
        .insert(ServerId("nd".into()), Arc::new(Fake::new(SourceKind::Subsonic, vec![own_copy("Africa")])));
    assert_eq!(subs.resolve(&sources, &yt, None).await.unwrap().id, "nd-1");
}

#[tokio::test]
async fn the_best_copy_of_several_servers_wins() {
    let far = song(SourceKind::Subsonic, "nd", "far", "Africa", 203);
    let (mut sources, _) = setup(Fake::new(SourceKind::Subsonic, vec![far]));
    let exact = song(SourceKind::Jellyfin, "jf", "exact", "Africa", 200);
    sources.insert(ServerId("jf".into()), Arc::new(Fake::new(SourceKind::Jellyfin, vec![exact])));
    let subs = Substitutes::default();
    let found = subs.resolve(&sources, &ytm("v1", "Africa"), None).await.unwrap();
    assert_eq!(found.id, "exact");
}
