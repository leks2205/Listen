//! Free-tier bookkeeping: how many breaks were voiced today, and whether
//! the speech model's daily quota ran out. Google resets daily quotas at
//! midnight Pacific time; a "day" here is counted the same way.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Day number in Pacific time (UTC-8; an hour off during DST is harmless).
pub fn pacific_day(now: SystemTime) -> u64 {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    secs.saturating_sub(8 * 3600) / 86_400
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Quota {
    pub day: u64,
    /// Breaks spoken (TTS requests made) on `day`.
    pub voiced: u32,
    /// Text-model requests on `day`.
    pub planned: u32,
    /// The speech model reported its daily quota as used up on `day`.
    pub tts_exhausted: bool,
    /// When that was reported (Unix seconds); speech is retried after [`EXHAUSTED_PAUSE_SECS`].
    pub exhausted_at: u64,
}

/// How long a "quota used up" report keeps the DJ from speaking.
pub const EXHAUSTED_PAUSE_SECS: u64 = 3600;

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Quota {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(self)
            && let Err(e) = std::fs::write(path, text)
        {
            tracing::warn!("could not save the AI DJ quota counter: {e}");
        }
    }

    /// Starts a fresh count when the Pacific day changed.
    pub fn roll(&mut self, today: u64) {
        if self.day != today {
            *self = Quota { day: today, ..Default::default() };
        }
    }

    /// Whether a break may be voiced now (`now`: Unix seconds). There is no daily cap of our own:
    /// speech only pauses for a while after Google reports its quota as used up.
    pub fn can_voice(&self, now: u64) -> bool {
        !(self.tts_exhausted && now.saturating_sub(self.exhausted_at) < EXHAUSTED_PAUSE_SECS)
    }

    pub fn mark_exhausted(&mut self, now: u64) {
        self.tts_exhausted = true;
        self.exhausted_at = now;
    }

    /// Speech worked (or the key changed): the quota isn't used up.
    pub fn clear_exhausted(&mut self) {
        self.tts_exhausted = false;
        self.exhausted_at = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn day_rolls_at_pacific_midnight() {
        // 07:59 UTC on 2026-09-29 is 00:59 PDT but still 23:59 on the fixed UTC-8 clock.
        let t = UNIX_EPOCH + Duration::from_secs(1_790_668_740);
        assert_eq!(pacific_day(t) + 1, pacific_day(t + Duration::from_secs(61)));
    }

    #[test]
    fn counting_and_limits() {
        let mut q = Quota { day: 5, voiced: 15, planned: 40, tts_exhausted: true, exhausted_at: 1000 };
        assert!(!q.can_voice(1000));
        q.roll(5);
        assert_eq!(q.voiced, 15);
        q.roll(6);
        assert_eq!(q, Quota { day: 6, ..Default::default() });
        assert!(q.can_voice(0));
        q.voiced = 500;
        assert!(q.can_voice(0), "no daily cap");
    }

    #[test]
    fn exhausted_pauses_speech_for_an_hour_only() {
        let mut q = Quota { day: 1, ..Default::default() };
        q.mark_exhausted(10_000);
        assert!(!q.can_voice(10_000 + 60));
        assert!(q.can_voice(10_000 + EXHAUSTED_PAUSE_SECS));
        // Old counter files had no timestamp: speech is tried again right away.
        let old: Quota =
            serde_json::from_str(r#"{"day":1,"voiced":5,"planned":5,"tts_exhausted":true}"#).unwrap();
        assert!(old.can_voice(unix_now()));
        q.clear_exhausted();
        assert!(q.can_voice(10_000));
    }
}
