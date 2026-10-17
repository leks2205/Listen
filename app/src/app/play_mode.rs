//! The play mode cycle (Off -> Shuffle -> Auto-DJ -> AI DJ) for the shuffle button,
//! queue header, hotkey, MPRIS and remote, on top of the queue shuffle, the Auto-DJ
//! setting and `dj_switch`. Modes are exclusive; when several are on, AI DJ > Shuffle >
//! Auto-DJ. AI DJ is skipped without a Gemini key.

use super::{Listen, Message};
use crate::app::settings_msg::Setting;
use iced::Task;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    Off,
    Shuffle,
    AutoDj,
    AiDj,
}

impl PlayMode {
    pub const ALL: [PlayMode; 4] = [Self::Off, Self::Shuffle, Self::AutoDj, Self::AiDj];

    /// The next mode in the cycle; AI DJ is skipped when it is unavailable.
    pub fn next(self, ai_available: bool) -> Self {
        match self {
            Self::Off => Self::Shuffle,
            Self::Shuffle => Self::AutoDj,
            Self::AutoDj if ai_available => Self::AiDj,
            Self::AutoDj | Self::AiDj => Self::Off,
        }
    }

    /// The player bar icon of the mode.
    pub fn icon(self) -> crate::icons::Icon {
        use crate::icons::Icon;
        match self {
            Self::Off | Self::Shuffle => Icon::LuShuffle,
            Self::AutoDj => Icon::LuDisc3,
            Self::AiDj => Icon::LuMicVocal,
        }
    }

    /// The short name for tags: "Shuffle", "Auto", "AI".
    pub fn tag(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Shuffle => "Shuffle",
            Self::AutoDj => "Auto",
            Self::AiDj => "AI",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Shuffle => "Shuffle",
            Self::AutoDj => "Auto-DJ",
            Self::AiDj => "AI DJ",
        }
    }
}

impl Listen {
    /// The mode the queue and settings are in now.
    pub fn play_mode(&self) -> PlayMode {
        if self.config.settings.ai_dj.enabled {
            PlayMode::AiDj
        } else if self.queue.shuffle() {
            PlayMode::Shuffle
        } else if self.config.settings.auto_dj.enabled {
            PlayMode::AutoDj
        } else {
            PlayMode::Off
        }
    }

    /// The AI DJ can be switched on (a Gemini key is set).
    pub fn ai_dj_available(&self) -> bool {
        self.dj.has_key()
    }

    /// The next mode for the shuffle button, hotkey and remote.
    pub fn next_play_mode(&self) -> PlayMode {
        self.play_mode().next(self.ai_dj_available())
    }

    pub(super) fn set_play_mode(&mut self, mode: PlayMode) -> Task<Message> {
        if mode == PlayMode::AiDj && !self.ai_dj_available() {
            self.toast_warning("AI DJ: add a Gemini API key in Settings → AI DJ first.".into());
            return Task::none();
        }
        let mut tasks = Vec::new();
        // The queue shuffle is on only in Shuffle mode.
        if self.queue.shuffle() != (mode == PlayMode::Shuffle) {
            self.queue.toggle_shuffle();
            tasks.push(self.refresh_next());
        }
        // Auto-DJ is on only in Auto-DJ mode (AI DJ brings its own refill).
        if self.config.settings.auto_dj.enabled != (mode == PlayMode::AutoDj) {
            tasks.push(self.change_setting(Setting::AutoDj(mode == PlayMode::AutoDj)));
        }
        if self.config.settings.ai_dj.enabled != (mode == PlayMode::AiDj) {
            tasks.push(self.dj_switch(mode == PlayMode::AiDj));
            tasks.push(self.apply_audio());
        }
        Task::batch(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Args, demo};

    /// Starts in Off (Auto-DJ is on by default).
    fn boot() -> Listen {
        let mut s = Listen::boot(&Args { demo: true, ..Args::default() }).0;
        s.config.settings.auto_dj.enabled = false;
        s
    }

    #[test]
    fn a_default_install_starts_in_auto_dj() {
        let s = Listen::boot(&Args { demo: true, ..Args::default() }).0;
        assert_eq!(s.play_mode(), PlayMode::AutoDj);
    }

    #[test]
    fn cycle_order_and_skipping_the_ai_dj() {
        use PlayMode::*;
        assert_eq!(Off.next(true), Shuffle);
        assert_eq!(Shuffle.next(true), AutoDj);
        assert_eq!(AutoDj.next(true), AiDj);
        assert_eq!(AiDj.next(true), Off);
        assert_eq!(AutoDj.next(false), Off);
    }

    #[test]
    fn the_button_cycles_without_a_key() {
        let mut s = boot();
        assert_eq!(s.play_mode(), PlayMode::Off);
        let seen: Vec<_> = (0..4)
            .map(|_| {
                let _ = s.update(Message::ToggleShuffle);
                s.play_mode()
            })
            .collect();
        assert_eq!(seen, [PlayMode::Shuffle, PlayMode::AutoDj, PlayMode::Off, PlayMode::Shuffle]);
    }

    #[test]
    fn each_switch_sets_queue_and_settings() {
        let mut s = boot();
        s.dj.key = Some("k".into());
        s.queue.set(demo::album_songs(0), 0);
        let _ = s.update(Message::SetPlayMode(PlayMode::Shuffle));
        assert!(s.queue.shuffle() && !s.config.settings.auto_dj.enabled);
        let _ = s.update(Message::SetPlayMode(PlayMode::AutoDj));
        assert!(!s.queue.shuffle(), "leaving shuffle un-shuffles");
        assert!(s.config.settings.auto_dj.enabled && !s.config.settings.ai_dj.enabled);
        let _ = s.update(Message::SetPlayMode(PlayMode::AiDj));
        assert!(s.config.settings.ai_dj.enabled && !s.config.settings.auto_dj.enabled);
        assert_eq!(s.play_mode(), PlayMode::AiDj);
        let _ = s.update(Message::SetPlayMode(PlayMode::Off));
        assert_eq!(s.play_mode(), PlayMode::Off);
        assert!(!s.config.settings.ai_dj.enabled && !s.config.settings.auto_dj.enabled);
    }

    #[test]
    fn leaving_auto_dj_keeps_the_queued_auto_songs() {
        let mut s = boot();
        s.queue.set(demo::album_songs(0), 0);
        let _ = s.update(Message::SetPlayMode(PlayMode::AutoDj));
        let _ = s.update(Message::AutoDjLoaded(Ok(demo::album_songs(3))));
        let n = s.queue.len();
        assert!(s.queue.iter().any(|i| i.auto));
        let _ = s.update(Message::SetPlayMode(PlayMode::Off));
        assert_eq!(s.queue.len(), n);
        assert!(s.queue.iter().any(|i| i.auto));
    }

    #[test]
    fn ai_dj_without_a_key_stays_off() {
        let mut s = boot();
        let _ = s.update(Message::SetPlayMode(PlayMode::AiDj));
        assert_eq!(s.play_mode(), PlayMode::Off);
        assert!(!s.config.settings.ai_dj.enabled);
    }
}
