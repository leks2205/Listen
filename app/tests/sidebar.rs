//! The sidebar: playlist list (toggle, filter, folders, manual order, shared),
//! collections, the collapsed rail, settings reorder and the app menu.

use iced_test::simulator::Simulator;
use listen::app::sidebar_state::{DragKind, Popup, Scope, SidebarMsg, SidebarSetting};
use listen::app::{INTER, Listen, Message, Route};
use listen::{Args, FONTS};
use listen_core::model::{Playlist, ServerId, SourceKind};
use listen_core::settings::{Collection, FolderView};
use std::time::Duration;

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

const SIZE: iced::Size = iced::Size::new(1600.0, 1500.0);

fn playlist(id: &str, name: &str, owner: Option<&str>, source: SourceKind) -> Playlist {
    Playlist {
        id: id.into(),
        server_id: ServerId("s".into()),
        source,
        name: name.into(),
        description: None,
        owner: owner.map(Into::into),
        image: None,
        song_count: Some(3),
        duration: Duration::from_secs(600),
        public: false,
        editable: owner.is_none(),
    }
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let _ = s.update(Message::WindowResized(SIZE));
    s.library.playlists = vec![
        playlist("liked", "Liked Music", None, SourceKind::YouTubeMusic),
        playlist("uploads", "Uploads", None, SourceKind::YouTubeMusic),
        playlist("a", "Alpha", None, SourceKind::Subsonic),
        playlist("b", "Remixes/Bravo", None, SourceKind::Subsonic),
        playlist("c", "Charlie", None, SourceKind::Subsonic),
        playlist("amy", "Amy's picks", Some("amy"), SourceKind::Subsonic),
    ];
    s.config.settings.lists.collections = vec![
        Collection { name: "Unrated".into(), ..Default::default() },
        Collection { name: "Loved".into(), ..Default::default() },
    ];
    s
}

fn shows(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), SIZE, s.view());
    ui.find(text).is_ok()
}

