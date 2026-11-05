//! Rolling listening stats: this week against last week, the last four weeks' top lists,
//! artists heard for the first time this week and the songs skipped most.
//!
//! All windows are rolling and end at `now`: "this week" is `now − 7 d <= ts <= now`, "last
//! week" `now − 14 d <= ts < now − 7 d`, "4 weeks" `now − 28 d <= ts <= now` (d = 86 400 s). Only plays count as plays (see
//! [`Entry::played`]); skips are counted separately and never as plays.

use crate::history::{self, Entry, Ranked};
use chrono::FixedOffset;
use std::collections::{HashMap, HashSet};

pub const WEEK_SECS: i64 = 7 * 86_400;

/// Totals of one window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Totals {
    pub plays: u32,
    pub listening_secs: u64,
    pub skips: u32,
    /// Different songs played (server + song id).
    pub songs: u32,
    /// Different artists played (every credited artist, by name).
    pub artists: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Weekly {
    pub this_week: Totals,
    pub last_week: Totals,
    /// Plays, time and top lists of the last 28 days.
    pub four_weeks: history::Stats,
    /// Artists with a play this week and none before it, most played first.
    pub new_artists: Vec<Ranked>,
    /// How many new artists there are (the list stops at `top`).
    pub new_artists_total: u32,
    /// Songs skipped in the last 28 days, most skipped first (`plays` = plays in the 28 days).
    pub most_skipped: Vec<Ranked>,
}

fn totals<'a>(entries: impl Iterator<Item = &'a Entry>) -> Totals {
    let mut t = Totals::default();
    let mut songs = HashSet::new();
    let mut artists = HashSet::new();
    for e in entries {
        if !e.played() {
            t.skips += 1;
            continue;
        }
        t.plays += 1;
        t.listening_secs += u64::from(e.duration_secs);
        songs.insert((&e.server_id, e.song_id.as_str()));
        for a in &e.artists {
            artists.insert(a.name.as_str());
        }
    }
    t.songs = u32::try_from(songs.len()).unwrap_or(u32::MAX);
    t.artists = u32::try_from(artists.len()).unwrap_or(u32::MAX);
    t
}

fn within(e: &Entry, start: i64, end: i64) -> bool {
    e.ts >= start && e.ts < end
}

/// The rolling stats at `now` (unix seconds); top lists hold at most `top` rows.
pub fn weekly(entries: &[Entry], now: i64, tz: FixedOffset, top: usize) -> Weekly {
    let end = now + 1;
    let week = now - WEEK_SECS;
    let prev = week - WEEK_SECS;
    let month = now - 4 * WEEK_SECS;
    let this_week = totals(entries.iter().filter(|e| within(e, week, end)));
    let last_week = totals(entries.iter().filter(|e| within(e, prev, week)));
    let recent: Vec<Entry> = entries.iter().filter(|e| within(e, month, end)).cloned().collect();
    let mut four_weeks = history::stats(&recent, None, tz, top);
    four_weeks.years.clear();

    // New artists: played this week, never played before the week started.
    let before: HashSet<&str> = entries
        .iter()
        .filter(|e| e.played() && e.ts < week)
        .flat_map(|e| e.artists.iter().map(|a| a.name.as_str()))
        .collect();
    let mut fresh: HashMap<&str, Ranked> = HashMap::new();
    for e in entries.iter().filter(|e| e.played() && within(e, week, end)) {
        for a in e.artists.iter().filter(|a| !before.contains(a.name.as_str())) {
            let r = fresh.entry(&a.name).or_insert_with(|| ranked(&a.name, ""));
            r.plays += 1;
            r.secs += u64::from(e.duration_secs);
            // The detail is the first song heard by this artist this week.
            if r.detail.is_empty() {
                r.detail = e.title.clone();
            }
        }
    }
    let mut new_artists: Vec<Ranked> = fresh.into_values().collect();
    new_artists.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.secs.cmp(&a.secs)).then(a.name.cmp(&b.name)));
    let new_artists_total = u32::try_from(new_artists.len()).unwrap_or(u32::MAX);
    new_artists.truncate(top);

    // Most skipped songs of the 28 days.
    let mut skipped: HashMap<(&str, &str), Ranked> = HashMap::new();
    for e in recent.iter().filter(|e| !e.played()) {
        let artist = e.artists.first().map_or("", |a| a.name.as_str());
        let key = (e.server_id.0.as_str(), e.song_id.as_str());
        skipped.entry(key).or_insert_with(|| ranked(&e.title, artist)).skips += 1;
    }
    let mut plays: HashMap<(&str, &str), u32> = HashMap::new();
    for e in recent.iter().filter(|e| e.played()) {
        *plays.entry((e.server_id.0.as_str(), e.song_id.as_str())).or_default() += 1;
    }
    let mut most_skipped: Vec<Ranked> = skipped
        .into_iter()
        .map(|(k, mut r)| {
            r.plays = plays.get(&k).copied().unwrap_or(0);
            r
        })
        .collect();
    most_skipped.sort_by(|a, b| b.skips.cmp(&a.skips).then(a.plays.cmp(&b.plays)).then(a.name.cmp(&b.name)));
    most_skipped.truncate(top);

    Weekly { this_week, last_week, four_weeks, new_artists, new_artists_total, most_skipped }
}

