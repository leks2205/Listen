//! Every volume move, combined: song fade-in/out, the AI DJ ducking the music under its
//! voice (linear in dB) and the short pause/resume fade.
//! mpv's `volume` is cubic (gain = (v/100)³), so gain `g` on user volume `V` is sent as `V·∛g`.

use super::{Listen, Message};
use iced::Task;
use std::time::{Duration, Instant};

/// How far the DJ turns the music down while talking over it.
pub const DUCK_DB: f32 = -12.0;
const PAUSE_SECS: f32 = 0.3;
pub const TICK: Duration = Duration::from_millis(30);

#[derive(Debug)]
pub struct Fader {
    duck_db: f32,
    duck_target: f32,
    /// dB per second.
    duck_rate: f32,
    /// 0 (silent) to 1: the pause/resume fade.
    pause: f32,
    pause_target: f32,
    /// Fading out to pause; mpv is paused when it reaches 0.
    pending_pause: bool,
    /// mpv itself is paused by a finished pause fade.
    mpv_paused: bool,
    pub(super) last_sent: Option<f32>,
    last_tick: Option<Instant>,
    /// When `playback.position` was last reported (to extrapolate between reports).
    pub pos_at: Option<Instant>,
}

impl Default for Fader {
    fn default() -> Self {
        Self {
            duck_db: 0.0,
            duck_target: 0.0,
            duck_rate: 12.0,
            pause: 1.0,
            pause_target: 1.0,
            pending_pause: false,
            mpv_paused: false,
            last_sent: None,
            last_tick: None,
            pos_at: None,
        }
    }
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl Listen {
    fn track_fade_secs(&self) -> f32 {
        if self.mock_playback() { 0.0 } else { self.config.settings.playback.track_fade_secs.max(0.0) }
    }

    /// How long the DJ takes to turn the music down or up.
    pub(super) fn knob_secs(&self) -> f32 {
        self.config.settings.playback.track_fade_secs.max(1.0)
    }

    /// Turns the music down under the DJ's voice (or back up), gliding like
    /// a volume knob; `instant` jumps there (a song that starts already down).
    pub(super) fn duck(&mut self, down: bool, instant: bool) {
        let rate = DUCK_DB.abs() / self.knob_secs();
        let f = &mut self.fader;
        f.duck_target = if down { DUCK_DB } else { 0.0 };
        f.duck_rate = rate;
        if instant {
            f.duck_db = f.duck_target;
        }
    }

    pub(super) fn ducked(&self) -> bool {
        self.fader.duck_target < 0.0
    }

    /// The playback position now, extrapolated since mpv's last report. Not at position 0:
    /// after a gapless switch mpv holds 0 until the previous song's buffer has played.
    fn est_position(&self) -> Duration {
        let mut pos = self.playback.position;
        if self.playback.playing
            && !self.fader.pending_pause
            && pos > Duration::from_millis(50)
            && let Some(at) = self.fader.pos_at
        {
            pos += at.elapsed().min(Duration::from_millis(150));
        }
        let total = self.duration();
        if total > Duration::ZERO { pos.min(total) } else { pos }
    }

    /// The song's own fade-in/out at this moment (1 = full). Edges joined
    /// by a DJ transition have the transition's gains instead.
    fn track_gain(&self) -> f32 {
        let fade = self.track_fade_secs();
        let Some(current) = self.queue.current() else { return 1.0 };
        if fade <= 0.0 || self.dj_airing() {
            return 1.0;
        }
        let mixed_in = self.mix.entered_uid == Some(current.uid);
        let mixed_out = self.mix.owns_next();
        let pos = self.est_position().as_secs_f32();
        let total = self.duration().as_secs_f32();
        // Very short tracks fade over at most a third of their length.
        let fade = if total > 0.0 { fade.min(total / 3.0) } else { fade };
        let fade_in = if mixed_in { 1.0 } else { (pos / fade).min(1.0) };
        let fade_out = if total > 0.0 && !mixed_out { ((total - pos) / fade).clamp(0.0, 1.0) } else { 1.0 };
        fade_in.min(fade_out)
    }

    /// What mpv's volume property should be now (the active deck).
    pub fn effective_volume(&self) -> f32 {
        self.deck_volume(self.decks.active)
    }

    /// Volume of a deck: the user's volume × song fade (active deck only) ×
    /// DJ ducking × pause fade × the transition's gain for that deck.
    pub(super) fn deck_volume(&self, deck: usize) -> f32 {
        let track = if deck == self.decks.active { self.track_gain() } else { 1.0 };
        let gain = track * db_to_gain(self.fader.duck_db) * self.fader.pause * self.mix.gain(deck);
        f32::from(self.playback.volume) * gain.clamp(0.0, 1.0).cbrt()
    }

    /// Whether the volume is moving (the fade clock needs to tick).
    pub fn fade_active(&self) -> bool {
        if !self.decks.has_active() || self.mock_playback() {
            return false;
        }
        if self.mix.phase != super::mix::Phase::Idle {
            return true;
        }
        let f = &self.fader;
        if f.pending_pause
            || (f.duck_db - f.duck_target).abs() > 0.01
            || (f.pause - f.pause_target).abs() > 0.001
        {
            return true;
        }
        let fade = self.track_fade_secs();
        if fade > 0.0 && self.playback.playing && self.queue.current().is_some() {
            let pos = self.est_position().as_secs_f32();
            let total = self.duration().as_secs_f32();
            if pos < fade + 0.3 || (total > 0.0 && total - pos < fade + 0.6) {
                return true;
            }
        }
        f.last_sent.is_some_and(|v| (v - self.effective_volume()).abs() > 0.15)
    }

    /// One step of the fade clock.
    pub(super) fn fade_tick(&mut self) -> Task<Message> {
        let now = Instant::now();
        let dt = self.fader.last_tick.map_or(0.03, |t| now.duration_since(t).as_secs_f32().min(0.1));
        self.fader.last_tick = Some(now);
        let f = &mut self.fader;
        let step = f.duck_rate * dt;
        f.duck_db = if f.duck_db > f.duck_target {
            (f.duck_db - step).max(f.duck_target)
        } else {
            (f.duck_db + step).min(f.duck_target)
        };
        let step = dt / PAUSE_SECS;
        f.pause = if f.pause > f.pause_target {
            (f.pause - step).max(f.pause_target)
        } else {
            (f.pause + step).min(f.pause_target)
        };
        let mut tasks = vec![self.mix_tick(), self.push_volume(false)];
        if self.fader.pending_pause && self.fader.pause <= 0.0 {
            self.fader.pending_pause = false;
            self.fader.mpv_paused = true;
            tasks.push(self.player_task(|p| async move { p.set_paused(true).await }));
            tasks.push(self.voice_task(|v| async move { v.set_paused(true).await }));
        }
        Task::batch(tasks)
    }

    /// Sends the volume to mpv if it changed (`force`: always).
    pub(super) fn push_volume(&mut self, force: bool) -> Task<Message> {
        let v = self.effective_volume();
        if !force && self.fader.last_sent.is_some_and(|l| (l - v).abs() < 0.15) {
            return Task::none();
        }
        self.fader.last_sent = Some(v);
        self.player_task(move |p| async move { p.set_volume(v).await })
    }

    /// Pause (`true`) or resume with a short fade (Feishin's "fade on pause").
    pub(super) fn fade_pause(&mut self, pause: bool) -> Task<Message> {
        self.fader.last_tick = None;
        if pause {
            self.fader.pending_pause = true;
            self.fader.pause_target = 0.0;
            return Task::none();
        }
        self.fader.pending_pause = false;
        self.fader.pause_target = 1.0;
        if !std::mem::take(&mut self.fader.mpv_paused) {
            return Task::none(); // resumed during the fade-out: just fade back in
        }
        let v = self.effective_volume();
        self.fader.last_sent = Some(v);
        Task::batch([
            self.player_task(move |p| async move {
                p.set_volume(v).await?;
                p.set_paused(false).await
            }),
            self.voice_task(|v| async move { v.set_paused(false).await }),
        ])
    }

    /// Instant pause/resume state (no fade), e.g. after Stop.
    pub(super) fn reset_pause_fade(&mut self) {
        let f = &mut self.fader;
        f.pending_pause = false;
        f.mpv_paused = false;
        f.pause = 1.0;
        f.pause_target = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duck_level_and_cubic_volume() {
        assert!((db_to_gain(DUCK_DB) - 0.251).abs() < 0.001);
        // -12 dB on top of volume 80 → 80·∛0.251 ≈ 50.4
        assert!((80.0 * db_to_gain(DUCK_DB).cbrt() - 50.4).abs() < 0.2);
    }
}
