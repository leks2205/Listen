//! A memory model of the listening history (ACT-R base-level activation): how present a song is in the
//! listener's mind right now. Each play adds `(now − t)^−d` with `d = 0.5`, so recent and frequent plays
//! both count and old ones fade by a power law: `A = ln Σ (now − tᵢ)^−d` over the
//! [`PlayStatus::Played`](crate::history::PlayStatus) entries (time in days).
//!
//! Two uses:
//! - **Forgotten favourites**: played a lot once ([`FORGOTTEN_MIN_PLAYS`]+, at a high peak activation),
//!   nothing for [`FORGOTTEN_QUIET_DAYS`], low activation now. They fill the Auto-DJ's familiar slot,
//!   the AI DJ pool and the Rediscover mix.
//! - **Worn out**: skipped again and again lately ([`WORN_SKIPS`] times in [`WORN_DAYS`] days), or skipped
//!   while its activation is very high. Rested from every automatic pick until the skips age out; songs
//!   the user starts always play.
//!
//! Songs are keyed by [`song_key`], so all versions of a song share one memory.

use crate::history::Entry;
use crate::model::Song;
use crate::versions::song_key;
use std::collections::{HashMap, HashSet};

/// The ACT-R decay.
pub const DECAY: f64 = 0.5;
/// A forgotten favourite has at least this many plays …
pub const FORGOTTEN_MIN_PLAYS: u32 = 5;
/// … and none in this many days.
pub const FORGOTTEN_QUIET_DAYS: i64 = 30;
/// Skips within [`WORN_DAYS`] that rest a song from automatic picks.
pub const WORN_SKIPS: u32 = 3;
/// The window for recent skips.
pub const WORN_DAYS: i64 = 7;
/// With this activation or more (about four or five plays in the last week), [`WORN_HOT_SKIPS`] skips
/// already rest a song.
pub const WORN_HOT_ACTIVATION: f64 = 1.0;
pub const WORN_HOT_SKIPS: u32 = 2;
const DAY: i64 = 86_400;
/// The youngest play counts as one hour old, so a play that just ended has a finite weight.
const MIN_AGE_DAYS: f64 = 1.0 / 24.0;

/// Base-level activation at `now` from play times (Unix seconds); plays after `now` are ignored.
/// `None` without plays.
pub fn activation(plays: &[i64], now: i64) -> Option<f64> {
    let sum: f64 = plays
        .iter()
        .filter(|&&t| t <= now)
        .map(|&t| ((now - t) as f64 / DAY as f64).max(MIN_AGE_DAYS).powf(-DECAY))
        .sum();
    (sum > 0.0).then(|| sum.ln())
}

/// What the history says about one song.
#[derive(Debug, Clone, PartialEq)]
pub struct SongMemory {
    pub key: String,
    /// The song as last played (or skipped), playable.
    pub song: Song,
    pub plays: u32,
    /// Last play (Unix seconds); 0 when it was only ever skipped.
    pub last_played: i64,
    /// Activation now (`f64::NEG_INFINITY` without plays).
    pub activation: f64,
    /// Highest activation the song ever had, one day after each of its plays.
    pub peak: f64,
    /// Skips within the last [`WORN_DAYS`] days.
    pub recent_skips: u32,
}

impl SongMemory {
    /// Forgotten-favourite score: how much loved (plays, peak) and how much faded since.
    pub fn forgotten_score(&self) -> f64 {
        f64::from(self.plays).ln() + self.peak - self.activation
    }

    /// A forgotten favourite at `now` (see the module docs).
    pub fn is_forgotten(&self, now: i64) -> bool {
        self.plays >= FORGOTTEN_MIN_PLAYS && self.last_played < now - FORGOTTEN_QUIET_DAYS * DAY
    }

    /// Worn out: rested from automatic picks (see the module docs).
    pub fn is_worn_out(&self) -> bool {
        self.recent_skips >= WORN_SKIPS
            || (self.recent_skips >= WORN_HOT_SKIPS && self.activation >= WORN_HOT_ACTIVATION)
    }
}

