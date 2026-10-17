//! Local listening history: a JSON-lines file, never sent anywhere. Statistics and mixes are
//! computed from it on demand, so deleting entries removes everything derived from them.

use crate::model::{ArtistRef, ImageRef, ServerId, Song, SourceKind};
use chrono::{DateTime, Datelike, FixedOffset, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const FILE_NAME: &str = "history.jsonl";
const DAY: i64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayStatus {
    /// Played past the scrobble threshold.
    Played,
}

/// One play: track id, source, time and status, plus the display fields needed to list it offline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub ts: i64,
    pub server_id: ServerId,
    pub source: SourceKind,
    pub song_id: String,
    pub status: PlayStatus,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub image: Option<ImageRef>,
    pub duration_secs: u32,
}

impl Entry {
    pub fn new(song: &Song, ts: i64) -> Self {
        Self {
            ts,
            server_id: song.server_id.clone(),
            source: song.source,
            song_id: song.id.clone(),
            status: PlayStatus::Played,
            title: song.title.clone(),
            artists: song.artists.clone(),
            album: song.album.clone(),
            album_id: song.album_id.clone(),
            image: song.image.clone(),
            duration_secs: u32::try_from(song.duration.as_secs()).unwrap_or(u32::MAX),
        }
    }

    /// Key of the track across servers.
    fn key(&self) -> (&ServerId, &str) {
        (&self.server_id, &self.song_id)
    }

    /// A playable song rebuilt from the stored fields.
    pub fn to_song(&self) -> Song {
        Song {
            mbz: Default::default(),
            extra: Default::default(),
            id: self.song_id.clone(),
            server_id: self.server_id.clone(),
            source: self.source,
            title: self.title.clone(),
            artists: self.artists.clone(),
            album_artists: Vec::new(),
            album: self.album.clone(),
            album_id: self.album_id.clone(),
            image: self.image.clone(),
            duration: Duration::from_secs(u64::from(self.duration_secs)),
            track_number: None,
            disc_number: None,
            year: None,
            genres: Vec::new(),
            codec: None,
            bit_rate: None,
            sample_rate: None,
            bit_depth: None,
            bpm: None,
            favorite: false,
            rating: None,
            play_count: None,
            gain: Default::default(),
            explicit: false,
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: None,
            comment: None,
        }
    }
}

/// The history file, loaded into memory (oldest first).
#[derive(Debug, Clone, Default)]
pub struct History {
    pub entries: Vec<Entry>,
    path: Option<PathBuf>,
}

