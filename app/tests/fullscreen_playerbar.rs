//! Full-screen player and player bar: panel tabs, Escape, settings, the
//! visualizer overlay, ratings, links, the right-hand buttons and the sleep timer.

use iced::keyboard::{Event as KeyEvent, Key, Location, Modifiers, key::Named};
use iced::mouse::{Button, Event as MouseEvent};
use iced_test::simulator::Simulator;
use listen::app::fullscreen_state::{FsSetting, FullscreenMsg};
use listen::app::sleep_timer::{SleepMode, SleepMsg};
use listen::app::{FullscreenTab, INTER, Listen, Message, Route, SourceHandle};
use listen::{Args, FONTS, demo};
use listen_core::model::{ServerId, SourceKind};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// See `compact_layout.rs`: one Vulkan instance at a time.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    s.queue.set(demo::album_songs(0), 0);
    let _ = s.update(Message::WindowResized(iced::Size::new(1600.0, 900.0)));
    s
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn fs(m: FullscreenMsg) -> Message {
    Message::Fullscreen(m)
}

fn sleep(m: SleepMsg) -> Message {
    fs(FullscreenMsg::Sleep(m))
}

fn escape() -> Message {
    Message::Keyboard(KeyEvent::KeyPressed {
        key: Key::Named(Named::Escape),
        modified_key: Key::Named(Named::Escape),
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.find(text).is_ok()
}

/// Left-clicks `at` and feeds the resulting messages back.
fn click(s: &mut Listen, at: (f32, f32)) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        ui.point_at(at);
        let _ = ui.simulate([
            iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)),
            iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Left)),
        ]);
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

/// Clicks the text `text` (found in the current view).
fn click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        ui.click(text).unwrap_or_else(|e| panic!("no {text:?}: {e:?}"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn connect(s: &mut Listen, kind: SourceKind, rating: Option<u8>) {
    let source: Arc<dyn listen_source::MusicSource> = Arc::new(
        listen_subsonic::Subsonic::new(ServerId("srv".into()), "https://srv.example", "me", "pw").unwrap(),
    );
    let _ = s.update(Message::Connected(ServerId("srv".into()), Ok(SourceHandle(source))));
    let mut songs = demo::album_songs(0);
    for song in &mut songs {
        song.server_id = ServerId("srv".into());
        song.source = kind;
        song.rating = rating;
    }
    s.queue.set(songs, 0);
}

#[test]
fn escape_closes_the_player_the_overlay_and_the_popover_in_turn() {
    let mut s = state();
    let _ = s.update(Message::ToggleFullscreen);
    assert!(s.fullscreen);
    // The sleep popover goes first, the player stays.
    let _ = s.update(sleep(SleepMsg::Toggle));
    let _ = s.update(escape());
    assert!(s.fullscreen && !s.fs.sleep_ui.open);
    let _ = s.update(escape());
    assert!(!s.fullscreen);
    // Nothing open: Escape does nothing.
    let _ = s.update(escape());
    assert!(!s.fullscreen);

    // The full-window visualizer shrinks back to the player it came from.
    let _ = s.update(Message::OpenFullscreenTab(FullscreenTab::Visualizer));
    let _ = s.update(fs(FullscreenMsg::ExpandVisualizer));
    assert!(s.fs.visualizer_expanded && !s.fullscreen);
    let _ = s.update(escape());
    assert!(!s.fs.visualizer_expanded && s.fullscreen);
    let _ = s.update(escape());
    assert!(!s.fullscreen);

    // Expanded from outside the player: shrinking leaves it closed.
    let _ = s.update(fs(FullscreenMsg::ExpandVisualizer));
    let _ = s.update(escape());
    assert!(!s.fs.visualizer_expanded && !s.fullscreen);
}

#[test]
fn panel_tabs_toggle_and_the_player_stays_open() {
    let mut s = state();
    // Closed: the lyrics button opens the player on the lyrics tab.
    let _ = s.update(Message::OpenFullscreenTab(FullscreenTab::Lyrics));
    assert!(s.fullscreen);
    assert_eq!(s.fullscreen_tab, FullscreenTab::Lyrics);
    // Open: it toggles the panel, not the overlay.
    let _ = s.update(Message::OpenFullscreenTab(FullscreenTab::Lyrics));
    assert!(s.fullscreen);
    assert_eq!(s.fullscreen_tab, FullscreenTab::Hidden);
    let _ = s.update(Message::OpenFullscreenTab(FullscreenTab::Lyrics));
    assert_eq!(s.fullscreen_tab, FullscreenTab::Lyrics);
    // The tab buttons switch and close the same way.
    let _ = s.update(fs(FullscreenMsg::TogglePanel(FullscreenTab::Related)));
    assert_eq!(s.fullscreen_tab, FullscreenTab::Related);
    let _ = s.update(fs(FullscreenMsg::TogglePanel(FullscreenTab::Related)));
    assert_eq!(s.fullscreen_tab, FullscreenTab::Hidden);
    // A closed panel is not a visible visualizer.
    assert!(!s.visualizer_wanted());
    let _ = s.update(fs(FullscreenMsg::TogglePanel(FullscreenTab::Visualizer)));
    assert!(s.visualizer_wanted());
}

#[test]
fn a_tab_button_click_toggles_the_panel() {
    let mut s = state();
    let _ = s.update(Message::OpenFullscreenTab(FullscreenTab::Queue));
    // The four tab buttons sit top right: Up Next, Related, Lyrics, Visualizer (36 px, 4 px apart).
    let right = 1600.0 - 32.0;
    let lyrics = (right - 40.0 - 18.0, 38.0 + 8.0 + 18.0);
    let messages = click(&mut s, lyrics);
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Fullscreen(FullscreenMsg::TogglePanel(FullscreenTab::Lyrics)))),
        "{messages:?}"
    );
    assert_eq!(s.fullscreen_tab, FullscreenTab::Lyrics);
    let messages = click(&mut s, lyrics);
    assert!(!messages.is_empty());
    assert_eq!(s.fullscreen_tab, FullscreenTab::Hidden);
}

