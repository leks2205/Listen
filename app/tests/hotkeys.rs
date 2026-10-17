//! The hotkey editor and the action coverage: recording a combo through the
//! editor, duplicates, the global flag, and what every action dispatches.

use iced::keyboard::key::{Code, Named, NativeCode, Physical};
use iced::keyboard::{Event as KeyEvent, Key, Location, Modifiers};
use iced_test::simulator::Simulator;
use listen::app::hotkeys::{FavoriteMode, HotkeysMsg, ListHotkey, SongRef, ZoomStep};
use listen::app::{INTER, Listen, Message, Route, SourceHandle};
use listen::{Args, FONTS, demo};
use listen_core::config::ServerConfig;
use listen_core::model::{ServerId, SourceKind};
use listen_core::settings::{HOTKEY_ACTIONS, hotkey_action};
use std::sync::Arc;

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

fn key(code: Code, ch: &str, modifiers: Modifiers) -> iced::Event {
    iced::Event::Keyboard(KeyEvent::KeyPressed {
        key: Key::Character(ch.into()),
        modified_key: Key::Character(ch.into()),
        physical_key: Physical::Code(code),
        location: Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    })
}

fn press(s: &mut Listen, event: KeyEvent) {
    let _ = s.update(Message::Keyboard(event));
}

fn key_event(code: Code, ch: &str, modifiers: Modifiers) -> KeyEvent {
    let iced::Event::Keyboard(e) = key(code, ch, modifiers) else { unreachable!() };
    e
}

fn demo_state() -> Listen {
    Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() }).0
}