fn click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        ui.click(text).unwrap_or_else(|e| panic!("no {text:?} on screen: {e:?}"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn set(s: &mut Listen, x: SidebarSetting) {
    let _ = s.update(Message::Sidebar(SidebarMsg::Setting(x)));
}

fn sb(s: &mut Listen, m: SidebarMsg) {
    let _ = s.update(Message::Sidebar(m));
}

fn ids(s: &Listen, scope: Scope) -> Vec<String> {
    s.sidebar_playlists(scope).iter().map(|p| p.id.clone()).collect()
}

#[test]
fn ytm_playlists_show_open_and_survive_every_option() {
    let mut s = state();
    assert!(shows(&s, "Liked Music") && shows(&s, "Uploads"));
    let messages = click_text(&mut s, "Liked Music");
    assert!(
        messages.iter().any(|m| matches!(m, Message::Navigate(Route::Playlist(id)) if id == "liked")),
        "{messages:?}"
    );
    assert_eq!(s.route, Route::Playlist("liked".into()));
    // Compact, folders and manual sorting still list them.
    set(&mut s, SidebarSetting::Mode(listen_core::settings::PlaylistMode::Compact));
    set(&mut s, SidebarSetting::Folders(true));
    set(&mut s, SidebarSetting::Sorting(true));
    assert!(shows(&s, "Liked Music") && shows(&s, "Uploads"));
}

#[test]
fn playlist_list_toggle_hides_both_lists() {
    let mut s = state();
    assert!(shows(&s, "Amy's picks") && shows(&s, "Shared Playlists"));
    set(&mut s, SidebarSetting::PlaylistList(false));
    assert!(!shows(&s, "Alpha") && !shows(&s, "Amy's picks"));
    assert!(shows(&s, "My Library"));
}

#[test]
fn regex_hides_matches_and_a_bad_one_hides_nothing() {
    let mut s = state();
    set(&mut s, SidebarSetting::FilterRegex("^ALPHA|charlie".into()));
    assert_eq!(ids(&s, Scope::Owned), ["liked", "uploads", "b"]);
    set(&mut s, SidebarSetting::FilterRegex("(".into()));
    assert_eq!(ids(&s, Scope::Owned).len(), 5);
    assert_eq!(ids(&s, Scope::Shared), ["amy"]);
}

#[test]
fn folders_collapse_and_open_in_the_tree() {
    let mut s = state();
    set(&mut s, SidebarSetting::Folders(true));
    assert!(shows(&s, "Remixes") && !shows(&s, "Bravo"));
    click_text(&mut s, "Remixes");
    assert!(s.sb.folder_open(Scope::Owned, "Remixes"));
    assert!(shows(&s, "Bravo"));
    click_text(&mut s, "Remixes");
    assert!(!shows(&s, "Bravo"));
}

#[test]
fn navigation_view_enters_a_folder_and_goes_back() {
    let mut s = state();
    set(&mut s, SidebarSetting::Folders(true));
    set(&mut s, SidebarSetting::FolderView(FolderView::Navigation));
    click_text(&mut s, "Remixes");
    assert_eq!(s.sb.nav(Scope::Owned), &vec!["Remixes".to_string()]);
    assert!(shows(&s, "Bravo") && !shows(&s, "Alpha"));
    sb(&mut s, SidebarMsg::NavUp(Scope::Owned));
    assert!(shows(&s, "Alpha"));
}

#[test]
fn manual_order_is_dragged_and_saved() {
    let mut s = state();
    set(&mut s, SidebarSetting::Sorting(true));
    sb(&mut s, SidebarMsg::DragStart(DragKind::Playlist(Scope::Owned), "liked".into()));
    sb(&mut s, SidebarMsg::DragOver("a".into()));
    sb(&mut s, SidebarMsg::DragEnd);
    assert_eq!(ids(&s, Scope::Owned), ["uploads", "a", "liked", "b", "c"]);
    assert_eq!(s.config.settings.sidebar.playlist_order.len(), 1);
    // A click without moving opens the playlist.
    sb(&mut s, SidebarMsg::DragStart(DragKind::Playlist(Scope::Owned), "c".into()));
    sb(&mut s, SidebarMsg::DragEnd);
    assert_eq!(s.route, Route::Playlist("c".into()));
    // Turning sorting off goes back to the source's order.
    set(&mut s, SidebarSetting::Sorting(false));
    assert_eq!(ids(&s, Scope::Owned)[0], "liked");
}

#[test]
fn shared_and_collections_sections_fold() {
    let mut s = state();
    assert!(shows(&s, "Unrated") && shows(&s, "Amy's picks"));
    click_text(&mut s, "Collections");
    assert!(!shows(&s, "Unrated"));
    click_text(&mut s, "Shared Playlists");
    assert!(!shows(&s, "Amy's picks"));
}

#[test]
fn collection_rename_and_delete() {
    let mut s = state();
    sb(&mut s, SidebarMsg::OpenCollection(0, iced::Point::new(200.0, 400.0)));
    assert!(shows(&s, "Rename") && shows(&s, "Delete"));
    click_text(&mut s, "Rename");
    sb(&mut s, SidebarMsg::RenameInput("Never played".into()));
    sb(&mut s, SidebarMsg::RenameSave);
    assert_eq!(s.config.settings.lists.collections[0].name, "Never played");
    assert!(s.sb.popup.is_none());
    sb(&mut s, SidebarMsg::OpenCollection(1, iced::Point::new(200.0, 400.0)));
    click_text(&mut s, "Delete");
    assert_eq!(s.config.settings.lists.collections.len(), 1);
}

#[test]
fn collapsed_rail_navigation_toggle() {
    let mut s = state();
    let _ = s.update(Message::ToggleSidebar);
    assert!(s.config.settings.sidebar.collapsed);
    assert!(shows(&s, "Menu") && shows(&s, "Collections"));
    set(&mut s, SidebarSetting::CollapsedNavigation(false));
    assert!(!s.config.settings.sidebar.collapsed_navigation);
    set(&mut s, SidebarSetting::PlaylistList(false));
    assert!(shows(&s, "Menu"));
}

#[test]
fn app_menu_opens_navigates_and_closes() {
    let mut s = state();
    sb(&mut s, SidebarMsg::OpenAppMenu(iced::Point::new(300.0, 70.0)));
    assert!(matches!(s.sb.popup, Some(Popup::AppMenu(_))));
    for text in ["Open command palette", "Collapse sidebar", "Settings", "Quit"] {
        assert!(shows(&s, text), "{text}");
    }
    click_text(&mut s, "Settings");
    assert_eq!(s.route, Route::Settings);
    assert!(s.sb.popup.is_none());

    sb(&mut s, SidebarMsg::OpenAppMenu(iced::Point::new(300.0, 70.0)));
    click_text(&mut s, "Collapse sidebar");
    assert!(s.config.settings.sidebar.collapsed);
    sb(&mut s, SidebarMsg::OpenAppMenu(iced::Point::new(100.0, 70.0)));
    assert!(shows(&s, "Expand sidebar") && shows(&s, "Go back"));
}

#[test]
fn menu_button_opens_the_app_menu() {
    let s = state();
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        // The menu button is the second of the four squares right of the search box.
        ui.point_at((272.0, 62.0));
        let _ = ui.simulate(iced_test::simulator::click());
        ui.into_messages().collect()
    };
    assert!(
        messages.iter().any(|m| matches!(m, Message::Sidebar(SidebarMsg::OpenAppMenu(_)))),
        "{messages:?}"
    );
}

#[test]
fn settings_reorder_and_visibility() {
    let mut s = state();
    let order = |s: &Listen| s.config.settings.sidebar.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let before = order(&s);
    sb(&mut s, SidebarMsg::DragStart(DragKind::Item, "Radio".into()));
    sb(&mut s, SidebarMsg::DragOver("Home".into()));
    sb(&mut s, SidebarMsg::DragEnd);
    let after = order(&s);
    let home = before.iter().position(|i| i == "Home").unwrap();
    assert_eq!(after[home], "Radio");
    assert_eq!(after.len(), before.len());
    // The rows are listed on the settings page.
    let _ = s.update(Message::Navigate(Route::Settings));
    assert!(shows(&s, "Sidebar configuration") && shows(&s, "Sidebar playlist sorting"));
    // Folder options appear only with folders on.
    assert!(!shows(&s, "Folder separator"));
    set(&mut s, SidebarSetting::Folders(true));
    assert!(shows(&s, "Folder separator") && shows(&s, "Tree indent"));
}

#[test]
fn empty_separator_is_not_applied() {
    let mut s = state();
    set(&mut s, SidebarSetting::Folders(true));
    set(&mut s, SidebarSetting::Separator(String::new()));
    assert_eq!(s.config.settings.sidebar.playlist_folder_separator, "/");
    set(&mut s, SidebarSetting::Separator("::".into()));
    assert_eq!(s.config.settings.sidebar.playlist_folder_separator, "::");
}