#[test]
fn related_tab_shows_the_hook_list_and_a_placeholder() {
    let mut s = state();
    let _ = s.update(Message::OpenFullscreenTab(FullscreenTab::Related));
    assert!(has_text(&s, "No similar songs yet."));
    let id = s.queue.current().unwrap().song.id.clone();
    let similar = demo::album_songs(2);
    let first = similar[0].title.clone();
    let _ = s.update(fs(FullscreenMsg::RelatedLoading(true)));
    assert!(has_text(&s, "Looking for similar songs…"));
    let _ = s.update(fs(FullscreenMsg::SetRelated(id, similar.clone())));
    assert!(!s.fs.related_loading);
    assert!(has_text(&s, &first));
    // A list for another song is stale: placeholder again.
    let _ = s.update(fs(FullscreenMsg::SetRelated("other".into(), similar)));
    assert!(has_text(&s, "No similar songs yet."));
}

#[test]
fn the_gear_opens_the_settings_and_changes_apply() {
    let mut s = state();
    let _ = s.update(Message::ToggleFullscreen);
    // The gear is the second button top left.
    let messages = click(&mut s, (16.0 + 40.0 + 18.0, 38.0 + 8.0 + 18.0));
    assert!(
        messages.iter().any(|m| matches!(m, Message::Fullscreen(FullscreenMsg::OpenSettings))),
        "{messages:?}"
    );
    for text in ["Dynamic background", "Image blur", "Cover size", "Alignment", "Metadata badges"] {
        assert!(has_text(&s, text), "settings lack {text}");
    }
    let _ = s.update(fs(FullscreenMsg::Setting(FsSetting::Opacity(60))));
    let _ = s.update(fs(FullscreenMsg::Setting(FsSetting::CoverSize(10))));
    let _ = s.update(fs(FullscreenMsg::Setting(FsSetting::ImageBlur(9.0))));
    let f = &s.config.settings.fullscreen;
    assert_eq!((f.opacity, f.cover_size), (60, 55), "cover size is clamped to 55-100");
    assert_eq!(f.image_blur, 6.0);
    // Badges join / leave the shared list.
    let _ = s.update(fs(FullscreenMsg::Setting(FsSetting::Badge("bpm".into(), true))));
    assert!(s.config.settings.player_bar.info_items.contains(&"bpm".to_string()));
    let _ = s.update(fs(FullscreenMsg::Setting(FsSetting::Badge("bpm".into(), false))));
    assert!(!s.config.settings.player_bar.info_items.contains(&"bpm".to_string()));
}