impl History {
    /// Loads the file; a missing file is an empty history, broken lines are skipped.
    pub fn load(path: &Path) -> Self {
        let entries = std::fs::read_to_string(path)
            .map(|t| t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
            .unwrap_or_default();
        Self { entries, path: Some(path.to_path_buf()) }
    }

    /// A history that is never written to disk (demo mode, tests).
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn add(&mut self, entry: Entry) -> std::io::Result<()> {
        if let Some(path) = &self.path {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
            writeln!(f, "{}", serde_json::to_string(&entry).map_err(std::io::Error::other)?)?;
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Removes the entry at `index` of `entries` (memory changes only if the file does).
    pub fn remove(&mut self, index: usize) -> std::io::Result<()> {
        if index < self.entries.len() {
            let mut left = self.entries.clone();
            left.remove(index);
            self.write(&left)?;
            self.entries = left;
        }
        Ok(())
    }

    pub fn clear(&mut self) -> std::io::Result<()> {
        if let Some(p) = &self.path {
            match std::fs::remove_file(p) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        self.entries.clear();
        Ok(())
    }

    /// Drops entries older than `max_age_days`. Returns how many were removed.
    pub fn prune(&mut self, max_age_days: Option<i64>, now: i64) -> std::io::Result<usize> {
        let Some(days) = max_age_days else { return Ok(0) };
        let left: Vec<Entry> = self.entries.iter().filter(|e| e.ts >= now - days * DAY).cloned().collect();
        let removed = self.entries.len() - left.len();
        if removed > 0 {
            self.write(&left)?;
            self.entries = left;
        }
        Ok(removed)
    }

    fn write(&self, entries: &[Entry]) -> std::io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        let mut text = String::new();
        for e in entries {
            text.push_str(&serde_json::to_string(e).map_err(std::io::Error::other)?);
            text.push('\n');
        }
        let tmp = path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    /// Newest first.
    pub fn recent(&self, limit: usize) -> Vec<(usize, &Entry)> {
        self.entries.iter().enumerate().rev().take(limit).collect()
    }

    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(&self.entries).unwrap_or_else(|_| "[]".into())
    }

    pub fn export_csv(&self) -> String {
        let mut out =
            String::from("timestamp,server_id,source,song_id,status,title,artists,album,duration_secs\n");
        for e in &self.entries {
            let artists = e.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join("; ");
            let fields = [
                e.ts.to_string(),
                e.server_id.0.clone(),
                format!("{:?}", e.source).to_lowercase(),
                e.song_id.clone(),
                "played".to_string(),
                e.title.clone(),
                artists,
                e.album.clone().unwrap_or_default(),
                e.duration_secs.to_string(),
            ];
            out.push_str(&fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
            out.push('\n');
        }
        out
    }
}

/// Quotes a CSV field and neutralises spreadsheet formulas.
fn csv_field(v: &str) -> String {
    let v = if v.starts_with(['=', '+', '-', '@']) { format!("'{v}") } else { v.to_string() };
    if v.contains([',', '"', '\n', '\r']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub name: String,
    /// Second line (artist of a song / album).
    pub detail: String,
    pub plays: u32,
    pub secs: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub plays: u32,
    pub listening_secs: u64,
    pub songs: Vec<Ranked>,
    pub artists: Vec<Ranked>,
    pub albums: Vec<Ranked>,
    /// Years that have entries, newest first.
    pub years: Vec<i32>,
}

fn local(e: &Entry, tz: FixedOffset) -> Option<DateTime<FixedOffset>> {
    DateTime::from_timestamp(e.ts, 0).map(|d| d.with_timezone(&tz))
}

/// Top lists for one calendar year (`None` = all time).
pub fn stats(entries: &[Entry], year: Option<i32>, tz: FixedOffset, top: usize) -> Stats {
    let mut years: Vec<i32> = entries.iter().filter_map(|e| local(e, tz)).map(|d| d.year()).collect();
    years.sort_unstable_by(|a, b| b.cmp(a));
    years.dedup();
    let mut out = Stats { years, ..Default::default() };
    let mut songs: HashMap<(&ServerId, &str), Ranked> = HashMap::new();
    let mut artists: HashMap<&str, Ranked> = HashMap::new();
    let mut albums: HashMap<(&str, &str), Ranked> = HashMap::new();
    for e in entries {
        if year.is_some() && local(e, tz).map(|d| d.year()) != year {
            continue;
        }
        let secs = u64::from(e.duration_secs);
        out.plays += 1;
        out.listening_secs += secs;
        let first = e.artists.first().map_or("", |a| a.name.as_str());
        bump(songs.entry(e.key()).or_insert_with(|| ranked(&e.title, first)), secs);
        for a in &e.artists {
            bump(artists.entry(&a.name).or_insert_with(|| ranked(&a.name, "")), secs);
        }
        if let Some(album) = &e.album {
            bump(albums.entry((album, first)).or_insert_with(|| ranked(album, first)), secs);
        }
    }
    out.songs = top_n(songs.into_values(), top);
    out.artists = top_n(artists.into_values(), top);
    out.albums = top_n(albums.into_values(), top);
    out
}

fn ranked(name: &str, detail: &str) -> Ranked {
    Ranked { name: name.into(), detail: detail.into(), plays: 0, secs: 0 }
}

fn bump(r: &mut Ranked, secs: u64) {
    r.plays += 1;
    r.secs += secs;
}

fn top_n(items: impl Iterator<Item = Ranked>, n: usize) -> Vec<Ranked> {
    let mut v: Vec<Ranked> = items.collect();
    v.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.secs.cmp(&a.secs)).then(a.name.cmp(&b.name)));
    v.truncate(n);
    v
}

/// An artist's most played songs with play counts, most played first (the artist page's "Personal").
/// Entries match by artist id, or by name where the entry has no id.
pub fn artist_top_songs(
    entries: &[Entry],
    server: &ServerId,
    artist_id: &str,
    artist_name: &str,
    limit: usize,
) -> Vec<(Song, u32)> {
    let by = |e: &Entry| {
        e.artists.iter().any(|a| match &a.id {
            Some(id) => id == artist_id,
            None => a.name.eq_ignore_ascii_case(artist_name),
        })
    };
    let mut counts: HashMap<&str, (u32, &Entry)> = HashMap::new();
    for e in entries.iter().filter(|e| &e.server_id == server && by(e)) {
        let slot = counts.entry(&e.song_id).or_insert((0, e));
        slot.0 += 1;
        if e.ts >= slot.1.ts {
            slot.1 = e;
        }
    }
    let mut v: Vec<(u32, &Entry)> = counts.into_values().collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.ts.cmp(&a.1.ts)).then(a.1.title.cmp(&b.1.title)));
    v.into_iter().take(limit).map(|(n, e)| (e.to_song(), n)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixKind {
    /// Songs you usually play around this time of day.
    Daylist,
    /// Most played in the last 30 days.
    HeavyRotation,
    /// Played often before, not at all in the last 60 days.
    Rediscover,
    /// All-time most played.
    AllTimeFavorites,
}

impl MixKind {
    pub const ALL: [Self; 4] = [Self::Daylist, Self::HeavyRotation, Self::Rediscover, Self::AllTimeFavorites];

    pub fn title(self) -> &'static str {
        match self {
            Self::Daylist => "Daylist",
            Self::HeavyRotation => "Heavy rotation",
            Self::Rediscover => "Rediscover",
            Self::AllTimeFavorites => "All-time favourites",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Self::Daylist => "What you usually play at this time of day",
            Self::HeavyRotation => "Your most played songs of the last 30 days",
            Self::Rediscover => "Songs you used to play a lot, not heard for two months",
            Self::AllTimeFavorites => "Your most played songs ever",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Daylist => "daylist",
            Self::HeavyRotation => "heavy",
            Self::Rediscover => "rediscover",
            Self::AllTimeFavorites => "all-time",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
}

/// A mix from the played songs in the history; a song's most recent entry supplies its details.
pub fn mix(entries: &[Entry], kind: MixKind, now: i64, tz: FixedOffset, limit: usize) -> Vec<Entry> {
    let hour = DateTime::from_timestamp(now, 0).map(|d| d.with_timezone(&tz).hour()).unwrap_or(12);
    // Plays per song that count for this mix.
    let mut counts: HashMap<(&ServerId, &str), (u32, &Entry)> = HashMap::new();
    let mut last_play: HashMap<(&ServerId, &str), i64> = HashMap::new();
    for e in entries {
        let l = last_play.entry(e.key()).or_insert(e.ts);
        *l = (*l).max(e.ts);
        let counts_here = match kind {
            MixKind::Daylist => local(e, tz).is_some_and(|d| hour_distance(d.hour(), hour) <= 1),
            MixKind::HeavyRotation => e.ts >= now - 30 * DAY,
            MixKind::Rediscover | MixKind::AllTimeFavorites => true,
        };
        if counts_here {
            let c = counts.entry(e.key()).or_insert((0, e));
            c.0 += 1;
            if e.ts >= c.1.ts {
                c.1 = e;
            }
        }
    }
    let mut v: Vec<(u32, &Entry)> = counts
        .into_iter()
        .filter(|(k, (n, _))| match kind {
            MixKind::Rediscover => *n >= 2 && last_play.get(k).is_some_and(|l| *l < now - 60 * DAY),
            _ => *n >= 1,
        })
        .map(|(_, v)| v)
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.ts.cmp(&a.1.ts)).then(a.1.title.cmp(&b.1.title)));
    v.into_iter().take(limit).map(|(_, e)| e.clone()).collect()
}

fn hour_distance(a: u32, b: u32) -> u32 {
    let d = a.abs_diff(b);
    d.min(24 - d)
}

pub fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

/// This computer's current UTC offset.
pub fn local_offset() -> FixedOffset {
    *chrono::Local::now().offset()
}

/// Current calendar year in `tz`.
pub fn current_year(tz: FixedOffset) -> i32 {
    chrono::Utc::now().with_timezone(&tz).year()
}

/// Sample plays over the last weeks for `--demo`.
pub fn demo_entries(songs: &[Song], now: i64) -> Vec<Entry> {
    if songs.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..120_i64 {
        let song = &songs[(i as usize * 7 + (i as usize % 5)) % songs.len().min(12)];
        let ts = now - (i / 3) * DAY - (i % 3) * 3600 * 5;
        out.push(Entry::new(song, ts));
    }
    out.sort_by_key(|e| e.ts);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, title: &str, artist: &str, album: &str) -> Song {
        let mut s = Entry::new(&dummy(), 0).to_song();
        s.id = id.into();
        s.title = title.into();
        s.artists = vec![ArtistRef { id: None, name: artist.into() }];
        s.album = Some(album.into());
        s.duration = Duration::from_secs(200);
        s
    }

    fn dummy() -> Song {
        Song {
            mbz: Default::default(),
            extra: Default::default(),
            id: String::new(),
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            title: String::new(),
            artists: vec![],
            album_artists: vec![],
            album: None,
            album_id: None,
            image: None,
            duration: Duration::ZERO,
            track_number: None,
            disc_number: None,
            year: None,
            genres: vec![],
            codec: None,
            bit_rate: None,
            sample_rate: None,
            bit_depth: None,
            bpm: None,
            favorite: false,
            rating: None,
            play_count: None,
            gain: Default::default(),
            explicit: false,
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: None,
            comment: None,
        }
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    /// 2025-06-15 00:00:00 UTC.
    const BASE: i64 = 1_749_945_600;

    fn at(day: i64, hour: i64, id: &str) -> Entry {
        Entry::new(&song(id, &format!("T{id}"), &format!("A{id}"), "Alb"), BASE + day * DAY + hour * 3600)
    }

    #[test]
    fn personal_top_songs_of_an_artist() {
        let by_id = |id: &str, artist_id: Option<&str>, artist: &str, day: i64| {
            let mut s = song(id, &format!("T{id}"), artist, "Alb");
            s.artists = vec![ArtistRef { id: artist_id.map(str::to_string), name: artist.into() }];
            Entry::new(&s, BASE + day * DAY)
        };
        let entries = vec![
            by_id("a", Some("ar1"), "Mira", 0),
            by_id("b", Some("ar1"), "Mira", 1),
            by_id("b", Some("ar1"), "Mira", 2),
            by_id("c", Some("ar2"), "Other", 3),
            by_id("d", None, "MIRA", 4),
        ];
        let top = artist_top_songs(&entries, &ServerId("s".into()), "ar1", "Mira", 10);
        let ids: Vec<(&str, u32)> = top.iter().map(|(s, n)| (s.id.as_str(), *n)).collect();
        assert_eq!(ids, [("b", 2), ("d", 1), ("a", 1)]);
        assert_eq!(artist_top_songs(&entries, &ServerId("s".into()), "ar1", "Mira", 1).len(), 1);
        assert!(artist_top_songs(&entries, &ServerId("x".into()), "ar1", "Mira", 5).is_empty());
    }

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("listen-hist-{name}-{}", std::process::id())).join(FILE_NAME)
    }

