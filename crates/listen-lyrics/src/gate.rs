//! Politeness towards the lyrics providers: a minimum gap between two requests
//! to the same one, shared by every lookup in the process.

use crate::providers::Source;
use std::sync::Mutex;
use std::time::{Duration, Instant};

impl Source {
    /// The least time between two requests to this provider.
    pub fn min_interval(self) -> Duration {
        Duration::from_millis(match self {
            Source::Lrclib => 250,
            Source::NetEase => 600,
            Source::Genius => 1200,
            Source::SimpMusic => 600,
        })
    }
}

static LAST: [Mutex<Option<Instant>>; 4] =
    [Mutex::new(None), Mutex::new(None), Mutex::new(None), Mutex::new(None)];

/// When a request made at `now` may go out, given the last slot handed out.
pub fn next_slot(last: Option<Instant>, interval: Duration, now: Instant) -> Instant {
    match last {
        Some(l) if l + interval > now => l + interval,
        _ => now,
    }
}

/// Waits for this provider's next free slot (concurrent callers queue up).
pub async fn wait(source: Source) {
    let at = {
        let mut last = LAST[source as usize].lock().unwrap_or_else(|e| e.into_inner());
        let at = next_slot(*last, source.min_interval(), Instant::now());
        *last = Some(at);
        at
    };
    let now = Instant::now();
    if at > now {
        tokio::time::sleep(at - now).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_spaced_by_the_interval() {
        let now = Instant::now();
        let gap = Duration::from_millis(500);
        assert_eq!(next_slot(None, gap, now), now);
        let first = next_slot(None, gap, now);
        let second = next_slot(Some(first), gap, now);
        assert_eq!(second, now + gap);
        let third = next_slot(Some(second), gap, now);
        assert_eq!(third, now + gap * 2);
        // Long after the last request: no wait.
        let later = now + Duration::from_secs(10);
        assert_eq!(next_slot(Some(third), gap, later), later);
    }

    #[test]
    fn slower_providers_get_longer_gaps() {
        assert!(Source::Genius.min_interval() > Source::NetEase.min_interval());
        assert!(Source::NetEase.min_interval() > Source::Lrclib.min_interval());
    }
}
