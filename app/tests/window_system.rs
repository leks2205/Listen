//! Window system: saved window state, the Window tab, bar styles, the title
//! and minimizing without a tray.

use iced::window;
use iced_test::simulator::Simulator;
use listen::app::{INTER, Listen, Message, Route, Setting, WindowAction, WindowSetting, WindowStateMsg};
use listen::{Args, FONTS, demo};
use listen_core::settings::WindowBarStyle;

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    s.queue.set(demo::album_songs(0), 1);
    s
}

fn sim_settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn has_text(s: &Listen, size: iced::Size, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(sim_settings(), size, s.view());
    ui.find(text).is_ok()
}

fn change(s: &mut Listen, c: WindowSetting) {
    let _ = s.update(Message::Setting(Setting::Window(c)));
}

#[test]
fn window_tab_rows() {
    let mut s = state();
    let _ = s.update(Message::Navigate(Route::Settings));
    let _ = s.update(Message::Shell(listen::app::settings_shell::ShellMsg::Tab(
        listen_core::settings::SettingsTab::Window,
    )));
    let big = iced::Size::new(1600.0, 4096.0);
    for text in [
        "Window bar style",
        "Track info in window title",
        "Tray icon",
        "Minimize to tray",
        "Close to tray",
        "Start minimized",
        "Prevent display sleep while playing",
        "Prevent system sleep while playing",
        "Discord Rich Presence",
    ] {
        assert!(has_text(&s, big, text), "missing row: {text}");
    }
    // Tray rows go away with the tray.
    let _ = s.update(Message::Setting(Setting::Tray(false)));
    assert!(!has_text(&s, big, "Minimize to tray"));
    assert!(has_text(&s, big, "Prevent display sleep while playing"));
}

#[test]
fn settings_change_the_config() {
    let mut s = state();
    change(&mut s, WindowSetting::MinimizeToTray(true));
    change(&mut s, WindowSetting::StartMinimized(true));
    change(&mut s, WindowSetting::PreventDisplaySleep(true));
    change(&mut s, WindowSetting::PreventSystemSleep(true));
    change(&mut s, WindowSetting::TrackInfo(false));
    let w = &s.config.settings.window;
    assert!(w.minimize_to_tray && w.start_minimized && w.prevent_display_sleep && w.prevent_system_sleep);
    assert!(!w.title_bar_track_info);
    change(&mut s, WindowSetting::BarStyle(WindowBarStyle::Native));
    assert!(!s.config.settings.window.custom_title_bar);
    change(&mut s, WindowSetting::BarStyle(WindowBarStyle::Macos));
    assert_eq!(s.config.settings.window.bar_style(), WindowBarStyle::Macos);
}

#[test]
fn title_follows_playback_and_settings() {
    let mut s = state();
    let playing = s.title();
    // The demo is paused until something plays.
    assert!(playing.starts_with("Paused (2 / "), "{playing}");
    assert!(playing.ends_with(" — Listen"), "{playing}");
    change(&mut s, WindowSetting::TrackInfo(false));
    assert_eq!(s.title(), "Listen");
    change(&mut s, WindowSetting::TrackInfo(true));
    s.history_view.private = true;
    assert!(s.title().ends_with("— Listen (Private mode)"));
}

#[test]
fn minimize_without_a_tray_still_minimizes() {
    let mut s = state();
    change(&mut s, WindowSetting::MinimizeToTray(true));
    let main = s.main_window;
    let _ = s.update(Message::Window(WindowAction::Minimize));
    // No tray icon is running in a demo: hiding would strand the window.
    assert_eq!(s.main_window, main);
}

#[test]
fn window_geometry_is_tracked_and_saved() {
    let mut s = state();
    let id = s.main_window.expect("main window");
    assert!(s.config.settings.window.state.is_none());
    let _ = s.update(Message::WindowEvent(id, window::Event::Resized(iced::Size::new(1234.0, 777.0))));
    let _ = s.update(Message::WindowEvent(id, window::Event::Moved(iced::Point::new(50.0, 60.0))));
    // The window reported it is a normal one; the debounce timer ran out.
    let _ = s.update(Message::WindowState(WindowStateMsg::Probe { maximized: false, fullscreen: false }));
    let _ = s.update(Message::WindowState(WindowStateMsg::Flush(1)));
    assert!(s.config.settings.window.state.is_none(), "stale timers are ignored");
    let _ = s.update(Message::WindowState(WindowStateMsg::Flush(2)));
    let saved = s.config.settings.window.state.expect("saved");
    assert_eq!((saved.width, saved.height), (1234.0, 777.0));
    assert_eq!((saved.x, saved.y), (Some(50.0), Some(60.0)));
    assert!(!saved.maximized);

    // Maximizing keeps the normal size and records the flag.
    let _ = s.update(Message::WindowEvent(id, window::Event::Resized(iced::Size::new(2560.0, 1400.0))));
    let _ = s.update(Message::WindowState(WindowStateMsg::Probe { maximized: true, fullscreen: false }));
    let _ = s.update(Message::WindowState(WindowStateMsg::Flush(3)));
    let saved = s.config.settings.window.state.expect("saved");
    assert_eq!((saved.width, saved.height), (1234.0, 777.0));
    assert!(saved.maximized);
}
