//! Renders Listen's main screens headlessly (tiny-skia) and writes PNGs to
//! `target/snapshots/` for visual review. Run with:
//! `cargo test -p listen --test snapshots -- --nocapture`

use iced_test::simulator::Simulator;
use listen::app::{INTER, Listen};
use listen::{Args, FONTS};
use std::path::PathBuf;

/// Held while a `Simulator` lives: creating and dropping several Vulkan instances at once races in the
/// Vulkan loader and occasionally crashes the test process.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn render(name: &str, args: Args) {
    let (state, _task) = Listen::boot(&args);
    render_state(name, state);
}

fn render_state(name: &str, state: Listen) {
    render_sized(name, state, iced::Size::new(1600.0, 950.0));
}

fn render_sized(name: &str, state: Listen, size: iced::Size) {
    let settings = iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    };
    let theme = state.theme();
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings, size, state.view());
    let snapshot = ui.snapshot(&theme).expect("snapshot renders");

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    // `matches_image` writes the PNG when none exists yet; remove the old one first.
    let path = dir.join(name);
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    assert!(snapshot.matches_image(&path).expect("png written"));
    println!("wrote {}-*.png", path.display());
}

fn demo(theme: &str, queue: bool) -> Args {
    Args { demo: true, show_queue: queue, theme: Some(theme.into()), ..Args::default() }
}

#[test]
fn home_tokyo_night() {
    render("home-tokyo-night", demo("tokyoNight", false));
}

#[test]
fn home_with_queue_default_dark() {
    render("home-queue-default-dark", demo("defaultDark", true));
}

#[test]
fn home_default_light() {
    render("home-default-light", demo("defaultLight", false));
}

#[test]
fn tracks_page_demo() {
    render("tracks-demo", Args { route: Some("tracks".into()), ..demo("tokyoNight", false) });
}

#[test]
fn albums_page_demo() {
    render("albums-demo", Args { route: Some("albums".into()), ..demo("tokyoNight", false) });
}

#[test]
fn genres_page_demo() {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Genres));
    render_state("genres-demo", state);
}

#[test]
fn genre_detail_demo() {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Genre("Synthwave".into())));
    render_state("genre-detail-demo", state);
}

#[test]
fn favorites_tabs_demo() {
    use listen::app::library_extra::ExtraMsg;
    use listen::app::list::FavoritesTab;
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Favorites));
    render_state("favorites-tracks-demo", state);
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Favorites));
    let _ = state.update(Message::Extra(ExtraMsg::FavTab(FavoritesTab::Albums)));
    render_state("favorites-albums-demo", state);
}

#[test]
fn folder_browser_demo() {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let id = listen::demo::albums()[0].id.clone();
    let _ = state.update(Message::Navigate(Route::Folder(Some(id.clone()))));
    let _ = state.update(Message::Extra(listen::app::library_extra::ExtraMsg::TreeToggle(
        listen::demo::albums()[1].id.clone(),
    )));
    render_state("folders-demo", state);
}

#[test]
fn login_page() {
    // Not demo and signed in nowhere: the sign-in page.
    render("login", Args { theme: Some("tokyoNight".into()), ..Args::default() });
}

fn fullscreen_state(tab: listen::app::FullscreenTab) -> Listen {
    use listen::app::Message;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::OpenFullscreenTab(tab));
    let id = state.queue.current().unwrap().song.id.clone();
    let lrc = "[00:00.50]Neon lights across the bay\n[00:05.00]We drive until the break of day\n[00:10.00]Midnight signal on the air\n[00:15.00]Afterglow is everywhere\n[00:20.00]Hold the wheel and hold me near\n[00:25.00]Only static left to hear";
    let _ = state.update(Message::LyricsLoaded(id, Some(listen_lyrics::parse_lrc(lrc))));
    let _ = state.update(Message::Deck(
        0,
        listen::playback::PlayerMsg::Event(listen_player::PlayerEvent::Position(
            std::time::Duration::from_secs(11),
        )),
    ));
    // A synthetic, music-like spectrum.
    let mut analyzer = listen_visualizer::Analyzer::new(listen_visualizer::Config::default());
    let samples: Vec<f32> = (0..16384)
        .flat_map(|i| {
            let t = i as f32 / 48_000.0;
            let v: f32 = [(60.0, 0.3), (220.0, 0.15), (880.0, 0.05), (3500.0, 0.01), (9000.0, 0.003)]
                .iter()
                .map(|(f, a)| a * (std::f32::consts::TAU * f * t).sin())
                .sum();
            [v, v]
        })
        .collect();
    analyzer.push_stereo(&samples);
    for _ in 0..8 {
        let _ = analyzer.frame();
    }
    let _ = state.update(listen::app::Message::Spectrum(analyzer.frame()));
    state
}

#[test]
fn fullscreen_lyrics() {
    render_state("fullscreen-lyrics", fullscreen_state(listen::app::FullscreenTab::Lyrics));
}

#[test]
fn fullscreen_visualizer() {
    render_state("fullscreen-visualizer", fullscreen_state(listen::app::FullscreenTab::Visualizer));
}

#[test]
fn settings_page_demo() {
    render("settings-demo", Args { route: Some("settings".into()), ..demo("tokyoNight", false) });
}

#[test]
fn servers_in_sidebar_and_settings() {
    use listen::app::{Message, Route};
    let mut state = servers_state();
    let _ = state.update(Message::ConfirmRemoveServer(Some(listen_core::model::ServerId("yt".into()))));
    let _ = state.update(Message::Navigate(Route::Settings));
    render_state("servers-settings", state);
}

/// The sign-in page lists saved servers too, so a broken one can be removed there.
#[test]
fn servers_on_sign_in_page() {
    let mut state = servers_state();
    let _ = state.update(listen::app::Message::AddServer);
    render_state("servers-on-sign-in", state);
}

fn servers_state() -> Listen {
    use listen::app::{Message, SourceHandle};
    use listen_core::config::ServerConfig;
    use listen_core::model::{ServerId, SourceKind};
    let (mut state, _) =
        Listen::boot(&Args { theme: Some("tokyoNight".into()), ephemeral: true, ..Args::default() });
    state.config.servers = vec![
        ServerConfig {
            id: ServerId("nd".into()),
            name: "music.example.com".into(),
            kind: SourceKind::Subsonic,
            url: "https://music.example.com".into(),
            username: "alex".into(),
        },
        ServerConfig {
            id: ServerId("yt".into()),
            name: "YouTube Music".into(),
            kind: SourceKind::YouTubeMusic,
            url: String::new(),
            username: String::new(),
        },
    ];
    let nd: std::sync::Arc<dyn listen_source::MusicSource> = std::sync::Arc::new(
        listen_subsonic::Subsonic::new(ServerId("nd".into()), "https://music.example.com", "alex", "x")
            .unwrap(),
    );
    let _ = state.update(Message::Connected(ServerId("nd".into()), Ok(SourceHandle(nd))));
    state
}

fn ytm_login_state(browser: Result<listen_ytmusic::browser::Profile, String>) -> Listen {
    use listen::app::{LoginMsg, Message};
    let (mut state, _) =
        Listen::boot(&Args { theme: Some("tokyoNight".into()), ephemeral: true, ..Args::default() });
    let _ = state.update(Message::Login(LoginMsg::Kind(listen_core::model::SourceKind::YouTubeMusic)));
    let _ = state.update(Message::Login(LoginMsg::BrowserFound(browser.map(|p| vec![(p, Ok(()))]))));
    state
}

#[test]
fn youtube_music_sign_in() {
    let profile = listen_ytmusic::browser::Profile {
        browser: "Ecosia".into(),
        profile: "Default".into(),
        engine: listen_ytmusic::browser::Engine::Chromium,
        cookies: "/home/me/.var/app/org.ecosia.Browser/config/EcosiaBrowser/Default/Cookies".into(),
        keyring_apps: vec![],
        page_id: None,
        channel: None,
    };
    render_state("login-youtube-music", ytm_login_state(Ok(profile)));
}

#[test]
fn youtube_music_channel_picker() {
    use listen_ytmusic::channels::Channel;
    let profile = listen_ytmusic::browser::Profile {
        browser: "Ecosia".into(),
        profile: "Default".into(),
        engine: listen_ytmusic::browser::Engine::Chromium,
        cookies: "/tmp/Cookies".into(),
        keyring_apps: vec![],
        page_id: None,
        channel: None,
    };
    let mut state = ytm_login_state(Ok(profile));
    let ch = |name: &str, handle: &str, songs, more| Channel {
        name: name.into(),
        handle: handle.into(),
        page_id: Some("1".into()),
        songs,
        more,
    };
    state.login.channels = vec![
        ch("Mixtape", "@mixtape", 26, true),
        ch("Alex", "@own", 0, false),
        ch("Side", "@side", 0, false),
    ];
    render_state("ytm-login-channels", state);
}

#[test]
fn youtube_music_browser_not_signed_in() {
    let e =
        "Ecosia isn't signed in to YouTube — open music.youtube.com in Ecosia and sign in, then try again";
    render_state("ytm-login-no-browser", ytm_login_state(Err(e.into())));
}

#[test]
fn youtube_music_cookie_alternative() {
    use listen::app::{LoginMsg, Message};
    let (mut state, _) =
        Listen::boot(&Args { theme: Some("tokyoNight".into()), ephemeral: true, ..Args::default() });
    let _ = state.update(Message::Login(LoginMsg::Kind(listen_core::model::SourceKind::YouTubeMusic)));
    let _ = state.update(Message::Login(LoginMsg::ToggleCookies));
    state.login.url = "~/Downloads/music.youtube.com_cookies.json".into();
    state.login.info = Some("✓ 27 youtube.com cookies loaded — click Connect.".into());
    render_state("ytm-login-cookies", state);
}

/// Home with feed shelves (songs, playlists) and Korean titles.
#[test]
fn home_feed_shelves() {
    use listen_core::model::Playlist;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let mut songs = listen::demo::album_songs(0);
    let titles = ["봄날", "Dynamite", "밤편지", "Love wins all", "좋은 날", "Hype Boy"];
    let artists = ["방탄소년단", "BTS", "아이유", "IU", "아이유", "NewJeans"];
    for (i, s) in songs.iter_mut().enumerate() {
        s.title = titles[i % titles.len()].into();
        s.artists[0].name = artists[i % artists.len()].into();
    }
    let playlist = |name: &str| Playlist {
        id: format!("VL{name}"),
        server_id: listen_core::model::ServerId("demo".into()),
        source: listen_core::model::SourceKind::YouTubeMusic,
        name: name.into(),
        description: None,
        owner: Some("YouTube Music".into()),
        image: None,
        song_count: None,
        duration: std::time::Duration::ZERO,
        public: false,
        editable: false,
    };
    let feed = vec![
        listen_source::HomeShelf { title: "Quick picks".into(), albums: vec![], playlists: vec![], songs },
        listen_source::HomeShelf {
            title: "Mixed for you".into(),
            albums: vec![],
            playlists: vec![playlist("My Supermix"), playlist("K-Pop Mix"), playlist("Discover Mix")],
            songs: vec![],
        },
    ];
    let albums = std::mem::take(&mut state.library.home);
    state.library.home = feed.into_iter().chain(albums).collect();
    render_state("home-feed", state);
}

