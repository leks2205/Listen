//! Dragging the dividers resizes the left sidebar and the queue panel.

use iced_test::simulator::Simulator;
use listen::Args;
use listen::app::{Listen, Message, PanelSide};

fn state() -> Listen {
    let (s, _) = Listen::boot(&Args {
        demo: true,
        show_queue: true,
        theme: Some("tokyoNight".into()),
        ..Args::default()
    });
    s
}

#[test]
fn dragging_follows_the_cursor_within_limits() {
    let mut s = state();
    let (left, right) = (s.config.settings.sidebar.width, s.config.settings.sidebar.queue_width);

    // Feishin keeps the sidebar between 260 and 400 px.
    s.config.settings.sidebar.width = 300.0;
    let _ = s.update(Message::ResizeStart(PanelSide::Left));
    for x in [400.0, 440.0, 450.0] {
        let _ = s.update(Message::ResizeMove(x));
    }
    let _ = s.update(Message::ResizeEnd);
    assert_eq!(s.config.settings.sidebar.width, 350.0);
    let _ = left;

    // The queue grows when its divider moves left.
    let _ = s.update(Message::ResizeStart(PanelSide::Right));
    let _ = s.update(Message::ResizeMove(1000.0));
    let _ = s.update(Message::ResizeMove(900.0));
    let _ = s.update(Message::ResizeEnd);
    assert_eq!(s.config.settings.sidebar.queue_width, right + 100.0);

    // Limits: at most 400 px.
    let _ = s.update(Message::ResizeStart(PanelSide::Left));
    let _ = s.update(Message::ResizeMove(500.0));
    let _ = s.update(Message::ResizeMove(1500.0));
    assert_eq!(s.config.settings.sidebar.width, 400.0);
    let _ = s.update(Message::ResizeEnd);
    // Moves after release change nothing.
    let before = s.config.settings.sidebar.clone();
    let _ = s.update(Message::ResizeMove(3000.0));
    assert_eq!(s.config.settings.sidebar, before);
}

#[test]
fn dragging_under_160_px_collapses_and_back_expands() {
    let mut s = state();
    s.config.settings.sidebar.width = 300.0;
    let _ = s.update(Message::ResizeStart(PanelSide::Left));
    let _ = s.update(Message::ResizeMove(300.0));
    let _ = s.update(Message::ResizeMove(170.0)); // 300 - 130 = 170: still open, clamped
    assert!(!s.config.settings.sidebar.collapsed);
    assert_eq!(s.config.settings.sidebar.width, 260.0);
    let _ = s.update(Message::ResizeMove(150.0)); // 150 < 160
    assert!(s.config.settings.sidebar.collapsed);
    let _ = s.update(Message::ResizeMove(300.0));
    assert!(!s.config.settings.sidebar.collapsed);
    let _ = s.update(Message::ResizeEnd);
}

#[test]
fn pressing_the_divider_starts_a_drag() {
    let s = state();
    let x = s.config.settings.sidebar.width + 2.5; // the 5 px handle right of the sidebar
    let mut ui = Simulator::with_size(iced::Settings::default(), iced::Size::new(1600.0, 950.0), s.view());
    ui.point_at((x, 400.0));
    let _ = ui.simulate(iced_test::simulator::click());
    let messages: Vec<Message> = ui.into_messages().collect();
    assert!(messages.iter().any(|m| matches!(m, Message::ResizeStart(PanelSide::Left))), "{messages:?}");
}
