//! Toasts, the generic modal and the confirm dialog (state + messages); drawn by `ui::overlay`.
//!
//! - `self.toast_info(text)`, `toast_success`, `toast_error`, `toast_warning`
//!   (any `Into<String>`), `self.toast(Toast::error("Title", "text").persistent())`.
//! - `self.confirm(Confirm::new("Delete playlist", "This cannot be undone.", Message::..))`.
//! - `self.open_modal(key)` / `self.close_modal()` for a custom modal whose body
//!   `ui::overlay::custom_modal` renders by `key` (add an arm there).

use super::{Listen, Message};
use iced::Task;
use std::time::{Duration, Instant};

/// Toasts closing themselves after this long (Feishin: 5 s).
pub const AUTO_CLOSE: Duration = Duration::from_secs(5);
/// At most this many toasts are on screen; the oldest goes first.
pub const MAX_VISIBLE: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
    Info,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub title: Option<String>,
    pub message: String,
    /// Stays until closed.
    pub persistent: bool,
    /// A button on the toast (label, message), e.g. "Relaunch".
    pub action: Option<(String, Box<Message>)>,
    pub created: Instant,
}

impl Toast {
    fn new(kind: ToastKind, title: Option<String>, message: String) -> Self {
        Self { id: 0, kind, title, message, persistent: false, action: None, created: Instant::now() }
    }
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(ToastKind::Info, None, message.into())
    }
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(ToastKind::Success, None, message.into())
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(ToastKind::Error, None, message.into())
    }
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(ToastKind::Warning, None, message.into())
    }
    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    pub fn persistent(mut self) -> Self {
        self.persistent = true;
        self
    }
    pub fn action(mut self, label: impl Into<String>, on_press: Message) -> Self {
        self.action = Some((label.into(), Box::new(on_press)));
        self
    }
}

/// A confirm dialog: title, text and what pressing the buttons sends.
#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: String,
    pub text: String,
    pub confirm_label: String,
    pub cancel_label: String,
    /// Sent when confirmed.
    pub on_confirm: Box<Message>,
    /// Sent when cancelled (besides closing the dialog).
    pub on_cancel: Option<Box<Message>>,
    /// Destructive: the confirm button is drawn in the error colour.
    pub danger: bool,
}

