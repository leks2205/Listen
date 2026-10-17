//! The play-mode cycle (Off -> Shuffle -> Auto-DJ -> AI DJ) from every control, and its effects.

use iced::mouse::{Button, Event as MouseEvent};
use iced_test::simulator::Simulator;
use listen::app::play_mode::PlayMode;
use listen::app::{INTER, Listen, Message};
use listen::{Args, FONTS, demo};

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args {
        demo: true,
        show_queue: true,
        theme: Some("tokyoNight".into()),
        ..Args::default()
    });
    s.config.settings.auto_dj.enabled = false; // on by default; start in Off
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

fn feed(s: &mut Listen, messages: Vec<Message>) {
    for m in messages {
        let _ = s.update(m);
    }
}

fn click_text(s: &mut Listen, text: &str) {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        ui.click(text).unwrap_or_else(|e| panic!("no {text:?}: {e:?}"));
        ui.into_messages().collect()
    };
    feed(s, messages);
}

fn click_at(s: &mut Listen, at: (f32, f32)) -> Vec<Message> {
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
    feed(s, messages.clone());
    messages
}

/// Finds the shuffle button by clicking along the player bar's centre row.
fn find_bar_button(s: &mut Listen) -> (f32, f32) {
    for dy in [30.0, 45.0, 60.0, 20.0] {
        let y = s.window_size.height - dy;
        for x in (300..1300).step_by(12) {
            let at = (x as f32, y);
            let probe = click_at(s, at);
            if probe.iter().any(|m| matches!(m, Message::ToggleShuffle)) {
                return at;
            }
        }
    }
    panic!("the player bar has a shuffle button");
}

#[test]
fn the_player_bar_button_cycles_the_modes() {
    let mut s = state();
    // The probe click that found the button was the first press.
    let at = find_bar_button(&mut s);
    let mut seen = vec![s.play_mode()];
    for _ in 0..3 {
        click_at(&mut s, at);
        seen.push(s.play_mode());
    }
    use PlayMode::*;
    assert_eq!(seen, [Shuffle, AutoDj, Off, Shuffle]);
}

/// The queue header's mode button names the current mode and each click moves to the next.
#[test]
fn the_queue_toggle_cycles_the_modes() {
    let mut s = state();
    assert_eq!(s.play_mode(), PlayMode::Off);
    click_text(&mut s, "Off");
    assert_eq!(s.play_mode(), PlayMode::Shuffle);
    assert!(s.queue.shuffle());
    click_text(&mut s, "Shuffle");
    assert_eq!(s.play_mode(), PlayMode::AutoDj);
    assert!(s.config.settings.auto_dj.enabled && !s.queue.shuffle());
    // Without a Gemini key the AI DJ is skipped.
    assert!(!s.ai_dj_available());
    click_text(&mut s, "Auto-DJ");
    assert_eq!(s.play_mode(), PlayMode::Off);
    assert!(!s.config.settings.auto_dj.enabled);
}

#[test]
fn the_hotkey_advances_the_mode() {
    let mut s = state();
    let m = s.hotkey_message("toggleShuffle");
    assert!(matches!(m, Some(Message::ToggleShuffle)));
    let _ = s.update(m.unwrap());
    assert_eq!(s.play_mode(), PlayMode::Shuffle);
    let _ = s.update(Message::ToggleShuffle);
    assert_eq!(s.play_mode(), PlayMode::AutoDj);
}

#[cfg(target_os = "linux")]
#[test]
fn mpris_shuffle_is_true_only_for_shuffle() {
    use listen::mpris::{MprisCmd, MprisMsg};
    let mut s = state();
    let _ = s.update(Message::Mpris(MprisMsg::Command(MprisCmd::Shuffle(true))));
    assert_eq!(s.play_mode(), PlayMode::Shuffle);
    let _ = s.update(Message::Mpris(MprisMsg::Command(MprisCmd::Shuffle(false))));
    assert_eq!(s.play_mode(), PlayMode::Off);
    // False while Auto-DJ runs: setting false changes nothing.
    let _ = s.update(Message::SetPlayMode(PlayMode::AutoDj));
    let _ = s.update(Message::Mpris(MprisMsg::Command(MprisCmd::Shuffle(false))));
    assert_eq!(s.play_mode(), PlayMode::AutoDj);
    // Asking for true from Auto-DJ moves to Shuffle (and so clears Auto-DJ).
    let _ = s.update(Message::Mpris(MprisMsg::Command(MprisCmd::Shuffle(true))));
    assert_eq!(s.play_mode(), PlayMode::Shuffle);
    assert!(!s.config.settings.auto_dj.enabled);
}
