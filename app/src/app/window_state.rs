//! The main window: saved size/place, hiding to the tray, the title text and
//! the sleep inhibitor.

use super::*;
use listen_core::settings::{Rect, Restored, WindowState};

/// Messages of the window-state tracking.
#[derive(Debug, Clone)]
pub enum WindowStateMsg {
    Moved(iced::Point),
    /// Asked after a resize or move: the window's mode right now.
    Probe {
        maximized: bool,
        fullscreen: bool,
    },
    /// The debounce timer of that generation ran out.
    Flush(u64),
}

/// A change on the Window tab.
#[derive(Debug, Clone)]
pub enum WindowSetting {
    BarStyle(listen_core::settings::WindowBarStyle),
    TrackInfo(bool),
    MinimizeToTray(bool),
    StartMinimized(bool),
    PreventDisplaySleep(bool),
    PreventSystemSleep(bool),
}

/// What is known about the main window's geometry.
#[derive(Debug, Default)]
pub struct WinTrack {
    last_size: Option<iced::Size>,
    last_pos: Option<iced::Point>,
    /// Geometry of the normal window (not maximized, not fullscreen).
    normal_size: Option<iced::Size>,
    normal_pos: Option<iced::Point>,
    maximized: bool,
    fullscreen: bool,
    generation: u64,
    /// Opens the next window with this (set on start and after hiding).
    initial: Option<Restored>,
}

const MIN_SIZE: (f32, f32) = (480.0, 120.0);
const DEBOUNCE: Duration = Duration::from_millis(600);

/// Winit opens a Wayland window unless told otherwise; Wayland clients
/// can neither read nor set their position.
pub fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
        && std::env::var("WINIT_UNIX_BACKEND").map_or(true, |b| b != "x11")
}

/// `xrandr --listmonitors` lines such as ` 0: +*DP-1 2560/597x1440/336+1920+0  DP-1`.
pub fn parse_monitors(output: &str) -> Vec<Rect> {
    output
        .lines()
        .filter_map(|line| {
            let geometry = line.split_whitespace().find(|w| w.contains('x') && w.contains('+'))?;
            let (size, rest) = geometry.split_once('+')?;
            let (x, y) = rest.split_once('+')?;
            let (w, h) = size.split_once('x')?;
            let px = |s: &str| s.split('/').next()?.parse::<f32>().ok();
            Some(Rect::new(x.parse().ok()?, y.parse().ok()?, px(w)?, px(h)?))
        })
        .collect()
}

