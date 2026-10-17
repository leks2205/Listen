//! The settings page shell: remembered tab, search, reset to default, the
//! restart-required toast and the player-bar / sidebar / Discord controls.

use iced_test::simulator::Simulator;
use listen::app::overlay::Modal;
use listen::app::settings_shell::ShellMsg;
use listen::app::{INTER, Listen, Message};
use listen::{Args, FONTS};
use listen_core::config::ServerConfig;
use listen_core::model::{ServerId, SourceKind};
use listen_core::settings::{HomeFeatureStyle, Settings, SettingsTab};

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() });
    let _ = s.update(Message::Navigate(listen::app::Route::Settings));
    s
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 3000.0), s.view());
    ui.find(text).is_ok()
}

/// Clicks the widget showing `text` and feeds the messages back.
fn click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 3000.0), s.view());
        ui.click(text).expect("text is on the page");
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn search(s: &mut Listen, q: &str) {
    let _ = s.update(Message::Shell(ShellMsg::Search(q.into())));
}

#[test]
fn clicking_a_tab_switches_and_remembers_it() {
    let mut s = state();
    assert_eq!(s.config.settings.settings_tab, SettingsTab::General);
    assert!(has_text(&s, "Player bar button size"));
    assert!(!has_text(&s, "Gapless playback"));
    let sent = click_text(&mut s, "Playback");
    assert!(sent.iter().any(|m| matches!(m, Message::Shell(ShellMsg::Tab(SettingsTab::Playback)))));
    assert_eq!(s.config.settings.settings_tab, SettingsTab::Playback);
    assert!(has_text(&s, "Gapless playback"));
    assert!(!has_text(&s, "Player bar button size"));
    // The choice is part of the saved settings.
    let saved = toml::to_string(&s.config.settings).unwrap();
    let back: Settings = toml::from_str(&saved).unwrap();
    assert_eq!(back.settings_tab, SettingsTab::Playback);
}

#[test]
fn search_filters_rows_across_tabs_by_title_and_description() {
    let mut s = state();
    // Case-insensitive, matches the title, and finds rows on other tabs.
    search(&mut s, "VOLUME");
    assert!(has_text(&s, "Volume slider width"));
    assert!(has_text(&s, "Volume wheel step"));
    assert!(!has_text(&s, "Player bar button size"));
    assert!(!has_text(&s, "Gapless playback"));
    search(&mut s, "tray");
    assert!(has_text(&s, "Tray icon"), "Window tab row found while on the General tab");
    assert!(!has_text(&s, "Volume slider width"));
    // The description counts too.
    search(&mut s, "rich presence");
    assert!(has_text(&s, "Discord Rich Presence"));
    assert!(!has_text(&s, "Tray icon"));
    search(&mut s, "zzzz");
    assert!(has_text(&s, "No settings match your search."));
    search(&mut s, "");
    assert!(has_text(&s, "Player bar button size"));
}

#[test]
fn reset_to_default_confirms_then_keeps_servers() {
    let mut s = state();
    s.config.servers = vec![ServerConfig {
        id: ServerId("home".into()),
        name: "Home".into(),
        kind: SourceKind::Subsonic,
        url: "https://music.example".into(),
        username: "me".into(),
    }];
    s.config.settings.player_bar.button_size = 28;
    s.config.settings.appearance.theme = "nord".into();
    s.config.settings.settings_tab = SettingsTab::Window;
    click_text(&mut s, "Reset to default");
    assert!(matches!(s.overlay.modal, Some(Modal::Confirm(_))), "asks first");
    assert_eq!(s.config.settings.player_bar.button_size, 28, "nothing changed yet");
    let _ = s.update(Message::Overlay(listen::app::overlay::OverlayMsg::Confirmed));
    let defaults = Settings::default();
    assert_eq!(s.config.settings.player_bar, defaults.player_bar);
    assert_eq!(s.config.settings.appearance.theme, defaults.appearance.theme);
    assert_eq!(s.config.settings.settings_tab, SettingsTab::Window, "stays on the tab");
    assert_eq!(s.config.servers.len(), 1, "servers are kept");
}

#[test]
fn restart_required_toast_offers_relaunch() {
    let mut s = state();
    // No current setting needs a restart, so raise the toast directly.
    s.toast_restart_required("The window bar changes after restarting Listen.");
    let toast = s.overlay.toasts.last().expect("a toast");
    assert!(toast.persistent);
    assert_eq!(toast.title.as_deref(), Some("Restart required"));
    let (label, msg) = toast.action.as_ref().expect("a Relaunch action");
    assert_eq!(label, "Relaunch");
    assert!(matches!(**msg, Message::Shell(ShellMsg::Relaunch)));
    assert!(has_text(&s, "Relaunch"));
}

#[test]
fn player_bar_sidebar_home_and_discord_controls_change_settings() {
    let mut s = state();
    let send = |s: &mut Listen, m: ShellMsg| {
        let _ = s.update(Message::Shell(m));
    };
    send(&mut s, ShellMsg::ButtonSize(40));
    assert_eq!(s.config.settings.player_bar.button_size, 30, "clamped like Feishin");
    send(&mut s, ShellMsg::SkipButtons(false));
    send(&mut s, ShellMsg::SkipBack(15));
    send(&mut s, ShellMsg::SkipForward(30));
    send(&mut s, ShellMsg::VolumeWidth(120));
    send(&mut s, ShellMsg::VolumeWheelStep(0));
    let pb = &s.config.settings.player_bar;
    assert!(!pb.skip_buttons);
    assert_eq!((pb.skip_backward_secs, pb.skip_forward_secs), (15, 30));
    assert_eq!((pb.volume_width, pb.volume_wheel_step), (120, 1));

    let first = s.config.settings.sidebar.items[0].id.clone();
    let second = s.config.settings.sidebar.items[1].id.clone();
    send(&mut s, ShellMsg::SidebarMove(0, 1));
    assert_eq!(s.config.settings.sidebar.items[1].id, first);
    assert_eq!(s.config.settings.sidebar.items[0].id, second);
    send(&mut s, ShellMsg::SidebarMove(0, -1)); // already first: no-op
    assert_eq!(s.config.settings.sidebar.items[0].id, second);
    send(&mut s, ShellMsg::SidebarVisible(0, false));
    assert!(!s.config.settings.sidebar.items[0].visible);

    send(&mut s, ShellMsg::HomeFeature(false));
    send(&mut s, ShellMsg::HomeFeatureStyle(HomeFeatureStyle::Multiple));
    assert!(!s.config.settings.home.feature);
    assert_eq!(s.config.settings.home.feature_style, HomeFeatureStyle::Multiple);

    send(&mut s, ShellMsg::TitleBarTrackInfo(false));
    assert!(!s.config.settings.window.title_bar_track_info);
    send(&mut s, ShellMsg::DiscordIdInput("123".into()));
    assert_eq!(
        s.config.settings.window.discord_client_id, "1560790551337640127",
        "typing alone does not save"
    );
    send(&mut s, ShellMsg::DiscordIdSave);
    assert_eq!(s.config.settings.window.discord_client_id, "123");
    send(&mut s, ShellMsg::DiscordIdInput("  ".into()));
    send(&mut s, ShellMsg::DiscordIdSave);
    assert_eq!(s.config.settings.window.discord_client_id, "1560790551337640127", "empty = default");
}
