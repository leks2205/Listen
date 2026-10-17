//! Request budget for YouTube (yt-dlp resolves and googlevideo fetches for analysis
//! and mixing). Guests get ~300 videos/hour per IP, 5–10 s apart (yt-dlp wiki), else
//! "confirm you're not a bot". Urgent work always passes but is counted; background
//! work gets the slots left.

use listen_source::Urgency;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Background requests at most this often (any request, urgent or not,
/// pushes the next background slot back).
pub const BACKGROUND_MIN_GAP: Duration = Duration::from_secs(10);
/// Background requests start only while fewer than this many requests of
/// any kind were made in the last hour (leaves headroom under ~300/h).
pub const BACKGROUND_PER_HOUR: usize = 150;
/// How long a background request waits for its slot before giving up
/// (the caller retries later).
pub const BACKGROUND_MAX_WAIT: Duration = Duration::from_secs(15);

const HOUR: Duration = Duration::from_secs(3600);

pub struct Budget {
    min_gap: Duration,
    per_hour: usize,
    /// Start times of requests in the last hour; may hold reserved future slots.
    log: Mutex<Vec<Instant>>,
}

impl Budget {
    pub const fn new(min_gap: Duration, per_hour: usize) -> Self {
        Self { min_gap, per_hour, log: Mutex::new(Vec::new()) }
    }

    /// Counts a request that is made regardless of the budget (playback,
    /// [`Urgency::Now`], retries).
    pub fn note(&self) {
        self.note_at(Instant::now());
    }

    fn note_at(&self, now: Instant) {
        self.log.lock().expect("budget").push(now);
    }

    /// Takes a slot: `Now` immediately, `Background` after waiting at most
    /// `max_wait` for one. `false`: no slot, try again later.
    pub async fn acquire(&self, urgency: Urgency, max_wait: Duration) -> bool {
        let now = Instant::now();
        if urgency == Urgency::Now {
            self.note_at(now);
            return true;
        }
        match self.reserve(now, max_wait) {
            Some(at) => {
                tokio::time::sleep_until(at.into()).await;
                true
            }
            None => false,
        }
    }

    /// Reserves the earliest background slot if it starts within `max_wait`.
    fn reserve(&self, now: Instant, max_wait: Duration) -> Option<Instant> {
        let mut log = self.log.lock().expect("budget");
        log.retain(|t| now.saturating_duration_since(*t) < HOUR);
        let mut at = log.iter().max().map_or(now, |last| (*last + self.min_gap).max(now));
        if log.len() >= self.per_hour {
            // Wait until enough of the hour's requests have aged out.
            let mut times = log.clone();
            times.sort();
            at = at.max(times[log.len() - self.per_hour] + HOUR);
        }
        if at.duration_since(now) > max_wait {
            return None;
        }
        log.push(at);
        Some(at)
    }
}

/// The one budget for the whole process (one IP).
pub static BUDGET: Budget = Budget::new(BACKGROUND_MIN_GAP, BACKGROUND_PER_HOUR);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_waits_for_the_gap() {
        let b = Budget::new(Duration::from_secs(10), 100);
        let t0 = Instant::now();
        assert_eq!(b.reserve(t0, Duration::ZERO), Some(t0));
        // Next slot is 10 s later: refused without waiting, granted with.
        assert_eq!(b.reserve(t0 + Duration::from_secs(2), Duration::from_secs(5)), None);
        let at = b.reserve(t0 + Duration::from_secs(2), Duration::from_secs(8)).unwrap();
        assert_eq!(at, t0 + Duration::from_secs(10));
        // Reserved slots stack up.
        assert_eq!(
            b.reserve(t0 + Duration::from_secs(3), Duration::from_secs(20)),
            Some(t0 + Duration::from_secs(20))
        );
    }

    #[test]
    fn urgent_requests_push_background_back() {
        let b = Budget::new(Duration::from_secs(10), 100);
        let t0 = Instant::now();
        b.note_at(t0);
        assert_eq!(b.reserve(t0 + Duration::from_secs(1), Duration::from_secs(5)), None);
        assert!(b.reserve(t0 + Duration::from_secs(10), Duration::ZERO).is_some());
    }

    #[test]
    fn hourly_cap() {
        let b = Budget::new(Duration::ZERO, 3);
        let t0 = Instant::now();
        for i in 0..3 {
            b.note_at(t0 + Duration::from_secs(i));
        }
        let later = t0 + Duration::from_secs(60);
        assert_eq!(b.reserve(later, Duration::from_secs(60)), None);
        // Once the oldest request is an hour old, a slot opens.
        assert_eq!(b.reserve(later, HOUR), Some(t0 + HOUR));
        // Old entries are dropped.
        assert!(b.reserve(t0 + 3 * HOUR, Duration::ZERO).is_some());
    }

    #[tokio::test]
    async fn now_always_passes() {
        let b = Budget::new(Duration::from_secs(3600), 1);
        assert!(b.acquire(Urgency::Now, Duration::ZERO).await);
        assert!(b.acquire(Urgency::Now, Duration::ZERO).await);
        assert!(!b.acquire(Urgency::Background, Duration::from_millis(10)).await);
    }
}