/// The displays (X11 only; elsewhere nothing is placed anyway).
fn displays() -> Vec<Rect> {
    std::process::Command::new("xrandr")
        .arg("--listmonitors")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_monitors(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Feishin's window title: "Paused (3 / 40) Song — Artist — Listen", plus
/// "(Private mode)". With track info off it is just the app name.
pub fn format_title(
    track: Option<(&str, &str)>,
    position: (usize, usize),
    playing: bool,
    info: bool,
    private: bool,
) -> String {
    let suffix = if private { " (Private mode)" } else { "" };
    let Some((title, artist)) = track.filter(|_| info) else { return format!("Listen{suffix}") };
    let paused = if playing { "" } else { "Paused " };
    let artist = if artist.is_empty() { String::new() } else { format!(" — {artist}") };
    format!("{paused}({} / {}) {title}{artist} — Listen{suffix}", position.0, position.1)
}

impl Listen {
    /// Reads the saved window state at start (not for demos, tests and
    /// explicit `--size`) and applies it to the first window.
    pub(super) fn win_restore(&mut self, args: &Args) {
        let explicit_size = args.size != (1600.0, 950.0);
        if self.demo || self.ephemeral || args.screenshot.is_some() || explicit_size {
            return;
        }
        let Some(saved) = self.config.settings.window.state else { return };
        let wayland = is_wayland();
        let displays = if wayland || saved.x.is_none() { Vec::new() } else { displays() };
        let restored = saved.restore(&displays, MIN_SIZE, !wayland);
        self.window_size = iced::Size::new(restored.size.0, restored.size.1);
        self.win.normal_size = Some(self.window_size);
        self.win.normal_pos = restored.position.map(|(x, y)| iced::Point::new(x, y));
        self.win.maximized = restored.maximized;
        self.win.fullscreen = restored.fullscreen;
        self.win.initial = Some(restored);
    }

    /// How the next window opens: the state of the last one.
    pub(super) fn win_open_state(&self) -> Restored {
        let size = self.win.normal_size.unwrap_or(self.window_size);
        Restored {
            size: (size.width, size.height),
            position: self.win.normal_pos.map(|p| (p.x, p.y)),
            maximized: self.win.maximized,
            fullscreen: self.win.fullscreen,
        }
    }

    /// A resize or move of the main window: ask for its mode and start the
    /// save timer (each change restarts it).
    pub(super) fn win_event(&mut self, id: window::Id, event: &window::Event) -> Task<Message> {
        match event {
            window::Event::Resized(size) => self.win.last_size = Some(*size),
            window::Event::Moved(p) => self.win.last_pos = Some(*p),
            _ => return Task::none(),
        }
        if self.ephemeral {
            return Task::none();
        }
        self.win.generation += 1;
        let generation = self.win.generation;
        let probe = window::is_maximized(id).then(move |maximized| {
            window::mode(id).map(move |mode| {
                Message::WindowState(WindowStateMsg::Probe {
                    maximized,
                    fullscreen: mode == window::Mode::Fullscreen,
                })
            })
        });
        let timer = Task::future(async move {
            tokio::time::sleep(DEBOUNCE).await;
            Message::WindowState(WindowStateMsg::Flush(generation))
        });
        Task::batch([probe, timer])
    }

    pub(super) fn on_window_state(&mut self, msg: WindowStateMsg) -> Task<Message> {
        match msg {
            WindowStateMsg::Moved(p) => self.win.last_pos = Some(p),
            WindowStateMsg::Probe { maximized, fullscreen } => {
                self.win.maximized = maximized;
                self.win.fullscreen = fullscreen;
                if !maximized && !fullscreen {
                    self.win.normal_size = self.win.last_size.or(self.win.normal_size);
                    self.win.normal_pos = self.win.last_pos.or(self.win.normal_pos);
                }
            }
            WindowStateMsg::Flush(generation) => {
                if generation == self.win.generation {
                    self.save_window_state_now();
                }
            }
        }
        Task::none()
    }

    /// Writes the window state to the config when it changed.
    pub(super) fn save_window_state_now(&mut self) {
        if self.ephemeral || self.win.normal_size.is_none() && self.win.last_size.is_none() {
            return;
        }
        let open = self.win_open_state();
        let state = WindowState {
            width: open.size.0,
            height: open.size.1,
            x: open.position.map(|p| p.0),
            y: open.position.map(|p| p.1),
            maximized: open.maximized,
            fullscreen: open.fullscreen,
        };
        if self.config.settings.window.state != Some(state) {
            self.config.settings.window.state = Some(state);
            self.save_config();
        }
    }

    /// Closes the main window but keeps running in the tray; only when the tray icon exists.
    /// Wayland can't hide or un-minimize a window, so this closes it and
    /// [`Listen::reveal_main_window`] opens a new one.
    pub(super) fn hide_to_tray(&mut self) -> Option<Task<Message>> {
        #[cfg(any(target_os = "linux", windows))]
        self.tray.as_ref()?;
        #[cfg(not(any(target_os = "linux", windows)))]
        if true {
            return None;
        }
        let id = self.main_window.take()?;
        self.save_window_state_now();
        self.hidden = true;
        self.minimized = false;
        Some(window::close(id))
    }

    /// Applies a Window-tab change and saves it.
    pub(super) fn window_setting(&mut self, change: WindowSetting) -> Task<Message> {
        let w = &mut self.config.settings.window;
        let mut task = Task::none();
        match change {
            WindowSetting::BarStyle(style) => {
                let was_custom = w.custom_title_bar;
                w.set_bar_style(style);
                // Switching between the custom bar and the native frame takes effect now.
                if was_custom != w.custom_title_bar
                    && let Some(id) = self.main_window
                {
                    task = window::toggle_decorations(id);
                }
            }
            WindowSetting::TrackInfo(v) => w.title_bar_track_info = v,
            WindowSetting::MinimizeToTray(v) => w.minimize_to_tray = v,
            WindowSetting::StartMinimized(v) => w.start_minimized = v,
            WindowSetting::PreventDisplaySleep(v) => w.prevent_display_sleep = v,
            WindowSetting::PreventSystemSleep(v) => w.prevent_system_sleep = v,
        }
        self.save_config();
        task
    }

    /// Whether minimizing should hide to the tray.
    pub(super) fn minimize_to_tray(&self) -> bool {
        let w = &self.config.settings.window;
        w.minimize_to_tray && w.tray
    }

    /// Keeps the sleep inhibitor in step with playback and settings.
    pub(super) fn sync_inhibit(&mut self) {
        let w = &self.config.settings.window;
        let playing = self.playback.playing && self.queue.current().is_some();
        let level = crate::inhibit::Level::wanted(playing, w.prevent_display_sleep, w.prevent_system_sleep);
        self.inhibitor.set(level);
    }

    /// The window title (also the custom bar's text).
    pub fn title(&self) -> String {
        let item = self.queue.current();
        let artist = item.map(|i| i.song.artist_line());
        format_title(
            item.map(|i| i.song.title.as_str()).zip(artist.as_deref()),
            (self.queue.current_position().unwrap_or(0) + 1, self.queue.len()),
            self.playback.playing,
            self.config.settings.window.title_bar_track_info,
            self.history_view.private,
        )
    }

    /// Something asked the running instance to come forward.
    pub(super) fn on_single_instance(&mut self, forward: crate::single_instance::Forward) -> Task<Message> {
        use crate::single_instance::Forward;
        let reveal = self.reveal_main_window();
        match forward {
            Forward::Show => reveal,
            Forward::Route(name) => match parse_route(&name) {
                Some(route) => Task::batch([reveal, self.navigate(route)]),
                None => reveal,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_formats_like_feishin() {
        let t = |track, playing, info, private| format_title(track, (3, 40), playing, info, private);
        assert_eq!(t(Some(("Song", "Artist")), true, true, false), "(3 / 40) Song — Artist — Listen");
        assert_eq!(t(Some(("Song", "Artist")), false, true, false), "Paused (3 / 40) Song — Artist — Listen");
        assert_eq!(t(Some(("Song", "")), true, true, false), "(3 / 40) Song — Listen");
        assert_eq!(t(None, true, true, false), "Listen");
        assert_eq!(t(Some(("Song", "Artist")), true, false, false), "Listen");
        assert_eq!(
            t(Some(("Song", "Artist")), true, true, true),
            "(3 / 40) Song — Artist — Listen (Private mode)"
        );
        assert_eq!(t(None, false, true, true), "Listen (Private mode)");
    }

    #[test]
    fn xrandr_monitors_parse() {
        let out = "Monitors: 2\n 0: +*DP-1 2560/597x1440/336+1920+0  DP-1\n 1: +HDMI-1 1920/527x1080/296+0+0  HDMI-1\n";
        assert_eq!(
            parse_monitors(out),
            vec![Rect::new(1920.0, 0.0, 2560.0, 1440.0), Rect::new(0.0, 0.0, 1920.0, 1080.0)]
        );
        assert!(parse_monitors("Monitors: 0\n").is_empty());
    }
}
