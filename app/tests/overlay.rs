//! Toasts and modals rendered and clicked headlessly.

use iced_test::simulator::Simulator;
use listen::app::overlay::{Confirm, OverlayMsg, Toast};
use listen::app::{INTER, Listen, Message};
use listen::{Args, FONTS, demo};

/// One `Simulator` (Vulkan instance) at a time, see `compact_layout.rs`.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    s.queue.set(demo::album_songs(0), 0);
    let _ = s.update(Message::WindowResized(iced::Size::new(1200.0, 700.0)));
    s
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

/// Clicks the widget showing `text`; returns the messages it sent.
fn click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        ui.click(text).unwrap_or_else(|_| panic!("no widget with “{text}”"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.find(text).is_ok()
}

#[test]
fn toast_renders_until_cleared() {
    let mut s = state();
    s.toast(Toast::error("The disk is on fire").titled("Uh oh"));
    assert!(has_text(&s, "Uh oh"));
    assert!(has_text(&s, "The disk is on fire"));
    s.overlay.toasts.clear();
    assert!(!has_text(&s, "The disk is on fire"));
}

#[test]
fn confirm_dialog_confirm_runs_the_message() {
    let mut s = state();
    s.toast_info("pending toast".into());
    let id = s.overlay.toasts[0].id;
    s.confirm(Confirm::new("Delete it?", "Really delete it?", Message::Overlay(OverlayMsg::CloseToast(id))));
    assert!(has_text(&s, "Delete it?") && has_text(&s, "Really delete it?"));
    let sent = click_text(&mut s, "Confirm");
    assert!(matches!(sent.first(), Some(Message::Overlay(OverlayMsg::Confirmed))));
    assert!(s.overlay.modal.is_none());
    assert!(s.overlay.toasts.is_empty(), "the confirmed message ran");
}

#[test]
fn confirm_dialog_cancel_closes_without_running_it() {
    let mut s = state();
    s.toast_info("stays".into());
    let id = s.overlay.toasts[0].id;
    s.confirm(Confirm::new("Delete it?", "Really?", Message::Overlay(OverlayMsg::CloseToast(id))));
    let sent = click_text(&mut s, "Cancel");
    assert!(matches!(sent.first(), Some(Message::Overlay(OverlayMsg::CloseModal))));
    assert!(s.overlay.modal.is_none());
    assert_eq!(s.overlay.toasts.len(), 1);
}