#[test]
fn the_full_screen_player_shows_badges_and_links() {
    let mut s = state();
    let song = s.queue.current().unwrap().song.clone();
    let mut with_data = song.clone();
    with_data.bit_rate = Some(320);
    s.queue.for_each_song_mut(|x| {
        if x.id == song.id {
            x.bit_rate = Some(320);
        }
    });
    let _ = s.update(fs(FullscreenMsg::Setting(FsSetting::Badge("bit_rate".into(), true))));
    let _ = s.update(Message::OpenFullscreenTab(FullscreenTab::Hidden));
    assert!(has_text(&s, "320 kbps"));
    // The artist is a link; going to its page closes the player.
    let artist = song.artists[0].name.clone();
    let messages = click_text(&mut s, &artist);
    assert!(
        messages.iter().any(|m| matches!(m, Message::Navigate(Route::Artist(_))))
            || song.artists[0].id.is_none(),
        "{messages:?}"
    );
    if song.artists[0].id.is_some() {
        assert!(!s.fullscreen, "a page change closes the player");
    }
}

#[test]
fn the_bar_title_links_to_now_playing_and_the_menu_button_opens_the_song_menu() {
    let mut s = state();
    let title = s.queue.current().unwrap().song.title.clone();
    let messages = click_text(&mut s, &title);
    assert!(messages.iter().any(|m| matches!(m, Message::Navigate(Route::NowPlaying))), "{messages:?}");
}

#[test]
fn clicking_the_empty_bar_opens_the_player_only_with_the_setting() {
    let mut s = state();
    // Empty spot in the bar's left column, right of the text.
    let at = (s.window_size.width * 0.27, s.window_size.height - 70.0);
    let messages = click(&mut s, at);
    assert!(!messages.iter().any(|m| matches!(m, Message::ToggleFullscreen)), "{messages:?}");
    assert!(!s.fullscreen);
    let mut s = state();
    s.config.settings.player_bar.open_drawer = true;
    let messages = click(&mut s, at);
    assert!(messages.iter().any(|m| matches!(m, Message::ToggleFullscreen)), "{messages:?}");
    assert!(s.fullscreen);
}

#[test]
fn clicking_the_duration_toggles_the_remaining_time() {
    let mut s = state();
    let total = listen_core::model::format_duration(s.duration());
    assert!(has_text(&s, &total));
    let messages = click_text(&mut s, &total);
    assert!(
        messages.iter().any(|m| matches!(m, Message::Fullscreen(FullscreenMsg::ToggleRemaining))),
        "{messages:?}"
    );
    assert!(s.config.settings.player_bar.show_time_remaining);
    let left = s.duration().saturating_sub(s.playback.position);
    assert!(has_text(&s, &format!("-{}", listen_core::model::format_duration(left))));
    let _ = s.update(fs(FullscreenMsg::ToggleRemaining));
    assert!(has_text(&s, &total));
}

