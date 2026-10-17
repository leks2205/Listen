//! The context-menu core: multi-target actions, submenus, keyboard and the
//! preview footer.

use async_trait::async_trait;
use iced::keyboard::{Event as KeyEvent, Key, Location, Modifiers, key::Named};
use iced::{Point, Task};
use iced_test::simulator::Simulator;
use listen::app::menu::{MenuAction, MenuKey, MenuMsg, MenuTarget};
use listen::app::{INTER, Listen, Message, SourceHandle};
use listen::{Args, FONTS, demo};
use listen_core::config::ServerConfig;
use listen_core::model::{
    Album, Artist, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
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
        .take(n)
        .map(|mut s| {
            s.server_id = ServerId("srv".into());
            s.source = kind;
            s.favorite = false;
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

fn shows(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), iced::Size::new(1200.0, 800.0), s.view());
    ui.find(text).is_ok()
}

fn press(s: &mut Listen, key: Named) {
    let event = KeyEvent::KeyPressed {
        key: Key::Named(key),
        modified_key: Key::Named(key),
        physical_key: iced::keyboard::key::Physical::Unidentified(
            iced::keyboard::key::NativeCode::Unidentified,
        ),
        location: Location::Standard,
        modifiers: Modifiers::default(),
        text: None,
        repeat: false,
    };
    let _ = run(s.update(Message::Keyboard(event)));
}

fn focus(s: &Listen) -> (Option<usize>, Option<(usize, Option<usize>)>) {
    let m = s.menu.as_ref().expect("menu open");
    (m.focus, m.sub.as_ref().map(|x| (x.index, x.focus)))
}

// --- multi-target actions ---

#[test]
fn like_and_add_to_playlist_apply_to_every_selected_ytm_song() {
    let (mut s, calls) = app(SourceKind::YouTubeMusic, listen_ytmusic::capabilities(false));
    let songs = songs_of(SourceKind::YouTubeMusic, 3);
    let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
    s.library.songs = songs.clone();

    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::songs(&songs)));
    assert!(shows(&s, "Favorite") && shows(&s, "Add to playlist…"));
    assert!(!shows(&s, "Set rating"), "YouTube Music has no ratings");
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::Favorite(true)));
    assert!(s.menu.is_none());
    assert!(s.library.songs.iter().all(|x| x.favorite), "the hearts change everywhere");
    {
        let mut got: Vec<String> =
            calls.lock().unwrap().favorites.iter().flat_map(|(i, _)| i.clone()).collect();
        got.sort();
        let mut want = ids.clone();
        want.sort();
        assert_eq!(got, want);
        assert!(calls.lock().unwrap().favorites.iter().all(|(_, f)| *f));
    }

    // Add to an existing playlist.
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::songs(&songs)));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::ChoosePlaylist));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::AddToPlaylist("p1".into(), "Mine".into())));
    assert_eq!(calls.lock().unwrap().added, [("p1".to_string(), ids.clone())]);

    // Create a new one with all of them.
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::songs(&songs)));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::ChoosePlaylist));
    let _ = menu(&mut s, MenuMsg::NewName("Fresh".into()));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::CreatePlaylist));
    assert_eq!(calls.lock().unwrap().created, [("Fresh".to_string(), ids)]);
}

#[test]
fn queue_actions_and_ratings_apply_to_all_targets() {
    let (mut s, calls) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 3);
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::songs(&songs)));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::AddToQueue));
    assert_eq!(s.queue.iter().count(), 3);

    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::songs(&songs)));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::Rate(4)));
    let rated = calls.lock().unwrap().ratings.clone();
    assert_eq!(rated.len(), 3);
    assert!(rated.iter().all(|(k, _, r)| *k == ItemKind::Song && *r == 4));

    // Albums (their songs are loaded): play next queues the songs of both.
    let mut albums = demo::albums();
    albums.truncate(2);
    for (i, a) in albums.iter_mut().enumerate() {
        a.songs = demo::album_songs(i + 1);
    }
    let want: usize = albums.iter().map(|a| a.songs.len()).sum();
    let before = s.queue.iter().count();
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::Albums(albums)));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::PlayNext));
    assert_eq!(s.queue.iter().count(), before + want);
}

// --- submenus ---

