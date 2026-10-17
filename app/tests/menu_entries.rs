//! The entries of every context menu per capability set, the actions applied
//! to several targets, queue moves and snapshots of each kind's menu.

use async_trait::async_trait;
use iced::Task;
use iced_test::simulator::Simulator;
use listen::app::menu::{EntryKind, MenuAction, MenuMsg, MenuTarget, PlayHow, QueueMove, entries};
use listen::app::{INTER, Listen, Message, SourceHandle};
use listen::{Args, FONTS, demo};
use listen_core::config::ServerConfig;
use listen_core::model::{
    Album, Artist, ArtistRef, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
};
use listen_source::{
    AlbumQuery, Capabilities, ItemKind, ListQuery, MusicSource, Result, SongQuery, SourceError, StreamSource,
};
use std::sync::{Arc, Mutex};

static GPU: Mutex<()> = Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

/// What the fake server was asked to do.
#[derive(Default, Debug)]
struct Calls {
    favorites: Vec<(Vec<String>, bool)>,
    added: Vec<(String, Vec<String>)>,
    created: Vec<(String, Vec<String>)>,
    ratings: Vec<(ItemKind, String, u8)>,
    item_favorites: Vec<(ItemKind, Vec<String>, bool)>,
    radio_seeds: Vec<String>,
}

struct Fake(SourceKind, Capabilities, Arc<Mutex<Calls>>);

#[async_trait]
impl MusicSource for Fake {
    fn kind(&self) -> SourceKind {
        self.0
    }
    fn capabilities(&self) -> Capabilities {
        self.1
    }
    async fn ping(&self) -> Result<()> {
        Ok(())
    }
    async fn albums(&self, _: &AlbumQuery) -> Result<Page<Album>> {
        Err(SourceError::Unsupported)
    }
    async fn album(&self, id: &str) -> Result<Album> {
        let mut album = album_of(self.0, 0);
        album.id = id.into();
        album.songs = songs_of(self.0, 2);
        Ok(album)
    }
    async fn similar_songs(&self, seed: &Song, count: usize) -> Result<Vec<Song>> {
        self.2.lock().unwrap().radio_seeds.push(seed.id.clone());
        Ok(songs_of(self.0, 6).into_iter().skip(2).take(count).collect())
    }
    async fn artist_radio(&self, artist: &Artist, count: usize) -> Result<Vec<Song>> {
        self.2.lock().unwrap().radio_seeds.push(format!("artist:{}", artist.id));
        Ok(songs_of(self.0, 4).into_iter().take(count).collect())
    }
    async fn set_favorite_items(&self, kind: ItemKind, ids: &[String], favorite: bool) -> Result<()> {
        self.2.lock().unwrap().item_favorites.push((kind, ids.to_vec(), favorite));
        Ok(())
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
        Ok(vec![])
    }
    async fn playlist_songs(&self, _: &str) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn create_playlist(&self, name: &str, songs: &[String]) -> Result<Playlist> {
        self.2.lock().unwrap().created.push((name.into(), songs.to_vec()));
        Err(SourceError::Unsupported)
    }
    async fn add_to_playlist(&self, id: &str, songs: &[String]) -> Result<()> {
        self.2.lock().unwrap().added.push((id.into(), songs.to_vec()));
        Ok(())
    }
    async fn set_favorite(&self, ids: &[String], favorite: bool) -> Result<()> {
        self.2.lock().unwrap().favorites.push((ids.to_vec(), favorite));
        Ok(())
    }
    async fn set_item_rating(&self, kind: ItemKind, id: &str, rating: u8) -> Result<()> {
        self.2.lock().unwrap().ratings.push((kind, id.into(), rating));
        Ok(())
    }
    async fn stream(&self, _: &Song) -> Result<StreamSource> {
        Err(SourceError::Unsupported)
    }
    fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
        None
    }
}