/// The memory of every song in a history.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    pub now: i64,
    pub songs: HashMap<String, SongMemory>,
}

impl Memory {
    /// Builds the memory at `now` from history entries (any order; entries after `now` are ignored).
    pub fn new(entries: &[Entry], now: i64) -> Self {
        struct Acc<'a> {
            plays: Vec<i64>,
            skips: u32,
            last: &'a Entry,
        }
        let mut acc: HashMap<String, Acc> = HashMap::new();
        for e in entries.iter().filter(|e| e.ts <= now) {
            let a = acc.entry(song_key(&e.to_song())).or_insert(Acc { plays: Vec::new(), skips: 0, last: e });
            if e.played() {
                a.plays.push(e.ts);
            } else if e.ts >= now - WORN_DAYS * DAY {
                a.skips += 1;
            }
            if e.ts >= a.last.ts {
                a.last = e;
            }
        }
        let songs = acc
            .into_iter()
            .map(|(key, mut a)| {
                a.plays.sort_unstable();
                let peak = (0..a.plays.len())
                    .filter_map(|i| activation(&a.plays[..=i], a.plays[i] + DAY))
                    .fold(f64::NEG_INFINITY, f64::max);
                let m = SongMemory {
                    song: a.last.to_song(),
                    plays: a.plays.len() as u32,
                    last_played: a.plays.last().copied().unwrap_or(0),
                    activation: activation(&a.plays, now).unwrap_or(f64::NEG_INFINITY),
                    peak,
                    recent_skips: a.skips,
                    key: key.clone(),
                };
                (key, m)
            })
            .collect();
        Self { now, songs }
    }

    /// Forgotten favourites, most loved and most faded first.
    pub fn forgotten(&self, limit: usize) -> Vec<&SongMemory> {
        let mut v: Vec<&SongMemory> = self.songs.values().filter(|m| m.is_forgotten(self.now)).collect();
        sort_by_score(&mut v);
        v.truncate(limit);
        v
    }

    /// Songs with at least `min_plays` plays and none in the last `quiet_days`, ranked like
    /// [`Self::forgotten`] (the Rediscover mix).
    pub fn faded(&self, min_plays: u32, quiet_days: i64, limit: usize) -> Vec<&SongMemory> {
        let mut v: Vec<&SongMemory> = self
            .songs
            .values()
            .filter(|m| m.plays >= min_plays && m.last_played < self.now - quiet_days * DAY)
            .collect();
        sort_by_score(&mut v);
        v.truncate(limit);
        v
    }

    /// Keys of the worn-out songs, rested from automatic picks now.
    pub fn worn_out(&self) -> HashSet<String> {
        self.songs.values().filter(|m| m.is_worn_out()).map(|m| m.key.clone()).collect()
    }

    pub fn get(&self, song: &Song) -> Option<&SongMemory> {
        self.songs.get(&song_key(song))
    }
}

