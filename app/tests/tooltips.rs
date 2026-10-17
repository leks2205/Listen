//! Hover tooltips: the function plus the live hotkey binding of the action.

use iced::mouse::{Button, Event as MouseEvent};
use iced_test::simulator::Simulator;
use listen::app::{INTER, Listen, Message};
use listen::ui::tip::{binding_text, tip_text};
use listen::{Args, FONTS, demo};
use std::time::Duration;

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    s.config.settings.auto_dj.enabled = false;
    s.queue.set(demo::album_songs(0), 0);
    let _ = s.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    s
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

/// Presses along `y` over `xs` in one interface and reports whether `wanted` was sent.
fn presses(s: &Listen, y: f32, xs: &[f32], wanted: fn(&Message) -> bool) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    for x in xs {
        ui.point_at((*x, y));
        let _ = ui.simulate([
            iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)),
            iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Left)),
        ]);
    }
    ui.into_messages().any(|m| wanted(&m))
}

/// Where the player bar button that sends `wanted` sits (found by bisecting presses).
fn find_button(s: &Listen, wanted: fn(&Message) -> bool) -> (f32, f32) {
    let ys: Vec<f32> = [30.0, 45.0, 60.0, 20.0].iter().map(|dy| s.window_size.height - dy).collect();
    find_at(s, &ys, wanted)
}

/// [`find_button`] over the given rows.
fn find_at(s: &Listen, ys: &[f32], wanted: fn(&Message) -> bool) -> (f32, f32) {
    for &y in ys {
        let mut xs: Vec<f32> = (100..1500).step_by(4).map(|x| x as f32).collect();
        if !presses(s, y, &xs, wanted) {
            continue;
        }
        while xs.len() > 1 {
            let right = xs.split_off(xs.len() / 2);
            if !presses(s, y, &xs, wanted) {
                xs = right;
            }
        }
        return (xs[0], y);
    }
    panic!("button not found");
}

/// Hovers `at`, waits past the tooltip delay and returns the picture with the tooltip open.
fn hovered(s: &Listen, at: (f32, f32)) -> iced_test::simulator::Snapshot {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.point_at(at);
    let moved = iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: at.into() });
    let _ = ui.simulate([moved.clone()]);
    std::thread::sleep(Duration::from_millis(700));
    let _ = ui.simulate([moved]);
    ui.snapshot(&s.theme()).expect("snapshot renders")
}

/// The picture without any hover.
fn idle(s: &Listen) -> iced_test::simulator::Snapshot {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.snapshot(&s.theme()).expect("snapshot renders")
}

/// A scratch hash file: `first` writes it, later calls compare with it.
fn hash_path(name: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/tooltip-hashes");
    std::fs::create_dir_all(&dir).unwrap();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
            std::fs::remove_file(e.path()).unwrap();
        }
    }
    dir.join(name)
}

#[test]
fn binding_text_is_readable() {
    assert_eq!(binding_text("space").as_deref(), Some("Space"));
    assert_eq!(binding_text("mod+arrowright").as_deref(), Some("Ctrl+→"));
    assert_eq!(binding_text("shift+mod+alt+arrowup").as_deref(), Some("Ctrl+Alt+Shift+↑"));
    assert_eq!(binding_text("mod+k").as_deref(), Some("Ctrl+K"));
    assert_eq!(binding_text("f11").as_deref(), Some("F11"));
}

#[test]
fn text_follows_the_bindings_live() {
    let mut s = state();
    // Unbound: just the function.
    s.config.settings.hotkeys.remove("next");
    assert_eq!(tip_text(&s, "Next track", Some("next")), "Next track");
    s.config.settings.hotkeys.insert("next".into(), "mod+arrowright".into());
    assert_eq!(tip_text(&s, "Next track", Some("next")), "Next track · Ctrl+→");
    // Rebinding shows at once, a cleared binding drops the key again.
    s.config.settings.hotkeys.insert("next".into(), "shift+n".into());
    assert_eq!(tip_text(&s, "Next track", Some("next")), "Next track · Shift+N");
    s.config.settings.hotkeys.insert("next".into(), String::new());
    assert_eq!(tip_text(&s, "Next track", Some("next")), "Next track");
    // A button without an action has no key.
    assert_eq!(tip_text(&s, "Refresh", None), "Refresh");
    // Global bindings read the same.
    s.config.settings.hotkeys.insert("playPause".into(), "mod+alt+space".into());
    s.config.settings.hotkeys_global.insert("playPause".into());
    assert_eq!(tip_text(&s, "Pause", Some("playPause")), "Pause · Ctrl+Alt+Space");
}

