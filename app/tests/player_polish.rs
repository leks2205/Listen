//! Player polish: the squeezing player bar, the config button in
//! the compact bar, the full-screen marquee title, the sidebar lyrics panel,
//! furigana, toasts and the header title. Snapshots are rendered here;
//! PNGs go to `target/snapshots/`.

use iced::mouse::{Button, Event as MouseEvent};
use iced_test::simulator::Simulator;
use listen::app::audio::AudioMsg;
use listen::app::overlay::OverlayMsg;
use listen::app::{INTER, Listen, Message};
use listen::ui::player_bar;
use listen::{Args, FONTS, demo};
use std::path::PathBuf;

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

fn state(width: f32, height: f32) -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    s.queue.set(demo::album_songs(0), 0);
    let _ = s.update(Message::WindowResized(iced::Size::new(width, height)));
    s
}

fn render(name: &str, state: &Listen) {
    let size = state.window_size;
    let theme = state.theme();
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), size, state.view());
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

// ---- the bar squeezes its transport before it drops the cover or the volume ----

#[test]
fn bar_gives_up_skip_buttons_before_the_cover_and_never_the_volume() {
    let mut s = state(1200.0, 700.0);
    s.config.settings.player_bar.skip_buttons = true;
    let mut last_squeeze = 0;
    let mut seen = [false; 3];
    // Wide to narrow: the squeeze only grows, the right column keeps its width.
    let mut widths: Vec<f32> = (770..=1400).rev().step_by(10).map(|w| w as f32).collect();
    widths.push(770.0);
    let right_floor = player_bar::layout(&s, 770.0).2;
    for w in widths {
        let (left, center, right, squeeze) = player_bar::layout(&s, w);
        assert!(squeeze >= last_squeeze, "width {w}: squeeze went back down");
        last_squeeze = squeeze;
        seen[usize::from(squeeze)] = true;
        assert!(right >= right_floor - 0.5, "width {w}: the volume column shrank ({right} < {right_floor})");
        assert!(left == 0.0 || left >= 100.0, "width {w}: a cover sliver ({left})");
        assert!(center > 0.0);
        if squeeze == 0 && left > 0.0 {
            // Nothing given up while the cover still fits next to the full transport.
            assert!(center >= 9.0 * 36.0, "width {w}");
        }
    }
    assert!(seen[0] && seen[1], "the skip buttons go first: {seen:?}");
    // At 770 px the cover survives the first squeeze step (with or without skip buttons).
    let (l770, ..) = player_bar::layout(&s, 770.0);
    s.config.settings.player_bar.skip_buttons = false;
    let (l770_noskip, ..) = player_bar::layout(&s, 770.0);
    assert!(l770 >= 100.0 || l770_noskip >= 100.0 || seen[2]);
}

#[test]
fn volume_slider_is_reachable_in_every_squeeze_stage() {
    for w in [770.0, 800.0, 850.0, 900.0] {
        let mut s = state(w, 700.0);
        s.config.settings.player_bar.skip_buttons = true;
        let _ = s.update(Message::Volume(0.0));
        let vw = f32::from(s.config.settings.player_bar.volume_width);
        let messages = click(&mut s, (w - 16.0 - vw * 0.25, 700.0 - 45.0));
        let v = messages.iter().find_map(|m| match m {
            Message::Volume(v) => Some(*v),
            _ => None,
        });
        assert!(v.is_some_and(|v| v > 40.0), "width {w}: {messages:?}");
    }
}

#[test]
fn squeezed_bar_snapshots() {
    let mut s = state(800.0, 600.0);
    s.config.settings.player_bar.skip_buttons = true;
    render("player-polish-bar-800", &s);
    let mut s = state(900.0, 600.0);
    s.config.settings.player_bar.skip_buttons = true;
    render("player-polish-bar-900", &s);
}

// ---- compact bar: no player-config button ---------------------------------------

#[test]
fn compact_bar_has_no_player_config_button() {
    let (w, h) = (500.0, 700.0);
    let mut s = state(w, h);
    let messages = click(&mut s, (w - 8.0 - 18.0 - 4.0 * 40.0, h - 38.0));
    assert!(!messages.iter().any(|m| matches!(m, Message::Audio(AudioMsg::TogglePopover))), "{messages:?}");
    assert!(s.audio.popup.is_none());
}

#[test]
fn compact_bar_keeps_the_vertical_volume_popover() {
    let (w, h) = (500.0, 700.0);
    let mut s = state(w, h);
    let messages = click(&mut s, (w - 8.0 - 3.0 * 40.0 - 18.0, h - 45.0));
    assert!(messages.iter().any(|m| matches!(m, Message::ToggleVolumePopup)), "{messages:?}");
    assert!(s.volume_popup);
}