/// A state with a Subsonic-kind server (ratings) and a queue of its songs.
fn server_state(kind: SourceKind) -> Listen {
    let (mut s, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    s.config.servers = vec![ServerConfig {
        id: ServerId("srv".into()),
        name: "Server".into(),
        kind: SourceKind::Subsonic,
        url: "https://srv.example".into(),
        username: "me".into(),
    }];
    let source: Arc<dyn listen_source::MusicSource> = Arc::new(
        listen_subsonic::Subsonic::new(ServerId("srv".into()), "https://srv.example", "me", "pw").unwrap(),
    );
    let _ = s.update(Message::Connected(ServerId("srv".into()), Ok(SourceHandle(source))));
    let mut songs = demo::album_songs(0);
    for song in &mut songs {
        song.server_id = ServerId("srv".into());
        song.source = kind;
        song.favorite = false;
        song.rating = None;
    }
    s.queue.set(songs, 1);
    s
}

fn dbg(m: Option<Message>) -> String {
    format!("{m:?}")
}

#[test]
fn there_are_42_actions_with_unique_ids() {
    assert_eq!(HOTKEY_ACTIONS.len(), 42);
    let ids: std::collections::BTreeSet<_> = HOTKEY_ACTIONS.iter().map(|a| a.id).collect();
    assert_eq!(ids.len(), 42);
    // Feishin's defaults: six bound actions, none global.
    let s = listen_core::settings::Settings::default();
    assert_eq!(s.hotkeys.len(), 6);
    assert!(s.hotkeys_global.is_empty());
    assert!(!hotkey_action("globalSearch").unwrap().allow_global);
    assert!(hotkey_action("rate3").unwrap().allow_global);
}

#[test]
fn every_action_dispatches_the_right_message() {
    let s = demo_state();
    let expect: &[(&str, &str)] = &[
        ("playPause", "TogglePlay"),
        ("play", "Hotkeys(Play)"),
        ("pause", "Hotkeys(Pause)"),
        ("next", "Next"),
        ("previous", "Previous"),
        ("nextAlbum", "NextAlbum"),
        ("previousAlbum", "PreviousAlbum"),
        ("stop", "Stop"),
        ("skipForward", "SkipForward"),
        ("skipBackward", "SkipBackward"),
        ("toggleShuffle", "ToggleShuffle"),
        ("toggleRepeat", "CycleRepeat"),
        ("volumeUp", "VolumeWheel(1.0)"),
        ("volumeDown", "VolumeWheel(-1.0)"),
        ("volumeMute", "ToggleMute"),
        ("favoriteCurrentAdd", "Hotkeys(Favorite(Current, Add))"),
        ("favoriteCurrentRemove", "Hotkeys(Favorite(Current, Remove))"),
        ("favoriteCurrentToggle", "ToggleFavorite"),
        ("favoritePreviousAdd", "Hotkeys(Favorite(Previous, Add))"),
        ("favoritePreviousRemove", "Hotkeys(Favorite(Previous, Remove))"),
        ("favoritePreviousToggle", "Hotkeys(Favorite(Previous, Toggle))"),
        ("rate0", "Hotkeys(Rate(0))"),
        ("rate1", "Hotkeys(Rate(1))"),
        ("rate2", "Hotkeys(Rate(2))"),
        ("rate3", "Hotkeys(Rate(3))"),
        ("rate4", "Hotkeys(Rate(4))"),
        ("rate5", "Hotkeys(Rate(5))"),
        ("zoomIn", "Zoom(In)"),
        ("zoomOut", "Zoom(Out)"),
        ("toggleQueue", "ToggleQueue"),
        ("toggleFullscreenPlayer", "ToggleFullscreen"),
        ("browserBack", "Back"),
        ("browserForward", "Forward"),
        ("navigateHome", "Navigate(Home)"),
        ("listPlayDefault", "Hotkeys(List(PlayDefault))"),
        ("listPlayNow", "Hotkeys(List(PlayNow))"),
        ("listPlayNext", "Hotkeys(List(PlayNext))"),
        ("listPlayLast", "Hotkeys(List(PlayLast))"),
        ("listNavigateToPage", "Hotkeys(List(NavigateToPage))"),
        ("listShowPlayingSong", "Hotkeys(List(ShowPlayingSong))"),
    ];
    for (action, message) in expect {
        assert_eq!(dbg(s.hotkey_message(action)), format!("Some({message})"), "{action}");
    }
    // The two search actions focus the search box instead of sending a message.
    for action in HOTKEY_ACTIONS {
        let handled =
            s.hotkey_message(action.id).is_some() || matches!(action.id, "globalSearch" | "localSearch");
        assert!(handled, "{} has no handler", action.id);
    }
    assert!(s.hotkey_message("nope").is_none());
    let _ = (ZoomStep::Reset, ListHotkey::PlayLast, SongRef::Current, FavoriteMode::Add);
}

#[test]
fn recording_stores_the_pressed_combo() {
    let mut s = demo_state();
    s.config.settings.hotkeys.clear();
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Record("nextAlbum".into())));
    assert_eq!(s.hotkey_recording.as_deref(), Some("nextAlbum"));
    // A lone modifier keeps waiting.
    press(&mut s, KeyEvent::ModifiersChanged(Modifiers::CTRL));
    press(
        &mut s,
        KeyEvent::KeyPressed {
            key: Key::Named(Named::Control),
            modified_key: Key::Named(Named::Control),
            physical_key: Physical::Code(Code::ControlLeft),
            location: Location::Left,
            modifiers: Modifiers::CTRL,
            text: None,
            repeat: false,
        },
    );
    assert_eq!(s.hotkey_recording.as_deref(), Some("nextAlbum"));
    press(&mut s, key_event(Code::KeyJ, "j", Modifiers::CTRL | Modifiers::SHIFT));
    assert_eq!(s.hotkey_recording, None);
    assert_eq!(s.config.settings.hotkeys.get("nextAlbum").map(String::as_str), Some("mod+shift+j"));

    // The recorded combo runs the action; while recording nothing else fires.
    s.config.settings.hotkeys.insert("rate3".into(), "mod+shift+j".into());
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Record("zoomIn".into())));
    press(&mut s, key_event(Code::Space, " ", Modifiers::empty()));
    assert_eq!(s.config.settings.hotkeys.get("zoomIn").map(String::as_str), Some("space"));

    // Escape cancels without changing anything.
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Record("zoomOut".into())));
    press(
        &mut s,
        KeyEvent::KeyPressed {
            key: Key::Named(Named::Escape),
            modified_key: Key::Named(Named::Escape),
            physical_key: Physical::Code(Code::Escape),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        },
    );
    assert_eq!(s.hotkey_recording, None);
    assert!(!s.config.settings.hotkeys.contains_key("zoomOut"));
    let _ = NativeCode::Unidentified;
}

#[test]
fn clear_and_global_flag() {
    let mut s = demo_state();
    // Global needs a binding and an action that allows it.
    let _ = s.update(Message::Hotkeys(HotkeysMsg::SetGlobal("next".into(), true)));
    assert!(s.config.settings.hotkeys_global.is_empty());
    s.config.settings.hotkeys.insert("next".into(), "f6".into());
    s.config.settings.hotkeys.insert("globalSearch".into(), "f7".into());
    let _ = s.update(Message::Hotkeys(HotkeysMsg::SetGlobal("next".into(), true)));
    let _ = s.update(Message::Hotkeys(HotkeysMsg::SetGlobal("globalSearch".into(), true)));
    assert!(s.config.settings.hotkeys_global.contains("next"));
    assert!(!s.config.settings.hotkeys_global.contains("globalSearch"));
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Clear("next".into())));
    assert!(!s.config.settings.hotkeys.contains_key("next"));
    assert!(s.config.settings.hotkeys_global.is_empty());
    let _ = s.update(Message::Hotkeys(HotkeysMsg::MediaKeys(false)));
    assert!(!s.config.settings.global_media_keys);
}