fn app(kind: SourceKind, caps: Capabilities) -> (Listen, Arc<Mutex<Calls>>) {
    let (mut s, _) =
        Listen::boot(&Args { ephemeral: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let id = ServerId("srv".into());
    s.config.servers = vec![ServerConfig {
        id: id.clone(),
        name: "srv".into(),
        kind,
        url: "https://x.example".into(),
        username: "me".into(),
    }];
    let calls = Arc::new(Mutex::new(Calls::default()));
    let _ = s.update(Message::Connected(id, Ok(SourceHandle(Arc::new(Fake(kind, caps, calls.clone()))))));
    let _ = s.update(Message::WindowResized(iced::Size::new(1200.0, 800.0)));
    (s, calls)
}

fn songs_of(kind: SourceKind, n: usize) -> Vec<Song> {
    demo::album_songs(0)
        .into_iter()
        .cycle()
        .take(n)
        .map(|mut s| {
            s.server_id = ServerId("srv".into());
            s.source = kind;
            s.favorite = false;
            s.artists = vec![ArtistRef { id: Some("ar1".into()), name: "Neon Atlas".into() }];
            s
        })
        .collect()
}

/// Runs a task's futures to completion and returns the messages it produced.
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

fn menu(s: &mut Listen, msg: MenuMsg) -> Vec<Message> {
    run(s.update(Message::Menu(msg)))
}

fn labels_of(entries: &[listen::app::menu::Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| match &e.kind {
            EntryKind::Divider => "-".to_string(),
            EntryKind::Submenu(children) => {
                let inner: Vec<String> = labels_of(children);
                let grey = if e.enabled { "" } else { " (off)" };
                format!("{}{grey} > [{}]", e.label, inner.join(", "))
            }
            EntryKind::Action(_) => {
                if e.enabled {
                    e.label.clone()
                } else {
                    format!("{} (off)", e.label)
                }
            }
        })
        .collect()
}

fn menu_labels(s: &mut Listen, target: MenuTarget) -> Vec<String> {
    let _ = menu(s, MenuMsg::Open(target));
    let m = s.menu.clone().expect("menu open");
    labels_of(&entries(s, &m))
}

const SETS: [&str; 5] = ["navidrome", "jellyfin", "ytm", "ytm-guest", "local"];

fn set(name: &str) -> (SourceKind, Capabilities) {
    match name {
        "navidrome" => (SourceKind::Subsonic, listen_subsonic::capabilities()),
        "jellyfin" => (SourceKind::Jellyfin, listen_jellyfin::capabilities()),
        "ytm" => (SourceKind::YouTubeMusic, listen_ytmusic::capabilities(false)),
        "ytm-guest" => (SourceKind::YouTubeMusic, listen_ytmusic::capabilities(true)),
        _ => (SourceKind::Local, Capabilities::GENRES | Capabilities::ALBUM_ARTISTS),
    }
}

fn browsing(name: &str) -> (Listen, Arc<Mutex<Calls>>, SourceKind) {
    let (kind, caps) = set(name);
    let (mut s, calls) = app(kind, caps);
    s.config.active_server = Some(ServerId("srv".into()));
    (s, calls, kind)
}

fn album_of(kind: SourceKind, n: usize) -> Album {
    let mut a = demo::albums().remove(n);
    a.server_id = ServerId("srv".into());
    a.source = kind;
    a.songs = vec![];
    a.album_artists = vec![ArtistRef { id: Some("ar1".into()), name: "Neon Atlas".into() }];
    a
}

fn artist_of(kind: SourceKind) -> Artist {
    Artist {
        id: "ar1".into(),
        server_id: ServerId("srv".into()),
        source: kind,
        name: "Neon Atlas".into(),
        image: None,
        album_count: Some(2),
        song_count: Some(20),
        favorite: false,
        rating: None,
        biography: None,
        similar: vec![],
    }
}

fn target(kind_name: &str, kind: SourceKind) -> MenuTarget {
    let songs = songs_of(kind, 3);
    match kind_name {
        "song" => MenuTarget::song(&songs[0]),
        "songs" => MenuTarget::songs(&songs),
        "queue" => MenuTarget::queue_item(&songs[1], 2),
        "album" => MenuTarget::Albums(vec![album_of(kind, 0)]),
        "albums" => MenuTarget::Albums(vec![album_of(kind, 0), album_of(kind, 1)]),
        "artist" => MenuTarget::Artists(vec![artist_of(kind)]),
        "genre" => MenuTarget::Genres(vec![listen_core::model::Genre {
            id: "g1".into(),
            name: "Synthwave".into(),
            album_count: Some(3),
            song_count: Some(30),
        }]),
        _ => MenuTarget::Folders(vec![listen_core::model::FolderEntry {
            id: "f1".into(),
            name: "Folder".into(),
        }]),
    }
}

const EXPECTED: &[(&str, &str, &[&str])] = &[
    (
        "song",
        "navidrome",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Start radio",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "Set rating > [☆☆☆☆☆ ✓, ★☆☆☆☆, ★★☆☆☆, ★★★☆☆, ★★★★☆, ★★★★★]",
            "-",
            "Download",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Share…",
            "Save file…",
        ],
    ),
    (
        "song",
        "jellyfin",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Start radio",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Save file…",
        ],
    ),
    (
        "song",
        "ytm",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Start radio",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Like, Remove from Liked Music]",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
        ],
    ),
    (
        "song",
        "ytm-guest",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Start radio",
            "-",
            "Favorite > [Like, Remove from Liked Music]",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
        ],
    ),
    (
        "song",
        "local",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Start radio",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Edit metadata…",
        ],
    ),
    (
        "songs",
        "navidrome",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Start radio (off)",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "Set rating > [☆☆☆☆☆ ✓, ★☆☆☆☆, ★★☆☆☆, ★★★☆☆, ★★★★☆, ★★★★★]",
            "-",
            "Download",
            "-",
            "Go to (off) > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Share…",
            "Save file…",
        ],
    ),
    (
        "songs",
        "jellyfin",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Start radio (off)",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "-",
            "Go to (off) > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Save file…",
        ],
    ),
    (
        "songs",
        "ytm",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Start radio (off)",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Like, Remove from Liked Music]",
            "-",
            "Go to (off) > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
        ],
    ),
    (
        "songs",
        "ytm-guest",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Start radio (off)",
            "-",
            "Favorite > [Like, Remove from Liked Music]",
            "-",
            "Go to (off) > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
        ],
    ),
    (
        "songs",
        "local",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Start radio (off)",
            "-",
            "Go to (off) > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Edit metadata…",
        ],
    ),
    (
        "queue",
        "navidrome",
        &[
            "Remove from queue",
            "-",
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Move items > [Move to top, Move to next, Move to bottom]",
            "Shuffle > [Shuffle selected, Shuffle all]",
            "-",
            "Start radio",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "Set rating > [☆☆☆☆☆ ✓, ★☆☆☆☆, ★★☆☆☆, ★★★☆☆, ★★★★☆, ★★★★★]",
            "-",
            "Download",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Share…",
            "Save file…",
        ],
    ),
    (
        "queue",
        "jellyfin",
        &[
            "Remove from queue",
            "-",
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Move items > [Move to top, Move to next, Move to bottom]",
            "Shuffle > [Shuffle selected, Shuffle all]",
            "-",
            "Start radio",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Save file…",
        ],
    ),
    (
        "queue",
        "ytm",
        &[
            "Remove from queue",
            "-",
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Move items > [Move to top, Move to next, Move to bottom]",
            "Shuffle > [Shuffle selected, Shuffle all]",
            "-",
            "Start radio",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Like, Remove from Liked Music]",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
        ],
    ),
    (
        "queue",
        "ytm-guest",
        &[
            "Remove from queue",
            "-",
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Move items > [Move to top, Move to next, Move to bottom]",
            "Shuffle > [Shuffle selected, Shuffle all]",
            "-",
            "Start radio",
            "-",
            "Favorite > [Like, Remove from Liked Music]",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
        ],
    ),
    (
        "queue",
        "local",
        &[
            "Remove from queue",
            "-",
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Move items > [Move to top, Move to next, Move to bottom]",
            "Shuffle > [Shuffle selected, Shuffle all]",
            "-",
            "Start radio",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Edit metadata…",
        ],
    ),
    (
        "album",
        "navidrome",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Album radio > [Play, Next, Last]",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "Set rating > [☆☆☆☆☆ ✓, ★☆☆☆☆, ★★☆☆☆, ★★★☆☆, ★★★★☆, ★★★★★]",
            "-",
            "Download",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Share…",
            "Save file…",
        ],
    ),
    (
        "album",
        "jellyfin",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Album radio > [Play, Next, Last]",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Save file…",
        ],
    ),
    (
        "album",
        "ytm",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Album radio > [Play, Next, Last]",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Save to library, Remove from library]",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Copy link",
        ],
    ),
    (
        "album",
        "ytm-guest",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Album radio > [Play, Next, Last]",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Copy link",
        ],
    ),
    (
        "album",
        "local",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "-",
            "Go to > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
        ],
    ),
    (
        "albums",
        "navidrome",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Album radio (off) > [Play, Next, Last]",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "Set rating > [☆☆☆☆☆ ✓, ★☆☆☆☆, ★★☆☆☆, ★★★☆☆, ★★★★☆, ★★★★★]",
            "-",
            "Download",
            "-",
            "Go to (off) > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Share…",
            "Save file…",
        ],
    ),
    (
        "albums",
        "jellyfin",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Album radio (off) > [Play, Next, Last]",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "-",
            "Go to (off) > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
            "Save file…",
        ],
    ),
    (
        "albums",
        "ytm",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Album radio (off) > [Play, Next, Last]",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Save to library, Remove from library]",
            "-",
            "Go to (off) > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
        ],
    ),
    (
        "albums",
        "ytm-guest",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Album radio (off) > [Play, Next, Last]",
            "-",
            "Go to (off) > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
        ],
    ),
    (
        "albums",
        "local",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "-",
            "Go to (off) > [Go to album, Go to Neon Atlas]",
            "-",
            "Get info",
        ],
    ),
    (
        "artist",
        "navidrome",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Artist radio > [Play, Next, Last]",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "Set rating > [☆☆☆☆☆ ✓, ★☆☆☆☆, ★★☆☆☆, ★★★☆☆, ★★★★☆, ★★★★★]",
            "-",
            "Get info",
            "Share…",
        ],
    ),
    (
        "artist",
        "jellyfin",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Artist radio > [Play, Next, Last]",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Add to favorites, Remove from favorites]",
            "-",
            "Get info",
        ],
    ),
    (
        "artist",
        "ytm",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Artist radio > [Play, Next, Last]",
            "-",
            "Add to playlist…",
            "-",
            "Favorite > [Subscribe, Unsubscribe]",
            "-",
            "Get info",
            "Copy link",
        ],
    ),
    (
        "artist",
        "ytm-guest",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "Artist radio > [Play, Next, Last]",
            "-",
            "Get info",
            "Copy link",
        ],
    ),
    (
        "artist",
        "local",
        &["Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]", "-", "Get info"],
    ),
    (
        "genre",
        "navidrome",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "-",
            "Add to playlist…",
        ],
    ),
    (
        "genre",
        "jellyfin",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "-",
            "Add to playlist…",
        ],
    ),
    (
        "genre",
        "ytm",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "-",
            "Add to playlist…",
        ],
    ),
    (
        "genre",
        "ytm-guest",
        &["Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]"],
    ),
    ("genre", "local", &["Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]"]),
    (
        "folder",
        "navidrome",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "-",
            "Add to playlist…",
            "-",
            "Share…",
            "Save file…",
        ],
    ),
    (
        "folder",
        "jellyfin",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "-",
            "Add to playlist…",
            "-",
            "Save file…",
        ],
    ),
    (
        "folder",
        "ytm",
        &[
            "Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]",
            "-",
            "Add to playlist…",
        ],
    ),
    (
        "folder",
        "ytm-guest",
        &["Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]"],
    ),
    ("folder", "local", &["Play > [Play, Next, Last, -, Play (shuffled), Next (shuffled), Last (shuffled)]"]),
];

