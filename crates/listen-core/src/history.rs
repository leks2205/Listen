//! Local listening history: a JSON-lines file, sent nowhere unless the user turns on merging with
//! their own listen-server. Statistics and mixes are computed from it on demand, so deleting entries removes everything derived from them.

use crate::model::{ArtistRef, ImageRef, ServerId, Song, SourceKind};
use chrono::{DateTime, Datelike, FixedOffset, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const FILE_NAME: &str = "history.jsonl";
const DAY: i64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayStatus {
    /// Played past the scrobble threshold.
    Played,
    /// Left by the user (next, or another song started by hand) before the scrobble threshold. Never a
    /// play: statistics, mixes, Last.fm and play counts read only [`PlayStatus::Played`]. Automatic
    /// handovers (a DJ mix, the end of the song) are never skips. Apps before 0.41 drop these lines.
    Skipped,
}

/// The play mode the player was in when an entry was recorded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    #[default]
    Off,
    Shuffle,
    AutoDj,
    AiDj,
    Radio,
    Focus,
    Tempo,
    /// A mode written by a newer version.
    #[serde(other)]
    Other,
}

/// Who put a song in the queue (history context and queue reasons).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Picker {
    /// Started or queued by hand (also a playlist or album the user started).
    #[default]
    User,
    AutoDj,
    AiDj,
    Bridge,
    Radio,
    Rediscover,
    Tempo,
    /// Picked for a listener request to the AI DJ ("Tell the DJ").
    Request,
    /// A picker written by a newer version.
    #[serde(other)]
    Other,
}

impl Picker {
    /// Picked by the player rather than the user.
    pub fn automatic(self) -> bool {
        self != Self::User
    }
}

/// The context of a play or skip.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayCtx {
    #[serde(default)]
    pub mode: PlayMode,
    #[serde(default)]
    pub picker: Picker,
    /// The recording device's machine id (handoff's device id); empty when unknown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device: String,
}

/// Reads an optional field written by any version: a value this version cannot read becomes `None`
/// instead of dropping the whole line.
pub(crate) fn lenient<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(v.and_then(|v| serde_json::from_value(v).ok()))
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
    /// Seconds into the song when it ended (skips; plays recorded from 0.41 on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_s: Option<f32>,
    /// `at_s` as a share of the song's length, 0–1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frac: Option<f32>,
    /// Play mode, picker and device at the time.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub ctx: Option<PlayCtx>,
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
            at_s: None,
            frac: None,
            ctx: None,
        }
    }

    /// A skip of `song` at `at_s` seconds (see [`PlayStatus::Skipped`]); `frac` from the song's length.
    pub fn skipped(song: &Song, ts: i64, at_s: f32, ctx: PlayCtx) -> Self {
        let len = song.duration.as_secs_f32();
        Self {
            status: PlayStatus::Skipped,
            at_s: Some(at_s.max(0.0)),
            frac: (len > 0.0).then(|| (at_s / len).clamp(0.0, 1.0)),
            ctx: Some(ctx),
            ..Self::new(song, ts)
        }
    }

    /// Counts as a play (statistics, mixes, Last.fm, play counts). Skips do not.
    pub fn played(&self) -> bool {
        self.status == PlayStatus::Played
    }

    /// Key of the track across servers.
    fn key(&self) -> (&ServerId, &str) {
        (&self.server_id, &self.song_id)
    }

    /// A playable song rebuilt from the stored fields.
    pub fn to_song(&self) -> Song {
        Song {
            artists: self.artists.clone(),
            album: self.album.clone(),
            album_id: self.album_id.clone(),
            image: self.image.clone(),
            duration: Duration::from_secs(u64::from(self.duration_secs)),
            ..Song::new(self.song_id.clone(), self.server_id.clone(), self.source, self.title.clone())
        }
    }
}

/// The history file, loaded into memory (oldest first).
#[derive(Debug, Clone, Default)]
pub struct History {
    pub entries: Vec<Entry>,
    path: Option<PathBuf>,
    /// Well-formed lines this version cannot read (a newer version's status or fields). Every rewrite of
    /// the file puts them back, so an older app never deletes a newer one's plays.
    unknown: Vec<Unknown>,
    /// Damaged lines skipped by `load` (a torn append, a bad byte).
    skipped: usize,
}

