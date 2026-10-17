//! The settings page shell: remembered tab, search, reset to default, the restart-required toast, and the
//! small controls without a feature of their own. Rendering is in `ui::settings`.

use super::overlay::{Confirm, Toast};
use super::{Listen, Message};
use iced::Task;
use listen_core::settings::{HomeFeatureStyle, Settings, SettingsTab};
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum ShellMsg {
    Tab(SettingsTab),
    Search(String),
    /// "Reset to default": opens the confirm dialog.
    ResetAsk,
    ResetConfirmed,
    /// Start Listen again with the same arguments and quit this instance.
    Relaunch,
    ButtonSize(u16),
    SkipButtons(bool),
    SkipBack(u32),
    SkipForward(u32),
    VolumeWidth(u16),
    VolumeWheelStep(u8),
    /// Move the sidebar entry at this index up (-1) or down (+1).
    SidebarMove(usize, i8),
    SidebarVisible(usize, bool),
    HomeFeature(bool),
    HomeFeatureStyle(HomeFeatureStyle),
    TitleBarTrackInfo(bool),
    DiscordIdInput(String),
    DiscordIdSave,
}

/// Program and arguments that restart Listen: this executable (or the AppImage) with the same arguments.
pub fn relaunch_command(
    exe: PathBuf,
    appimage: Option<OsString>,
    args: impl IntoIterator<Item = OsString>,
) -> (PathBuf, Vec<OsString>) {
    let program = appimage.map(PathBuf::from).unwrap_or_else(|| {
        // Linux marks an executable replaced by an update as "... (deleted)".
        let text = exe.to_string_lossy();
        text.strip_suffix(" (deleted)").map_or(exe.clone(), PathBuf::from)
    });
    (program, args.into_iter().skip(1).collect())
}

impl Listen {
    /// A toast that stays until closed and offers "Relaunch" (Feishin's restart-required toast).
    pub fn toast_restart_required(&mut self, message: &str) {
        self.toast(
            Toast::warning(message)
                .titled("Restart required")
                .persistent()
                .action("Relaunch", Message::Shell(ShellMsg::Relaunch)),
        );
    }

    pub(super) fn on_shell(&mut self, msg: ShellMsg) -> Task<Message> {
        let s = &mut self.config.settings;
        match msg {
            ShellMsg::Tab(tab) => {
                s.settings_tab = tab;
                self.settings_search.clear();
            }
            ShellMsg::Search(text) => {
                self.settings_search = text;
                return Task::none();
            }
            ShellMsg::ResetAsk => {
                self.confirm(
                    Confirm::new(
                        "Reset to default",
                        "Are you sure? Every setting goes back to its default. Your servers and sign-ins are kept.",
                        Message::Shell(ShellMsg::ResetConfirmed),
                    )
                    .labels("Reset", "Cancel")
                    .danger(),
                );
                return Task::none();
            }
            ShellMsg::ResetConfirmed => return self.reset_settings(),
            ShellMsg::Relaunch => return self.relaunch(),
            ShellMsg::ButtonSize(v) => s.player_bar.button_size = v.clamp(15, 30),
            ShellMsg::SkipButtons(v) => s.player_bar.skip_buttons = v,
            ShellMsg::SkipBack(v) => s.player_bar.skip_backward_secs = v,
            ShellMsg::SkipForward(v) => s.player_bar.skip_forward_secs = v,
            ShellMsg::VolumeWidth(v) => s.player_bar.volume_width = v.clamp(25, 300),
            ShellMsg::VolumeWheelStep(v) => s.player_bar.volume_wheel_step = v.clamp(1, 20),
            ShellMsg::SidebarMove(i, dir) => {
                let j = i.checked_add_signed(isize::from(dir)).filter(|j| *j < s.sidebar.items.len());
                if let (Some(j), true) = (j, i < s.sidebar.items.len()) {
                    s.sidebar.items.swap(i, j);
                }
            }
            ShellMsg::SidebarVisible(i, v) => {
                if let Some(item) = s.sidebar.items.get_mut(i) {
                    item.visible = v;
                }
            }
            ShellMsg::HomeFeature(v) => s.home.feature = v,
            ShellMsg::HomeFeatureStyle(v) => s.home.feature_style = v,
            ShellMsg::TitleBarTrackInfo(v) => s.window.title_bar_track_info = v,
            ShellMsg::DiscordIdInput(text) => {
                self.discord_id_input = Some(text);
                return Task::none();
            }
            ShellMsg::DiscordIdSave => {
                if let Some(text) = self.discord_id_input.take() {
                    let text = text.trim();
                    s.window.discord_client_id = if text.is_empty() {
                        Settings::default().window.discord_client_id
                    } else {
                        text.to_string()
                    };
                    self.integ.discord_dirty();
                }
            }
        }
        self.save_config();
        Task::none()
    }

    /// Everything but the servers (and their sign-ins, which live outside `Settings`).
    fn reset_settings(&mut self) -> Task<Message> {
        let tab = self.config.settings.settings_tab;
        self.config.settings = Settings { settings_tab: tab, ..Settings::default() };
        self.accent_input.clear();
        self.settings_search.clear();
        self.discord_id_input = None;
        self.global_error = None;
        self.global_triggers.clear();
        self.hotkey_recording = None;
        let quality = self.config.settings.playback.stream_quality;
        for source in self.sources.values().chain(self.source.iter()) {
            source.set_stream_quality(quality);
        }
        self.desktop_theme = self
            .config
            .settings
            .appearance
            .follow_desktop_theme
            .then(|| {
                listen_core::theme::desktop::config_dir().and_then(|d| listen_core::theme::desktop::read(&d))
            })
            .flatten();
        self.apply_theme();
        self.save_config();
        self.toast_success("Settings reset to default.".into());
        self.apply_audio()
    }

    fn relaunch(&mut self) -> Task<Message> {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                self.toast_error(format!("Could not relaunch: {e}"));
                return Task::none();
            }
        };
        let (program, args) = relaunch_command(exe, std::env::var_os("APPIMAGE"), std::env::args_os());
        self.save_queue_now();
        self.save_config();
        // A short delay lets this instance release its single-instance lock and the audio device.
        let mut cmd = if cfg!(unix) {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", "sleep 0.5; exec \"$0\" \"$@\""]).arg(&program);
            c
        } else {
            // No `sh` here: the new process waits itself (see `startup_delay` in main.rs).
            let mut c = std::process::Command::new(&program);
            c.env("LISTEN_STARTUP_DELAY_MS", "700");
            c
        };
        match cmd.args(&args).spawn() {
            Ok(_) => iced::exit(),
            Err(e) => {
                self.toast_error(format!("Could not relaunch {}: {e}", program.display()));
                Task::none()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaunch_keeps_the_arguments_and_prefers_the_appimage() {
        let args = ["/usr/bin/listen", "--demo", "--theme", "nord"].map(OsString::from);
        let (p, a) = relaunch_command("/usr/bin/listen".into(), None, args.clone());
        assert_eq!(p, PathBuf::from("/usr/bin/listen"));
        assert_eq!(a, ["--demo", "--theme", "nord"].map(OsString::from));
        let (p, _) =
            relaunch_command("/tmp/.mount_x/listen".into(), Some("/home/u/Listen.AppImage".into()), args);
        assert_eq!(p, PathBuf::from("/home/u/Listen.AppImage"));
        let (p, _) = relaunch_command("/usr/bin/listen (deleted)".into(), None, []);
        assert_eq!(p, PathBuf::from("/usr/bin/listen"));
    }
}