/// Every kind's entries, in Feishin's order, per capability set. A submenu
/// shows its children in brackets, `-` is a divider and `(off)` a greyed row.
#[test]
fn entries_of_every_kind_per_capability_set() {
    let mut wrong = Vec::new();
    for (kind_name, name, expected) in EXPECTED {
        let (mut s, _, kind) = browsing(name);
        let got = menu_labels(&mut s, target(kind_name, kind));
        if got != *expected {
            wrong.push(format!("{kind_name} / {name}:\n  got      {got:?}\n  expected {expected:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert_eq!(EXPECTED.len(), 8 * SETS.len());
}

// --- actions on several targets ---

/// Feeds the messages a task produced back into the app (one round).
fn feed(s: &mut Listen, messages: Vec<Message>) -> Vec<Message> {
    messages.into_iter().flat_map(|m| run(s.update(m))).collect()
}

fn ids(songs: &[Song]) -> Vec<String> {
    songs.iter().map(|s| s.id.clone()).collect()
}

#[test]
fn favorite_and_rating_apply_to_every_selected_album_and_artist() {
    let (mut s, calls, kind) = browsing("navidrome");
    let albums = vec![album_of(kind, 0), album_of(kind, 1), album_of(kind, 2)];
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::Albums(albums.clone())));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::Favorite(true)));
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::Albums(albums.clone())));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::Rate(3)));
    let artists = vec![artist_of(kind), Artist { id: "ar2".into(), ..artist_of(kind) }];
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::Artists(artists.clone())));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::Favorite(false)));
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::Artists(artists)));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::Rate(5)));
    let c = calls.lock().unwrap();
    let mut fav: Vec<_> = c.item_favorites.iter().map(|(k, i, f)| (*k, i.clone(), *f)).collect();
    fav.sort_by(|a, b| a.1.cmp(&b.1));
    let want_albums: Vec<String> = albums.iter().map(|a| a.id.clone()).collect();
    assert_eq!(fav.iter().filter(|(k, _, f)| *k == ItemKind::Album && *f).count(), 3);
    assert_eq!(fav.iter().filter(|(k, _, f)| *k == ItemKind::Artist && !*f).count(), 2);
    for id in &want_albums {
        assert!(fav.iter().any(|(_, i, _)| i == std::slice::from_ref(id)), "{id} favorited");
    }
    assert_eq!(c.ratings.iter().filter(|(k, _, r)| *k == ItemKind::Album && *r == 3).count(), 3);
    assert_eq!(c.ratings.iter().filter(|(k, _, r)| *k == ItemKind::Artist && *r == 5).count(), 2);
}