/// A line kept verbatim, with the play key `merge` and `prune` need.
#[derive(Debug, Clone)]
struct Unknown {
    ts: i64,
    song_id: String,
    line: String,
}

impl History {
    /// Loads the file. A missing file is an empty history. Each line stands on its own: a damaged one costs
    /// only itself ([`History::skipped_lines`]), and well-formed lines this version cannot read are kept for
    /// the next rewrite. A file that exists but cannot be read gives a history that is never written
    /// ([`History::path`] is `None`), so an empty copy can never replace it.
    pub fn load(path: &Path) -> Self {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                tracing::warn!(
                    "history {} is unreadable, keeping it untouched this run: {e}",
                    path.display()
                );
                return Self::in_memory();
            }
        };
        let mut h = Self { path: Some(path.to_path_buf()), ..Self::default() };
        for line in bytes.split(|&b| b == b'\n').map(<[u8]>::trim_ascii).filter(|l| !l.is_empty()) {
            if let Ok(entry) = serde_json::from_slice::<Entry>(line) {
                h.entries.push(entry);
                continue;
            }
            match serde_json::from_slice::<serde_json::Value>(line) {
                Ok(v @ serde_json::Value::Object(_)) => h.unknown.push(Unknown {
                    ts: v.get("ts").and_then(serde_json::Value::as_i64).unwrap_or(0),
                    song_id: v
                        .get("song_id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    line: String::from_utf8_lossy(line).into_owned(),
                }),
                _ => h.skipped += 1,
            }
        }
        if h.skipped > 0 {
            tracing::warn!("history {}: skipped {} damaged line(s)", path.display(), h.skipped);
        }
        h
    }

    /// How many damaged lines the last [`History::load`] skipped.
    pub fn skipped_lines(&self) -> usize {
        self.skipped
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
            let mut line = serde_json::to_string(&entry).map_err(std::io::Error::other)?;
            line.push('\n');
            let mut f = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path)?;
            // A crash mid-append leaves a line without its newline; gluing this entry onto it would lose both.
            if f.metadata()?.len() > 0 {
                let mut last = [0u8];
                f.seek(SeekFrom::End(-1))?;
                f.read_exact(&mut last)?;
                if last[0] != b'\n' {
                    line.insert(0, '\n');
                }
            }
            // One write per entry, so the line lands whole or not at all as far as the OS allows.
            f.write_all(line.as_bytes())?;
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Removes the entry at `index` of `entries` (memory changes only if the file does). `index` must come
    /// from the current `entries`: `add`, `prune` and `merge` shift them, so code that holds on to a play
    /// (a menu, a view) uses [`History::remove_play`] instead.
    pub fn remove(&mut self, index: usize) -> std::io::Result<()> {
        if index < self.entries.len() {
            let mut left = self.entries.clone();
            left.remove(index);
            self.write(&left, &self.unknown)?;
            self.entries = left;
        }
        Ok(())
    }

    /// Removes the play recorded at `ts` for `song_id` (the key `merge` matches plays by on every device,
    /// which stays right while entries move) and returns it; `None` when it is gone already.
    pub fn remove_play(&mut self, ts: i64, song_id: &str) -> std::io::Result<Option<Entry>> {
        let Some(index) = self.entries.iter().position(|e| e.ts == ts && e.song_id == song_id) else {
            return Ok(None);
        };
        let entry = self.entries[index].clone();
        self.remove(index)?;
        Ok(Some(entry))
    }

    pub fn clear(&mut self) -> std::io::Result<()> {
        if let Some(p) = &self.path {
            match std::fs::remove_file(p) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        self.entries.clear();
        self.unknown.clear();
        Ok(())
    }

    /// The file, `None` for a history that is never written.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Takes in other devices' plays and removals. A play is the same on every device when second and
    /// song match. `entries` end up sorted by time. Returns how many plays came or went.
    pub fn merge(&mut self, incoming: Vec<Entry>, removed: &[(i64, String)]) -> std::io::Result<usize> {
        let same = |e: &Entry| (e.ts, e.song_id.clone());
        let gone: HashSet<&(i64, String)> = removed.iter().collect();
        let mut left: Vec<Entry> =
            self.entries.iter().filter(|e| !gone.contains(&same(e))).cloned().collect();
        let mut changed = self.entries.len() - left.len();
        let unknown: Vec<Unknown> =
            self.unknown.iter().filter(|u| !gone.contains(&(u.ts, u.song_id.clone()))).cloned().collect();
        let unknown_gone = unknown.len() != self.unknown.len();
        let mut seen: HashSet<(i64, String)> = left.iter().map(same).collect();
        for e in incoming {
            let k = same(&e);
            if !gone.contains(&k) && seen.insert(k) {
                left.push(e);
                changed += 1;
            }
        }
        if changed > 0 || unknown_gone {
            left.sort_by_key(|e| e.ts);
            self.write(&left, &unknown)?;
            self.entries = left;
            self.unknown = unknown;
        }
        Ok(changed)
    }

    /// Drops entries older than `max_age_days`. Returns how many were removed.
    pub fn prune(&mut self, max_age_days: Option<i64>, now: i64) -> std::io::Result<usize> {
        let Some(days) = max_age_days else { return Ok(0) };
        let cutoff = now - days * DAY;
        let left: Vec<Entry> = self.entries.iter().filter(|e| e.ts >= cutoff).cloned().collect();
        let unknown: Vec<Unknown> = self.unknown.iter().filter(|u| u.ts >= cutoff).cloned().collect();
        let removed = self.entries.len() - left.len();
        if removed > 0 || unknown.len() != self.unknown.len() {
            self.write(&left, &unknown)?;
            self.entries = left;
            self.unknown = unknown;
        }
        Ok(removed)
    }

    /// Replaces the file atomically (temp file + rename) with `entries` in their order and the kept
    /// unknown lines, each before the first entry that is later than it.
    fn write(&self, entries: &[Entry], unknown: &[Unknown]) -> std::io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        let mut rest: Vec<&Unknown> = unknown.iter().collect();
        rest.sort_by_key(|u| u.ts);
        let mut rest = rest.into_iter().peekable();
        let mut text = String::new();
        for e in entries {
            while let Some(u) = rest.next_if(|u| u.ts <= e.ts) {
                text.push_str(&u.line);
                text.push('\n');
            }
            text.push_str(&serde_json::to_string(e).map_err(std::io::Error::other)?);
            text.push('\n');
        }
        for u in rest {
            text.push_str(&u.line);
            text.push('\n');
        }
        let tmp = path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    /// The latest plays (skips left out), newest first, with their index in `entries` (valid until the
    /// next change; hold `(ts, song_id)` to act on a play later).
    pub fn recent(&self, limit: usize) -> Vec<(usize, &Entry)> {
        self.entries.iter().enumerate().rev().filter(|(_, e)| e.played()).take(limit).collect()
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
                if e.played() { "played" } else { "skipped" }.to_string(),
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
    /// Skips in the same period (shown, never counted as plays).
    pub skips: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub plays: u32,
    pub listening_secs: u64,
    pub skips: u32,
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
        if !e.played() {
            out.skips += 1;
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
    // Skips of the songs, artists and albums that were played in the period.
    for e in entries.iter().filter(|e| !e.played()) {
        if year.is_some() && local(e, tz).map(|d| d.year()) != year {
            continue;
        }
        let first = e.artists.first().map_or("", |a| a.name.as_str());
        if let Some(r) = songs.get_mut(&e.key()) {
            r.skips += 1;
        }
        for a in &e.artists {
            if let Some(r) = artists.get_mut(a.name.as_str()) {
                r.skips += 1;
            }
        }
        if let Some(r) = e.album.as_deref().and_then(|al| albums.get_mut(&(al, first))) {
            r.skips += 1;
        }
    }
    out.songs = top_n(songs.into_values(), top);
    out.artists = top_n(artists.into_values(), top);
    out.albums = top_n(albums.into_values(), top);
    out
}

fn ranked(name: &str, detail: &str) -> Ranked {
    Ranked { name: name.into(), detail: detail.into(), plays: 0, secs: 0, skips: 0 }
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
    for e in entries.iter().filter(|e| e.played() && &e.server_id == server && by(e)) {
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
    /// Played often before, not at all in the last 60 days; most loved and most faded first
    /// ([`crate::memory`]).
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
        // A recent skip still counts as heard (Rediscover leaves it out), never as a play.
        if counts_here && e.played() {
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
    if kind == MixKind::Rediscover {
        // Most loved and most faded first (the memory model's forgotten-favourite score).
        let memory = crate::memory::Memory::new(entries, now);
        let score = |e: &Entry| {
            memory.get(&e.to_song()).map_or(f64::NEG_INFINITY, crate::memory::SongMemory::forgotten_score)
        };
        v.sort_by(|a, b| score(b.1).total_cmp(&score(a.1)));
    }
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
        Song::new(String::new(), ServerId("s".into()), SourceKind::Subsonic, String::new())
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
    fn merge_adds_new_plays_drops_removed_ones_and_sorts() {
        let path = temp("merge");
        let _ = std::fs::remove_file(&path);
        let mut h = History::load(&path);
        let (a, b, c) = (at(0, 9, "1"), at(2, 9, "2"), at(1, 9, "3"));
        h.add(a.clone()).unwrap();
        h.add(b.clone()).unwrap();
        // The same play from another device has another server id.
        let mut twin = a.clone();
        twin.server_id = ServerId("other".into());
        let removed = [(b.ts, b.song_id.clone())];
        let mut gone_too = b.clone();
        gone_too.server_id = ServerId("other".into());
        assert_eq!(h.merge(vec![twin, c.clone(), gone_too], &removed).unwrap(), 2);
        assert_eq!(h.entries, [a.clone(), c.clone()]);
        assert_eq!(History::load(&path).entries, [a, c]);
        assert_eq!(h.merge(Vec::new(), &removed).unwrap(), 0);
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
    fn a_torn_last_line_costs_only_itself() {
        let path = temp("torn");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut korean = at(1, 9, "2");
        korean.title = "한국어 노래".into();
        let whole = serde_json::to_string(&at(0, 9, "1")).unwrap();
        let torn = serde_json::to_vec(&korean).unwrap();
        // Cut inside the first Hangul syllable: the file is no longer valid UTF-8.
        let cut = torn.windows(3).position(|w| w == "한".as_bytes()).unwrap() + 1;
        let mut bytes = format!("{whole}\n").into_bytes();
        bytes.extend_from_slice(&torn[..cut]);
        std::fs::write(&path, bytes).unwrap();
        let mut h = History::load(&path);
        assert_eq!((h.entries.len(), h.skipped_lines()), (1, 1));
        h.add(at(2, 9, "3")).unwrap();
        let again = History::load(&path);
        assert_eq!(again.entries.len(), 2, "the next append starts on a fresh line");
        assert_eq!(again.skipped_lines(), 1);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_unreadable_file_is_never_overwritten() {
        // A directory where the file should be cannot be read as a file.
        let path = temp("unreadable");
        std::fs::create_dir_all(&path).unwrap();
        let mut h = History::load(&path);
        assert_eq!(h.path(), None);
        h.add(at(0, 9, "1")).unwrap();
        assert_eq!(h.merge(vec![at(1, 9, "2")], &[]).unwrap(), 1);
        assert!(path.is_dir(), "nothing was written over it");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn lines_this_version_cannot_read_survive_a_prune() {
        let path = temp("unknown");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = serde_json::to_string(&at(0, 9, "1")).unwrap();
        let newer = serde_json::to_string(&at(300, 9, "2")).unwrap().replace("\"played\"", "\"paused\"");
        let kept = serde_json::to_string(&at(301, 9, "3")).unwrap();
        std::fs::write(&path, format!("{old}\n{newer}\n{kept}\n")).unwrap();
        let mut h = History::load(&path);
        assert_eq!((h.entries.len(), h.skipped_lines()), (2, 0));
        assert_eq!(h.prune(Some(365), BASE + 400 * DAY).unwrap(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{newer}\n{kept}\n"));
        // A removal from another device reaches it too.
        h.merge(Vec::new(), &[(BASE + 300 * DAY + 9 * 3600, "2".into())]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{kept}\n"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn removing_a_play_by_key_survives_a_prune_in_between() {
        let path = temp("key");
        let _ = std::fs::remove_file(&path);
        let mut h = History::load(&path);
        for d in [0, 100, 200] {
            h.add(at(d, 9, &d.to_string())).unwrap();
        }
        // A view picks the newest play, then a prune drops the oldest and moves every index.
        let (index, picked) = h.recent(1)[0];
        let key = (picked.ts, picked.song_id.clone());
        assert_eq!(index, 2);
        assert_eq!(h.prune(Some(365), BASE + 400 * DAY).unwrap(), 1);
        assert_eq!(h.remove_play(key.0, &key.1).unwrap().map(|e| e.song_id), Some("200".into()));
        assert_eq!(
            History::load(&path).entries.iter().map(|e| e.song_id.as_str()).collect::<Vec<_>>(),
            ["100"]
        );
        assert_eq!(h.remove_play(key.0, &key.1).unwrap(), None, "a play already gone removes nothing");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
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

    #[test]
    fn old_lines_load_and_new_fields_roundtrip() {
        // A line written by 0.40: no at_s/frac/ctx.
        let old = r#"{"ts":1700000000,"server_id":"s","source":"subsonic","song_id":"1","status":"played","title":"T","artists":[{"id":null,"name":"A"}],"album":null,"album_id":null,"image":null,"duration_secs":200}"#;
        let e: Entry = serde_json::from_str(old).unwrap();
        assert!(e.played() && e.at_s.is_none() && e.ctx.is_none());
        // Unchanged entries are written exactly as before.
        assert_eq!(serde_json::to_string(&e).unwrap(), old);

        let ctx = PlayCtx { mode: PlayMode::AiDj, picker: Picker::AiDj, device: "pc".into() };
        let s = Entry::skipped(&e.to_song(), 1_700_000_100, 10.0, ctx.clone());
        assert_eq!((s.status, s.at_s, s.frac), (PlayStatus::Skipped, Some(10.0), Some(0.05)));
        let line = serde_json::to_string(&s).unwrap();
        assert!(line.contains(r#""status":"skipped""#) && line.contains(r#""mode":"ai_dj""#), "{line}");
        assert_eq!(serde_json::from_str::<Entry>(&line).unwrap(), s);

        // A newer version's unknown mode or picker still loads; a broken ctx is dropped, not the line.
        let newer = line.replace("ai_dj", "moon_mode");
        let n: Entry = serde_json::from_str(&newer).unwrap();
        assert_eq!(n.ctx.as_ref().map(|c| (c.mode, c.picker)), Some((PlayMode::Other, Picker::Other)));
        let broken = line.replace(r#""device":"pc""#, r#""device":7"#);
        let b: Entry = serde_json::from_str(&broken).unwrap();
        assert_eq!((b.ctx, b.at_s), (None, Some(10.0)));
    }

    #[test]
    fn skips_are_never_plays() {
        let mut entries: Vec<Entry> = (0..3).map(|d| at(d, 9, "1")).collect();
        let song = entries[0].to_song();
        for d in 3..10 {
            entries.push(Entry::skipped(&song, BASE + d * DAY, 5.0, PlayCtx::default()));
            entries.push(Entry::skipped(
                &at(0, 0, "2").to_song(),
                BASE + d * DAY + 1,
                5.0,
                PlayCtx::default(),
            ));
        }
        let st = stats(&entries, None, utc(), 10);
        assert_eq!((st.plays, st.skips), (3, 14));
        assert_eq!((st.songs.len(), st.songs[0].plays, st.songs[0].skips), (1, 3, 7));
        assert_eq!(st.artists[0].skips, 7);
        assert!(!st.songs.iter().any(|r| r.name == "T2"));
        let all = mix(&entries, MixKind::AllTimeFavorites, BASE + 20 * DAY, utc(), 10);
        assert_eq!(all.iter().map(|e| e.song_id.as_str()).collect::<Vec<_>>(), ["1"]);
        assert!(all[0].played());
        let top = artist_top_songs(&entries, &song.server_id, "", "A1", 10);
        assert_eq!(top.iter().map(|(_, n)| *n).collect::<Vec<_>>(), [3]);
        // A skip is heard: the song is not "not heard for two months".
        let mut old = vec![at(0, 9, "3"), at(1, 9, "3")];
        assert_eq!(mix(&old, MixKind::Rediscover, BASE + 90 * DAY, utc(), 10).len(), 1);
        old.push(Entry::skipped(&old[0].to_song(), BASE + 85 * DAY, 3.0, PlayCtx::default()));
        assert!(mix(&old, MixKind::Rediscover, BASE + 90 * DAY, utc(), 10).is_empty());
        let mut h = History::in_memory();
        h.entries = old;
        assert!(h.export_csv().contains(",skipped,"));
        assert_eq!(h.recent(10).iter().map(|(i, _)| *i).collect::<Vec<_>>(), [1, 0], "skips are not listed");
    }
}