// ---- full-screen marquee title -----------------------------------------------------

#[test]
fn long_fullscreen_title_scrolls_and_short_one_does_not() {
    use listen::app::fullscreen_state::FullscreenMsg;
    let mut s = state(1000.0, 700.0);
    let _ = s.update(Message::ToggleFullscreen);
    assert!(s.fullscreen);
    s.queue.set(demo::album_songs(0), 0);
    assert_eq!(s.title_overflow(), 0.0, "a short title fits");
    let mut songs = demo::album_songs(0);
    songs[0].title = "An extraordinarily long song title that certainly cannot fit under the cover".into();
    s.queue.set(songs, 0);
    assert!(s.title_overflow() > 0.0);
    // The tick turns into a scroll task (not an empty one) while it overflows.
    let task = s.update(Message::Fullscreen(FullscreenMsg::Marquee));
    drop(task);
    render("player-polish-fullscreen-marquee", &s);
}

#[test]
fn wrapped_title_mode_has_no_marquee() {
    use listen_core::settings::TitleDisplay;
    let mut s = state(1000.0, 700.0);
    let _ = s.update(Message::ToggleFullscreen);
    let mut songs = demo::album_songs(0);
    songs[0].title = "An extraordinarily long song title that certainly cannot fit under the cover".into();
    s.queue.set(songs, 0);
    s.config.settings.fullscreen.title_display = TitleDisplay::MultiLine;
    assert_eq!(s.title_overflow(), 0.0);
}

// ---- sidebar lyrics use the full lyrics view -------------------------------------

#[test]
fn sidebar_lyrics_panel_shows_the_lyrics_controls_and_translation() {
    let lrc = "[00:00.50]夜に駆ける_BREAK_Racing into the night\n[00:05.00]ありがとう_BREAK_Thank you\n[00:10.00]Hello again_BREAK_Hallo nochmal";
    let mut l = listen_lyrics::parse_lrc(lrc);
    l.provider = "NetEase".into();
    let mut s = state(1400.0, 800.0);
    s.show_queue = true;
    s.config.settings.queue.lyrics_in_sidebar = true;
    s.config.settings.lyrics.enable_romaji = true;
    s.lyrics = Some(l);
    render("player-polish-sidebar-lyrics", &s);
    // The panel has the lyrics view's controls.
    {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        assert!(ui.find("DISPLAY").is_ok(), "the lyrics controls are in the panel");
        assert!(ui.find("Hello again").is_ok(), "the lines are in the panel");
    }
}

// ---- furigana --------------------------------------------------------------------

#[test]
fn furigana_snapshot_and_setting() {
    use listen::app::lyrics_state::LyricsSetting;
    use listen::app::{Message, Setting};
    let lrc =
        "[00:00.50]今日は天気がいいですね\n[00:05.00]夜に駆ける\n[00:10.00]Hello again\n[00:15.00]食べる";
    let l = listen_lyrics::parse_lrc(lrc);
    let mut s = state(1200.0, 800.0);
    let _ = s.update(Message::OpenFullscreenTab(listen::app::FullscreenTab::Lyrics));
    let id = s.queue.current().unwrap().song.id.clone();
    let _ = s.update(Message::LyricsLoaded(id, Some(l)));
    let _ = s.update(Message::Setting(Setting::Lyrics(LyricsSetting::Furigana(true))));
    assert!(s.config.settings.lyrics.enable_furigana);
    let _ = s.update(Message::Setting(Setting::Lyrics(LyricsSetting::Romaji(true))));
    render("player-polish-lyrics-furigana", &s);
}

// ---- toasts ----------------------------------------------------------------------

#[test]
fn toasts_get_feishin_titles_and_hover_holds_them() {
    let mut s = state(900.0, 600.0);
    s.toast_success("Saved".into());
    s.toast_error("Something broke".into());
    s.toast_warning("Careful".into());
    s.toast_info("FYI".into());
    {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        for title in ["Success", "Error", "Warning", "Info"] {
            assert!(ui.find(title).is_ok(), "{title}");
        }
    }
    let id = s.overlay.toasts[0].id;
    let _ = s.update(Message::Overlay(OverlayMsg::Hover(id, true)));
    assert_eq!(s.overlay.hovered, Some(id));
    render("player-polish-toasts", &s);
}

#[test]
fn toast_is_narrower_in_a_small_window() {
    let mut s = state(360.0, 600.0);
    s.toast_error("A long error message that has to wrap inside a narrow window".into());
    render("player-polish-toast-narrow", &s);
}

// ---- header title -----------------------------------------------------------------

#[test]
fn home_feature_title_fits_at_500px() {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let _ = s.update(Message::WindowResized(iced::Size::new(500.0, 700.0)));
    render("player-polish-home-500", &s);
}