/// A 1000-song playlist: only visible rows are built (virtualized).
#[test]
fn big_playlist_page() {
    use listen::app::{Message, PlaylistDetail, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let base = listen::demo::album_songs(0);
    let songs: Vec<_> = (0..1000)
        .map(|i| {
            let mut s = base[i % base.len()].clone();
            s.id = format!("s{i}");
            s
        })
        .collect();
    let playlist = listen_core::model::Playlist {
        id: "big".into(),
        server_id: listen_core::model::ServerId("demo".into()),
        source: listen_core::model::SourceKind::YouTubeMusic,
        name: "Liked Music".into(),
        description: None,
        owner: Some("Mixtape".into()),
        image: None,
        song_count: None,
        duration: std::time::Duration::ZERO,
        public: false,
        editable: false,
    };
    let _ = state.update(Message::Navigate(Route::Playlist("big".into())));
    let started = std::time::Instant::now();
    let _ = state.update(Message::PlaylistLoaded(Ok(PlaylistDetail { playlist, songs })));
    render_state("big-playlist", state);
    println!("load + render: {:?}", started.elapsed());
}

/// Feishin-style visualizer strip under the queue (line graph + reflection).
#[test]
fn queue_panel_visualizer() {
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    let mut analyzer = listen_visualizer::Analyzer::new(listen_visualizer::Config::default());
    // Noisy, music-like signal: several partials plus deterministic noise.
    let mut seed = 12345u32;
    let samples: Vec<f32> = (0..16384)
        .flat_map(|i| {
            let t = i as f32 / 48_000.0;
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let noise = ((seed >> 16) as f32 / 65536.0 - 0.5) * 0.02;
            let v: f32 =
                [(80.0, 0.25), (180.0, 0.12), (440.0, 0.08), (1200.0, 0.04), (3000.0, 0.02), (7000.0, 0.008)]
                    .iter()
                    .map(|(f, a)| a * (std::f32::consts::TAU * f * t).sin())
                    .sum::<f32>()
                    + noise;
            [v, v]
        })
        .collect();
    analyzer.push_stereo(&samples);
    let _ = state.update(listen::app::Message::Spectrum(analyzer.frame()));
    render_state("queue-visualizer", state);
}

/// Collapsed left sidebar (Feishin's icon rail) with the queue open.
#[test]
fn collapsed_sidebar() {
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    let _ = state.update(listen::app::Message::ToggleSidebar);
    render_state("sidebar-collapsed", state);
}

/// Narrow panels: long titles are cut with "…", durations stay in their column.
#[test]
fn narrow_panels_keep_durations() {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    let titles = [
        "RIGHT HAND GIRL",
        "I’m gonna TOESA (Narr. KIAN84)",
        "권은비 (KWON EUNBI) 'DEJAVU' MV",
        "Dear my crazy soulmate",
        "Unknown Planet",
        "A Very Long Title That Would Never Fit Into This Narrow Panel",
    ];
    let mut songs = listen::demo::album_songs(0);
    for (s, t) in songs.iter_mut().zip(titles) {
        s.title = t.into();
    }
    state.queue.set(songs.clone(), 3);
    state.library.songs = songs;
    state.config.settings.sidebar.queue_width = 340.0;
    state.config.settings.sidebar.width = 420.0;
    let _ = state.update(Message::Navigate(Route::Tracks));
    render_state("narrow-panels", state);
}

/// The AI DJ: its settings section (a tall window shows the whole page), the
/// queue header toggle, and a break line shown as text in the player bar.
#[test]
fn ai_dj_settings_and_caption() {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    let _ = state.update(Message::Navigate(Route::Settings));
    let _ = state.update(Message::Shell(listen::app::settings_shell::ShellMsg::Tab(
        listen_core::settings::SettingsTab::Playback,
    )));
    render_sized("ai-dj-settings", state, iced::Size::new(1600.0, 4096.0));

    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    state.dj.caption = Some((
        "Nach so viel Neon wird's jetzt etwas ruhiger – Neon Coast bleibt, nur mit mehr Nacht.".into(),
        false,
    ));
    render_state("ai-dj-caption", state);
}

/// The player bar and layout at several window widths: full at 1200 and 800,
/// compact (Feishin's mobile layout) at 700 and 500.
#[test]
fn responsive_window_sizes() {
    use listen::app::Message;
    for (w, h) in [(1200.0, 800.0), (800.0, 600.0), (700.0, 600.0), (500.0, 700.0)] {
        let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
        let _ = state.update(Message::WindowResized(iced::Size::new(w, h)));
        render_sized(&format!("responsive-{w}x{h}"), state, iced::Size::new(w, h));
    }
}

/// Compact layout extras: the sidebar drawer and the mobile full-screen player.
#[test]
fn compact_drawer_and_fullscreen() {
    use listen::app::Message;
    let size = iced::Size::new(500.0, 700.0);
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(size));
    let _ = state.update(Message::ToggleDrawer);
    render_sized("compact-drawer", state, size);
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(size));
    let _ = state.update(Message::ToggleFullscreen);
    render_sized("compact-fullscreen", state, size);
}

/// Every settings tab, plus a search and the restart-required toast.
#[test]
fn settings_tabs_search_and_restart_toast() {
    use listen::app::settings_shell::ShellMsg;
    use listen::app::{Message, Route};
    use listen_core::settings::SettingsTab;
    for tab in SettingsTab::ALL {
        let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
        let _ = state.update(Message::Navigate(Route::Settings));
        let _ = state.update(Message::Shell(ShellMsg::Tab(tab)));
        render_sized(
            &format!("settings-tab-{}", tab.label().to_lowercase()),
            state,
            iced::Size::new(1600.0, 1800.0),
        );
    }
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Settings));
    let _ = state.update(Message::Shell(ShellMsg::Search("volume".into())));
    render_state("settings-search", state);
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Settings));
    let _ = state.update(Message::Setting(listen::app::Setting::CustomTitleBar(false)));
    render_state("settings-restart-toast", state);
}

/// The hotkeys tab: one row per action, a duplicate outlined red, a row recording.
#[test]
fn hotkeys_tab() {
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let s = &mut state.config.settings;
    s.hotkeys.insert("play".into(), "space".into());
    s.hotkeys.insert("next".into(), "mod+alt+arrowright".into());
    s.hotkeys.insert("rate5".into(), "mod+5".into());
    s.hotkeys_global.insert("next".into());
    state.hotkey_recording = Some("pause".into());
    let settings = iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    };
    let theme = state.theme();
    let tab = iced::widget::container(listen::ui::settings::hotkeys::global_shortcuts(&state))
        .padding(24)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .style(listen::style::app_background(state.tokens));
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings, iced::Size::new(1000.0, 2250.0), tab);
    let snapshot = ui.snapshot(&theme).expect("snapshot renders");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hotkeys-tab");
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with("hotkeys-tab-") {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    assert!(snapshot.matches_image(&path).expect("png written"));
}

/// The Window section of the settings page (a tall window shows the whole page)
/// and the macOS-style window bar.
#[test]
fn window_settings_and_mac_bar() {
    use listen::app::{Message, Route, Setting, WindowSetting};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Settings));
    render_sized("window-settings", state, iced::Size::new(1600.0, 4096.0));

    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    let _ = state.update(Message::Setting(Setting::Window(WindowSetting::BarStyle(
        listen_core::settings::WindowBarStyle::Macos,
    ))));
    render_state("window-mac-bar", state);
}

fn settings_state(configure: impl FnOnce(&mut Listen)) -> Listen {
    use listen::app::appearance::AppearanceMsg;
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Settings));
    state.config.settings.appearance.use_theme_accent = false;
    configure(&mut state);
    let _ = state.update(Message::Appearance(AppearanceMsg::ZoomDraft(100)));
    state
}

#[test]
fn appearance_custom_accent_and_shade() {
    use listen::app::Message;
    use listen::app::appearance::AppearanceMsg as A;
    let state = settings_state(|s| {
        let _ = s.update(Message::Appearance(A::AccentSwatch(listen_core::theme::Rgb(240, 170, 22))));
        let _ = s.update(Message::Appearance(A::UsePrimaryShade(false)));
        let _ = s.update(Message::Appearance(A::PrimaryShade(7)));
    });
    render_sized("appearance-custom-accent", state, iced::Size::new(1400.0, 1900.0));
}

#[test]
fn appearance_zoom_125_settings_and_home() {
    use listen::app::Message;
    use listen::app::appearance::AppearanceMsg as A;
    let state = settings_state(|s| {
        let _ = s.update(Message::Appearance(A::ZoomSet(125)));
    });
    render_sized("appearance-zoom-125-settings", state, iced::Size::new(1400.0, 1900.0));
    // The window of a 1600x950 monitor at 125 % is laid out at 1280x760.
    let (mut home, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = home.update(Message::Appearance(A::ZoomSet(125)));
    let _ = home.update(Message::WindowResized(iced::Size::new(1280.0, 760.0)));
    render_sized("appearance-zoom-125-home", home, iced::Size::new(1280.0, 760.0));
}

#[test]
fn appearance_fonts_and_custom_theme() {
    use listen::app::Message;
    use listen::app::appearance::AppearanceMsg as A;
    use listen_core::theme::custom;
    let dir = std::env::temp_dir().join(format!("listen-snap-themes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("sunset.json"),
        r##"{"extends":"tokyoNight","colors":{"background":"#1d1214","surface":"#2a1a1d","primary":"#ff8844"}}"##,
    )
    .unwrap();
    std::fs::write(dir.join("overrides.toml"), "[radius]\nmd = 12\nsm = 8\n").unwrap();
    std::fs::write(dir.join("oops.json"), "{ not json").unwrap();
    let loaded = custom::load_dir(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
    let state = settings_state(|s| {
        s.config.settings.appearance.follow_system_theme = false;
        s.config.settings.appearance.theme = "sunset".into();
        let _ = s.update(Message::Appearance(A::ThemesChanged(std::sync::Arc::new(loaded))));
    });
    render_sized("appearance-custom-theme", state, iced::Size::new(1400.0, 1900.0));
}

/// Album header with a blurred cover behind it (on) and without (off).
fn album_with_blur(background: bool, blur: f32) -> Listen {
    use listen::app::appearance::{AppearanceMsg as A, BlurKind};
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Album("demo-0".into())));
    let image = listen_core::model::ImageRef::Id("cover".into());
    if let Some(album) = state.library.album.as_mut() {
        album.image = Some(image.clone());
    }
    let mut img = image::RgbaImage::new(400, 400);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba([(x * 255 / 400) as u8, 60, (y * 255 / 400) as u8, 255]);
        if (x / 50 + y / 50) % 2 == 0 {
            p.0[1] = 200;
        }
    }
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let _ = state.update(Message::Appearance(A::AlbumBackground(background)));
    let _ = state.update(Message::Appearance(A::AlbumBlur(blur)));
    let sigma = state.blur_sigma(BlurKind::Album);
    let handle = listen::images::blur(png.get_ref(), sigma);
    let key = state.blur_key(&image, BlurKind::Album);
    let _ = state.update(Message::BlurLoaded(key, handle));
    state
}

#[test]
fn album_background_blur_on() {
    render_state("album-background-blur-on", album_with_blur(true, 3.0));
}

#[test]
fn album_background_blur_off() {
    render_state("album-background-blur-off", album_with_blur(false, 3.0));
}

#[test]
fn album_background_no_blur_slider_zero() {
    render_state("album-background-blur-0", album_with_blur(true, 0.0));
}

/// The player-config popover (sliders button) over the player bar.
#[test]
fn player_config_popover() {
    use listen::app::Message;
    use listen::app::audio::AudioMsg;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    state.queue.set(listen::demo::album_songs(0), 0);
    let _ = state.update(Message::Audio(AudioMsg::PreservePitch(false)));
    let _ = state.update(Message::Audio(AudioMsg::SetSpeed(1.25)));
    let _ = state.update(Message::Audio(AudioMsg::TogglePopover));
    render_state("player-config-popover", state);
}

/// The output-device menu opened by a right-click on the volume icon.
#[test]
fn volume_device_menu() {
    use listen::app::Message;
    use listen::app::audio::AudioMsg;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    state.queue.set(listen::demo::album_songs(0), 0);
    let _ = state.update(Message::RightClickAt(iced::Point::new(1380.0, 905.0)));
    let _ = state.update(Message::Audio(AudioMsg::OpenDeviceMenu));
    let _ = state.update(Message::Audio(AudioMsg::SetDevice("pipewire/bluez_output.headphones".into())));
    let _ = state.update(Message::RightClickAt(iced::Point::new(1380.0, 905.0)));
    let _ = state.update(Message::Audio(AudioMsg::OpenDeviceMenu));
    render_state("volume-device-menu", state);
}

