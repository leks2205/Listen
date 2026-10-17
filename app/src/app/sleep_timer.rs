//! The sleep timer (Feishin's `sleep-timer.store.ts` + `sleep-timer-button.tsx`):
//! pause after N minutes of playing, at the end of the song, or at the end of
//! the album. The countdown only ticks while a song plays.

use super::{Listen, Message};
use iced::Task;
use listen_core::model::Song;
use std::time::Duration;

/// Minutes of the preset grid (Feishin: 5 .. 240).
pub const PRESETS: [u32; 9] = [5, 10, 15, 30, 45, 60, 120, 180, 240];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepMode {
    Timed,
    EndOfSong,
    EndOfAlbum,
}

/// A running timer.
#[derive(Debug, Clone, PartialEq)]
pub struct SleepTimer {
    pub mode: SleepMode,
    /// Time left (timed mode); ticks only while playing.
    pub remaining: Duration,
    /// The album the timer waits for the end of (end-of-album mode).
    pub album: Option<String>,
}

/// What a track change means for the timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Continue,
    /// Pause now and clear the timer.
    Pause,
}

impl SleepTimer {
    pub fn timed(secs: u64) -> Self {
        Self { mode: SleepMode::Timed, remaining: Duration::from_secs(secs), album: None }
    }

    pub fn end_of_song() -> Self {
        Self { mode: SleepMode::EndOfSong, remaining: Duration::ZERO, album: None }
    }

    /// Waits for the end of the album of `current`.
    pub fn end_of_album(current: Option<&Song>) -> Self {
        Self {
            mode: SleepMode::EndOfAlbum,
            remaining: Duration::ZERO,
            album: current.and_then(|s| s.album_id.clone()),
        }
    }

    /// Counts `by` down while `playing`; true when the time is up.
    pub fn tick(&mut self, by: Duration, playing: bool) -> bool {
        if self.mode != SleepMode::Timed || !playing {
            return false;
        }
        self.remaining = self.remaining.saturating_sub(by);
        self.remaining.is_zero()
    }

    /// The song changed on its own (the previous one ended): end of song stops
    /// here, end of album stops once the album is over.
    pub fn on_advance(&self, next: Option<&Song>) -> Verdict {
        match self.mode {
            SleepMode::Timed => Verdict::Continue,
            SleepMode::EndOfSong => Verdict::Pause,
            SleepMode::EndOfAlbum => {
                let same = next.is_some_and(|s| s.album_id.is_some() && s.album_id == self.album);
                if same { Verdict::Continue } else { Verdict::Pause }
            }
        }
    }

    /// The listener started another song: end of album now waits for that song's album.
    pub fn on_manual_start(&mut self, song: Option<&Song>) {
        if self.mode == SleepMode::EndOfAlbum {
            self.album = song.and_then(|s| s.album_id.clone());
        }
    }
}

/// "m:ss", or "h:mm:ss" from an hour on (Feishin `formatRemaining`).
pub fn format_remaining(d: Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// The popover's own state.
#[derive(Debug, Clone, Default)]
pub struct SleepUi {
    pub open: bool,
    /// The custom form: hours, minutes, seconds as typed.
    pub custom: Option<[String; 3]>,
}

/// Which field of the custom form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomField {
    Hours,
    Minutes,
    Seconds,
}

#[derive(Debug, Clone)]
pub enum SleepMsg {
    /// The button: open / close the popover.
    Toggle,
    Close,
    EndOfSong,
    EndOfAlbum,
    Minutes(u32),
    Cancel,
    CustomOpen,
    CustomClose,
    CustomEdit(CustomField, String),
    CustomStart,
    Tick,
}

/// Seconds of the custom form; hours <= 23, minutes and seconds <= 59.
pub fn custom_seconds(fields: &[String; 3]) -> u64 {
    let n = |s: &String| s.trim().parse::<u64>().unwrap_or(0);
    n(&fields[0]).min(23) * 3600 + n(&fields[1]).min(59) * 60 + n(&fields[2]).min(59)
}

impl Listen {
    pub(super) fn on_sleep(&mut self, msg: SleepMsg) -> Task<Message> {
        match msg {
            SleepMsg::Toggle => {
                self.fs.sleep_ui.open = !self.fs.sleep_ui.open;
                self.fs.sleep_ui.custom = None;
            }
            SleepMsg::Close => {
                self.fs.sleep_ui.open = false;
                self.fs.sleep_ui.custom = None;
            }
            SleepMsg::EndOfSong => self.sleep_start(SleepTimer::end_of_song()),
            SleepMsg::EndOfAlbum => {
                // Track shuffle scatters the albums: the end of one means nothing.
                if self.queue.shuffle() {
                    return Task::none();
                }
                let timer = SleepTimer::end_of_album(self.queue.current().map(|i| &i.song));
                self.sleep_start(timer);
            }
            SleepMsg::Minutes(m) => self.sleep_start(SleepTimer::timed(u64::from(m) * 60)),
            SleepMsg::Cancel => {
                self.sleep_timer = None;
                self.fs.sleep_ui.custom = None;
                self.toast_info("Sleep timer off.".into());
            }
            SleepMsg::CustomOpen => {
                self.fs.sleep_ui.custom = Some(["0".into(), "20".into(), "0".into()]);
            }
            SleepMsg::CustomClose => self.fs.sleep_ui.custom = None,
            SleepMsg::CustomEdit(field, text) => {
                let digits: String = text.chars().filter(char::is_ascii_digit).take(2).collect();
                if let Some(c) = &mut self.fs.sleep_ui.custom {
                    c[field as usize] = digits;
                }
            }
            SleepMsg::CustomStart => {
                let secs = self.fs.sleep_ui.custom.as_ref().map_or(0, custom_seconds);
                if secs > 0 {
                    self.sleep_start(SleepTimer::timed(secs));
                }
            }
            SleepMsg::Tick => {
                let now = std::time::Instant::now();
                let by = self.fs.sleep_tick.map_or(Duration::from_secs(1), |t| now.duration_since(t));
                self.fs.sleep_tick = Some(now);
                let playing = self.playback.playing;
                // After a pause the first tick would count the whole pause.
                let by = by.min(Duration::from_secs(2));
                if self.sleep_timer.as_mut().is_some_and(|t| t.tick(by, playing)) {
                    return self.sleep_fire();
                }
            }
        }
        Task::none()
    }

