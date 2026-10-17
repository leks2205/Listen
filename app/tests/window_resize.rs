//! Diagonal window resize and MPRIS Play.

use iced::window::Direction;
use iced_test::simulator::Simulator;
use listen::app::{INTER, Listen, Message, WindowAction};
use listen::mpris::{MprisCmd, MprisMsg};
use listen::{Args, FONTS, demo};

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn press_at(at: (f32, f32)) -> Vec<Message> {
    let size = iced::Size::new(800.0, 600.0);
    let settings = iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    };
    let _gpu = gpu();
    let content: iced::Element<'_, Message> = iced::widget::space().width(800).height(600).into();
    let mut ui = Simulator::with_size(settings, size, listen::ui::title_bar::resize_edges(content));
    ui.point_at(at);
    let _ = ui.simulate([
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: iced::Point::new(at.0, at.1) }),
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)),
    ]);
    ui.into_messages().collect()
}

fn resize_dir(messages: &[Message]) -> Option<Direction> {
    messages.iter().find_map(|m| match m {
        Message::Window(WindowAction::Resize(d)) => Some(*d),
        _ => None,
    })
}

#[test]
fn corners_start_a_diagonal_resize() {
    for (at, want) in [
        ((3.0, 3.0), Direction::NorthWest),
        ((796.0, 3.0), Direction::NorthEast),
        ((3.0, 596.0), Direction::SouthWest),
        ((796.0, 596.0), Direction::SouthEast),
        // Still inside the 10 px corner zone, past the 5 px edge strip.
        ((8.0, 8.0), Direction::NorthWest),
    ] {
        assert_eq!(
            format!("{:?}", resize_dir(&press_at(at))),
            format!("{:?}", Some(want)),
            "press at {at:?}"
        );
    }
}

#[test]
fn edges_still_resize_straight() {
    for (at, want) in [
        ((400.0, 2.0), Direction::North),
        ((400.0, 597.0), Direction::South),
        ((2.0, 300.0), Direction::West),
        ((797.0, 300.0), Direction::East),
    ] {
        assert_eq!(
            format!("{:?}", resize_dir(&press_at(at))),
            format!("{:?}", Some(want)),
            "press at {at:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn mpris_play_resumes_when_paused_and_pause_pauses() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    s.queue.set(demo::album_songs(0), 0);
    let _ = s.update(Message::TogglePlay);
    let was = s.playback.playing;
    // Paused -> Play resumes, Play again is a no-op, Pause pauses, PlayPause resumes.
    if was {
        let _ = s.update(Message::Mpris(MprisMsg::Command(MprisCmd::Pause)));
    }
    assert!(!s.playback.playing);
    let _ = s.update(Message::Mpris(MprisMsg::Command(MprisCmd::Play)));
    assert!(s.playback.playing, "Play while paused must resume");
    let _ = s.update(Message::Mpris(MprisMsg::Command(MprisCmd::Play)));
    assert!(s.playback.playing, "Play while playing stays playing");
    let _ = s.update(Message::Mpris(MprisMsg::Command(MprisCmd::Pause)));
    assert!(!s.playback.playing);
    let _ = s.update(Message::Mpris(MprisMsg::Command(MprisCmd::PlayPause)));
    assert!(s.playback.playing);
}

/// A queue restored at start-up stays paused despite the idle mpv reporting "not paused".
#[test]
fn restored_song_stays_paused_when_the_idle_mpv_reports_not_paused() {
    use listen::playback::PlayerMsg;
    use listen_player::PlayerEvent;
    let (mut s, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    s.queue.set(listen::demo::album_songs(0), 2);
    s.restore_for_test(std::time::Duration::from_secs(42));
    let _ = s.update(Message::Deck(0, PlayerMsg::Event(PlayerEvent::Paused(false))));
    assert!(!s.playback.playing, "the restored song must show the play icon");
}