fn ranked(name: &str, detail: &str) -> Ranked {
    Ranked { name: name.into(), detail: detail.into(), plays: 0, secs: 0, skips: 0 }
}

/// The change from `before` to `now` as a signed percentage, `None` when `before` is 0.
pub fn change_pct(before: u64, now: u64) -> Option<i64> {
    if before == 0 {
        return None;
    }
    let d = (now as f64 - before as f64) / before as f64 * 100.0;
    Some(d.round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;

    fn e(ts: i64, id: &str, artists: &[&str], played: bool) -> Entry {
        let artists: Vec<String> = artists.iter().map(|a| format!(r#"{{"id":null,"name":"{a}"}}"#)).collect();
        serde_json::from_str(&format!(
            r#"{{"ts":{ts},"server_id":"s","source":"subsonic","song_id":"{id}","status":"{}",
                "title":"T{id}","artists":[{}],"album":"A","album_id":null,"image":null,"duration_secs":200}}"#,
            if played { "played" } else { "skipped" },
            artists.join(",")
        ))
        .unwrap()
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    #[test]
    fn windows_split_this_week_last_week_and_four_weeks() {
        let h = vec![
            e(NOW - 40 * DAY, "old", &["Old"], true), // outside every window
            e(NOW - 20 * DAY, "1", &["A"], true),     // 4 weeks only
            e(NOW - 10 * DAY, "1", &["A"], true),     // last week
            e(NOW - 8 * DAY, "2", &["B"], false),     // last week, skip
            e(NOW - 7 * DAY, "2", &["B"], true),      // boundary: this week
            e(NOW - DAY, "3", &["A", "C"], true),
            e(NOW, "3", &["A", "C"], true),
        ];
        let w = weekly(&h, NOW, utc(), 10);
        assert_eq!(w.this_week, Totals { plays: 3, listening_secs: 600, skips: 0, songs: 2, artists: 3 });
        assert_eq!(w.last_week, Totals { plays: 1, listening_secs: 200, skips: 1, songs: 1, artists: 1 });
        assert_eq!(w.four_weeks.plays, 5);
        assert_eq!(w.four_weeks.skips, 1);
        assert_eq!((w.four_weeks.songs[0].name.as_str(), w.four_weeks.songs[0].plays), ("T1", 2));
        assert_eq!(w.four_weeks.artists[0].name, "A");
        assert!(w.four_weeks.years.is_empty());
    }

    #[test]
    fn new_artists_have_no_play_before_this_week() {
        let h = vec![
            e(NOW - 30 * DAY, "1", &["Known"], true),
            e(NOW - 9 * DAY, "9", &["Skipped once"], false), // a skip does not make it known
            e(NOW - 2 * DAY, "2", &["Known", "Guest"], true),
            e(NOW - DAY, "3", &["Skipped once"], true),
            e(NOW - DAY, "3", &["Skipped once"], true),
        ];
        let w = weekly(&h, NOW, utc(), 10);
        let names: Vec<&str> = w.new_artists.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Skipped once", "Guest"]);
        assert_eq!(w.new_artists_total, 2);
        assert_eq!(w.new_artists[0].plays, 2);
        assert_eq!(w.new_artists[1].detail, "T2");
    }

    #[test]
    fn most_skipped_ranks_by_skips_and_keeps_the_plays() {
        let h = vec![
            e(NOW - 3 * DAY, "1", &["A"], false),
            e(NOW - 2 * DAY, "1", &["A"], true),
            e(NOW - DAY, "1", &["A"], false),
            e(NOW - DAY, "2", &["B"], false),
            e(NOW - 2 * DAY, "3", &["C"], true),
            e(NOW - 50 * DAY, "2", &["B"], false), // too old
        ];
        let w = weekly(&h, NOW, utc(), 10);
        assert_eq!(w.most_skipped.len(), 2);
        assert_eq!((w.most_skipped[0].name.as_str(), w.most_skipped[0].skips), ("T1", 2));
        assert_eq!(w.most_skipped[0].plays, 1);
        assert_eq!((w.most_skipped[1].name.as_str(), w.most_skipped[1].skips), ("T2", 1));
    }

    #[test]
    fn change_is_relative_and_absent_from_zero() {
        assert_eq!(change_pct(0, 5), None);
        assert_eq!(change_pct(4, 5), Some(25));
        assert_eq!(change_pct(10, 4), Some(-60));
    }
}
