//! Touchpad momentum: a swipe keeps gliding after the fingers lift; holding
//! still before lifting doesn't fling.

use iced::mouse::ScrollDelta;
use iced::widget::{column, container, scrollable};
use iced::{Element, Event, Length, mouse, window};
use iced_test::simulator::Simulator;
use listen::ui::kinetic::{factor, kinetic};

/// A typical laptop: scroll speed 100 %, display scale 1.35.
fn laptop() -> f32 {
    factor(1.0, 1.35)
}
use std::time::Duration;

fn view<'a>() -> Element<'a, f32> {
    let rows = (0..3000).map(|_| container("row").height(40).into());
    kinetic(
        scrollable(column(rows))
            .on_scroll(|v| v.absolute_offset().y)
            .width(Length::Fill)
            .height(Length::Fixed(400.0)),
        laptop(),
    )
    .into()
}

fn wheel(y: f32) -> Event {
    Event::Mouse(mouse::Event::WheelScrolled { delta: ScrollDelta::Pixels { x: 0.0, y } })
}

fn frames(ui: &mut Simulator<'_, f32>, n: usize) {
    for _ in 0..n {
        std::thread::sleep(Duration::from_millis(16));
        ui.simulate([Event::Window(window::Event::RedrawRequested(iced::time::Instant::now()))]);
    }
}

/// Five quick 20 px swipe events, then `pause`, then fingers lift, then 1 s of frames.
fn swipe(pause: Duration) -> (f32, f32) {
    let mut ui = Simulator::with_size(iced::Settings::default(), iced::Size::new(300.0, 400.0), view());
    ui.point_at((100.0, 100.0));
    for _ in 0..5 {
        ui.simulate([wheel(-20.0)]);
        std::thread::sleep(Duration::from_millis(8));
    }
    std::thread::sleep(pause);
    ui.simulate([wheel(0.0)]); // Wayland axis_stop
    let during = 5.0 * 20.0 * laptop();
    frames(&mut ui, 60);
    let offset = ui.into_messages().last().unwrap_or(0.0);
    (during, offset)
}

#[test]
fn swipe_keeps_gliding_after_release() {
    let (during, offset) = swipe(Duration::ZERO);
    println!("while touching: {during} px, after glide: {offset} px");
    assert!(
        offset > during * 2.0,
        "glide should continue well past the finger movement ({offset} vs {during})"
    );
}

#[test]
fn holding_still_before_lifting_does_not_fling() {
    let (during, offset) = swipe(Duration::from_millis(200));
    assert!((offset - during).abs() < 1.0, "no fling after holding still ({offset} vs {during})");
}