#[test]
fn ytm_favorites_save_albums_and_subscribe_to_artists() {
    let (mut s, calls, kind) = browsing("ytm");
    let _ = menu(&mut s, MenuMsg::Open(target("albums", kind)));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::Favorite(true)));
    let _ = menu(&mut s, MenuMsg::Open(target("artist", kind)));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::Favorite(true)));
    let c = calls.lock().unwrap();
    assert_eq!(c.item_favorites.len(), 3);
    assert_eq!(c.item_favorites.iter().filter(|(k, _, _)| *k == ItemKind::Album).count(), 2);
    // A guest has no FAVORITE_ITEMS: nothing is sent.
    drop(c);
    let (mut g, gcalls, kind) = browsing("ytm-guest");
    let _ = menu(&mut g, MenuMsg::Open(target("album", kind)));
    let _ = menu(&mut g, MenuMsg::Do(MenuAction::Favorite(true)));
    assert!(gcalls.lock().unwrap().item_favorites.is_empty());
}

#[test]
fn shuffled_next_and_last_queue_every_song_once() {
    let (mut s, _, kind) = browsing("navidrome");
    let current = songs_of(kind, 6);
    s.queue.set(current[..2].to_vec(), 0);
    let picked = current[2..6].to_vec();
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::songs(&picked)));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::PlayNextShuffled));
    // The four songs follow the playing one, as the manual queue, in some order.
    let queued: Vec<_> = s.queue.iter().skip(1).take(4).map(|i| (i.song.id.clone(), i.queued)).collect();
    assert!(queued.iter().all(|(_, q)| *q));
    let mut got: Vec<_> = queued.into_iter().map(|(i, _)| i).collect();
    got.sort();
    let mut want = ids(&picked);
    want.sort();
    assert_eq!(got, want);

    let before = s.queue.iter().count();
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::songs(&picked)));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::AddToQueueShuffled));
    assert_eq!(s.queue.iter().count(), before + 4);
    assert_eq!(s.queue.manual_len(), 8, "queued behind the manual queue");
}