#[test]
fn a_submenu_opens_on_hover_toggles_on_click_and_closes_with_escape() {
    let (mut s, _) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 1);
    let _ = s.update(Message::RightClickAt(Point::new(300.0, 100.0)));
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::song(&songs[0])));
    assert!(!shows(&s, "Next (shuffled)"), "closed until asked");
    // Row 0 of a song menu is "Play ▸", row 1 "Start radio".
    let _ = menu(&mut s, MenuMsg::Hover(0));
    assert_eq!(focus(&s), (Some(0), Some((0, None))));
    assert!(shows(&s, "Next (shuffled)") && shows(&s, "Last"));
    // Another row closes it; a click on the submenu row opens and closes it.
    let _ = menu(&mut s, MenuMsg::Hover(1));
    assert!(s.menu.as_ref().unwrap().sub.is_none());
    let _ = menu(&mut s, MenuMsg::ToggleSub(0));
    assert!(s.menu.as_ref().unwrap().sub.is_some());
    let _ = menu(&mut s, MenuMsg::ToggleSub(0));
    assert!(s.menu.as_ref().unwrap().sub.is_none());
    // Escape closes the submenu first, then the menu.
    let _ = menu(&mut s, MenuMsg::ToggleSub(0));
    press(&mut s, Named::Escape);
    assert!(s.menu.as_ref().is_some_and(|m| m.sub.is_none()));
    press(&mut s, Named::Escape);
    assert!(s.menu.is_none());
    // A child runs its action and closes everything.
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::song(&songs[0])));
    let _ = menu(&mut s, MenuMsg::Hover(0));
    let _ = menu(&mut s, MenuMsg::Do(MenuAction::PlayNext));
    assert!(s.menu.is_none());
    assert_eq!(s.queue.iter().count(), 1);
}

#[test]
fn a_submenu_flips_to_the_left_at_the_window_edge() {
    let x_of = |at: f32, text: &str| {
        let (mut s, _) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
        let _ = menu(
            &mut s,
            MenuMsg::OpenAt(MenuTarget::song(&songs_of(SourceKind::Subsonic, 1)[0]), Point::new(at, 100.0)),
        );
        let _ = menu(&mut s, MenuMsg::Hover(0));
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), iced::Size::new(1200.0, 800.0), s.view());
        ui.find(text).expect("text").bounds().x
    };
    // "Start radio" is in the main panel, "Next (shuffled)" in the submenu.
    assert!(x_of(100.0, "Next (shuffled)") > x_of(100.0, "Start radio"), "right of the menu");
    assert!(x_of(900.0, "Next (shuffled)") < x_of(900.0, "Start radio"), "left of the menu");
}

// --- keyboard ---

#[test]
fn arrows_enter_and_left_drive_the_menu() {
    let (mut s, _) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 1);
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::song(&songs[0])));
    assert_eq!(focus(&s), (None, None));
    press(&mut s, Named::ArrowDown);
    assert_eq!(focus(&s).0, Some(0), "the Play row");
    press(&mut s, Named::ArrowUp);
    assert!(focus(&s).0.unwrap() > 3, "up from the first row wraps to the last");
    press(&mut s, Named::ArrowDown);
    assert_eq!(focus(&s).0, Some(0));
    press(&mut s, Named::ArrowRight);
    assert_eq!(focus(&s).1, Some((0, Some(0))), "into the submenu, on its first child");
    press(&mut s, Named::ArrowDown);
    assert_eq!(focus(&s).1, Some((0, Some(1))));
    press(&mut s, Named::ArrowLeft);
    assert_eq!(focus(&s).1, None);
    // Enter on the submenu row opens it, Enter on a child runs it ("Next").
    press(&mut s, Named::Enter);
    assert_eq!(focus(&s).1, Some((0, Some(0))));
    press(&mut s, Named::ArrowDown);
    press(&mut s, Named::Enter);
    assert!(s.menu.is_none());
    assert_eq!(s.queue.iter().count(), 1);
    // The keys are the menu's only while it is open.
    assert!(MenuKey::from_key(&Key::Named(Named::ArrowDown)).is_some());
}

// --- footer ---

#[test]
fn the_footer_names_the_item_or_counts_them() {
    let (mut s, _) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 3);
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::song(&songs[0])));
    assert!(shows(&s, &songs[0].title));
    let _ = menu(&mut s, MenuMsg::Open(MenuTarget::songs(&songs)));
    assert!(shows(&s, "+2 more"));
}
