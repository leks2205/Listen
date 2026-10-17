//! System-wide shortcuts through the xdg-desktop-portal GlobalShortcuts interface (works on Wayland).
//! Triggers are only suggestions; the desktop owns the final key choice.

use crate::app::Message;
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use iced::Subscription;
use iced::futures::{SinkExt, Stream, StreamExt, channel::mpsc};

#[derive(Debug, Clone)]
pub enum ShortcutMsg {
    /// The portal bound the shortcuts; (action, trigger description) as the desktop shows them.
    Bound(Vec<(String, String)>),
    /// A bound shortcut fired (action id).
    Activated(String),
    /// The portal is missing or refused.
    Unavailable(String),
}

/// (action, label, preferred trigger) for every shortcut we register.
pub type Wanted = Vec<(String, String, String)>;

/// Hardware media keys the portal can grab: (shortcut id, label, trigger, action it runs).
pub const MEDIA_KEYS: &[(&str, &str, &str, &str)] = &[
    ("media:playPause", "Media key: play / pause", "XF86AudioPlay", "playPause"),
    ("media:next", "Media key: next track", "XF86AudioNext", "next"),
    ("media:previous", "Media key: previous track", "XF86AudioPrev", "previous"),
    ("media:stop", "Media key: stop", "XF86AudioStop", "stop"),
];

/// Shortcuts to register: every bound action flagged global, plus the media keys when
/// `global_media_keys` is on and the desktop doesn't already route them to MPRIS (they'd fire twice).
pub fn wanted(settings: &listen_core::settings::Settings, desktop_has_media_keys: bool) -> Wanted {
    let mut out: Wanted = Vec::new();
    for a in listen_core::settings::HOTKEY_ACTIONS {
        if !a.allow_global || !settings.hotkeys_global.contains(a.id) {
            continue;
        }
        if let Some(b) = settings.hotkeys.get(a.id).and_then(|h| crate::hotkeys::parse(h)) {
            out.push((a.id.to_string(), a.label.to_string(), b.portal_trigger()));
        }
    }
    if settings.global_media_keys && !desktop_has_media_keys {
        out.extend(MEDIA_KEYS.iter().map(|(id, label, trigger, _)| {
            ((*id).to_string(), (*label).to_string(), (*trigger).to_string())
        }));
    }
    out
}

/// Whether this desktop (`XDG_CURRENT_DESKTOP`, colon-separated) already sends XF86Audio* keys to MPRIS
/// players (KDE Plasma, GNOME, Cinnamon and Budgie do).
pub fn desktop_handles_media_keys(xdg_current_desktop: &str) -> bool {
    xdg_current_desktop.split(':').any(|d| {
        let d = d.trim().to_ascii_lowercase();
        matches!(d.as_str(), "kde" | "gnome" | "x-cinnamon" | "cinnamon" | "budgie")
    })
}

/// [`desktop_handles_media_keys`] for the running session.
pub fn session_handles_media_keys() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| desktop_handles_media_keys(&d))
}

/// The hotkey action a portal shortcut id stands for (media keys map to playback actions).
pub fn action_of(shortcut_id: &str) -> &str {
    MEDIA_KEYS.iter().find(|(id, ..)| *id == shortcut_id).map_or(shortcut_id, |(.., action)| action)
}

pub fn subscription(wanted: Wanted) -> Subscription<Message> {
    Subscription::run_with(wanted, worker)
}

fn worker(wanted: &Wanted) -> impl Stream<Item = Message> + use<> {
    let wanted = wanted.clone();
    iced::stream::channel(16, async move |mut out: mpsc::Sender<Message>| {
        if let Err(e) = run(&wanted, &mut out).await {
            tracing::warn!("global shortcuts unavailable: {e}");
            let _ = out.send(Message::GlobalShortcut(ShortcutMsg::Unavailable(e.to_string()))).await;
        }
        std::future::pending::<()>().await;
    })
}

async fn run(wanted: &Wanted, out: &mut mpsc::Sender<Message>) -> ashpd::Result<()> {
    let portal = GlobalShortcuts::new().await?;
    let session = portal.create_session().await?;
    // Subscribe before binding so no activation is missed.
    let mut activated = portal.receive_activated().await?;
    let shortcuts: Vec<NewShortcut> = wanted
        .iter()
        .map(|(id, label, trigger)| {
            let s = NewShortcut::new(id.as_str(), label.as_str());
            if trigger.is_empty() { s } else { s.preferred_trigger(Some(trigger.as_str())) }
        })
        .collect();
    let bound = portal.bind_shortcuts(&session, &shortcuts, None).await?.response()?;
    let triggers =
        bound.shortcuts().iter().map(|s| (s.id().to_string(), s.trigger_description().to_string())).collect();
    let _ = out.send(Message::GlobalShortcut(ShortcutMsg::Bound(triggers))).await;
    while let Some(event) = activated.next().await {
        let id = event.shortcut_id().to_string();
        if out.send(Message::GlobalShortcut(ShortcutMsg::Activated(id))).await.is_err() {
            break;
        }
    }
    session.close().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::settings::Settings;

    #[test]
    fn registers_global_actions_and_media_keys() {
        let mut s = Settings { global_media_keys: false, ..Settings::default() };
        assert!(wanted(&s, false).is_empty());
        s.hotkeys.insert("next".into(), "mod+alt+arrowright".into());
        s.hotkeys_global.insert("next".into());
        // Flagged global but unbound, or bound but not flagged: not registered.
        s.hotkeys_global.insert("stop".into());
        s.hotkeys.insert("pause".into(), "f6".into());
        // Local-only actions are never registered, even if the config says so.
        s.hotkeys_global.insert("globalSearch".into());
        assert_eq!(
            wanted(&s, false),
            vec![("next".to_string(), "Next track".to_string(), "CTRL+ALT+Right".to_string())]
        );
        s.global_media_keys = true;
        let w = wanted(&s, false);
        assert_eq!(w.len(), 1 + MEDIA_KEYS.len());
        // Where the desktop already hands the keys to MPRIS they are not bound.
        assert_eq!(wanted(&s, true).len(), 1);
        assert!(w.iter().any(|(id, _, t)| id == "media:playPause" && t == "XF86AudioPlay"));
        assert_eq!(action_of("media:next"), "next");
        assert_eq!(action_of("volumeUp"), "volumeUp");
    }

    #[test]
    fn detects_desktops_with_mpris_media_keys() {
        assert!(desktop_handles_media_keys("KDE"));
        assert!(desktop_handles_media_keys("ubuntu:GNOME"));
        assert!(desktop_handles_media_keys("X-Cinnamon"));
        assert!(!desktop_handles_media_keys("sway"));
        assert!(!desktop_handles_media_keys("Hyprland"));
        assert!(!desktop_handles_media_keys(""));
    }
}