    #[test]
    fn stores_only_the_documented_fields_and_roundtrips() {
        let path = temp("rt");
        let _ = std::fs::remove_file(&path);
        let mut h = History::load(&path);
        h.add(at(0, 9, "1")).unwrap();
        h.add(at(1, 9, "2")).unwrap();
        let again = History::load(&path);
        assert_eq!(again.entries, h.entries);
        let line = std::fs::read_to_string(&path).unwrap();
        for forbidden in ["genres", "rating", "favorite", "play_count", "bpm"] {
            assert!(!line.contains(forbidden), "{forbidden} must not be stored");
        }
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn remove_clear_and_prune() {
        let path = temp("del");
        let mut h = History::load(&path);
        for d in 0..3 {
            h.add(at(d, 9, "1")).unwrap();
        }
        h.remove(1).unwrap();
        assert_eq!(History::load(&path).entries.len(), 2);
        let now = BASE + 400 * DAY;
        assert_eq!(h.prune(Some(365), now).unwrap(), 2);
        assert!(History::load(&path).is_empty());
        h.add(at(0, 9, "1")).unwrap();
        h.clear().unwrap();
        assert!(!path.exists());
        assert!(History::load(&path).is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn prune_forever_keeps_everything() {
        let mut h = History::in_memory();
        h.add(at(0, 9, "1")).unwrap();
        assert_eq!(h.prune(None, BASE + 9999 * DAY).unwrap(), 0);
        assert_eq!(h.entries.len(), 1);
    }

    #[test]
    fn stats_rank_and_filter_by_year() {
        let mut e = vec![at(0, 9, "1"), at(1, 9, "1"), at(2, 9, "2")];
        e.push(Entry { ts: BASE - 300 * DAY, ..at(0, 0, "3") }); // 2024
        let all = stats(&e, None, utc(), 10);
        assert_eq!(all.plays, 4);
        assert_eq!(all.listening_secs, 800);
        assert_eq!(all.songs[0].name, "T1");
        assert_eq!(all.songs[0].plays, 2);
        assert_eq!(all.years, vec![2025, 2024]);
        let y = stats(&e, Some(2025), utc(), 10);
        assert_eq!(y.plays, 3);
        // Albums are told apart by artist: "Alb" by A1 (2 plays) and by A2 (1 play).
        assert_eq!(y.albums.len(), 2);
        assert_eq!(y.albums[0].plays, 2);
        assert_eq!(stats(&e, Some(2020), utc(), 10).plays, 0);
    }

    #[test]
    fn daylist_uses_the_time_of_day() {
        let e = vec![at(0, 8, "1"), at(1, 9, "1"), at(2, 20, "2")];
        let now = BASE + 10 * DAY + 9 * 3600;
        let m = mix(&e, MixKind::Daylist, now, utc(), 50);
        assert_eq!(m.iter().map(|x| x.song_id.as_str()).collect::<Vec<_>>(), vec!["1"]);
        let evening = mix(&e, MixKind::Daylist, BASE + 10 * DAY + 20 * 3600, utc(), 50);
        assert_eq!(evening[0].song_id, "2");
    }

    #[test]
    fn heavy_rotation_and_rediscover() {
        let now = BASE + 100 * DAY;
        let e = vec![at(0, 9, "old"), at(1, 9, "old"), at(95, 9, "new"), at(96, 9, "new"), at(97, 9, "once")];
        let heavy = mix(&e, MixKind::HeavyRotation, now, utc(), 50);
        assert_eq!(heavy[0].song_id, "new");
        assert!(heavy.iter().all(|x| x.song_id != "old"));
        let re = mix(&e, MixKind::Rediscover, now, utc(), 50);
        assert_eq!(re.iter().map(|x| x.song_id.as_str()).collect::<Vec<_>>(), vec!["old"]);
        let all = mix(&e, MixKind::AllTimeFavorites, now, utc(), 2);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn midnight_wraps_for_the_daylist() {
        assert_eq!(hour_distance(23, 0), 1);
        assert_eq!(hour_distance(1, 23), 2);
    }

    #[test]
    fn csv_escapes_and_neutralises_formulas() {
        let mut h = History::in_memory();
        let mut e = at(0, 9, "1");
        e.title = "=HYPERLINK(\"x\")".into();
        e.artists[0].name = "A, B".into();
        h.add(e).unwrap();
        let csv = h.export_csv();
        assert!(csv.contains("\"'=HYPERLINK(\"\"x\"\")\""));
        assert!(csv.contains("\"A, B\""));
        assert!(h.export_json().contains("\"song_id\""));
    }

    #[test]
    fn to_song_is_playable() {
        let e = at(0, 9, "7");
        let s = e.to_song();
        assert_eq!((s.id.as_str(), s.title.as_str()), ("7", "T7"));
        assert_eq!(s.duration, Duration::from_secs(200));
    }
}