/// The Playback tab's audio sections: the page scrolled down to them.
#[test]
fn playback_settings_audio() {
    use iced::mouse::{Event as MouseEvent, ScrollDelta};
    use listen::app::Message;
    use listen::app::audio::AudioMsg;
    let (mut state, _) = Listen::boot(&Args { route: Some("settings".into()), ..demo("tokyoNight", false) });
    let _ = state.update(Message::Audio(AudioMsg::EqPreset("V-Shape".into())));
    let _ = state.update(Message::Audio(AudioMsg::CompEnabled(true)));
    let _ = state.update(Message::Audio(AudioMsg::TranscodeEnabled(true)));
    let _ = state.update(Message::Audio(AudioMsg::TranscodeFormat("opus".into())));
    let _ = state.update(Message::Audio(AudioMsg::RefreshDevices));
    for (name, lines) in [
        ("playback-settings-audio-1", 0.0),
        ("playback-settings-audio-2", -3.6),
        ("playback-settings-audio-3", -1.6),
    ] {
        let settings = iced::Settings {
            fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
            default_font: INTER,
            ..Default::default()
        };
        let theme = state.theme();
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings, iced::Size::new(1600.0, 1100.0), state.view());
        ui.point_at((800.0, 500.0));
        if lines != 0.0 {
            let _ = ui.simulate([iced::Event::Mouse(MouseEvent::WheelScrolled {
                delta: ScrollDelta::Lines { x: 0.0, y: lines * 10.0 },
            })]);
        }
        let snapshot = ui.snapshot(&theme).expect("snapshot renders");
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            if entry.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
        assert!(snapshot.matches_image(&path).expect("png written"));
    }
}

// ---- item lists (table, grid, detail, selection bar) -----------------------------

fn list_state(route: listen::app::Route) -> Listen {
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(listen::app::Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    let _ = state.update(listen::app::Message::Navigate(route));
    state
}

fn tracks_table(size: listen_core::settings::RowSize) -> Listen {
    let mut state = list_state(listen::app::Route::Tracks);
    state.config.settings.lists.edit(listen_core::settings::ListKind::Songs, |c| c.table.size = size);
    state
}

#[test]
fn list_table_compact() {
    render_state("list-table-compact", tracks_table(listen_core::settings::RowSize::Compact));
}

#[test]
fn list_table_default() {
    render_state("list-table-default", tracks_table(listen_core::settings::RowSize::Default));
}

#[test]
fn list_table_large() {
    render_state("list-table-large", tracks_table(listen_core::settings::RowSize::Large));
}

fn albums_grid(size: listen_core::settings::CardSize) -> Listen {
    let mut state = list_state(listen::app::Route::Albums);
    state.config.settings.lists.edit(listen_core::settings::ListKind::Albums, |c| c.grid.size = size);
    state
}

#[test]
fn list_grid_compact() {
    render_state("list-grid-compact", albums_grid(listen_core::settings::CardSize::Compact));
}

#[test]
fn list_grid_default() {
    render_state("list-grid-default", albums_grid(listen_core::settings::CardSize::Default));
}

#[test]
fn list_grid_large() {
    render_state("list-grid-large", albums_grid(listen_core::settings::CardSize::Large));
}

#[test]
fn list_detail_albums() {
    let mut state = list_state(listen::app::Route::Albums);
    state.config.settings.lists.edit(listen_core::settings::ListKind::Albums, |c| {
        c.display = listen_core::settings::DisplayType::Detail
    });
    render_state("list-detail", state);
}

#[test]
fn list_selection_bar() {
    use listen::app::list::{ListId, ListMsg};
    let mut state = tracks_table(listen_core::settings::RowSize::Default);
    let _ = state.update(listen::app::Message::List(ListMsg::Press(ListId::Songs, 1)));
    state.modifiers = iced::keyboard::Modifiers::SHIFT;
    let _ = state.update(listen::app::Message::List(ListMsg::Press(ListId::Songs, 4)));
    render_state("list-selection-bar", state);
}

#[test]
fn list_grid_selection_and_hover_controls() {
    use listen::app::list::{ListId, ListMsg};
    let mut state = albums_grid(listen_core::settings::CardSize::Default);
    let _ = state.update(listen::app::Message::List(ListMsg::Press(ListId::Albums, 1)));
    let _ = state.update(listen::app::Message::List(ListMsg::Hover(ListId::Albums, 2)));
    render_state("list-grid-selected", state);
}

#[test]
fn list_options_popover() {
    use listen::app::list::{ListId, ListMsg};
    let mut state = tracks_table(listen_core::settings::RowSize::Default);
    let _ = state.update(listen::app::Message::List(ListMsg::ConfigMenu(
        ListId::Songs,
        iced::Point::new(1100.0, 90.0),
    )));
    render_state("list-options-popover", state);
}

#[test]
fn list_table_hover_play_control() {
    use listen::app::list::{ListId, ListMsg};
    let mut state = tracks_table(listen_core::settings::RowSize::Default);
    let _ = state.update(listen::app::Message::List(ListMsg::Hover(ListId::Songs, 2)));
    render_state("list-table-hover", state);
}

fn album_page_state(album: listen_core::model::Album) -> Listen {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    let _ = state.update(Message::Navigate(Route::Album(album.id.clone())));
    let _ = state.update(Message::AlbumLoaded(Ok(album)));
    state
}

#[test]
fn album_page_navidrome() {
    render_state("album-page-nd-main", album_page_state(listen::demo::navidrome_album()));
}

#[test]
fn album_page_navidrome_menu_and_selection() {
    use listen::app::album_page::AlbumPageMsg;
    let mut state = album_page_state(listen::demo::navidrome_album());
    let _ = state.update(listen::app::Message::AlbumPage(AlbumPageMsg::TogglePlayMenu));
    let _ = state.update(listen::app::Message::AlbumPage(AlbumPageMsg::Disc(2)));
    let _ = state.update(listen::app::Message::AlbumPage(AlbumPageMsg::ToggleComment));
    render_state("album-page-nd-menu", state);
}

#[test]
fn album_page_ytm() {
    render_state("album-page-yt-main", album_page_state(listen::demo::ytm_album()));
}

#[test]
fn album_page_pills_and_carousels() {
    use listen::app::album_page::{AlbumPageMsg, Carousel};
    let mut state = album_page_state(listen::demo::navidrome_album());
    for (i, title) in
        ["More from Night Drive", "More from Synthwave", "More from Electronic"].iter().enumerate()
    {
        let c = Carousel {
            title: (*title).into(),
            albums: listen::demo::albums(),
            genre: (i > 0).then(|| "x".into()),
        };
        let _ = state.update(listen::app::Message::AlbumPage(AlbumPageMsg::Carousel(
            "nd-album-neon".into(),
            i,
            Ok(c),
        )));
    }
    // Tall enough to show the whole page without scrolling.
    render_sized("album-page-nd-pills", state, iced::Size::new(1600.0, 2300.0));
}

/// Home on a Navidrome-style server: feature carousel, genres, album rows.
#[test]
fn home_rows_navidrome() {
    render("home-rows-navidrome", demo("tokyoNight", false));
}

/// Home with the "multiple" feature style and a YouTube Music feed above the rows.
#[test]
fn home_feature_multiple() {
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    state.config.settings.home.feature_style = listen_core::settings::HomeFeatureStyle::Multiple;
    render_state("home-feature-multiple", state);
}

fn search_results() -> listen_core::model::SearchResults {
    listen_core::model::SearchResults {
        albums: listen::demo::albums().into_iter().take(4).collect(),
        songs: listen::demo::album_songs(0),
        ..Default::default()
    }
}

/// The command palette: first page, then with live results.
#[test]
fn command_palette() {
    use listen::app::Message;
    use listen::app::palette::PaletteMsg;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.open_palette();
    render_state("palette-home", state);

    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.open_palette();
    let _ = state.update(Message::Palette(PaletteMsg::Input("ab".into())));
    let _ = state.update(Message::Palette(PaletteMsg::Loaded(0, Ok(search_results()))));
    let _ = state.update(Message::Palette(PaletteMsg::Move(1)));
    render_state("palette-results", state);
}

/// The search page with results in the Albums tab.
#[test]
fn search_page_tabs() {
    use listen::app::list::SearchTab;
    use listen::app::search::SearchMsg;
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Search));
    let _ = state.update(Message::SearchInput("ab".into()));
    let _ = state.update(Message::SearchLoaded(0, Ok(search_results())));
    render_state("search-tracks", {
        let (mut s, _) = Listen::boot(&demo("tokyoNight", false));
        let _ = s.update(Message::Navigate(Route::Search));
        let _ = s.update(Message::SearchInput("ab".into()));
        let _ = s.update(Message::SearchLoaded(0, Ok(search_results())));
        s
    });
    let _ = state.update(Message::Search(SearchMsg::Tab(SearchTab::Albums)));
    render_state("search-albums", state);
}

// ---- list headers (sort, filters, search) ---------------------------------------

#[test]
fn list_header_albums() {
    let mut state = list_state(listen::app::Route::Albums);
    let _ = state.update(listen::app::Message::ListQuery(
        listen::app::list_query::ListQueryMsg::SearchInput(listen::app::list::ListId::Albums, "n".into()),
    ));
    render_state("list-header-albums", state);
}

#[test]
fn list_header_sort_dropdown() {
    use listen::app::list::ListId;
    use listen::app::list_query::ListQueryMsg;
    let mut state = list_state(listen::app::Route::Tracks);
    let _ = state.update(listen::app::Message::ListQuery(ListQueryMsg::OpenSort(
        ListId::Songs,
        iced::Point::new(60.0, 150.0),
    )));
    render_state("list-header-sort", state);
}

#[test]
fn list_header_filters_modal() {
    use listen::app::list::ListId;
    use listen::app::list_query::{FilterEdit, ListQueryMsg};
    let mut state = list_state(listen::app::Route::Albums);
    let m = |msg| listen::app::Message::ListQuery(msg);
    let _ = state.update(m(ListQueryMsg::OpenFilters(ListId::Albums)));
    let _ = state.update(m(ListQueryMsg::Filter(ListId::Albums, FilterEdit::Favorite(Some(true)))));
    let _ = state.update(m(ListQueryMsg::Filter(ListId::Albums, FilterEdit::YearFrom("2020".into()))));
    render_state("list-header-filters", state);
}

#[test]
fn list_header_collections() {
    use listen::app::list::ListId;
    use listen::app::list_query::{FilterEdit, ListQueryMsg};
    let mut state = list_state(listen::app::Route::Albums);
    let m = |msg| listen::app::Message::ListQuery(msg);
    let _ = state.update(m(ListQueryMsg::Filter(ListId::Albums, FilterEdit::YearFrom("2020".into()))));
    let _ = state.update(m(ListQueryMsg::CollectionName("Recent releases".into())));
    let _ = state.update(m(ListQueryMsg::SaveCollection(ListId::Albums)));
    let _ = state.update(m(ListQueryMsg::OpenCollections(ListId::Albums, iced::Point::new(330.0, 150.0))));
    render_state("list-header-collections", state);
}

/// The context menu: a song menu, a multi-select menu with the preview
/// footer, and an open submenu.
#[test]
fn context_menu_song_multi_and_submenu() {
    use listen::app::Message;
    use listen::app::list::{ListId, ListMsg};
    use listen::app::menu::{MenuMsg, MenuTarget};
    let song = listen::demo::album_songs(1)[0].clone();
    let mut state = tracks_table(listen_core::settings::RowSize::Default);
    let _ = state.update(Message::RightClickAt(iced::Point::new(500.0, 200.0)));
    let _ = state.update(Message::Menu(MenuMsg::Open(MenuTarget::song(&song))));
    render_state("context-menu-song", state);

    let mut state = tracks_table(listen_core::settings::RowSize::Default);
    for i in [0, 2, 3] {
        let _ = state.update(Message::List(ListMsg::Press(ListId::Songs, i)));
        state.modifiers = iced::keyboard::Modifiers::CTRL;
    }
    state.modifiers = iced::keyboard::Modifiers::default();
    let _ =
        state.update(Message::List(ListMsg::SelectionMenu(ListId::Songs, iced::Point::new(500.0, 200.0))));
    render_state("context-menu-multi", state);

    let mut state = tracks_table(listen_core::settings::RowSize::Default);
    let _ = state.update(Message::RightClickAt(iced::Point::new(500.0, 200.0)));
    let _ = state.update(Message::Menu(MenuMsg::Open(MenuTarget::song(&song))));
    let _ = state.update(Message::Menu(MenuMsg::Hover(3)));
    render_state("context-menu-submenu", state);
}

// ---- playlists --------------------------------------------------------------

