//! Right-click menus, the manual queue and the expandable cover.

use iced::mouse::{Button, Event as MouseEvent};
use iced_test::simulator::Simulator;
use listen::app::menu::{MenuAction, MenuMsg, MenuTarget};
use listen::app::{INTER, Listen, Message};
use listen::{Args, FONTS, demo};
use std::path::PathBuf;

/// Held while a `Simulator` lives: creating several Vulkan instances at once races in the
/// Vulkan loader and intermittently crashes the process.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args {
        demo: true,
        show_queue: true,
        theme: Some("tokyoNight".into()),
        ..Args::default()
    });
    s.queue.set(demo::album_songs(0), 0);
    s
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

/// Right-clicks at `at` and feeds the resulting messages back into the app.
fn right_click(s: &mut Listen, at: (f32, f32)) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 950.0), s.view());
        ui.point_at(at);
        let _ = ui.simulate([
            iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Right)),
            iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Right)),
        ]);
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn titles(s: &Listen) -> Vec<String> {
    s.queue.iter().map(|i| i.song.title.clone()).collect()
}

fn top(s: &Listen) -> f32 {
    if s.config.settings.window.custom_title_bar { 31.0 } else { 0.0 }
}

#[test]
fn right_click_on_a_queue_row_opens_its_menu_at_the_cursor() {
    let mut s = state();
    let third = s.queue.iter().nth(2).unwrap().uid;
    // Queue panel on the right: header 65 px, mode switcher 32 px, toolbar 44 px, rows 64 px.
    let at = (1600.0 - 250.0, top(&s) + 65.0 + 32.0 + 44.0 + 64.0 * 2.0 + 32.0);
    let messages = right_click(&mut s, at);
    assert!(matches!(messages.first(), Some(Message::RightClickAt(_))), "{messages:?}");
    let menu = s.menu.as_ref().expect("menu open");
    assert_eq!((menu.at.x, menu.at.y), at);
    assert!(matches!(&menu.target, MenuTarget::Songs { queue_uids, .. } if *queue_uids == [third]));
    // A right-click on the backdrop closes it.
    let _ = right_click(&mut s, (800.0, 40.0));
    let _ = s.update(Message::Menu(MenuMsg::Close));
    assert!(s.menu.is_none());
}

#[test]
fn add_to_queue_goes_after_the_manual_queue_and_steers_the_dj() {
    let mut s = state();
    let before = titles(&s);
    let other = demo::album_songs(1);
    let queue = |s: &mut Listen, song: &listen_core::model::Song, action: MenuAction| {
        let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::song(song))));
        let _ = s.update(Message::Menu(MenuMsg::Do(action)));
        assert!(s.menu.is_none(), "the menu closes after an action");
    };
    queue(&mut s, &other[0], MenuAction::AddToQueue);
    queue(&mut s, &other[1], MenuAction::AddToQueue);
    queue(&mut s, &other[2], MenuAction::PlayNext);
    let now = titles(&s);
    assert_eq!(now[0], before[0]);
    assert_eq!(now[1..4], [other[2].title.clone(), other[0].title.clone(), other[1].title.clone()]);
    assert_eq!(now[4..], before[1..]);
    assert_eq!(s.queue.manual_len(), 3);
    assert_eq!(s.dj.listener_picks.len(), 3, "queued songs steer the AI DJ");
    // Remove from queue.
    let uid = s.queue.iter().nth(2).unwrap().uid;
    let song = s.queue.iter().nth(2).unwrap().song.clone();
    let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::queue_item(&song, uid))));
    let _ = s.update(Message::Menu(MenuMsg::Do(MenuAction::RemoveFromQueue(vec![uid]))));
    assert_eq!(s.queue.manual_len(), 2);
}

#[test]
fn cover_arrow_expands_the_cover_into_the_sidebar() {
    let mut s = state();
    assert!(!s.config.settings.sidebar.cover_expanded);
    // The player-bar cover: bottom left, 60 px; the arrow sits in its top-right corner.
    let bar_top = 950.0 - 90.0;
    let cover = (16.0, bar_top + 15.0);
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 950.0), s.view());
        ui.point_at((cover.0 + 48.0, cover.1 + 13.0));
        let _ = ui.simulate(iced_test::simulator::click());
        ui.into_messages().collect()
    };
    assert!(messages.iter().any(|m| matches!(m, Message::ToggleCoverExpanded)), "{messages:?}");
    let _ = s.update(Message::ToggleCoverExpanded);
    assert!(s.config.settings.sidebar.cover_expanded);
}

fn snapshot(name: &str, s: &Listen) {
    let theme = s.theme();
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 950.0), s.view());
    let shot = ui.snapshot(&theme).expect("snapshot renders");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    assert!(shot.matches_image(dir.join(name)).expect("png written"));
}

#[test]
fn snapshot_menu_and_expanded_cover() {
    let mut s = state();
    let _ = s.update(Message::ToggleCoverExpanded);
    let song = demo::album_songs(1)[0].clone();
    let _ = s.update(Message::Menu(MenuMsg::Do(MenuAction::AddToQueue))); // no menu: ignored
    let _ = s.update(Message::RightClickAt(iced::Point::new(1450.0, 300.0)));
    let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::song(&song))));
    snapshot("context-menu", &s);
    // Collapsed sidebar: the cover floats next to the rail.
    let _ = s.update(Message::Menu(MenuMsg::Close));
    let _ = s.update(Message::ToggleSidebar);
    snapshot("cover-rail", &s);
}