#[test]
fn album_and_artist_radio_play_next_or_last() {
    let (mut s, calls, kind) = browsing("navidrome");
    s.queue.set(songs_of(kind, 2), 0);
    let before = s.queue.iter().count();
    let msgs = menu(&mut s, MenuMsg::Open(target("album", kind)));
    assert!(msgs.is_empty());
    let msgs = menu(&mut s, MenuMsg::Do(MenuAction::Radio(PlayHow::Last)));
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    let _ = feed(&mut s, msgs);
    assert_eq!(calls.lock().unwrap().radio_seeds.len(), 1);
    assert!(s.queue.iter().count() > before, "the radio songs were added last");
    assert_eq!(s.queue.manual_len(), s.queue.iter().count() - before, "queued like \"Add to queue\"");

    let before = s.queue.iter().count();
    let _ = menu(&mut s, MenuMsg::Open(target("artist", kind)));
    let msgs = menu(&mut s, MenuMsg::Do(MenuAction::Radio(PlayHow::Next)));
    let _ = feed(&mut s, msgs);
    assert_eq!(calls.lock().unwrap().radio_seeds.last().map(String::as_str), Some("artist:ar1"));
    assert_eq!(s.queue.iter().count(), before + 4);
    assert!(s.queue.iter().nth(1).unwrap().queued, "next: right behind the playing song");
    // Several albums: the radio entry is greyed out and does nothing.
    let _ = menu(&mut s, MenuMsg::Open(target("albums", kind)));
    assert!(menu(&mut s, MenuMsg::Do(MenuAction::Radio(PlayHow::Now))).is_empty());
}