fn playlist_page_state(detail: listen::app::PlaylistDetail) -> Listen {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    state.library.playlists = listen::demo::playlists();
    let _ = state.update(Message::Navigate(Route::Playlist(detail.playlist.id.clone())));
    let _ = state.update(Message::PlaylistLoaded(Ok(detail)));
    state
}

#[test]
fn playlist_page_navidrome() {
    render_state("playlist-page-nd-main", playlist_page_state(listen::demo::navidrome_playlist()));
}

#[test]
fn playlist_page_ytm() {
    render_state("playlist-page-yt-main", playlist_page_state(listen::demo::ytm_playlist()));
}

#[test]
fn playlist_page_edit_mode() {
    use listen::app::Message;
    use listen::app::playlists::PlaylistPageMsg as P;
    let mut state = playlist_page_state(listen::demo::navidrome_playlist());
    let _ = state.update(Message::PlaylistPage(P::EditStart));
    let _ = state.update(Message::PlaylistDragStart(5));
    let _ = state.update(Message::PlaylistDragOver(1));
    let _ = state.update(Message::PlaylistDragEnd);
    let _ = state.update(Message::PlaylistDragStart(7));
    let _ = state.update(Message::PlaylistDragOver(3));
    render_state("playlist-page-edit", state);
}

#[test]
fn playlist_page_albums_view() {
    use listen::app::Message;
    use listen::app::playlists::PlaylistPageMsg as P;
    let mut state = playlist_page_state(listen::demo::navidrome_playlist());
    let _ = state.update(Message::PlaylistPage(P::Target(listen_core::settings::PlaylistTarget::Albums)));
    render_state("playlist-page-albums", state);
}

#[test]
fn playlist_page_filters_and_dialogs() {
    use listen::app::Message;
    use listen::app::playlists::{FilterEdit, PlaylistPageMsg as P};
    let mut state = playlist_page_state(listen::demo::navidrome_playlist());
    let _ = state.update(Message::PlaylistPage(P::Filter(FilterEdit::Favorite(Some(true)))));
    let _ = state.update(Message::PlaylistPage(P::OpenFilters));
    render_state("playlist-filters", state);

    let mut state = playlist_page_state(listen::demo::navidrome_playlist());
    let p = state.library.playlist.as_ref().unwrap().playlist.clone();
    let _ = state.update(Message::PlaylistPage(P::EditPlaylist(p)));
    render_state("playlist-edit-form", state);

    let mut state = playlist_page_state(listen::demo::ytm_playlist());
    let _ = state.update(Message::PlaylistPage(P::Create(Vec::new())));
    let _ = state.update(Message::PlaylistPage(P::FormName("Summer drive".into())));
    render_state("playlist-create-form", state);
}

#[test]
fn playlists_page() {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    state.library.playlists = listen::demo::playlists();
    let _ = state.update(Message::Navigate(Route::Playlists));
    render_state("playlists-page-table", state);
}

#[test]
fn playlists_page_grid() {
    use listen::app::list::{ConfigEdit, ListId, ListMsg};
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    state.library.playlists = listen::demo::playlists();
    let _ = state.update(Message::Navigate(Route::Playlists));
    let _ = state.update(Message::List(ListMsg::Config(
        ListId::Playlists,
        ConfigEdit::Display(listen_core::settings::DisplayType::Grid),
    )));
    render_state("playlists-page-grid", state);
}

#[test]
fn add_to_playlist_picker() {
    use listen::app::Message;
    use listen::app::menu::{MenuAction, MenuMsg, MenuPage, MenuTarget};
    use listen::app::playlists::PlaylistPageMsg as P;
    let song = listen::demo::album_songs(1)[0].clone();
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    let _ = state.update(Message::RightClickAt(iced::Point::new(500.0, 200.0)));
    let _ = state.update(Message::Menu(MenuMsg::Open(MenuTarget::song(&song))));
    let _ = state.update(Message::Menu(MenuMsg::Do(MenuAction::ChoosePlaylist)));
    // The menu stays open on the picker page; give it playlists (the last used one is pinned).
    let playlists = listen::demo::playlists();
    state.playlist_page.recent.insert(song.server_id.clone(), "nd-pl-chill".into());
    if let Some(m) = state.menu.as_mut() {
        m.page =
            MenuPage::Playlists { list: Some(Ok(playlists)), new_name: String::new(), search: String::new() };
    }
    render_state("playlist-picker", state);
    let _ = P::SkipDuplicates(true);
}

#[test]
fn smart_playlist_builder() {
    use listen::app::smart_playlist::{Editor, MODAL_KEY};
    use listen_source::SmartPlaylistDef;
    use listen_source::nsp::Nsp;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let mut editor = Editor::new(Some("p1".into()), &[]);
    editor.fill(SmartPlaylistDef {
        name: "Loved rock of the 90s".into(),
        comment: "Songs I starred".into(),
        public: false,
        sync: true,
        rules: Nsp::from_json(
            r#"{"all": [{"is": {"loved": true}}, {"inTheRange": {"year": [1990, 1999]}},
                {"after": {"dateadded": "2020-05-17"}}, {"inPlaylist": {"id": "x"}},
                {"any": [{"contains": {"genre": "rock"}}, {"startsWith": {"title": "The"}}]}],
                "sort": "-rating,+title", "limit": 100}"#,
        )
        .unwrap(),
    });
    state.smart.editor = Some(editor);
    state.open_modal(MODAL_KEY);
    render_state("smart-playlist-builder", state);
}

// --- item details, sharing, tag editor ---

fn item_state() -> Listen {
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(listen::app::Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    state
}

#[test]
fn item_details_modal() {
    use listen::app::item::details::{DetailsState, Entry, Load, Subject, rows};
    use listen_source::ItemDetails;
    let mut state = item_state();
    let mut song = listen::demo::album_songs(0).remove(0);
    song.favorite = true;
    song.rating = Some(4);
    let details = ItemDetails {
        path: Some("Daft Punk/Discovery/01 One More Time.flac".into()),
        size: Some(38_912_000),
        suffix: Some("flac".into()),
        bit_rate: Some(912),
        sample_rate: Some(44_100),
        bit_depth: Some(16),
        channels: Some(2),
        bpm: Some(123),
        created: Some("2024-03-01T10:00:00Z".into()),
        updated: Some("2024-03-02T08:30:00Z".into()),
        last_played: Some("2026-10-14T21:15:00Z".into()),
        play_count: Some(42),
        comment: Some("Remastered from the original tapes.".into()),
        musicbrainz_id: Some("4f1e5c8e-0f6d-4b6e-9c8e-1d2a3b4c5d6e".into()),
        gain: listen_core::model::GainInfo {
            track_gain: Some(-6.5),
            album_gain: Some(-7.25),
            track_peak: Some(0.98),
            album_peak: Some(1.0),
        },
        has_lyrics: Some(true),
        participants: vec![("producer".into(), "Thomas Bangalter".into())],
        ..Default::default()
    };
    let subject = Subject::Song(Box::new(song));
    let entry = Entry {
        rows: rows(&subject, Some(&details)),
        subject,
        load: Load::Done,
        details: Some(details),
        local_path: None,
    };
    state.item.details = Some(DetailsState { entries: vec![entry.clone(), entry], selected: 0 });
    state.open_modal("item-details");
    render_state("item-details-modal", state);
}

#[test]
fn item_share_modal() {
    use listen::app::item::share::ShareState;
    let mut state = item_state();
    let expiry = state.config.settings.sharing.expiration;
    state.item.share = Some(ShareState {
        server: listen_core::model::ServerId("demo".into()),
        kind: listen_source::ItemKind::Album,
        ids: vec!["al1".into()],
        title: "Discovery".into(),
        description: "For the road trip".into(),
        amount: expiry.amount.to_string(),
        expiry,
        busy: false,
        error: None,
    });
    state.open_modal("item-share");
    render_state("item-share-modal", state);
}

fn tag_editor_state(preview: bool) -> Listen {
    use listen::app::item::tags::{Step, TagsState, merge_fields};
    use listen_local::tags::{CoverEdit, CoverInfo, FileTags, TagField};
    let mut state = item_state();
    let songs: Vec<_> = listen::demo::album_songs(0).into_iter().take(3).collect();
    let file = |title: &str, track: &str| FileTags {
        fields: [
            (TagField::Title, title),
            (TagField::Artist, "Daft Punk"),
            (TagField::Album, "Discovery"),
            (TagField::AlbumArtist, "Daft Punk"),
            (TagField::Track, track),
            (TagField::Year, "2001"),
            (TagField::Genre, "Electronic; House"),
        ]
        .into_iter()
        .map(|(f, v)| (f, v.to_string()))
        .collect(),
        cover: Some(CoverInfo { mime: "image/jpeg".into(), bytes: 184_320 }),
    };
    let before = vec![file("One More Time", "1"), file("Aerodynamic", "2"), file("Digital Love", "3")];
    let mut fields = merge_fields(&before);
    if preview {
        for (f, v) in [(TagField::Year, "2021"), (TagField::Genre, "Electronic; House; Disco")] {
            let e = fields.get_mut(&f).unwrap();
            e.value = v.into();
            e.edited = true;
            e.mixed = false;
        }
    }
    state.item.tags = Some(TagsState {
        server: listen_core::model::ServerId("demo".into()),
        songs,
        paths: vec!["/music/1.flac".into(), "/music/2.flac".into(), "/music/3.flac".into()],
        before: Some(before),
        fields,
        lyrics: iced::widget::text_editor::Content::new(),
        cover_path: String::new(),
        cover: CoverEdit::Keep,
        step: if preview { Step::Preview } else { Step::Edit },
        error: None,
        failed: Vec::new(),
    });
    state.open_modal("item-tags");
    state
}

#[test]
fn item_tag_editor() {
    render_state("item-tags-edit", tag_editor_state(false));
}

#[test]
fn item_tag_editor_preview() {
    render_state("item-tags-preview", tag_editor_state(true));
}

fn radio_state() -> Listen {
    use listen::app::radio::RadioMsg;
    use listen::app::{Message, Route};
    let (mut s, _) = Listen::boot(&demo("tokyoNight", true));
    let _ = s.update(Message::Navigate(Route::Radio));
    let _ = s.update(Message::Radio(RadioMsg::Play("d1".into())));
    let _ = s.update(Message::Deck(
        0,
        listen::playback::PlayerMsg::Event(listen_player::PlayerEvent::StreamTitle(Some(
            "Miles Davis - So What".into(),
        ))),
    ));
    s
}

#[test]
fn radio_page_playing() {
    render_state("radio-page", radio_state());
}

#[test]
fn radio_create_dialog() {
    use listen::app::Message;
    use listen::app::radio::RadioMsg;
    let mut s = radio_state();
    let _ = s.update(Message::Radio(RadioMsg::OpenCreate));
    let _ = s.update(Message::Radio(RadioMsg::FormName("Rock FM".into())));
    render_state("radio-create-dialog", s);
}

// ---- full-screen player and player bar ---------------------------------------

fn fs_open(tab: listen::app::FullscreenTab) -> Listen {
    use listen::app::Message;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    state.queue.set(listen::demo::album_songs(0), 0);
    let _ = state.update(Message::OpenFullscreenTab(tab));
    let _ = state.update(Message::Deck(
        0,
        listen::playback::PlayerMsg::Event(listen_player::PlayerEvent::Position(
            std::time::Duration::from_secs(95),
        )),
    ));
    state
}

#[test]
fn fullscreen_queue_tab() {
    render_state("fullscreen-queue", fs_open(listen::app::FullscreenTab::Queue));
}

#[test]
fn fullscreen_related_tab() {
    use listen::app::Message;
    use listen::app::fullscreen_state::FullscreenMsg;
    let mut state = fs_open(listen::app::FullscreenTab::Related);
    let id = state.queue.current().unwrap().song.id.clone();
    let _ = state.update(Message::Fullscreen(FullscreenMsg::SetRelated(id, listen::demo::album_songs(2))));
    render_state("fullscreen-related", state);
}

/// No panel open: the cover is centred; badges, links and the cover as background.
#[test]
fn fullscreen_panel_closed_with_badges_and_image_background() {
    use listen::app::fullscreen_state::{Backdrop, FsSetting, FullscreenMsg};
    use listen::app::{FullscreenTab, Message};
    let mut state = fs_open(FullscreenTab::Queue);
    let _ = state.update(Message::Fullscreen(FullscreenMsg::TogglePanel(FullscreenTab::Queue)));
    assert_eq!(state.fullscreen_tab, FullscreenTab::Hidden);
    for id in ["bit_rate", "bit_depth", "sample_rate", "genres"] {
        let _ = state.update(Message::Fullscreen(FullscreenMsg::Setting(FsSetting::Badge(id.into(), true))));
    }
    let _ = state.update(Message::Fullscreen(FullscreenMsg::Setting(FsSetting::DynamicIsImage(true))));
    // A synthetic cover: diagonal colour ramp, already blurred.
    let (w, h) = (96u32, 96u32);
    let pixels: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(40 + x * 2) as u8, (90 + y) as u8, (200 - x) as u8, 255]
        })
        .collect();
    state.fs.backdrop = Some(Backdrop {
        key: "test".into(),
        color: iced::Color::from_rgb8(60, 110, 190),
        image: Some(iced::widget::image::Handle::from_rgba(w, h, pixels)),
    });
    render_state("fullscreen-panel-closed", state);
}