#[test]
fn ratings_show_for_subsonic_only_and_the_same_star_clears() {
    let mut s = state();
    assert!(!s.can_rate_current(), "demo songs have no source to rate on");
    connect(&mut s, SourceKind::Subsonic, Some(2));
    assert!(s.can_rate_current());
    let _ = s.update(fs(FullscreenMsg::Rate(4)));
    assert_eq!(s.queue.current().unwrap().song.rating, Some(4));
    let _ = s.update(fs(FullscreenMsg::Rate(4)));
    assert_eq!(s.queue.current().unwrap().song.rating, None);
    // Settings > Show ratings off hides them.
    s.config.settings.lists.show_ratings = false;
    assert!(!s.can_rate_current());
    s.config.settings.lists.show_ratings = true;
    // YouTube Music has no ratings, whatever the source claims.
    connect(&mut s, SourceKind::YouTubeMusic, None);
    assert!(!s.can_rate_current());
}

#[test]
fn right_controls_follow_the_setting() {
    let mut s = state();
    let (w, _, r_all) = (s.window_size.width, 0, listen::ui::player_bar::column_widths(&s, 1000.0).2);
    let _ = w;
    let index = |s: &Listen, id: &str| {
        s.config.settings.player_bar.right_controls.iter().position(|c| c.id == id).unwrap()
    };
    // Hide the queue button and the sleep timer.
    for id in ["queue", "sleep_timer"] {
        let i = index(&s, id);
        let _ = s.update(fs(FullscreenMsg::Setting(FsSetting::RightVisible(i, false))));
    }
    let r_less = listen::ui::player_bar::column_widths(&s, 1000.0).2;
    assert!(r_less < r_all, "{r_less} < {r_all}");
    assert_eq!(
        s.config.settings.player_bar.right_controls_ordered(),
        ["player_config", "visualizer", "lyrics", "favorite", "volume"]
    );
    // Moving the volume up two places (past the hidden queue button) puts it before the favourite.
    for _ in 0..2 {
        let i = index(&s, "volume");
        let _ = s.update(fs(FullscreenMsg::Setting(FsSetting::RightMove(i, -1))));
    }
    let order = s.config.settings.player_bar.right_controls_ordered();
    assert_eq!(order[order.len() - 2..], ["volume", "favorite"]);
}

#[test]
fn sleep_timer_popover_presets_and_countdown() {
    let mut s = state();
    let _ = s.update(Message::TogglePlay);
    assert!(s.playback.playing);
    // The button opens the popover.
    let _ = s.update(sleep(SleepMsg::Toggle));
    for text in ["Sleep timer", "End of song", "End of album", "5", "240", "Custom"] {
        assert!(has_text(&s, text), "popover lacks {text}");
    }
    let messages = click_text(&mut s, "30");
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Fullscreen(FullscreenMsg::Sleep(SleepMsg::Minutes(30)))))
    );
    let timer = s.sleep_timer.clone().expect("timer running");
    assert_eq!(timer.mode, SleepMode::Timed);
    assert_eq!(timer.remaining, Duration::from_secs(30 * 60));
    assert!(!s.fs.sleep_ui.open, "starting closes the popover");
    // The countdown is in the button.
    assert!(has_text(&s, "30:00"));
    // It ticks only while playing.
    s.sleep_timer.as_mut().unwrap().remaining = Duration::from_secs(2);
    let _ = s.update(Message::TogglePlay);
    assert!(!s.sleep_ticking());
    let _ = s.update(Message::TogglePlay);
    assert!(s.sleep_ticking());
    let _ = s.update(sleep(SleepMsg::Tick));
    assert!(s.sleep_timer.is_some(), "one second left");
    s.sleep_timer.as_mut().unwrap().remaining = Duration::from_secs(1);
    s.fs.sleep_tick = Instant::now().checked_sub(Duration::from_secs(1));
    let _ = s.update(sleep(SleepMsg::Tick));
    assert!(s.sleep_timer.is_none(), "time is up");
    assert!(!s.playback.playing, "and playback paused");
}