// --- the queue's own menu ---

/// a b c d e, `x` and `y` queued after a, an Auto-DJ `z` last.
fn queue_app() -> (Listen, Vec<u64>) {
    let (mut s, _, kind) = browsing("navidrome");
    let mut songs = songs_of(kind, 8);
    for (song, id) in songs.iter_mut().zip(["a", "b", "c", "d", "e", "x", "y", "z"]) {
        song.id = id.into();
        song.title = id.into();
    }
    s.queue.set(songs[..5].to_vec(), 0);
    s.queue.enqueue(songs[5..7].to_vec());
    s.queue.add_last(vec![songs[7].clone()], true);
    let uids = s.queue.iter().map(|i| i.uid).collect();
    (s, uids)
}

fn order(s: &Listen) -> String {
    s.queue.iter().map(|i| i.song.id.as_str()).collect::<Vec<_>>().join("")
}

fn queue_menu(s: &mut Listen, picks: &[&str]) -> Vec<String> {
    let rows: Vec<(Song, u64)> = s
        .queue
        .iter()
        .filter(|i| picks.contains(&i.song.id.as_str()))
        .map(|i| (i.song.clone(), i.uid))
        .collect();
    let target = MenuTarget::Songs {
        songs: rows.iter().map(|(s, _)| s.clone()).collect(),
        queue_uids: rows.iter().map(|(_, u)| *u).collect(),
        history_entries: vec![],
    };
    menu_labels(s, target)
}