#[test]
fn fullscreen_settings_gear() {
    use listen::app::Message;
    use listen::app::fullscreen_state::FullscreenMsg;
    let mut state = fs_open(listen::app::FullscreenTab::Lyrics);
    let _ = state.update(Message::Fullscreen(FullscreenMsg::OpenSettings));
    render_state("fullscreen-settings-gear", state);
}

#[test]
fn fullscreen_visualizer_overlay() {
    use listen::app::Message;
    use listen::app::fullscreen_state::FullscreenMsg;
    let mut state = fullscreen_state(listen::app::FullscreenTab::Visualizer);
    let _ = state.update(Message::Fullscreen(FullscreenMsg::ExpandVisualizer));
    render_state("fullscreen-visualizer-overlay", state);
}

/// A Subsonic-style song with a rating: stars above the right controls, the
/// title / artist / album as links with the "⋮" next to the title.
#[test]
fn player_bar_ratings_and_links() {
    use listen::app::{Message, SourceHandle};
    use listen_core::model::{ServerId, SourceKind};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let source: std::sync::Arc<dyn listen_source::MusicSource> = std::sync::Arc::new(
        listen_subsonic::Subsonic::new(ServerId("nd".into()), "https://nd.example", "me", "pw").unwrap(),
    );
    let _ = state.update(Message::Connected(ServerId("nd".into()), Ok(SourceHandle(source))));
    let mut songs = listen::demo::album_songs(0);
    for song in &mut songs {
        song.server_id = ServerId("nd".into());
        song.source = SourceKind::Subsonic;
        song.rating = Some(3);
    }
    state.queue.set(songs, 0);
    let _ = state.update(Message::Fullscreen(listen::app::fullscreen_state::FullscreenMsg::Sleep(
        listen::app::sleep_timer::SleepMsg::Minutes(30),
    )));
    render_sized("player-bar-ratings-links", state, iced::Size::new(1600.0, 500.0));
}

#[test]
fn sleep_timer_popover() {
    use listen::app::Message;
    use listen::app::fullscreen_state::FullscreenMsg;
    use listen::app::sleep_timer::SleepMsg;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    state.queue.set(listen::demo::album_songs(0), 0);
    let sleep = |m| Message::Fullscreen(FullscreenMsg::Sleep(m));
    let _ = state.update(sleep(SleepMsg::Minutes(45)));
    let _ = state.update(Message::TogglePlay);
    let _ = state.update(sleep(SleepMsg::Toggle));
    render_state("sleep-timer-popover", state);
}

// ---- the play queue ---------------------------------------------------------------

/// A queue with an Up Next block and the Auto-DJ's songs behind it.
fn busy_queue(state: &mut Listen) {
    use listen::app::Message;
    use listen::app::menu::{MenuAction, MenuMsg};
    let _ = state.update(Message::Menu(MenuMsg::Songs(
        MenuAction::PlayNext,
        Ok(listen::demo::album_songs(1)[..2].to_vec()),
    )));
    let _ = state.update(Message::AutoDjLoaded(Ok(listen::demo::album_songs(2))));
}

/// Feishin's Now Playing page: the queue toolbar and the queue table.
#[test]
fn now_playing_page() {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    busy_queue(&mut state);
    let _ = state.update(Message::Navigate(Route::NowPlaying));
    render_state("now-playing-page", state);
}

/// The drawer queue: the right panel is closed, the drawer slid in over the page.
#[test]
fn queue_drawer() {
    use listen::app::Message;
    use listen::app::queue_ui::{QueueMsg, QueueSetting};
    use listen_core::settings::SideQueueStyle;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    busy_queue(&mut state);
    let _ = state.update(Message::Queue(QueueMsg::Setting(QueueSetting::Style(SideQueueStyle::Detached))));
    // The edge strip first ...
    let (mut closed, _) = Listen::boot(&demo("tokyoNight", false));
    busy_queue(&mut closed);
    let _ = closed.update(Message::Queue(QueueMsg::Setting(QueueSetting::Style(SideQueueStyle::Detached))));
    render_state("queue-drawer-edge", closed);
    // ... then the open drawer.
    let _ = state.update(Message::Queue(QueueMsg::EdgeEnter));
    let generation = state.queue_ui.drawer_gen;
    let _ = state.update(Message::Queue(QueueMsg::DrawerOpen(generation)));
    render_state("queue-drawer-open", state);
}

/// Rows being dragged inside the queue: the landing row is marked.
#[test]
fn queue_drag_reorder() {
    use listen::app::Message;
    use listen::app::list::{ListId, ListMsg};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    busy_queue(&mut state);
    let _ = state.update(Message::List(ListMsg::Press(ListId::SideQueue, 6)));
    let _ = state.update(Message::List(ListMsg::PointerMoved(iced::Point::new(1100.0, 300.0))));
    let _ = state.update(Message::List(ListMsg::PointerMoved(iced::Point::new(1150.0, 330.0))));
    let _ = state.update(Message::List(ListMsg::Hover(ListId::SideQueue, 3)));
    render_state("queue-drag-reorder", state);
}

/// The toolbar folds into a "…" menu in a narrow panel; several rows selected.
#[test]
fn queue_toolbar_narrow_with_selection() {
    use listen::app::Message;
    use listen::app::list::ListId;
    use listen::app::queue_ui::QueueMsg;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    busy_queue(&mut state);
    state.config.settings.sidebar.queue_width = 340.0;
    state.lists.entry(&ListId::SideQueue).selection.set_many([1, 5, 6]);
    let _ = state.update(Message::Queue(QueueMsg::MoreMenu(Some(iced::Point::new(1380.0, 150.0)))));
    render_state("queue-toolbar-narrow", state);
}

/// The right sidebar with the queue, lyrics and the visualizer (lyrics first).
#[test]
fn sidebar_panels() {
    use listen::app::Message;
    use listen::app::queue_ui::{QueueMsg, QueueSetting};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    let lrc = "[00:00.50]Neon lights across the bay\n[00:05.00]We drive until the break of day\n[00:10.00]Midnight signal on the air\n[00:15.00]Afterglow is everywhere";
    state.lyrics = Some(listen_lyrics::parse_lrc(lrc));
    state.lyric_active = Some(1);
    let _ = state.update(Message::Queue(QueueMsg::Setting(QueueSetting::LyricsInSidebar(true))));
    let _ = state.update(Message::Queue(QueueMsg::Setting(QueueSetting::MovePanel("lyrics".into(), -1))));
    render_state("sidebar-panels", state);
}

/// The queue under the page (vertical layout).
#[test]
fn queue_vertical_layout() {
    use listen::app::Message;
    use listen::app::queue_ui::{QueueMsg, QueueSetting};
    use listen_core::settings::SideQueueLayout;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    busy_queue(&mut state);
    let _ = state.update(Message::Queue(QueueMsg::Setting(QueueSetting::Layout(SideQueueLayout::Vertical))));
    render_state("queue-vertical", state);
}

/// The queue settings: the Playback tab (a tall window shows the whole page).
#[test]
fn queue_settings() {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    state.config.settings.queue.filters.push(listen_core::settings::PlayerFilter {
        field: listen_core::settings::FilterField::Artist,
        op: listen_core::settings::FilterOp::Contains,
        value: "live".into(),
        enabled: true,
    });
    let _ = state.update(Message::Navigate(Route::Settings));
    let _ = state.update(Message::Shell(listen::app::settings_shell::ShellMsg::Tab(
        listen_core::settings::SettingsTab::Playback,
    )));
    render_sized("queue-settings", state, iced::Size::new(1600.0, 4000.0));
}

// ---- visualizer modes, options modal and the waveform seek bar ----

/// Full-screen visualizer with `settings` and a music-like stereo signal
/// analysed with exactly the options those settings map to.
fn viz_state(settings: listen_core::settings::Visualizer) -> Listen {
    use listen::app::{FullscreenTab, Message};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::OpenFullscreenTab(FullscreenTab::Visualizer));
    let cfg = listen::visualizer::analyzer_config(&settings);
    state.config.settings.visualizer = settings;
    let mut analyzer = listen_visualizer::Analyzer::new(cfg);
    let mut seed = 777u32;
    let samples: Vec<f32> = (0..32768)
        .flat_map(|i| {
            let t = i as f32 / 48_000.0;
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let noise = ((seed >> 16) as f32 / 65536.0 - 0.5) * 0.02;
            let part = |list: &[(f32, f32)]| -> f32 {
                list.iter().map(|(f, a)| a * (std::f32::consts::TAU * f * t).sin()).sum()
            };
            let l =
                part(&[(70.0, 0.3), (200.0, 0.12), (700.0, 0.06), (2500.0, 0.02), (9000.0, 0.006)]) + noise;
            let r =
                part(&[(90.0, 0.2), (330.0, 0.15), (1200.0, 0.05), (4000.0, 0.02), (12000.0, 0.004)]) + noise;
            [l, r]
        })
        .collect();
    analyzer.push_stereo(&samples);
    for _ in 0..6 {
        let _ = analyzer.frame();
    }
    let _ = state.update(Message::Spectrum(analyzer.frame()));
    state
}

fn viz(f: impl FnOnce(&mut listen_core::settings::Visualizer)) -> listen_core::settings::Visualizer {
    let mut v = listen_core::settings::Visualizer::default();
    f(&mut v);
    v
}

#[test]
fn visualizer_led_octave_bands() {
    render_state(
        "viz-led-octave",
        viz_state(viz(|v| {
            v.mode = 5;
            v.led_bars = true;
            v.show_peaks = true;
            v.bar_space = 0.3;
            v.reflex_ratio = 0.0;
            v.show_scale_x = true;
        })),
    );
}

#[test]
fn visualizer_bars_with_reflex_and_scale() {
    render_state(
        "viz-bars-reflex",
        viz_state(viz(|v| {
            v.mode = 6;
            v.gradient = "classic".into();
            v.round_bars = true;
            v.show_peaks = true;
            v.reflex_ratio = 0.3;
            v.reflex_alpha = 0.3;
            v.show_scale_x = true;
            v.show_scale_y = true;
        })),
    );
}

#[test]
fn visualizer_radial() {
    render_state(
        "viz-radial",
        viz_state(viz(|v| {
            v.mode = 10;
            v.radial = true;
            v.fill_alpha = 0.3;
            v.line_width = 2.5;
        })),
    );
}

#[test]
fn visualizer_dual_vertical_bars() {
    render_state(
        "viz-dual-vertical",
        viz_state(viz(|v| {
            v.mode = 7;
            v.channel_layout = listen_core::settings::ChannelLayout::DualVertical;
            v.gradient_left = Some("steelblue".into());
            v.gradient_right = Some("orangered".into());
            v.lumi_bars = false;
            v.alpha_bars = true;
        })),
    );
}

#[test]
fn visualizer_dual_combined_graph_mirrored() {
    render_state(
        "viz-dual-combined-mirror",
        viz_state(viz(|v| {
            v.mode = 10;
            v.channel_layout = listen_core::settings::ChannelLayout::DualCombined;
            v.gradient_left = Some("prism".into());
            v.gradient_right = Some("rainbow".into());
            v.fill_alpha = 0.25;
            v.mirror = -1;
            v.frequency_scale = listen_core::settings::FrequencyScale::Mel;
        })),
    );
}