/// Widget kinds of the view, depth first.
fn tags(s: &Listen) -> Vec<iced::advanced::widget::tree::Tag> {
    fn walk(tree: &iced::advanced::widget::Tree, out: &mut Vec<iced::advanced::widget::tree::Tag>) {
        out.push(tree.tag);
        tree.children.iter().for_each(|c| walk(c, out));
    }
    let view = s.view();
    let mut out = Vec::new();
    walk(&iced::advanced::widget::Tree::new(&view), &mut out);
    out
}

#[test]
fn opening_the_menu_keeps_the_page_widgets_and_their_scroll_position() {
    let mut s = state();
    let closed = tags(&s);
    let _ = s.update(Message::RightClickAt(iced::Point::new(800.0, 300.0)));
    let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::song(&demo::album_songs(1)[0]))));
    let open = tags(&s);
    // The menu only inserts widgets; a changed tree around them would reset
    // every scrollable to the top.
    let added = open.len() - closed.len();
    assert!(added > 0);
    let at = closed.iter().zip(&open).take_while(|(a, b)| a == b).count();
    assert_eq!(&open[..at], &closed[..at]);
    assert_eq!(&open[at + added..], &closed[at..]);
}

// --- Playlist entries per source (capability flags) ---

mod caps {
    use super::*;
    use async_trait::async_trait;
    use listen::app::SourceHandle;
    use listen_core::config::ServerConfig;
    use listen_core::model::{
        Album, Artist, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
    };
    use listen_source::{
        AlbumQuery, Capabilities, ListQuery, MusicSource, Result, SongQuery, SourceError, StreamSource,
    };
    use std::sync::Arc;

    /// Answers nothing; only `kind` and `capabilities` matter for the menu.
    struct Fake(SourceKind, Capabilities);

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
            Ok(vec![])
        }
        async fn playlist_songs(&self, _: &str) -> Result<Vec<Song>> {
            Ok(vec![])
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
    }

    fn playlist() -> Playlist {
        Playlist {
            id: "p1".into(),
            server_id: ServerId("srv".into()),
            source: SourceKind::YouTubeMusic,
            name: "Mine".into(),
            description: None,
            owner: None,
            image: None,
            song_count: Some(3),
            duration: std::time::Duration::ZERO,
            public: false,
            editable: true,
        }
    }

    /// An app with one connected source of `kind` having `caps`, and the
    /// context menu open on a song of it, a playlist of it, or neither.
    fn with_source(kind: SourceKind, caps: Capabilities) -> Listen {
        let (mut s, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
        let id = ServerId("srv".into());
        s.config.servers = vec![ServerConfig {
            id: id.clone(),
            name: "srv".into(),
            kind,
            url: "https://x.example".into(),
            username: "me".into(),
        }];
        let _ = s.update(Message::Connected(id.clone(), Ok(SourceHandle(Arc::new(Fake(kind, caps))))));
        let _ = s.update(Message::WindowResized(iced::Size::new(1200.0, 800.0)));
        s
    }

    fn song_menu(s: &mut Listen, kind: SourceKind) {
        let mut song = demo::album_songs(0).remove(0);
        song.server_id = ServerId("srv".into());
        song.source = kind;
        let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::song(&song))));
    }

    fn shows(s: &Listen, text: &str) -> bool {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), iced::Size::new(1200.0, 800.0), s.view());
        ui.find(text).is_ok()
    }

    #[test]
    fn youtube_music_keeps_add_to_playlist_rename_and_delete() {
        let mut s = with_source(SourceKind::YouTubeMusic, listen_ytmusic::capabilities(false));
        song_menu(&mut s, SourceKind::YouTubeMusic);
        assert!(shows(&s, "Add to playlist…"));
        assert!(shows(&s, "Favorite"));
        let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::playlist(&playlist()))));
        assert!(shows(&s, "Edit playlist…"));
        assert!(shows(&s, "Delete playlist…"));
        // The picker lets you create a playlist.
        song_menu(&mut s, SourceKind::YouTubeMusic);
        let _ = s.update(Message::Menu(MenuMsg::PlaylistsLoaded(Ok(vec![]))));
        let _ = s.update(Message::Menu(MenuMsg::Do(MenuAction::ChoosePlaylist)));
        assert!(shows(&s, "Create"));
    }

    #[test]
    fn navidrome_keeps_add_to_playlist_rename_and_delete() {
        let mut s = with_source(SourceKind::Subsonic, listen_subsonic::capabilities());
        song_menu(&mut s, SourceKind::Subsonic);
        assert!(shows(&s, "Add to playlist…"));
        let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::playlist(&playlist()))));
        assert!(shows(&s, "Edit playlist…"));
        assert!(shows(&s, "Delete playlist…"));
    }

    #[test]
    fn a_youtube_music_guest_gets_no_playlist_entries() {
        let mut s = with_source(SourceKind::YouTubeMusic, listen_ytmusic::capabilities(true));
        song_menu(&mut s, SourceKind::YouTubeMusic);
        assert!(!shows(&s, "Add to playlist…"));
    }

    #[test]
    fn jellyfin_offers_adding_but_not_create_rename_or_delete() {
        let mut s = with_source(SourceKind::Jellyfin, listen_jellyfin::capabilities());
        song_menu(&mut s, SourceKind::Jellyfin);
        assert!(shows(&s, "Add to playlist…"));
        let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::playlist(&playlist()))));
        assert!(!shows(&s, "Rename…"));
        assert!(!shows(&s, "Delete playlist…"));
        song_menu(&mut s, SourceKind::Jellyfin);
        let _ = s.update(Message::Menu(MenuMsg::Do(MenuAction::ChoosePlaylist)));
        let _ = s.update(Message::Menu(MenuMsg::PlaylistsLoaded(Ok(vec![]))));
        assert!(!shows(&s, "Create"), "Jellyfin cannot create playlists");
    }
}
