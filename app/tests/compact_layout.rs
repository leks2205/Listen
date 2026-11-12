//! The compact layout (window <= 768 px): breakpoint, sidebar drawer, volume
//! popover, the full bar keeping its volume slider, the seek bar.

mod common;

use common::layout;
use iced::mouse::{Button, Event as MouseEvent, ScrollDelta};
use iced_test::simulator::Simulator;
use listen::app::{Listen, Message};
use listen::style::metrics;
use listen::ui::LayoutMode;
use listen::ui::player_bar::compact;
use listen::ui::sidebar;
use listen::{Args, demo};

fn state(width: f32, height: f32) -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    s.queue.set(demo::album_songs(0), 0);
    let _ = s.update(Message::WindowResized(iced::Size::new(width, height)));
    s
}

fn size(s: &Listen) -> iced::Size {
    s.window_size
}

/// Runs `events` against the current view and feeds the messages back.
fn run(s: &mut Listen, at: (f32, f32), events: impl IntoIterator<Item = iced::Event>) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = common::gpu();
        let mut ui = Simulator::with_size(common::iced_settings(), size(s), s.view());
        ui.point_at(at);
        let _ = ui.simulate(events);
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn click(s: &mut Listen, at: (f32, f32)) -> Vec<Message> {
    run(
        s,
        at,
        [
            iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)),
            iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Left)),
        ],
    )
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(common::iced_settings(), size(s), s.view());
    ui.find(text).is_ok()
}

#[test]
fn layout_switches_at_768_and_769() {
    assert_eq!(state(768.0, 600.0).layout_mode(), LayoutMode::Compact);
    assert_eq!(state(769.0, 600.0).layout_mode(), LayoutMode::Full);
    assert_eq!(state(480.0, 700.0).layout_mode(), LayoutMode::Compact);
    // Compact: a Menu button and no sidebar; full: the sidebar and no Menu.
    assert!(has_text(&state(768.0, 600.0), "Menu"));
    assert!(!has_text(&state(768.0, 600.0), "My Library"));
    assert!(!has_text(&state(769.0, 600.0), "Menu"));
    assert!(has_text(&state(1200.0, 800.0), "My Library"));
    // The Menu button hides in a very short window.
    assert!(!has_text(&state(600.0, 190.0), "Menu"));
}

#[test]
fn the_pip_button_and_hotkey_are_gone() {
    let (s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    assert!(!s.config.settings.hotkeys.contains_key("toggleMiniPlayer"));
}

#[test]
fn drawer_opens_and_closes() {
    let mut s = state(700.0, 600.0);
    assert!(!s.drawer_open);
    // The floating Menu button, found by its label.
    let everywhere = iced::Rectangle::with_size(size(&s));
    let menu = layout::text_center(&s, size(&s), "Menu", everywhere).expect("the Menu button");
    let messages = click(&mut s, menu);
    assert!(messages.iter().any(|m| matches!(m, Message::ToggleDrawer)), "{messages:?}");
    assert!(s.drawer_open);
    assert!(has_text(&s, "My Library"), "the drawer shows the sidebar");
    // A click on the backdrop (right of the drawer) closes it.
    let backdrop = (sidebar::DRAWER_WIDTH + (size(&s).width - sidebar::DRAWER_WIDTH) / 2.0, 300.0);
    let messages = click(&mut s, backdrop);
    assert!(messages.iter().any(|m| matches!(m, Message::CloseDrawer)), "{messages:?}");
    assert!(!s.drawer_open);
    // Navigating closes it too.
    let _ = s.update(Message::ToggleDrawer);
    let _ = s.update(Message::Navigate(listen::app::Route::Albums));
    assert!(!s.drawer_open);
    // Growing the window past the breakpoint drops it.
    let _ = s.update(Message::ToggleDrawer);
    let _ = s.update(Message::WindowResized(iced::Size::new(1000.0, 600.0)));
    assert!(!s.drawer_open);
}

/// Middle of the compact bar's seek strip, along the bar's top edge.
fn seek_bar(s: &Listen) -> (f32, f32) {
    (size(s).width / 2.0, layout::bar_top(size(s)) + compact::SEEK_HEIGHT / 2.0)
}

fn speaker(s: &Listen) -> (f32, f32) {
    (compact::speaker_center_x(size(s).width), layout::bar_center_y(size(s)))
}

#[test]
fn volume_popover_opens_changes_volume_and_closes_on_outside_click() {
    let mut s = state(700.0, 600.0);
    let _ = s.update(Message::Volume(30.0));
    let at = speaker(&s);
    let messages = click(&mut s, at);
    assert!(messages.iter().any(|m| matches!(m, Message::ToggleVolumePopup)), "{messages:?}");
    assert!(s.volume_popup);

    // The popover is a horizontal slider box right above the speaker icon.
    let p = compact::popover_position(size(&s));
    const { assert!(compact::POPOVER.width > compact::POPOVER.height * 3.0) };
    // Directly above the speaker icon (27 px into the 90 px bar).
    assert!(p.y + compact::POPOVER.height <= speaker(&s).1 - 18.0 + 1.0, "above the speaker icon");
    const { assert!(compact::POPOVER.height <= 24.0, "a slim strip") };
    assert!((p.x + compact::POPOVER.width / 2.0 - speaker(&s).0).abs() <= 1.0, "centred over the speaker");
    // Click the middle of the 100 px track (8 px padding; the value sits right of it).
    let slider = (p.x + 8.0 + 50.0, p.y + compact::POPOVER.height / 2.0);
    let messages = click(&mut s, slider);
    let volume = messages.iter().find_map(|m| match m {
        Message::Volume(v) => Some(*v),
        _ => None,
    });
    let v = volume.unwrap_or_else(|| panic!("no volume message: {messages:?}"));
    assert!((30.0..=70.0).contains(&v) && (v - 30.0).abs() > 1.0, "slider midpoint gave {v}");
    assert_eq!(f32::from(s.playback.volume), v.round());
    assert!(s.volume_popup, "dragging the slider keeps it open");

    let messages = click(&mut s, (100.0, 100.0));
    assert!(messages.iter().any(|m| matches!(m, Message::CloseVolumePopup)), "{messages:?}");
    assert!(!s.volume_popup);
}

#[test]
fn wheel_over_the_speaker_changes_volume_by_the_step() {
    let mut s = state(700.0, 600.0);
    let _ = s.update(Message::Volume(50.0));
    s.config.settings.player_bar.volume_wheel_step = 7;
    let at = speaker(&s);
    let up = iced::Event::Mouse(MouseEvent::WheelScrolled { delta: ScrollDelta::Lines { x: 0.0, y: 1.0 } });
    let messages = run(&mut s, at, [up]);
    assert!(messages.iter().any(|m| matches!(m, Message::VolumeWheel(y) if *y > 0.0)), "{messages:?}");
    assert_eq!(s.playback.volume, 57);
    let _ = s.update(Message::VolumeWheel(-1.0));
    let _ = s.update(Message::VolumeWheel(-1.0));
    assert_eq!(s.playback.volume, 43);
    for _ in 0..20 {
        let _ = s.update(Message::VolumeWheel(-1.0));
    }
    assert_eq!(s.playback.volume, 0, "clamped");
}

#[test]
fn right_click_on_the_speaker_toggles_mute() {
    let mut s = state(700.0, 600.0);
    let at = speaker(&s);
    let messages = run(
        &mut s,
        at,
        [
            iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Right)),
            iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Right)),
        ],
    );
    assert!(messages.iter().any(|m| matches!(m, Message::ToggleMute)), "{messages:?}");
    assert!(s.playback.muted);
}