#[test]
fn visualizer_options_modal() {
    use listen::app::Message;
    use listen::app::viz::VizMsg;
    let mut state = viz_state(viz(|_| {}));
    let _ = state.update(Message::Viz(VizMsg::GradientAdd));
    let _ = state.update(Message::Viz(VizMsg::Open));
    render_sized("viz-options-modal", state, iced::Size::new(1200.0, 950.0));
}

#[test]
fn player_bar_waveform() {
    use listen::app::Message;
    use listen::app::viz::{SliderSetting, VizMsg};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Viz(VizMsg::Slider(SliderSetting::Type(
        listen_core::settings::SliderType::Waveform,
    ))));
    // A song-like envelope: quiet intro, loud verses, a breakdown, a fade out.
    let peaks: Vec<u8> = (0..1000)
        .map(|i| {
            let x = i as f32 / 1000.0;
            let env = if x < 0.08 {
                0.25
            } else if x < 0.45 {
                0.9
            } else if x < 0.55 {
                0.35
            } else if x < 0.9 {
                1.0
            } else {
                1.0 - (x - 0.9) * 9.0
            };
            let wobble = 0.65 + 0.35 * (i as f32 * 0.9).sin().abs() * (i as f32 * 0.13).cos().abs();
            (255.0 * env * wobble) as u8
        })
        .collect();
    state.viz.waveform.data =
        Some(std::sync::Arc::new(listen_analysis::waveform::Waveform { peaks, duration_ms: 214_000 }));
    let _ = state.update(Message::Deck(
        0,
        listen::playback::PlayerMsg::Event(listen_player::PlayerEvent::Position(
            std::time::Duration::from_secs(96),
        )),
    ));
    render_sized("player-bar-waveform", state, iced::Size::new(1400.0, 330.0));
    // Bottom-aligned thin bars with a rounded top, stretched.
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Viz(VizMsg::Slider(SliderSetting::Type(
        listen_core::settings::SliderType::Waveform,
    ))));
    let _ = state.update(Message::Viz(VizMsg::Slider(SliderSetting::Align(
        listen_core::settings::WaveformAlign::Bottom,
    ))));
    let _ = state.update(Message::Viz(VizMsg::Slider(SliderSetting::Stretch(true))));
    let _ = state.update(Message::Viz(VizMsg::Slider(SliderSetting::BarWidth(4))));
    let _ = state.update(Message::Viz(VizMsg::Slider(SliderSetting::Gap(2))));
    state.viz.waveform.data = Some(std::sync::Arc::new(listen_analysis::waveform::Waveform {
        peaks: (0..1000).map(|i| (40.0 + 60.0 * ((i as f32 * 0.05).sin().abs())) as u8).collect(),
        duration_ms: 214_000,
    }));
    render_sized("player-bar-waveform-bottom", state, iced::Size::new(1400.0, 330.0));
}

#[test]
fn settings_waveform_rows() {
    use listen::app::Message;
    use listen::app::viz::{SliderSetting, VizMsg};
    let (mut state, _) = Listen::boot(&Args { route: Some("settings".into()), ..demo("tokyoNight", false) });
    let _ = state.update(Message::Viz(VizMsg::Slider(SliderSetting::Type(
        listen_core::settings::SliderType::Waveform,
    ))));
    render_sized("settings-waveform", state, iced::Size::new(1400.0, 1500.0));
}

// ---- lyrics variants ---------------------------------------------------

fn lyrics_state(lyrics: listen_core::model::Lyrics, position_ms: u64) -> Listen {
    use listen::app::Message;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::OpenFullscreenTab(listen::app::FullscreenTab::Lyrics));
    let id = state.queue.current().unwrap().song.id.clone();
    let _ = state.update(Message::LyricsLoaded(id, Some(lyrics)));
    let _ = state.update(Message::Deck(
        0,
        listen::playback::PlayerMsg::Event(listen_player::PlayerEvent::Position(
            std::time::Duration::from_millis(position_ms),
        )),
    ));
    state
}

#[test]
fn lyrics_karaoke() {
    let lrc = "[00:08.00]<00:08.00>Neon <00:08.60>lights <00:09.20>across <00:10.00>the <00:10.40>bay\n[00:12.00]<00:12.00>We <00:12.50>drive <00:13.00>until <00:13.50>the <00:14.00>break <00:14.50>of <00:15.00>day\n[00:17.00]<00:17.00>Midnight <00:17.80>signal <00:18.60>on <00:19.00>the <00:19.40>air";
    let mut l = listen_lyrics::parse_lrc(lrc);
    l.provider = "lrclib.net".into();
    render_state("lyrics-karaoke", lyrics_state(l, 13_200));
}

#[test]
fn lyrics_translation_and_romaji() {
    let lrc = "[00:00.50]夜に駆ける_BREAK_Racing into the night\n[00:05.00]ありがとう_BREAK_Thank you\n[00:10.00]Hello again_BREAK_Hallo nochmal\n[00:15.00]さよなら_BREAK_Goodbye\n[00:20.00]きっと また あえる_BREAK_Surely we will meet again";
    let mut l = listen_lyrics::parse_lrc(lrc);
    l.provider = "NetEase".into();
    l.matched = Some("夜に駆ける — YOASOBI".into());
    let mut state = lyrics_state(l, 6_000);
    state.config.settings.lyrics.enable_romaji = true;
    state.config.settings.lyrics.show_match = true;
    state.config.settings.lyrics.alignment = "left".into();
    render_state("lyrics-translation-romaji", state);
}

#[test]
fn lyrics_unsynced_and_rtl() {
    let mut l = listen_lyrics::parse_lrc(
        "Neon lights across the bay\nWe drive until the break of day\nמה שלומך היום\nمرحبا بالعالم\nOnly static left to hear",
    );
    l.provider = "Genius".into();
    let mut state = lyrics_state(l, 1_000);
    state.config.settings.lyrics.alignment = "left".into();
    render_state("lyrics-unsynced-rtl", state);
}

#[test]
fn lyrics_search_dialog() {
    use listen::app::Message;
    use listen::app::lyrics_state::LyricsMsg;
    use listen_lyrics::providers::{SearchHit, Source};
    let mut state = lyrics_state(listen_lyrics::parse_lrc("[00:01.00]x"), 0);
    let _ = state.update(Message::Lyrics(LyricsMsg::SearchOpen));
    let token = state.lyrics_ui.search.as_ref().unwrap().token;
    let hit = |source, id: &str, name: &str, artist: &str, synced, score| SearchHit {
        source,
        id: id.into(),
        artist: artist.into(),
        name: name.into(),
        synced,
        score,
    };
    let hits = vec![
        hit(Source::Lrclib, "1204", "Neon Horizons", "Demo Artist", Some(true), 0.02),
        hit(Source::SimpMusic, "dQw4w9WgXcQ", "Neon Horizons (Live)", "Demo Artist", Some(true), 0.21),
        hit(Source::NetEase, "186016", "Neon Horizons", "Demo Artist, Guest", None, 0.12),
        hit(
            Source::Genius,
            "https://genius.com/Demo-artist-neon-horizons-lyrics",
            "Neon Horizons by Demo Artist",
            "Demo Artist",
            None,
            0.3,
        ),
    ];
    let _ = state.update(Message::Lyrics(LyricsMsg::SearchResults(token, hits)));
    let _ = state.update(Message::Lyrics(LyricsMsg::SearchPick(0)));
    let token = state.lyrics_ui.search.as_ref().unwrap().token;
    let preview = listen_lyrics::parse_lrc(
        "[00:01.00]Neon lights across the bay\n[00:05.00]We drive until the break of day\n[00:10.00]Midnight signal on the air",
    );
    let _ = state.update(Message::Lyrics(LyricsMsg::Preview(token, Some(preview))));
    render_state("lyrics-search-dialog", state);
}

#[test]
fn lyrics_export_and_display_dialogs() {
    use listen::app::Message;
    use listen::app::lyrics_state::LyricsMsg;
    let lrc = "[00:00.50]Neon lights across the bay\n[00:05.00]We drive until the break of day\n[00:10.00]Midnight signal on the air";
    let mut state = lyrics_state(listen_lyrics::parse_lrc(lrc), 0);
    let _ = state.update(Message::Lyrics(LyricsMsg::ExportOpen));
    render_state("lyrics-export-dialog", state);
    let mut state = lyrics_state(listen_lyrics::parse_lrc(lrc), 0);
    let _ = state.update(Message::Lyrics(LyricsMsg::SettingsOpen));
    render_state("lyrics-display-dialog", state);
}

#[test]
fn settings_lyrics_section() {
    let (mut state, _) = Listen::boot(&Args { route: Some("settings".into()), ..demo("tokyoNight", false) });
    state.config.settings.settings_tab = listen_core::settings::SettingsTab::Playback;
    // Only the lyrics section: the search hides the others.
    state.settings_search = "lyric".into();
    render_sized("settings-lyrics", state, iced::Size::new(1400.0, 1500.0));
}

fn sign_in_state() -> Listen {
    let (mut state, _) =
        Listen::boot(&Args { theme: Some("tokyoNight".into()), ephemeral: true, ..Args::default() });
    state.config = Default::default();
    state.connecting = false;
    state.window_size = iced::Size::new(1600.0, 950.0);
    state
}

#[test]
fn sign_in_with_discovered_servers() {
    use listen::app::servers::ServersMsg;
    use listen::app::{LoginMsg, Message};
    let mut state = sign_in_state();
    let _ = state.update(Message::Login(LoginMsg::Kind(listen_core::model::SourceKind::Jellyfin)));
    let found = |name: &str, n: u8| listen_jellyfin::discovery::Found {
        name: name.into(),
        url: format!("http://192.168.1.{n}:8096"),
        id: n.to_string(),
    };
    let _ = state.update(Message::Servers(ServersMsg::Discovered(vec![
        found("Living room NAS", 20),
        found("Cellar", 21),
    ])));
    render_sized("sign-in-discovered", state, iced::Size::new(1600.0, 1100.0));
}

#[test]
fn sign_in_navidrome_options() {
    use listen::app::Message;
    use listen::app::servers::ServersMsg;
    let mut state = sign_in_state();
    let _ = state.update(Message::Servers(ServersMsg::LegacyAuth(true)));
    let _ = state.update(Message::Servers(ServersMsg::IgnoreSsl(true)));
    render_sized("sign-in-navidrome-options", state, iced::Size::new(1600.0, 1100.0));
}

#[test]
fn sign_in_quick_connect_code() {
    use listen::app::servers::ServersMsg;
    use listen::app::{LoginMsg, Message};
    let mut state = sign_in_state();
    let _ = state.update(Message::Login(LoginMsg::Kind(listen_core::model::SourceKind::Jellyfin)));
    state.login.url = "https://jf.example.com".into();
    let _ = state.update(Message::Servers(ServersMsg::QuickConnectStart));
    let started = listen_jellyfin::quick_connect::Initiated { code: "482913".into(), secret: "s".into() };
    let _ = state.update(Message::Servers(ServersMsg::QuickConnectStarted(Ok(started))));
    render_sized("sign-in-quick-connect", state, iced::Size::new(1600.0, 1100.0));
}

fn saved_servers_state() -> Listen {
    use listen_core::config::ServerConfig;
    use listen_core::model::{ServerId, SourceKind};
    let mut state = sign_in_state();
    state.config.servers = vec![
        ServerConfig {
            id: ServerId("snap-nd".into()),
            name: "music.example.com".into(),
            kind: SourceKind::Subsonic,
            url: "https://music.example.com".into(),
            username: "alex".into(),
        },
        ServerConfig {
            id: ServerId("snap-yt".into()),
            name: "YouTube Music (Mixtape)".into(),
            kind: SourceKind::YouTubeMusic,
            url: String::new(),
            username: String::new(),
        },
    ];
    state.config.active_server = Some(ServerId("snap-nd".into()));
    state
}

#[test]
fn edit_server_dialog() {
    use listen::app::Message;
    use listen::app::servers::ServersMsg;
    let mut state = saved_servers_state();
    let _ = state.update(Message::Servers(ServersMsg::Edit(listen_core::model::ServerId("snap-nd".into()))));
    let _ = state.update(Message::Servers(ServersMsg::EditIgnoreSsl(true)));
    render_state("edit-server", state);
    let mut state = saved_servers_state();
    let _ = state.update(Message::Servers(ServersMsg::Edit(listen_core::model::ServerId("snap-yt".into()))));
    render_state("edit-server-ytm", state);
}