#[test]
fn sleep_timer_end_of_song_cancel_and_custom() {
    let mut s = state();
    let _ = s.update(Message::TogglePlay);
    let _ = s.update(sleep(SleepMsg::Toggle));
    let _ = click_text(&mut s, "End of song");
    assert_eq!(s.sleep_timer.as_ref().map(|t| t.mode), Some(SleepMode::EndOfSong));
    // The next song starting on its own pauses.
    let advance = Message::Deck(0, listen::playback::PlayerMsg::Event(listen_player::PlayerEvent::Advanced));
    let _ = s.update(advance.clone());
    assert!(s.sleep_timer.is_none() && !s.playback.playing);

    // Cancel from the popover.
    let _ = s.update(sleep(SleepMsg::EndOfSong));
    let _ = s.update(sleep(SleepMsg::Toggle));
    let _ = click_text(&mut s, "Cancel");
    assert!(s.sleep_timer.is_none());

    // Custom: 0 h 20 min 0 s by default; typing replaces, letters are dropped.
    let _ = s.update(sleep(SleepMsg::CustomOpen));
    let _ =
        s.update(sleep(SleepMsg::CustomEdit(listen::app::sleep_timer::CustomField::Seconds, "1x5".into())));
    assert_eq!(s.fs.sleep_ui.custom.as_ref().unwrap()[2], "15");
    let _ = s.update(sleep(SleepMsg::CustomStart));
    assert_eq!(s.sleep_timer.as_ref().unwrap().remaining, Duration::from_secs(20 * 60 + 15));
}

#[test]
fn sleep_timer_end_of_album_and_track_shuffle() {
    let mut s = state();
    let _ = s.update(Message::TogglePlay);
    let n = s.queue.len();
    assert!(n >= 3);
    let _ = s.update(sleep(SleepMsg::EndOfAlbum));
    assert_eq!(s.sleep_timer.as_ref().unwrap().mode, SleepMode::EndOfAlbum);
    let advance =
        || Message::Deck(0, listen::playback::PlayerMsg::Event(listen_player::PlayerEvent::Advanced));
    // Within the album the timer keeps waiting ...
    let _ = s.update(advance());
    assert!(s.sleep_timer.is_some() && s.playback.playing);
    // ... a song of another album ends it.
    s.queue.for_each_song_mut(|_| {});
    let other = demo::album_songs(3);
    s.queue.set(vec![s.queue.current().unwrap().song.clone(), other[0].clone()], 0);
    let _ = s.update(advance());
    assert!(s.sleep_timer.is_none() && !s.playback.playing);

    // Track shuffle disables the end of album.
    s.queue.set(demo::album_songs(0), 0);
    s.queue.toggle_shuffle();
    let _ = s.update(sleep(SleepMsg::EndOfAlbum));
    assert!(s.sleep_timer.is_none(), "no end of album under shuffle");
}

#[test]
fn the_idle_timeout_opens_the_player_while_playing() {
    let mut s = state();
    s.config.settings.fullscreen.auto_open_minutes = 1;
    let _ = s.update(Message::TogglePlay);
    let _ = s.update(fs(FullscreenMsg::IdleCheck));
    assert!(!s.fullscreen, "just had input");
    s.fs.last_input = Instant::now().checked_sub(Duration::from_secs(61)).unwrap();
    let _ = s.update(fs(FullscreenMsg::IdleCheck));
    assert!(s.fullscreen);
}

#[test]
fn expand_art_off_hides_the_arrow_and_collapses_the_sidebar_image() {
    let mut s = state();
    let _ = s.update(Message::ToggleCoverExpanded);
    assert!(s.config.settings.sidebar.cover_expanded);
    assert!(listen::ui::expanded_cover(&s, 240.0).is_some());
    let _ = s.update(fs(FullscreenMsg::Setting(FsSetting::ExpandArt(false))));
    assert!(!s.config.settings.sidebar.cover_expanded);
    assert!(listen::ui::expanded_cover(&s, 240.0).is_none());
}
