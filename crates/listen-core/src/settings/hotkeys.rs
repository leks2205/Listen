//! Hotkey actions (Feishin's binding table) and the legacy global shortcut list.

/// One bindable action: Feishin's id, its label in the editor, and whether it
/// may be registered system-wide (`allowGlobal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyAction {
    pub id: &'static str,
    pub label: &'static str,
    pub allow_global: bool,
}

const fn action(id: &'static str, label: &'static str, allow_global: bool) -> HotkeyAction {
    HotkeyAction { id, label, allow_global }
}

/// Every Feishin hotkey action, in the order of its settings table.
pub const HOTKEY_ACTIONS: &[HotkeyAction] = &[
    action("browserBack", "Browser back", false),
    action("browserForward", "Browser forward", false),
    action("favoriteCurrentAdd", "Favorite current song", true),
    action("favoriteCurrentRemove", "Unfavorite current song", true),
    action("favoriteCurrentToggle", "Toggle current song favorite", true),
    action("favoritePreviousAdd", "Favorite previous song", true),
    action("favoritePreviousRemove", "Unfavorite previous song", true),
    action("favoritePreviousToggle", "Toggle previous song favorite", true),
    action("globalSearch", "Global search", false),
    action("listNavigateToPage", "List: navigate to page", false),
    action("listPlayDefault", "List: play (default)", false),
    action("listPlayLast", "List: play last", false),
    action("listPlayNext", "List: play next", false),
    action("listPlayNow", "List: play now", false),
    action("listShowPlayingSong", "List: show playing song", false),
    action("localSearch", "Local search", false),
    action("navigateHome", "Navigate to home", false),
    action("next", "Next track", true),
    action("nextAlbum", "Next album", true),
    action("pause", "Pause", true),
    action("play", "Play", true),
    action("playPause", "Play / pause", true),
    action("previous", "Previous track", true),
    action("previousAlbum", "Previous album", true),
    action("rate0", "Rate 0 stars", true),
    action("rate1", "Rate 1 star", true),
    action("rate2", "Rate 2 stars", true),
    action("rate3", "Rate 3 stars", true),
    action("rate4", "Rate 4 stars", true),
    action("rate5", "Rate 5 stars", true),
    action("skipBackward", "Skip backward", true),
    action("skipForward", "Skip forward", true),
    action("stop", "Stop", true),
    action("toggleFullscreenPlayer", "Toggle full-screen player", false),
    action("toggleQueue", "Toggle queue", false),
    action("toggleRepeat", "Toggle repeat", true),
    action("toggleShuffle", "Toggle shuffle", true),
    action("volumeDown", "Volume down", true),
    action("volumeMute", "Volume mute", true),
    action("volumeUp", "Volume up", true),
    action("zoomIn", "Zoom in", true),
    action("zoomOut", "Zoom out", true),
];

/// Feishin's default bindings; every other action starts unbound.
pub const DEFAULT_HOTKEYS: &[(&str, &str)] = &[
    ("playPause", "space"),
    ("globalSearch", "mod+k"),
    ("localSearch", "mod+f"),
    ("listShowPlayingSong", "mod+l"),
    ("listNavigateToPage", "mod+g"),
    ("listPlayDefault", "enter"),
];

pub fn hotkey_action(id: &str) -> Option<&'static HotkeyAction> {
    HOTKEY_ACTIONS.iter().find(|a| a.id == id)
}

/// Legacy list of actions that can be bound system-wide, with their default preferred trigger
/// (kept so old configs keep loading; the editor drives the portal now).
pub const GLOBAL_ACTIONS: &[(&str, &str, &str)] = &[
    ("playPause", "Play / pause", "CTRL+ALT+space"),
    ("next", "Next track", "CTRL+ALT+Right"),
    ("previous", "Previous track", "CTRL+ALT+Left"),
    ("stop", "Stop", ""),
    ("volumeUp", "Volume up", "CTRL+ALT+Up"),
    ("volumeDown", "Volume down", "CTRL+ALT+Down"),
    ("volumeMute", "Mute", "CTRL+ALT+m"),
];