#[test]
fn action_required_pages() {
    use listen::app::Message;
    use listen_core::model::ServerId;
    let mut state = saved_servers_state();
    let _ = state.update(Message::Connected(
        ServerId("snap-nd".into()),
        Err("network error: error sending request (connection refused)".into()),
    ));
    render_state("action-required-unreachable", state);
    let mut state = saved_servers_state();
    let _ = state.update(Message::Connected(
        ServerId("snap-nd".into()),
        Err("Wrong username or password. (HTTP 401)".into()),
    ));
    render_state("action-required-credentials", state);
}

#[test]
fn settings_servers_section() {
    use listen::app::{Message, Route};
    let mut state = saved_servers_state();
    let nd: std::sync::Arc<dyn listen_source::MusicSource> = std::sync::Arc::new(
        listen_subsonic::Subsonic::new(
            listen_core::model::ServerId("snap-nd".into()),
            "https://music.example.com",
            "alex",
            "x",
        )
        .unwrap(),
    );
    let _ = state.update(Message::Connected(
        listen_core::model::ServerId("snap-nd".into()),
        Ok(listen::app::SourceHandle(nd)),
    ));
    let _ = state.update(Message::Navigate(Route::Settings));
    render_state("settings-servers", state);
}

/// The Advanced tab (a tall window shows every section) and the update dialog.
#[test]
fn advanced_tab_and_update_dialog() {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::Navigate(Route::Settings));
    let _ = state.update(Message::Shell(listen::app::settings_shell::ShellMsg::Tab(
        listen_core::settings::SettingsTab::Advanced,
    )));
    let _ = state.update(Message::Advanced(listen::app::advanced::AdvancedMsg::CacheSizes(vec![
        (listen::app::advanced::CacheKind::Covers, 48_300_000),
        (listen::app::advanced::CacheKind::YtmAudio, 1_740_000_000),
        (listen::app::advanced::CacheKind::Waveforms, 3_100_000),
    ])));
    render_sized("advanced-tab", state, iced::Size::new(1600.0, 3000.0));

    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let json = r###"[{"tag_name":"v9.1.0","prerelease":false,"body":"## What's new\n- In-app updater with release channels\n- Settings export and import\n- Clear caches from Settings\n\n## Fixes\n- Lyrics offset after seeking","assets":[]}]"###;
    let _ = state.update(Message::Update(listen::app::updater::UpdateMsg::Checked {
        manual: true,
        result: listen::app::updater::parse_releases(json),
    }));
    render_state("update-dialog", state);
}

// ---- sidebar ---------------------------------------------------------------------

mod sidebar_shots {
    use super::*;
    use listen::app::sidebar_state::{Popup, Scope, SidebarMsg, SidebarSetting};
    use listen::app::{Message, Route};
    use listen_core::model::{Playlist, ServerId, SourceKind};
    use std::time::Duration;

    fn playlist(id: &str, name: &str, owner: Option<&str>) -> Playlist {
        Playlist {
            id: id.into(),
            server_id: ServerId("nd".into()),
            source: SourceKind::Subsonic,
            name: name.into(),
            description: None,
            owner: owner.map(Into::into),
            image: None,
            song_count: Some(12),
            duration: Duration::from_secs(3000),
            public: false,
            editable: owner.is_none(),
        }
    }

    /// Demo state with playlists that have folders and a shared one, and two collections.
    pub fn state() -> Listen {
        let (mut s, _) = Listen::boot(&demo("tokyoNight", false));
        s.library.playlists = vec![
            playlist("liked", "Liked Music", None),
            playlist("gym", "Workout/Gym", None),
            playlist("run", "Workout/Cardio/Run", None),
            playlist("chill", "Chill evenings", None),
            playlist("daily", "Daily Mix 1", None),
            playlist("focus", "Study/Deep focus", None),
            playlist("amy1", "Party starters", Some("amy")),
            playlist("amy2", "Roadtrip", Some("amy")),
        ];
        s.config.settings.lists.collections = vec![
            listen_core::settings::Collection { name: "Unrated albums".into(), ..Default::default() },
            listen_core::settings::Collection {
                name: "Loved songs".into(),
                list: listen_core::settings::CollectionList::Songs,
                ..Default::default()
            },
        ];
        s
    }

    fn render_tall(name: &str, s: Listen) {
        render_sized(name, s, iced::Size::new(1600.0, 1500.0));
    }

    fn set(s: &mut Listen, setting: SidebarSetting) {
        let _ = s.update(Message::Sidebar(SidebarMsg::Setting(setting)));
    }

    #[test]
    fn playlists_expanded_with_collections_and_shared() {
        let mut s = state();
        let _ = s.update(Message::Navigate(Route::Home));
        render_tall("sidebar-playlists-expanded", s);
    }

    #[test]
    fn playlists_compact_with_filter() {
        let mut s = state();
        set(&mut s, SidebarSetting::Mode(listen_core::settings::PlaylistMode::Compact));
        set(&mut s, SidebarSetting::FilterRegex("^daily mix.*".into()));
        render_tall("sidebar-playlists-compact", s);
    }

    #[test]
    fn playlist_folders_tree() {
        let mut s = state();
        set(&mut s, SidebarSetting::Folders(true));
        let _ = s.update(Message::Sidebar(SidebarMsg::SetAllFolders(Scope::Owned, true)));
        render_tall("sidebar-folders-tree", s);
    }

    #[test]
    fn playlist_folders_navigation() {
        let mut s = state();
        set(&mut s, SidebarSetting::Folders(true));
        set(&mut s, SidebarSetting::FolderView(listen_core::settings::FolderView::Navigation));
        let _ = s.update(Message::Sidebar(SidebarMsg::NavEnter(Scope::Owned, "Workout".into())));
        render_tall("sidebar-folders-navigation", s);
    }

    #[test]
    fn collapsed_rail_with_playlists() {
        let mut s = state();
        let _ = s.update(Message::ToggleSidebar);
        render_tall("sidebar-rail-playlists", s);
    }

    #[test]
    fn collapsed_rail_without_navigation() {
        let mut s = state();
        let _ = s.update(Message::ToggleSidebar);
        set(&mut s, SidebarSetting::CollapsedNavigation(false));
        set(&mut s, SidebarSetting::PlaylistList(false));
        render_tall("sidebar-rail-bare", s);
    }

    #[test]
    fn app_menu_open() {
        let mut s = state();
        s.sb.popup = Some(Popup::AppMenu(iced::Point::new(290.0, 60.0)));
        render_tall("app-menu", s);
    }

    #[test]
    fn app_menu_on_the_rail() {
        let mut s = state();
        let _ = s.update(Message::ToggleSidebar);
        s.sb.popup = Some(Popup::AppMenu(iced::Point::new(88.0, 70.0)));
        render_tall("app-menu-rail", s);
    }

    #[test]
    fn settings_sidebar_section() {
        let mut s = state();
        set(&mut s, SidebarSetting::Folders(true));
        let _ = s.update(Message::Navigate(Route::Settings));
        s.config.settings.settings_tab = listen_core::settings::SettingsTab::General;
        s.config.settings.appearance.use_theme_accent = false;
        // The sidebar section is far down the page: search for it.
        let _ = s.update(Message::Shell(listen::app::settings_shell::ShellMsg::Search("sidebar".into())));
        render_tall("settings-sidebar", s);
    }
}

/// Scrobble options (General tab, minimum mode "Both" and "Seconds") and the
/// Window tab with the Discord rows, private mode on and the remote control section.
#[test]
fn integrations_settings() {
    use listen::app::integrations::{IntegrationSetting as I, IntegrationsMsg};
    use listen::app::settings_shell::ShellMsg;
    use listen::app::{Message, Route, Setting};
    use listen_core::settings::{DiscordDisplay, DiscordLink, ScrobbleMode, SettingsTab};
    let boot = |tab| {
        let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
        let _ = state.update(Message::Navigate(Route::Settings));
        let _ = state.update(Message::Shell(ShellMsg::Tab(tab)));
        state
    };
    let mut state = boot(SettingsTab::General);
    let _ = state.update(Message::Setting(Setting::Integrations(I::Notify(true))));
    render_sized("integrations-scrobble-both", state, iced::Size::new(1600.0, 1100.0));
    let mut state = boot(SettingsTab::General);
    let _ = state.update(Message::Setting(Setting::Integrations(I::ScrobbleMode(ScrobbleMode::Seconds))));
    render_sized("integrations-scrobble-seconds", state, iced::Size::new(1600.0, 1100.0));
    let mut state = boot(SettingsTab::Window);
    let _ = state.update(Message::Setting(Setting::Discord(true)));
    let _ = state.update(Message::Setting(Setting::Integrations(I::DiscordListening(true))));
    let _ = state.update(Message::Setting(Setting::Integrations(I::DiscordDisplay(DiscordDisplay::Song))));
    let _ = state.update(Message::Setting(Setting::Integrations(I::DiscordLink(DiscordLink::LastFm))));
    let _ = state.update(Message::Integrations(IntegrationsMsg::SetPrivate(true)));
    render_sized("integrations-window-discord-remote", state, iced::Size::new(1600.0, 2300.0));
}

// ---- library polish: columns, pinning, album group -----------------------------------

fn tracks_with(edits: Vec<listen::app::list::ConfigEdit>) -> Listen {
    use listen::app::list::{ListId, ListMsg};
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    let _ = state.update(Message::Navigate(Route::Tracks));
    for e in edits {
        let _ = state.update(Message::List(ListMsg::Config(ListId::Songs, e)));
    }
    state
}

#[test]
fn tracks_composer_date_and_last_played_columns() {
    use listen::app::list::ConfigEdit as E;
    use listen_core::settings::ColumnId as C;
    let state = tracks_with(vec![
        E::ColumnEnabled(C::Composer, true),
        E::ColumnEnabled(C::Date, true),
        E::ColumnEnabled(C::ReleaseDate, true),
        E::ColumnEnabled(C::LastPlayed, true),
        E::ColumnEnabled(C::Genres, false),
        E::ColumnEnabled(C::Year, false),
    ]);
    render_state("tracks-new-columns", state);
}

#[test]
fn tracks_pinned_columns_scroll_sideways() {
    use listen::app::list::ConfigEdit as E;
    use listen_core::settings::{ColumnId as C, Pin};
    let state = tracks_with(vec![
        E::AutoFit(false),
        E::ColumnEnabled(C::Composer, true),
        E::ColumnEnabled(C::Bpm, true),
        E::ColumnEnabled(C::Path, true),
        E::ColumnEnabled(C::Comment, true),
        E::ColumnEnabled(C::ReleaseDate, true),
        E::ColumnPin(C::RowIndex, Some(Pin::Left)),
        E::ColumnPin(C::TitleCombined, Some(Pin::Left)),
        E::ColumnPin(C::UserFavorite, Some(Pin::Right)),
    ]);
    render_state("tracks-pinned-columns", state);
}

#[test]
fn tracks_album_group_headers() {
    use listen::app::list::ConfigEdit as E;
    use listen_core::settings::{AlbumGroupItem as G, ColumnId as C};
    let mut state = tracks_with(vec![
        E::ColumnEnabled(C::AlbumGroup, true),
        E::ColumnEnabled(C::Album, false),
        E::GroupImageSize(96),
        E::GroupVertical(false),
        E::GroupItemEnabled(G::ReleaseYear, true),
        E::GroupItemEnabled(G::SongCount, true),
        E::GroupItemEnabled(G::Duration, true),
    ]);
    // Group the songs of three albums so that the headers have runs to stand over.
    let mut songs = Vec::new();
    for i in 0..3 {
        songs.extend(listen::demo::album_songs(i));
    }
    state.library.songs = songs;
    render_state("tracks-album-group", state);
}

#[test]
fn list_options_with_the_album_group() {
    use listen::app::Message;
    use listen::app::list::{ConfigEdit as E, ListId, ListMsg};
    use listen_core::settings::ColumnId as C;
    let mut state = tracks_with(vec![E::ColumnEnabled(C::AlbumGroup, true)]);
    let _ = state.update(Message::List(ListMsg::ConfigMenu(ListId::Songs, iced::Point::new(1000.0, 90.0))));
    render_sized("list-options-album-group", state, iced::Size::new(1600.0, 1100.0));
}