fn open_row(s: &mut Listen, id: &str) {
    let item = s.queue.iter().find(|i| i.song.id == id).unwrap();
    let target = MenuTarget::queue_item(&item.song, item.uid);
    let _ = menu(s, MenuMsg::Open(target));
}

fn uid(s: &Listen, id: &str) -> u64 {
    s.queue.iter().find(|i| i.song.id == id).unwrap().uid
}

#[test]
fn queue_moves_keep_the_manual_queue_and_the_auto_dj_songs() {
    let (mut s, _) = queue_app();
    assert_eq!(order(&s), "axybcdez");
    // To next: joins the manual queue, ahead of x and y.
    let d = uid(&s, "d");
    let _ = queue_menu(&mut s, &["d", "e"]);
    let e = uid(&s, "e");
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::MoveQueue(QueueMove::Next, vec![d, e])));
    assert_eq!(order(&s), "adexybcz");
    assert_eq!(s.queue.manual_len(), 4);
    // To bottom: leaves the manual queue; the Auto-DJ song keeps its flag.
    let x = uid(&s, "x");
    open_row(&mut s, "x");
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::MoveQueue(QueueMove::Bottom, vec![x])));
    assert_eq!(order(&s), "adeybczx");
    assert_eq!(s.queue.manual_len(), 3);
    assert!(!s.queue.iter().last().unwrap().queued);
    // To top: before the playing song, which keeps playing.
    let c = uid(&s, "c");
    open_row(&mut s, "c");
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::MoveQueue(QueueMove::Top, vec![c])));
    assert_eq!(order(&s), "cadeybzx");
    assert_eq!(s.queue.current().unwrap().song.id, "a");
    // Auto-DJ is untouched: its song is still the only auto one and new ones still append at the end.
    assert_eq!(s.queue.iter().filter(|i| i.auto).map(|i| i.song.id.as_str()).collect::<Vec<_>>(), ["z"]);
    let songs = songs_of(SourceKind::Subsonic, 1);
    s.queue.add_last(songs, true);
    assert!(s.queue.iter().last().unwrap().auto);
}

#[test]
fn queue_shuffle_selected_and_remove_work_on_the_selection() {
    let (mut s, _) = queue_app();
    let sel: Vec<u64> = ["x", "y", "c", "d"].iter().map(|i| uid(&s, i)).collect();
    for _ in 0..10 {
        let _ = menu(&mut s, MenuMsg::Do(MenuAction::ShuffleQueue(Some(sel.clone()))));
        let o = order(&s);
        assert!(o.starts_with('a') && o.ends_with("ez"));
        assert!(o[1..3].contains('x') && o[1..3].contains('y'), "{o}: manual songs stay in front");
        assert_eq!(&o[3..4], "b");
        assert!(o[4..6].contains('c') && o[4..6].contains('d'), "{o}");
    }
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::ShuffleQueue(None)));
    assert_eq!(s.queue.current().unwrap().song.id, "a");
    assert_eq!(s.queue.iter().count(), 8);
    // Remove several at once; the playing song is never removed.
    let remove = vec![uid(&s, "a"), uid(&s, "c"), uid(&s, "d")];
    let rows = s.queue.iter().filter(|i| remove.contains(&i.uid)).map(|i| i.song.clone()).collect::<Vec<_>>();
    let target = MenuTarget::Songs { songs: rows, queue_uids: remove.clone(), history_entries: vec![] };
    let _ = menu(&mut s, MenuMsg::Open(target));
    let removable = {
        let m = s.menu.clone().unwrap();
        entries(&s, &m).into_iter().find(|e| e.label == "Remove from queue").map(|e| match e.kind {
            EntryKind::Action(MenuAction::RemoveFromQueue(u)) => u,
            _ => vec![],
        })
    };
    assert_eq!(removable.as_ref().map(Vec::len), Some(2), "the playing song is left out");
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::RemoveFromQueue(remove)));
    assert_eq!(s.queue.iter().count(), 6);
    assert!(s.queue.iter().any(|i| i.song.id == "a"));
}