fn sort_by_score(v: &mut [&SongMemory]) {
    v.sort_by(|a, b| {
        b.forgotten_score()
            .total_cmp(&a.forgotten_score())
            .then(b.plays.cmp(&a.plays))
            .then(a.key.cmp(&b.key))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{PlayCtx, PlayStatus};
    use crate::model::{ArtistRef, ServerId, SourceKind};

    const NOW: i64 = 1_800_000_000;

    fn song(title: &str) -> Song {
        Entry {
            ts: 0,
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            song_id: title.to_lowercase(),
            status: PlayStatus::Played,
            title: title.into(),
            artists: vec![ArtistRef { id: None, name: "Artist".into() }],
            album: None,
            album_id: None,
            image: None,
            duration_secs: 200,
            at_s: None,
            frac: None,
            ctx: None,
        }
        .to_song()
    }

    fn plays(title: &str, days_ago: &[i64]) -> Vec<Entry> {
        days_ago.iter().map(|d| Entry::new(&song(title), NOW - d * DAY)).collect()
    }

    fn skips(title: &str, days_ago: &[i64]) -> Vec<Entry> {
        days_ago
            .iter()
            .map(|d| Entry::skipped(&song(title), NOW - d * DAY, 10.0, PlayCtx::default()))
            .collect()
    }

    #[test]
    fn activation_rises_with_recency_and_frequency() {
        let day = |d: i64| NOW - d * DAY;
        let recent = activation(&[day(1)], NOW).unwrap();
        let old = activation(&[day(100)], NOW).unwrap();
        let often = activation(&[day(100), day(101), day(102)], NOW).unwrap();
        assert!(recent > old && often > old);
        // One play a day ago: ln(1^−0.5) = 0.
        assert!(recent.abs() < 1e-9);
        assert_eq!(activation(&[], NOW), None);
        // A play in the future does not count.
        assert_eq!(activation(&[NOW + DAY], NOW), None);
        assert!(activation(&[NOW], NOW).unwrap().is_finite());
    }

    #[test]
    fn forgotten_favourites_have_many_plays_and_none_lately() {
        let mut e = Vec::new();
        e.extend(plays("Loved", &[200, 201, 202, 203, 204, 205, 206, 207]));
        e.extend(plays("Liked", &[90, 120, 150, 180, 210]));
        e.extend(plays("Current", &[1, 2, 3, 4, 5, 6]));
        e.extend(plays("Recent too", &[10, 200, 201, 202, 203, 204]));
        e.extend(plays("Few", &[300, 301]));
        let m = Memory::new(&e, NOW);
        let titles: Vec<&str> = m.forgotten(10).iter().map(|s| s.song.title.as_str()).collect();
        // A burst of eight plays beats five spread-out ones; recent and rare songs are no favourites.
        assert_eq!(titles, ["Loved", "Liked"]);
        let loved = m.get(&song("Loved")).unwrap();
        assert_eq!(loved.plays, 8);
        assert_eq!(loved.last_played, NOW - 200 * DAY);
        assert!(loved.peak > loved.activation);
        // The Rediscover mix's looser filter.
        assert_eq!(m.faded(2, 60, 10).len(), 3);
    }

    #[test]
    fn skips_are_no_plays_and_rest_worn_out_songs() {
        let mut e = Vec::new();
        e.extend(plays("Tired", &[20, 30]));
        e.extend(skips("Tired", &[1, 2, 3]));
        e.extend(plays("Old skips", &[20]));
        e.extend(skips("Old skips", &[10, 11, 12]));
        e.extend(plays("Hot", &[0, 1, 2, 3, 4]));
        e.extend(skips("Hot", &[0, 1]));
        e.extend(plays("Cold", &[40, 50]));
        e.extend(skips("Cold", &[0, 1]));
        let m = Memory::new(&e, NOW);
        assert_eq!(m.get(&song("Tired")).unwrap().plays, 2);
        assert_eq!(m.get(&song("Tired")).unwrap().recent_skips, 3);
        let mut worn: Vec<String> = m.worn_out().into_iter().collect();
        worn.sort();
        assert_eq!(worn, [song_key(&song("Hot")), song_key(&song("Tired"))]);
        // Skipped only: no plays, no activation.
        let only = Memory::new(&skips("New", &[1]), NOW);
        let s = only.get(&song("New")).unwrap();
        assert_eq!((s.plays, s.last_played), (0, 0));
        assert!(s.activation.is_infinite());
    }

    #[test]
    fn versions_share_one_memory() {
        let mut e = plays("Song", &[100, 101, 102]);
        e.extend(plays("Song (Radio Edit)", &[103, 104]));
        let m = Memory::new(&e, NOW);
        assert_eq!(m.songs.len(), 1);
        assert_eq!(m.forgotten(5).len(), 1);
        assert!(e.iter().all(|x| x.status == PlayStatus::Played));
    }
}