#[test]
fn full_bar_keeps_the_volume_slider_at_every_width() {
    for (w, h) in [(769.0, 600.0), (770.0, 600.0), (900.0, 700.0), (1200.0, 800.0), (2400.0, 1000.0)] {
        let mut s = state(w, h);
        let _ = s.update(Message::Volume(0.0));
        // The slider is the last control: `volume_width` px, `space_lg` from the right edge.
        let vw = f32::from(s.config.settings.player_bar.volume_width);
        let at = (w - metrics::space_lg() - vw * 0.25, layout::bar_center_y(size(&s)));
        let messages = click(&mut s, at);
        let v = messages.iter().find_map(|m| match m {
            Message::Volume(v) => Some(*v),
            _ => None,
        });
        assert!(v.is_some_and(|v| v > 40.0), "width {w}: no volume slider under the cursor: {messages:?}");
    }
}

#[test]
fn full_bar_wheel_over_the_slider_changes_volume() {
    let mut s = state(1200.0, 800.0);
    let vw = f32::from(s.config.settings.player_bar.volume_width);
    let up = iced::Event::Mouse(MouseEvent::WheelScrolled { delta: ScrollDelta::Lines { x: 0.0, y: 1.0 } });
    let at = (1200.0 - metrics::space_lg() - vw / 2.0, layout::bar_center_y(size(&s)));
    let messages = run(&mut s, at, [up]);
    assert!(messages.iter().any(|m| matches!(m, Message::VolumeWheel(y) if *y > 0.0)), "{messages:?}");
}

#[test]
fn compact_bar_has_a_seek_bar_that_seeks_on_release() {
    let mut s = state(600.0, 600.0);
    let at = seek_bar(&s);
    let messages = click(&mut s, at);
    assert!(messages.iter().any(|m| matches!(m, Message::SeekPreview(_))), "{messages:?}");
    assert!(messages.iter().any(|m| matches!(m, Message::SeekRelease)), "{messages:?}");
    assert!(s.seek_preview.is_none());
}

#[test]
fn compact_bar_has_no_seek_bar_for_radio() {
    let mut s = state(600.0, 600.0);
    s.radio.active = Some(listen::app::radio::ActiveStation {
        station: listen_source::InternetRadio {
            id: "r".into(),
            name: "Station".into(),
            stream_url: "https://radio.example/live".into(),
            home_page_url: None,
            image_id: None,
        },
        title: None,
    });
    let at = seek_bar(&s);
    let messages = click(&mut s, at);
    assert!(!messages.iter().any(|m| matches!(m, Message::SeekPreview(_))), "{messages:?}");
}

#[test]
fn compact_layout_wins_over_the_fullscreen_player() {
    let mut s = state(1000.0, 700.0);
    let _ = s.update(Message::ToggleFullscreen);
    assert!(s.fullscreen);
    let _ = s.update(Message::WindowResized(iced::Size::new(600.0, 700.0)));
    assert!(has_text(&s, "Menu"));
    // Toggling does nothing while compact; growing brings the player back.
    let _ = s.update(Message::ToggleFullscreen);
    assert!(s.fullscreen);
    let _ = s.update(Message::WindowResized(iced::Size::new(1000.0, 700.0)));
    assert!(!has_text(&s, "Menu"));
}