impl Confirm {
    pub fn new(title: impl Into<String>, text: impl Into<String>, on_confirm: Message) -> Self {
        Self {
            title: title.into(),
            text: text.into(),
            confirm_label: "Confirm".into(),
            cancel_label: "Cancel".into(),
            on_confirm: Box::new(on_confirm),
            on_cancel: None,
            danger: false,
        }
    }
    pub fn labels(mut self, confirm: impl Into<String>, cancel: impl Into<String>) -> Self {
        self.confirm_label = confirm.into();
        self.cancel_label = cancel.into();
        self
    }
    pub fn on_cancel(mut self, m: Message) -> Self {
        self.on_cancel = Some(Box::new(m));
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

#[derive(Debug, Clone)]
pub enum Modal {
    Confirm(Confirm),
    /// A modal whose content `ui::overlay::custom_modal` draws for this key.
    Custom(&'static str),
}

#[derive(Debug, Clone)]
pub enum OverlayMsg {
    /// Timer: close the toasts that ran out.
    Tick,
    CloseToast(u64),
    /// The pointer entered / left a toast: it stays while hovered, its timer restarts on leaving.
    Hover(u64, bool),
    /// The confirm dialog's confirm button.
    Confirmed,
    /// Cancel button, Esc or a click on the backdrop.
    CloseModal,
}

#[derive(Debug, Default)]
pub struct OverlayState {
    pub toasts: Vec<Toast>,
    pub modal: Option<Modal>,
    next_id: u64,
    /// The persistent toast of the audio player's error, if shown.
    player_error: Option<u64>,
    /// The toast under the pointer (it does not auto-close).
    pub hovered: Option<u64>,
}

impl OverlayState {
    /// Shows `toast`; an identical visible one only restarts its timer.
    pub fn push(&mut self, mut toast: Toast) -> u64 {
        if let Some(same) = self
            .toasts
            .iter_mut()
            .find(|t| t.kind == toast.kind && t.title == toast.title && t.message == toast.message)
        {
            same.created = toast.created;
            same.persistent = toast.persistent;
            return same.id;
        }
        self.next_id += 1;
        toast.id = self.next_id;
        self.toasts.push(toast);
        while self.toasts.len() > MAX_VISIBLE {
            self.toasts.remove(0);
        }
        self.next_id
    }

    pub fn dismiss(&mut self, id: u64) {
        self.toasts.retain(|t| t.id != id);
    }

    /// Drops the toasts that were shown for `AUTO_CLOSE` by `now`.
    pub fn expire(&mut self, now: Instant) {
        let hovered = self.hovered;
        self.toasts.retain(|t| {
            t.persistent || hovered == Some(t.id) || now.saturating_duration_since(t.created) < AUTO_CLOSE
        });
    }

    /// Any toast that will close by itself (the timer runs only then).
    pub fn timer_needed(&self) -> bool {
        self.toasts.iter().any(|t| !t.persistent)
    }
}

impl Listen {
    pub fn toast(&mut self, toast: Toast) {
        self.overlay.push(toast);
    }
    pub fn toast_info(&mut self, message: String) {
        self.toast(Toast::info(message));
    }
    pub fn toast_success(&mut self, message: String) {
        self.toast(Toast::success(message));
    }
    pub fn toast_error(&mut self, message: String) {
        self.toast(Toast::error(message));
    }
    pub fn toast_warning(&mut self, message: String) {
        self.toast(Toast::warning(message));
    }
    /// The audio player (mpv) failed: an error toast that stays until the player is back.
    pub fn set_player_error(&mut self, message: String) {
        self.clear_player_error();
        self.overlay.player_error = Some(self.overlay.push(Toast::error(message).persistent()));
    }
    pub fn clear_player_error(&mut self) {
        if let Some(id) = self.overlay.player_error.take() {
            self.overlay.dismiss(id);
        }
    }
    pub fn confirm(&mut self, confirm: Confirm) {
        self.overlay.modal = Some(Modal::Confirm(confirm));
    }
    pub fn open_modal(&mut self, key: &'static str) {
        self.overlay.modal = Some(Modal::Custom(key));
    }
    pub fn close_modal(&mut self) {
        self.overlay.modal = None;
    }

    pub(super) fn on_overlay(&mut self, msg: OverlayMsg) -> Task<Message> {
        match msg {
            OverlayMsg::Tick => self.overlay.expire(Instant::now()),
            OverlayMsg::CloseToast(id) => self.overlay.dismiss(id),
            OverlayMsg::Hover(id, on) => {
                if on {
                    self.overlay.hovered = Some(id);
                } else if self.overlay.hovered == Some(id) {
                    self.overlay.hovered = None;
                    if let Some(t) = self.overlay.toasts.iter_mut().find(|t| t.id == id) {
                        t.created = Instant::now();
                    }
                }
            }
            OverlayMsg::Confirmed => {
                if let Some(Modal::Confirm(c)) = self.overlay.modal.take() {
                    return self.update(*c.on_confirm);
                }
            }
            OverlayMsg::CloseModal => {
                if let Some(Modal::Confirm(Confirm { on_cancel: Some(m), .. })) = self.overlay.modal.take() {
                    return self.update(*m);
                }
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Args, app::Listen};

    fn rig() -> Listen {
        Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() }).0
    }

    #[test]
    fn toast_expires_after_five_seconds_unless_persistent() {
        let mut s = rig();
        s.toast_info("a".into());
        s.toast(Toast::error("b").persistent());
        let t0 = s.overlay.toasts[0].created;
        s.overlay.expire(t0 + Duration::from_millis(4900));
        assert_eq!(s.overlay.toasts.len(), 2);
        s.overlay.expire(t0 + AUTO_CLOSE + Duration::from_millis(1));
        assert_eq!(s.overlay.toasts.len(), 1);
        assert_eq!(s.overlay.toasts[0].message, "b");
        assert!(!s.overlay.timer_needed());
    }

    #[test]
    fn a_hovered_toast_stays_and_restarts_its_timer_on_leave() {
        let mut s = rig();
        s.toast_info("a".into());
        let id = s.overlay.toasts[0].id;
        let t0 = s.overlay.toasts[0].created;
        let _ = s.update(Message::Overlay(OverlayMsg::Hover(id, true)));
        s.overlay.expire(t0 + AUTO_CLOSE * 3);
        assert_eq!(s.overlay.toasts.len(), 1, "hovered: stays");
        let _ = s.update(Message::Overlay(OverlayMsg::Hover(id, false)));
        assert!(s.overlay.toasts[0].created > t0);
        s.overlay.expire(Instant::now() + Duration::from_secs(1));
        assert_eq!(s.overlay.toasts.len(), 1, "the timer started over");
        s.overlay.expire(Instant::now() + AUTO_CLOSE + Duration::from_secs(1));
        assert!(s.overlay.toasts.is_empty());
    }

    #[test]
    fn dismiss_closes_one_and_keeps_at_most_four() {
        let mut s = rig();
        for i in 0..6 {
            s.toast_success(format!("t{i}"));
        }
        assert_eq!(s.overlay.toasts.len(), MAX_VISIBLE);
        assert_eq!(s.overlay.toasts[0].message, "t2");
        let id = s.overlay.toasts[1].id;
        let _ = s.update(Message::Overlay(OverlayMsg::CloseToast(id)));
        assert_eq!(s.overlay.toasts.len(), 3);
        assert!(s.overlay.toasts.iter().all(|t| t.id != id));
    }

    #[test]
    fn identical_toast_is_not_stacked() {
        let mut s = rig();
        s.toast_error("boom".into());
        s.toast_error("boom".into());
        assert_eq!(s.overlay.toasts.len(), 1);
        s.toast_info("boom".into());
        assert_eq!(s.overlay.toasts.len(), 2);
    }

    #[test]
    fn confirm_sends_its_message_and_cancel_only_closes() {
        let mut s = rig();
        s.confirm(Confirm::new("Sure?", "text", Message::Back).on_cancel(Message::Forward));
        assert!(matches!(s.overlay.modal, Some(Modal::Confirm(_))));
        let _ = s.update(Message::Overlay(OverlayMsg::CloseModal));
        assert!(s.overlay.modal.is_none());

        s.toast_info("x".into());
        let id = s.overlay.toasts[0].id;
        s.confirm(Confirm::new("Sure?", "text", Message::Overlay(OverlayMsg::CloseToast(id))));
        let _ = s.update(Message::Overlay(OverlayMsg::Confirmed));
        assert!(s.overlay.modal.is_none());
        assert!(s.overlay.toasts.is_empty(), "the confirm message ran");
    }
}