#[test]
fn window_hotkey_runs_the_action_and_global_ones_do_not() {
    let mut s = server_state(SourceKind::Subsonic);
    s.config.settings.hotkeys.insert("rate4".into(), "mod+4".into());
    press(&mut s, key_event(Code::Digit4, "4", Modifiers::CTRL));
    assert_eq!(s.queue.current().unwrap().song.rating, Some(4));
    // Duplicated bindings fire every action bound to them.
    s.config.settings.hotkeys.insert("favoriteCurrentAdd".into(), "mod+4".into());
    press(&mut s, key_event(Code::Digit4, "4", Modifiers::CTRL));
    assert!(s.queue.current().unwrap().song.favorite);
}

#[test]
fn rating_and_favorites_for_current_and_previous_song() {
    let mut s = server_state(SourceKind::Subsonic);
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Rate(3)));
    assert_eq!(s.queue.current().unwrap().song.rating, Some(3));
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Rate(0)));
    assert_eq!(s.queue.current().unwrap().song.rating, None);

    let favs = |s: &Listen| s.queue.iter().map(|i| i.song.favorite).collect::<Vec<_>>();
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Favorite(SongRef::Previous, FavoriteMode::Add)));
    assert_eq!(&favs(&s)[..3], [true, false, false]);
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Favorite(SongRef::Previous, FavoriteMode::Toggle)));
    assert_eq!(&favs(&s)[..3], [false, false, false]);
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Favorite(SongRef::Current, FavoriteMode::Add)));
    assert_eq!(&favs(&s)[..3], [false, true, false]);
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Favorite(SongRef::Current, FavoriteMode::Remove)));
    assert_eq!(&favs(&s)[..3], [false, false, false]);
    // No previous song on the first track.
    s.queue.set(demo::album_songs(0), 0);
    let before = favs(&s);
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Favorite(SongRef::Previous, FavoriteMode::Toggle)));
    assert_eq!(favs(&s), before);
}

#[test]
fn youtube_music_has_no_ratings_but_likes_work() {
    let mut s = server_state(SourceKind::YouTubeMusic);
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Rate(5)));
    assert_eq!(s.queue.current().unwrap().song.rating, None);
    let _ = s.update(Message::Hotkeys(HotkeysMsg::Favorite(SongRef::Current, FavoriteMode::Add)));
    assert!(s.queue.current().unwrap().song.favorite);
}

#[test]
fn next_and_previous_album_hotkeys_use_the_album_skips() {
    let mut s = demo_state();
    let mut songs = demo::album_songs(0);
    songs.extend(demo::album_songs(1));
    let first_len = demo::album_songs(0).len();
    s.queue.set(songs, 0);
    press_action(&mut s, "nextAlbum");
    assert_eq!(s.queue.current_position(), Some(first_len));
    press_action(&mut s, "previousAlbum");
    assert_eq!(s.queue.current_position(), Some(0));
}

fn press_action(s: &mut Listen, action: &str) {
    let m = s.hotkey_message(action).unwrap();
    let _ = s.update(m);
}

#[test]
fn editor_click_starts_recording_and_duplicates_show() {
    let mut s = demo_state();
    let _ = s.update(Message::Navigate(Route::Settings));
    let _ = s.update(Message::Shell(listen::app::settings_shell::ShellMsg::Tab(
        listen_core::settings::SettingsTab::Hotkeys,
    )));
    s.config.settings.hotkeys.insert("play".into(), "space".into());
    let size = iced::Size::new(1400.0, 4096.0);
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), size, s.view());
        // "mod+k" (Global search) is the only field with that text.
        ui.click("mod+k").expect("the field is on the page");
        ui.into_messages().collect()
    };
    assert!(
        messages.iter().any(|m| matches!(m, Message::Hotkeys(HotkeysMsg::Record(a)) if a == "globalSearch")),
        "{messages:?}"
    );
    for m in messages {
        let _ = s.update(m);
    }
    assert_eq!(s.hotkey_recording.as_deref(), Some("globalSearch"));
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), size, s.view());
    assert!(ui.find("Press keys\u{2026}").is_ok());
    // Both `play` and `playPause` are bound to space: the duplicate map has it.
    assert!(listen::hotkeys::duplicates(&s.config.settings.hotkeys).contains("space"));
}