#[test]
fn the_queue_menu_follows_feishins_order() {
    let (mut s, _) = queue_app();
    let labels = queue_menu(&mut s, &["c", "d"]);
    let top: Vec<&str> = labels.iter().map(|l| l.split(" > ").next().unwrap()).collect();
    assert_eq!(
        top,
        [
            "Remove from queue",
            "-",
            "Play",
            "Move items",
            "Shuffle",
            "-",
            "Start radio (off)",
            "-",
            "Add to playlist…",
            "-",
            "Favorite",
            "Set rating",
            "-",
            "Download",
            "-",
            "Go to (off)",
            "-",
            "Get info",
            "Share…",
            "Save file…",
        ]
    );
}

// --- lists, folders ---

#[test]
fn the_dots_button_of_a_selected_row_opens_the_selections_menu() {
    use listen::app::list::{ListId, ListMsg};
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let _ = s.update(Message::WindowResized(iced::Size::new(1200.0, 800.0)));
    s.library.songs = (0..3).flat_map(demo::album_songs).collect();
    let _ = s.update(Message::Navigate(listen::app::Route::Tracks));
    let _ = s.update(Message::List(ListMsg::SelectAll(ListId::Songs)));
    let at = iced::Point::new(500.0, 200.0);
    let _ = s.update(Message::List(ListMsg::ContextAt(ListId::Songs, 1, at)));
    let m = s.menu.as_ref().expect("menu open");
    assert_eq!(m.target.len(), s.library.songs.len(), "the whole selection");
    assert_eq!(m.at, at);
    assert_eq!(m.list, Some(ListId::Songs));
}

#[test]
fn a_right_click_on_a_folder_row_opens_the_folder_menu() {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let size = iced::Size::new(1600.0, 950.0);
    let _ = s.update(Message::WindowResized(size));
    let _ = s.update(Message::Navigate(listen::app::Route::Folders));
    let name =
        s.extra.tree.children.get("").and_then(|c| c.last()).map(|f| f.name.clone()).expect("tree root");
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), size, s.view());
        let found = ui.find(name.as_str()).expect("a folder row");
        let at = iced_test::selector::Bounded::visible_bounds(&found).unwrap().center();
        ui.point_at(at);
        let _ = ui.simulate([
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Right)),
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Right)),
        ]);
        ui.into_messages().collect()
    };
    let _ = feed(&mut s, messages);
    let m = s.menu.as_ref().expect("the folder menu opened");
    assert!(
        matches!(&m.target, MenuTarget::Folders(f) if f.len() == 1 && f[0].name == name),
        "{:?}",
        m.target
    );
}

// --- snapshots ---

fn snapshot(name: &str, s: &Listen) {
    let theme = s.theme();
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), iced::Size::new(900.0, 700.0), s.view());
    let shot = ui.snapshot(&theme).expect("snapshot renders");
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    assert!(shot.matches_image(dir.join(name)).expect("png written"));
}

#[test]
fn snapshots_of_every_kinds_menu() {
    for (file, kind_name, set_name, submenu) in [
        ("menu-song", "song", "navidrome", Some(0)),
        ("menu-songs-multi", "songs", "navidrome", None),
        ("menu-queue", "queue", "navidrome", Some(3)),
        ("menu-album", "album", "navidrome", Some(5)),
        ("menu-artist", "artist", "navidrome", Some(1)),
        ("menu-genre", "genre", "navidrome", None),
        ("menu-folder", "folder", "navidrome", None),
        ("menu-song-ytm", "song", "ytm", Some(4)),
    ] {
        let (mut s, _, kind) = browsing(set_name);
        let _ = s.update(Message::WindowResized(iced::Size::new(900.0, 700.0)));
        let _ = s.update(Message::RightClickAt(iced::Point::new(120.0, 40.0)));
        let _ = menu(&mut s, MenuMsg::Open(target(kind_name, kind)));
        if let Some(i) = submenu {
            let _ = menu(&mut s, MenuMsg::Hover(i));
        }
        snapshot(file, &s);
    }
}