#[test]
fn tracks_row_play_popover() {
    use listen::app::list::{ListId, ListMsg};
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    let _ = state.update(Message::Navigate(Route::Tracks));
    let _ = state.update(Message::List(ListMsg::Hover(ListId::Songs, 2)));
    let rect = iced::Rectangle::new(iced::Point::new(423.0, 342.0), iced::Size::new(28.0, 28.0));
    let _ = state.update(Message::List(ListMsg::PlayHover(ListId::Songs, 2, rect)));
    let generation = state.lists.hover_gen;
    let _ = state.update(Message::List(ListMsg::PlayHoverTimer(generation, true)));
    render_state("tracks-row-play-popover", state);
}

#[test]
fn artists_grid_with_album_counts() {
    use listen::app::list::{ConfigEdit, ListId, ListMsg};
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    state.library.artists = ["Neon Coast", "Ivy Lane", "The Harbours", "Glass Cities"]
        .iter()
        .enumerate()
        .map(|(i, n)| listen_core::model::Artist {
            id: format!("ar{i}"),
            server_id: listen_core::model::ServerId("demo".into()),
            source: listen_core::model::SourceKind::Subsonic,
            name: (*n).into(),
            image: None,
            album_count: Some(i as u32 + 1),
            song_count: Some(12 * (i as u32 + 1)),
            favorite: false,
            rating: None,
            biography: None,
            similar: vec![],
        })
        .collect();
    state.library.artists_done = true;
    let _ = state.update(Message::Navigate(Route::Artists));
    let _ = state.update(Message::List(ListMsg::Config(
        ListId::Artists,
        ConfigEdit::Display(listen_core::settings::DisplayType::Grid),
    )));
    render_state("artists-grid-album-count", state);
}

#[test]
fn genre_page_with_list_header() {
    use listen::app::list_query::{FilterEdit, ListQueryMsg};
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    let _ = state.update(Message::Navigate(Route::Genre("Synthwave".into())));
    let id = listen::app::list::ListId::Genre("Synthwave".into());
    let _ = state.update(Message::ListQuery(ListQueryMsg::Filter(id, FilterEdit::Favorite(Some(true)))));
    render_state("genre-page-header", state);
}

#[test]
fn album_filters_with_tags() {
    use listen::app::list::ListId;
    use listen::app::list_query::{FilterEdit, ListQueryMsg};
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 1100.0)));
    let _ = state.update(Message::Navigate(Route::Albums));
    let _ = state.update(Message::ListQuery(ListQueryMsg::OpenFilters(ListId::Albums)));
    let _ = state.update(Message::ListQuery(ListQueryMsg::Filter(
        ListId::Albums,
        FilterEdit::Tag("mood".into(), "mood0".into()),
    )));
    render_sized("album-filters-tags", state, iced::Size::new(1600.0, 1100.0));
}

#[test]
fn albums_grid_with_an_expanded_album() {
    use listen::app::list::{ListId, ListMsg};
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    let _ = state.update(Message::Navigate(Route::Albums));
    let _ = state.update(Message::List(ListMsg::Hover(ListId::Albums, 1)));
    let _ = state.update(Message::List(ListMsg::Expand(ListId::Albums, 1)));
    render_state("albums-expanded-inline", state);
}

#[test]
fn albums_detail_view_with_badges_and_controls() {
    use listen::app::list::{ConfigEdit, ListId, ListMsg};
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    let _ = state.update(Message::Navigate(Route::Albums));
    state.library.albums[0].favorite = true;
    state.library.albums[0].rating = Some(4);
    let _ = state.update(Message::List(ListMsg::Config(
        ListId::Albums,
        ConfigEdit::Display(listen_core::settings::DisplayType::Detail),
    )));
    let _ = state.update(Message::List(ListMsg::Hover(ListId::Albums, 1)));
    render_state("albums-detail-badges", state);
}

/// The single feature half way through its slide to the next album.
#[test]
fn home_feature_sliding() {
    use listen::app::home::{FEATURE_SLIDE, HomeMsg};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let _ = state.update(listen::app::Message::Home(HomeMsg::Feature(1, 1)));
    // Held half way, so that the picture is the same every time.
    if let Some(a) = state.library.home_state.feature.anim.as_mut() {
        a.frozen = Some(0.5);
    }
    let _ = FEATURE_SLIDE;
    render_state("home-feature-sliding", state);
}

/// The "Get info" modal with Korean title and artist (drawn with the Korean font).
#[test]
fn get_info_korean() {
    use listen::app::Message;
    use listen::app::menu::{MenuAction, MenuMsg, MenuTarget};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    let mut songs = listen::demo::album_songs(0);
    songs[0].title = "봄날 (Spring Day)".into();
    songs[0].artists[0].name = "방탄소년단".into();
    songs[0].album = Some("좋은 날".into());
    let _ = state.update(Message::Menu(MenuMsg::Open(MenuTarget::songs(&songs[..1]))));
    let _ = state.update(Message::Menu(MenuMsg::Do(MenuAction::GetInfo)));
    render_state("get-info-korean", state);
}

/// A hovered "My Library" item: the hover box covers icon and text evenly.
#[test]
fn sidebar_item_hovered() {
    let (state, _) = Listen::boot(&demo("tokyoNight", true));
    let theme = state.theme();
    let _gpu = gpu();
    let mut ui = Simulator::with_size(
        iced::Settings {
            fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
            default_font: INTER,
            ..Default::default()
        },
        iced::Size::new(1600.0, 950.0),
        state.view(),
    );
    // "Albums" row of the sidebar.
    ui.point_at((100.0, 232.0));
    let _ = ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
        position: iced::Point::new(100.0, 232.0),
    })]);
    let snapshot = ui.snapshot(&theme).expect("snapshot renders");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sidebar-hover");
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with("sidebar-hover-") {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    assert!(snapshot.matches_image(&path).expect("png written"));
}

/// The play modes: the player bar button's icon per mode and the queue's
/// text switcher (Off, Shuffle, Auto-DJ, AI DJ).
fn play_mode_state(mode: &str) -> Listen {
    let (mut state, _) = Listen::boot(&demo("tokyoNight", true));
    state.queue.set(listen::demo::album_songs(0), 0);
    state.config.settings.auto_dj.enabled = mode == "auto-dj";
    state.config.settings.ai_dj.enabled = mode == "ai-dj";
    if mode == "shuffle" {
        state.queue.toggle_shuffle();
    }
    state
}

#[test]
fn play_mode_off() {
    render_sized("play-mode-off", play_mode_state("off"), iced::Size::new(1200.0, 700.0));
}

#[test]
fn play_mode_shuffle() {
    render_sized("play-mode-shuffle", play_mode_state("shuffle"), iced::Size::new(1200.0, 700.0));
}

#[test]
fn play_mode_auto_dj() {
    render_sized("play-mode-auto-dj", play_mode_state("auto-dj"), iced::Size::new(1200.0, 700.0));
}

#[test]
fn play_mode_ai_dj() {
    render_sized("play-mode-ai-dj", play_mode_state("ai-dj"), iced::Size::new(1200.0, 700.0));
}

/// Album page scrolled to the metadata: the external links are brand icons.
#[test]
fn album_external_link_icons() {
    use listen::app::{Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let album = listen::demo::navidrome_album();
    let _ = state.update(Message::Navigate(Route::Album(album.id.clone())));
    let _ = state.update(Message::AlbumLoaded(Ok(album)));
    render_sized("album-link-icons", state, iced::Size::new(1400.0, 1900.0));
}

#[test]
fn artist_external_link_icons() {
    use listen::app::{ArtistDetail, Message, Route};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    let albums = listen::demo::albums();
    let artist = listen_core::model::Artist {
        id: "UCdemo".into(),
        server_id: albums[0].server_id.clone(),
        source: listen_core::model::SourceKind::YouTubeMusic,
        name: "UNI".into(),
        image: None,
        album_count: Some(2),
        song_count: None,
        favorite: false,
        rating: None,
        biography: None,
        similar: vec![],
    };
    let _ = state.update(Message::Navigate(Route::Artist(artist.id.clone())));
    let _ = state.update(Message::ArtistLoaded(Ok(ArtistDetail {
        artist,
        albums: albums.into_iter().take(2).collect(),
        top_songs: listen::demo::album_songs(0),
    })));
    render_sized("artist-link-icons", state, iced::Size::new(1400.0, 1400.0));
}

/// The compact bar's slim volume strip in a window that is only the title bar
/// and the player bar high (the mini-player size).
#[test]
fn compact_volume_strip_in_a_short_window() {
    use listen::app::Message;
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    state.queue.set(listen::demo::album_songs(0), 0);
    let size = iced::Size::new(480.0, 130.0);
    let _ = state.update(Message::WindowResized(size));
    let _ = state.update(Message::Volume(32.0));
    let _ = state.update(Message::ToggleVolumePopup);
    render_sized("compact-volume-strip-short", state, size);
}

// ---- Milkdrop (libprojectM) --------------------------------------------------------

fn milkdrop_fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/milkdrop")
}

/// A real frame from libprojectM when it and an OpenGL display exist, else a gradient.
fn milkdrop_frame() -> Vec<u8> {
    use listen::milkdrop::{api, ffi::ProjectM, gl::Headless};
    let (w, h) = (960usize, 540usize);
    // EGL and a live Vulkan instance must not set up at the same time.
    let _gpu = gpu();
    #[cfg(target_os = "linux")]
    if let Ok(api) = api()
        && let Ok(gl) = Headless::new(w as i32, h as i32)
        && let Some(mut pm) = ProjectM::new(api, w, h, 30)
        && pm.load_preset(&milkdrop_fixtures().join("a-red.milk"), false).is_ok()
    {
        for i in 0..40 {
            let tone: Vec<f32> = (0..1152).map(|j| ((j / 2 + i * 576) as f32 * 0.058).sin() * 0.8).collect();
            pm.add_pcm_stereo(&tone);
            gl.bind();
            pm.render();
        }
        return gl.read_rgba();
    }
    (0..w * h).flat_map(|i| [(i % w * 255 / w) as u8, (i / w * 255 / h) as u8, 120, 255]).collect()
}

fn milkdrop_state(tab: listen::app::FullscreenTab) -> Listen {
    use listen::app::Message;
    use listen::app::Setting;
    use listen::app::milkdrop::{MilkdropMsg, MilkdropSetting};
    let (mut state, _) = Listen::boot(&demo("tokyoNight", false));
    state.config.settings.milkdrop.preset_dir = milkdrop_fixtures().to_string_lossy().into_owned();
    let _ = state.update(Message::Setting(Setting::Milkdrop(MilkdropSetting::Kind(
        listen_core::settings::VisualizerKind::Milkdrop,
    ))));
    let found = std::sync::Arc::new(listen::milkdrop::presets::scan(&milkdrop_fixtures()));
    let _ = state.update(Message::Milkdrop(MilkdropMsg::Scanned(milkdrop_fixtures(), found)));
    let _ = state.update(Message::OpenFullscreenTab(tab));
    let frame = iced::widget::image::Handle::from_rgba(960, 540, milkdrop_frame());
    let _ = state.update(Message::Milkdrop(MilkdropMsg::Frame(frame)));
    let _ = state.update(Message::Milkdrop(MilkdropMsg::Preset("a-red".into())));
    state
}

#[test]
fn milkdrop_fullscreen() {
    render_state("milkdrop-fullscreen", milkdrop_state(listen::app::FullscreenTab::Visualizer));
}

#[test]
fn milkdrop_queue_panel() {
    let mut state = milkdrop_state(listen::app::FullscreenTab::Hidden);
    let _ = state.update(listen::app::Message::ToggleFullscreen);
    render_state("milkdrop-queue-panel", state);
}

#[test]
fn milkdrop_settings() {
    let mut state = milkdrop_state(listen::app::FullscreenTab::Hidden);
    state.config.settings.settings_tab = listen_core::settings::SettingsTab::Playback;
    let _ = state.update(listen::app::Message::Navigate(listen::app::Route::Settings));
    render_sized("milkdrop-settings", state, iced::Size::new(1400.0, 4096.0));
}
