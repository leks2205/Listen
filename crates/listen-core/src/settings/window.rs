//! Window and tray behaviour.

use super::*;

/// Look of the custom window bar (Feishin's "Windows" / "macOS"), or the
/// desktop's own frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WindowBarStyle {
    #[default]
    Windows,
    Macos,
    /// The desktop draws the frame (no custom bar).
    Native,
}

impl WindowBarStyle {
    pub const ALL: [Self; 3] = [Self::Windows, Self::Macos, Self::Native];

    pub fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Macos => "macOS",
            Self::Native => "Native",
        }
    }
}

impl std::fmt::Display for WindowBarStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A rectangle in logical pixels (a display, or a window).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    fn intersection_area(&self, o: &Rect) -> f32 {
        let w = (self.x + self.w).min(o.x + o.w) - self.x.max(o.x);
        let h = (self.y + self.h).min(o.y + o.h) - self.y.max(o.y);
        if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
    }
}

/// The main window's size and place, saved on change and on close.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    /// Size and position of the normal (neither maximized nor fullscreen) window.
    pub width: f32,
    pub height: f32,
    /// `None` where the backend cannot place windows (Wayland).
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self { width: 1600.0, height: 950.0, x: None, y: None, maximized: false, fullscreen: false }
    }
}

/// What to open the window with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Restored {
    pub size: (f32, f32),
    pub position: Option<(f32, f32)>,
    pub maximized: bool,
    pub fullscreen: bool,
}

/// Part of the window (its top-left corner area) that must stay on a display
/// so it can still be grabbed.
const GRAB: f32 = 80.0;

impl WindowState {
    /// The saved state made safe to open: size at least `min` and within the display; position dropped
    /// when no display shows the title area, else moved fully onto one. `can_place`: false on Wayland.
    pub fn restore(&self, displays: &[Rect], min: (f32, f32), can_place: bool) -> Restored {
        let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        let mut w = finite(self.width, 1600.0).max(min.0);
        let mut h = finite(self.height, 950.0).max(min.1);
        let mut position = None;
        if can_place
            && !displays.is_empty()
            && let (Some(x), Some(y)) = (self.x, self.y)
            && x.is_finite()
            && y.is_finite()
        {
            // The display that shows most of the window; none = it is off screen.
            let window = Rect::new(x, y, w, h);
            let grab = Rect::new(x, y, w.min(GRAB * 2.0), GRAB.min(h));
            let best = displays
                .iter()
                .filter(|d| d.intersection_area(&grab) > 0.0)
                .max_by(|a, b| a.intersection_area(&window).total_cmp(&b.intersection_area(&window)));
            if let Some(d) = best {
                w = w.min(d.w).max(min.0.min(d.w));
                h = h.min(d.h).max(min.1.min(d.h));
                let nx = x.min(d.x + d.w - w).max(d.x);
                let ny = y.min(d.y + d.h - h).max(d.y);
                position = Some((nx, ny));
            }
        } else if let Some(d) = displays.first() {
            w = w.min(d.w).max(min.0.min(d.w));
            h = h.min(d.h).max(min.1.min(d.h));
        }
        Restored { size: (w, h), position, maximized: self.maximized, fullscreen: self.fullscreen }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    pub tray: bool,
    pub minimize_to_tray: bool,
    pub exit_to_tray: bool,
    pub start_minimized: bool,
    /// Off = the desktop's own frame (see [`WindowSettings::bar_style`]).
    pub custom_title_bar: bool,
    /// Look of the custom bar when `custom_title_bar` is on.
    pub custom_bar_style: WindowBarStyle,
    /// Show "(i / n) title — artist" in the custom window bar.
    pub title_bar_track_info: bool,
    pub discord_rpc: bool,
    /// Discord application id (Feishin's default).
    pub discord_client_id: String,
    /// Keep the display awake while playing (Feishin `preventSleepOnPlayback`).
    pub prevent_display_sleep: bool,
    /// Keep the system from suspending while playing (Feishin `preventSuspendOnPlayback`).
    pub prevent_system_sleep: bool,
    /// Last size and place of the main window.
    pub state: Option<WindowState>,
}

/// Listen's own Discord application (its name and logo show in the status).
pub const DEFAULT_DISCORD_CLIENT_ID: &str = "1560790551337640127";
/// Feishin's application: the old default, still stored in older configs.
pub const FEISHIN_DISCORD_CLIENT_ID: &str = "1165957668758900787";

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            tray: true,
            minimize_to_tray: false,
            exit_to_tray: false,
            start_minimized: false,
            custom_title_bar: true,
            custom_bar_style: WindowBarStyle::Windows,
            title_bar_track_info: true,
            discord_rpc: false,
            discord_client_id: DEFAULT_DISCORD_CLIENT_ID.into(),
            prevent_display_sleep: false,
            prevent_system_sleep: false,
            state: None,
        }
    }
}