    fn sleep_start(&mut self, timer: SleepTimer) {
        self.sleep_timer = Some(timer);
        self.fs.sleep_tick = None;
        self.fs.sleep_ui = SleepUi::default();
    }

    /// Time is up (or the song / album ended): pause and clear the timer.
    fn sleep_fire(&mut self) -> Task<Message> {
        self.sleep_timer = None;
        self.fs.sleep_tick = None;
        self.toast_info("Sleep timer: playback paused.".into());
        if self.playback.playing { self.handle(Message::TogglePlay) } else { Task::none() }
    }

    /// The previous song ended on its own and the next one has started.
    pub(super) fn sleep_advanced(&mut self) -> Task<Message> {
        let verdict = {
            let next = self.queue.current().map(|i| &i.song);
            self.sleep_timer.as_ref().map(|t| t.on_advance(next))
        };
        if verdict == Some(Verdict::Pause) { self.sleep_fire() } else { Task::none() }
    }

    /// A song was started by the listener (not by the previous one ending).
    pub(super) fn sleep_manual_start(&mut self) {
        let song = self.queue.current().map(|i| i.song.clone());
        if let Some(t) = &mut self.sleep_timer {
            t.on_manual_start(song.as_ref());
        }
    }

    /// Whether the once-a-second tick is needed.
    pub fn sleep_ticking(&self) -> bool {
        self.sleep_timer.as_ref().is_some_and(|t| t.mode == SleepMode::Timed) && self.playback.playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(album: Option<&str>) -> Song {
        let mut s = crate::demo::album_songs(0).remove(0);
        s.album_id = album.map(str::to_string);
        s
    }

    #[test]
    fn counts_down_only_while_playing() {
        let mut t = SleepTimer::timed(10);
        assert!(!t.tick(Duration::from_secs(4), false));
        assert_eq!(t.remaining, Duration::from_secs(10));
        assert!(!t.tick(Duration::from_secs(4), true));
        assert_eq!(t.remaining, Duration::from_secs(6));
        assert!(t.tick(Duration::from_secs(9), true));
        assert!(t.remaining.is_zero());
    }

    #[test]
    fn end_of_song_pauses_on_the_next_song() {
        let t = SleepTimer::end_of_song();
        assert_eq!(t.on_advance(Some(&song(Some("a")))), Verdict::Pause);
        assert!(!SleepTimer::end_of_song().clone().tick(Duration::from_secs(60), true));
    }

    #[test]
    fn end_of_album_pauses_when_the_album_changes() {
        let t = SleepTimer::end_of_album(Some(&song(Some("a"))));
        assert_eq!(t.album.as_deref(), Some("a"));
        assert_eq!(t.on_advance(Some(&song(Some("a")))), Verdict::Continue);
        assert_eq!(t.on_advance(Some(&song(Some("b")))), Verdict::Pause);
        assert_eq!(t.on_advance(None), Verdict::Pause);
        // Songs without an album never count as the same album.
        let single = SleepTimer::end_of_album(Some(&song(None)));
        assert_eq!(single.on_advance(Some(&song(None))), Verdict::Pause);
    }

    #[test]
    fn a_manual_start_retargets_the_album() {
        let mut t = SleepTimer::end_of_album(Some(&song(Some("a"))));
        t.on_manual_start(Some(&song(Some("b"))));
        assert_eq!(t.album.as_deref(), Some("b"));
        let mut timed = SleepTimer::timed(60);
        timed.on_manual_start(Some(&song(Some("b"))));
        assert_eq!(timed.album, None);
    }

    #[test]
    fn formats_like_feishin() {
        assert_eq!(format_remaining(Duration::from_secs(65)), "1:05");
        assert_eq!(format_remaining(Duration::from_secs(3725)), "1:02:05");
        assert_eq!(format_remaining(Duration::ZERO), "0:00");
    }

    #[test]
    fn custom_form_clamps_fields() {
        let f = |h: &str, m: &str, s: &str| [h.to_string(), m.to_string(), s.to_string()];
        assert_eq!(custom_seconds(&f("0", "20", "0")), 1200);
        assert_eq!(custom_seconds(&f("1", "99", "")), 3600 + 59 * 60);
        assert_eq!(custom_seconds(&f("", "", "")), 0);
    }
}