#[test]
fn hovering_next_opens_a_tooltip_that_follows_the_binding() {
    let mut s = state();
    s.config.settings.hotkeys.insert("next".into(), "mod+arrowright".into());
    let at = find_button(&s, |m| matches!(m, Message::NextClick));
    assert_eq!(tip_text(&s, "Next track", Some("next")), "Next track · Ctrl+→");

    let path = hash_path("next-ctrl-right");
    // Not hovered: no tooltip, so the picture differs from the hovered one.
    assert!(idle(&s).matches_hash(&path).unwrap());
    let open = hovered(&s, at);
    assert!(!open.matches_hash(&path).unwrap(), "the tooltip is drawn");

    // The same hover again draws the same pixels; another binding draws other text.
    let path = hash_path("next-ctrl-right-hover");
    assert!(hovered(&s, at).matches_hash(&path).unwrap());
    assert!(hovered(&s, at).matches_hash(&path).unwrap(), "deterministic");
    s.config.settings.hotkeys.insert("next".into(), "shift+n".into());
    assert_eq!(tip_text(&s, "Next track", Some("next")), "Next track · Shift+N");
    assert!(!hovered(&s, at).matches_hash(&path).unwrap(), "the rebound key shows");
}

#[test]
fn the_play_mode_button_names_the_mode_and_the_next() {
    use listen::ui::tip::{play_mode_tip, tip_text_raw};
    let mut s = state();
    s.config.settings.hotkeys.insert("toggleShuffle".into(), "s".into());
    assert_eq!(
        tip_text_raw(&s, &play_mode_tip(&s), Some("toggleShuffle")),
        "Play mode: Off → next: Shuffle · S"
    );
    let at = find_button(&s, |m| matches!(m, Message::ToggleShuffle));
    let path = hash_path("mode-off");
    assert!(hovered(&s, at).matches_hash(&path).unwrap());
    // After a click the text names the new mode, so the picture changes.
    let _ = s.update(Message::ToggleShuffle);
    assert_eq!(
        tip_text_raw(&s, &play_mode_tip(&s), Some("toggleShuffle")),
        "Play mode: Shuffle → next: Auto-DJ · S"
    );
    assert!(!hovered(&s, at).matches_hash(&path).unwrap());
}

#[test]
fn a_tooltip_waits_for_the_pointer_to_rest() {
    let s = state();
    let at = find_button(&s, |m| matches!(m, Message::NextClick));
    let path = hash_path("rest");
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.point_at(at);
    let moved = iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: at.into() });
    let _ = ui.simulate([moved.clone()]);
    assert!(ui.snapshot(&s.theme()).unwrap().matches_hash(&path).unwrap());
    std::thread::sleep(Duration::from_millis(700));
    let _ = ui.simulate([moved]);
    assert!(!ui.snapshot(&s.theme()).unwrap().matches_hash(&path).unwrap(), "shown after the delay");
}

/// Writes the hovered picture to `target/snapshots/<name>-*.png` for a look.
fn save(name: &str, s: &Listen, at: (f32, f32)) {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
            std::fs::remove_file(e.path()).unwrap();
        }
    }
    assert!(hovered(s, at).matches_image(dir.join(name)).unwrap());
}

#[test]
fn snapshot_player_bar_tooltip() {
    let mut s = state();
    s.config.settings.hotkeys.insert("next".into(), "mod+arrowright".into());
    let at = find_button(&s, |m| matches!(m, Message::NextClick));
    save("tooltip-player-bar", &s, at);
}

#[test]
fn snapshot_list_header_tooltip() {
    use listen::app::list_query::ListQueryMsg;
    let mut s = state();
    let _ = s.update(Message::Navigate(listen::app::Route::Tracks));
    let ys: Vec<f32> = (90..200).step_by(6).map(|y| y as f32).collect();
    let at = find_at(&s, &ys, |m| matches!(m, Message::ListQuery(ListQueryMsg::ToggleOrder(_))));
    save("tooltip-list-header", &s, at);
}