impl WindowSettings {
    /// The bar style shown in settings: Native when the custom bar is off.
    pub fn bar_style(&self) -> WindowBarStyle {
        if self.custom_title_bar { self.custom_bar_style } else { WindowBarStyle::Native }
    }

    pub fn set_bar_style(&mut self, style: WindowBarStyle) {
        match style {
            WindowBarStyle::Native => self.custom_title_bar = false,
            custom => {
                self.custom_title_bar = true;
                self.custom_bar_style = custom;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: (f32, f32) = (480.0, 120.0);

    fn screen() -> Vec<Rect> {
        vec![Rect::new(0.0, 0.0, 1920.0, 1080.0), Rect::new(1920.0, 0.0, 2560.0, 1440.0)]
    }

    #[test]
    fn serde_round_trip_and_old_configs() {
        let w = WindowSettings {
            state: Some(WindowState {
                width: 1200.0,
                height: 800.0,
                x: Some(40.0),
                y: Some(30.0),
                maximized: true,
                fullscreen: false,
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(serde_json::from_str::<WindowSettings>(&json).unwrap(), w);
        // A config written before window state existed still loads.
        let old: WindowSettings = serde_json::from_str(r#"{"tray":false,"custom_title_bar":false}"#).unwrap();
        assert!(old.state.is_none());
        assert_eq!(old.bar_style(), WindowBarStyle::Native);
        // A partial state fills in defaults.
        let partial: WindowState = serde_json::from_str(r#"{"width":900}"#).unwrap();
        assert_eq!((partial.width, partial.height), (900.0, 950.0));
    }

    #[test]
    fn bar_style_maps_to_the_flag() {
        let mut w = WindowSettings::default();
        assert_eq!(w.bar_style(), WindowBarStyle::Windows);
        w.set_bar_style(WindowBarStyle::Macos);
        assert!(w.custom_title_bar);
        assert_eq!(w.bar_style(), WindowBarStyle::Macos);
        w.set_bar_style(WindowBarStyle::Native);
        assert!(!w.custom_title_bar);
        assert_eq!(w.bar_style(), WindowBarStyle::Native);
        // Back to custom keeps the last custom look.
        w.set_bar_style(WindowBarStyle::Windows);
        assert_eq!(w.bar_style(), WindowBarStyle::Windows);
    }

    #[test]
    fn restore_keeps_a_visible_window() {
        let s =
            WindowState { width: 1000.0, height: 700.0, x: Some(100.0), y: Some(80.0), ..Default::default() };
        let r = s.restore(&screen(), MIN, true);
        assert_eq!(r.size, (1000.0, 700.0));
        assert_eq!(r.position, Some((100.0, 80.0)));
    }

    #[test]
    fn restore_drops_a_window_on_an_unplugged_display() {
        let s = WindowState { x: Some(6000.0), y: Some(100.0), ..Default::default() };
        assert_eq!(s.restore(&screen(), MIN, true).position, None);
        let s = WindowState { x: Some(-5000.0), y: Some(-5000.0), ..Default::default() };
        assert_eq!(s.restore(&screen(), MIN, true).position, None);
    }

    #[test]
    fn restore_pulls_a_hanging_window_back_and_shrinks_it() {
        // Hangs off the right edge of the second display.
        let s = WindowState {
            width: 1600.0,
            height: 900.0,
            x: Some(3500.0),
            y: Some(600.0),
            ..Default::default()
        };
        let r = s.restore(&screen(), MIN, true);
        assert_eq!(r.position, Some((4480.0 - 1600.0, 540.0)));
        // Bigger than the display it sits on.
        let s =
            WindowState { width: 3000.0, height: 2000.0, x: Some(10.0), y: Some(10.0), ..Default::default() };
        let r = s.restore(&screen(), MIN, true);
        assert_eq!(r.size, (1920.0, 1080.0));
        assert_eq!(r.position, Some((0.0, 0.0)));
    }

    #[test]
    fn restore_without_placement_keeps_only_size_and_flags() {
        let s = WindowState {
            width: 100.0,
            height: 50.0,
            x: Some(10.0),
            y: Some(10.0),
            maximized: true,
            fullscreen: true,
        };
        let r = s.restore(&screen(), MIN, false);
        assert_eq!(r.position, None);
        assert_eq!(r.size, MIN);
        assert!(r.maximized && r.fullscreen);
        // Unknown displays: no position either.
        assert_eq!(s.restore(&[], MIN, true).position, None);
    }

    #[test]
    fn restore_survives_garbage() {
        let s = WindowState {
            width: f32::NAN,
            height: f32::INFINITY,
            x: Some(f32::NAN),
            y: Some(0.0),
            ..Default::default()
        };
        let r = s.restore(&screen(), MIN, true);
        assert!(r.size.0.is_finite() && r.size.1.is_finite());
        assert_eq!(r.position, None);
    }
}
