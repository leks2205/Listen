//! Songs kept out of automatic picks: snoozed for a while, "Not interested" for good, or hidden in one
//! playlist. Stored in `exclusions.json` and synced between devices through listen-server
//! (`/v1/exclusions`, merged by timestamp). Songs are keyed by [`song_key`], so every version of a song
//! (single, album track, video) is covered and the key is the same on every device.
//!
//! [`Exclusions::allows`] is the one gate every automatic pool calls: Auto-DJ, the AI DJ pool, radio
//! fills, bridge candidates, Rediscover, tempo and focus fills.

use crate::model::Song;
use crate::versions::song_key;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const FILE_NAME: &str = "exclusions.json";
/// How long "Snooze" keeps a song out.
pub const SNOOZE_DAYS: i64 = 30;
const DAY: i64 = 86_400;

/// One flag with the time it was last changed (Unix seconds). Clearing a flag keeps it with `on: false`
/// so the clear wins a merge against an older set on another device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub at: i64,
    #[serde(default = "yes")]
    pub on: bool,
}

fn yes() -> bool {
    true
}

/// A snooze until `until` (Unix seconds), set or cleared at `at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snooze {
    pub until: i64,
    pub at: i64,
}

/// Where a song is about to be played from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx<'a> {
    /// An automatic pick (Auto-DJ, AI DJ, radio, bridge, Rediscover, tempo, focus) at Unix time `now`.
    Auto { now: i64 },
    /// Playback of the playlist with this id (the "Hide in this playlist" scope).
    Playlist { id: &'a str },
    /// The user picked the song by hand: always allowed.
    User,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exclusions {
    /// Song key → snooze.
    #[serde(default)]
    pub snoozed: BTreeMap<String, Snooze>,
    /// Song key → "Not interested" (reversible in Settings → Hidden songs).
    #[serde(default)]
    pub not_interested: BTreeMap<String, Mark>,
    /// Playlist id → song key → hidden in that playlist.
    #[serde(default)]
    pub hidden_in: BTreeMap<String, BTreeMap<String, Mark>>,
}

impl Exclusions {
    /// Loads the file; a missing or broken file is empty.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// Writes the file (atomically, through a temporary file next to it).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }

    /// May `song` play in `ctx`?
    pub fn allows(&self, song: &Song, ctx: Ctx) -> bool {
        self.allows_key(&song_key(song), ctx)
    }

    /// [`Self::allows`] for a precomputed [`song_key`].
    pub fn allows_key(&self, key: &str, ctx: Ctx) -> bool {
        match ctx {
            Ctx::User => true,
            Ctx::Playlist { id } => !self.hidden_in.get(id).and_then(|m| m.get(key)).is_some_and(|m| m.on),
            Ctx::Auto { now } => self.snoozed_until(key, now).is_none() && !self.is_not_interested(key),
        }
    }

    /// The end of a running snooze of the song `key`.
    pub fn snoozed_until(&self, key: &str, now: i64) -> Option<i64> {
        self.snoozed.get(key).map(|s| s.until).filter(|&u| u > now)
    }

    pub fn is_not_interested(&self, key: &str) -> bool {
        self.not_interested.get(key).is_some_and(|m| m.on)
    }

    pub fn is_hidden_in(&self, playlist: &str, key: &str) -> bool {
        !self.allows_key(key, Ctx::Playlist { id: playlist })
    }

    /// Song keys automatic picks leave out at `now` (running snoozes and "Not interested").
    pub fn blocked_keys(&self, now: i64) -> impl Iterator<Item = &String> {
        let snoozed = self.snoozed.iter().filter(move |(_, s)| s.until > now).map(|(k, _)| k);
        snoozed.chain(self.not_interested.iter().filter(|(_, m)| m.on).map(|(k, _)| k))
    }

    /// Song keys hidden in the playlist `id`.
    pub fn hidden_keys(&self, id: &str) -> impl Iterator<Item = &String> {
        self.hidden_in.get(id).into_iter().flatten().filter(|(_, m)| m.on).map(|(k, _)| k)
    }

    /// Whether anything is set (a running snooze, "Not interested" or a hidden song) at `now`.
    pub fn any_set(&self, now: i64) -> bool {
        self.blocked_keys(now).next().is_some() || self.hidden_in.values().flatten().any(|(_, m)| m.on)
    }

    /// Snoozes `song` for [`SNOOZE_DAYS`] from `now`.
    pub fn snooze(&mut self, song: &Song, now: i64) {
        let key = song_key(song);
        let at = stamp(self.snoozed.get(&key).map(|s| s.at), now);
        self.snoozed.insert(key, Snooze { until: now + SNOOZE_DAYS * DAY, at });
    }

    /// Ends a snooze early (kept as an expired snooze so the end syncs).
    pub fn unsnooze(&mut self, key: &str, now: i64) {
        if let Some(s) = self.snoozed.get_mut(key) {
            *s = Snooze { until: now, at: stamp(Some(s.at), now) };
        }
    }

    pub fn set_not_interested(&mut self, key: &str, on: bool, now: i64) {
        let at = stamp(self.not_interested.get(key).map(|m| m.at), now);
        self.not_interested.insert(key.to_string(), Mark { at, on });
    }

    pub fn set_hidden_in(&mut self, playlist: &str, key: &str, on: bool, now: i64) {
        let marks = self.hidden_in.entry(playlist.to_string()).or_default();
        let at = stamp(marks.get(key).map(|m| m.at), now);
        marks.insert(key.to_string(), Mark { at, on });
    }

    /// Takes in another device's state: per song (and playlist), the later change wins; on a tie, set
    /// beats cleared. Returns whether anything changed.
    pub fn merge(&mut self, other: &Exclusions) -> bool {
        let mut changed = false;
        for (k, s) in &other.snoozed {
            match self.snoozed.get(k) {
                Some(mine) if (mine.at, mine.until) >= (s.at, s.until) => {}
                _ => {
                    self.snoozed.insert(k.clone(), *s);
                    changed = true;
                }
            }
        }
        changed |= merge_marks(&mut self.not_interested, &other.not_interested);
        for (p, marks) in &other.hidden_in {
            changed |= merge_marks(self.hidden_in.entry(p.clone()).or_default(), marks);
        }
        changed
    }

    /// Drops snoozes that ended and cleared marks older than `keep_days` (long enough for every device
    /// to have synced the clear).
    pub fn prune(&mut self, now: i64, keep_days: i64) {
        let cutoff = now - keep_days * DAY;
        self.snoozed.retain(|_, s| s.until > now || s.at >= cutoff);
        self.not_interested.retain(|_, m| m.on || m.at >= cutoff);
        for marks in self.hidden_in.values_mut() {
            marks.retain(|_, m| m.on || m.at >= cutoff);
        }
        self.hidden_in.retain(|_, m| !m.is_empty());
    }
}

/// The time of a change on this device: after the change it replaces, even within the same second (an
/// Undo right after Snooze must win the next merge against the server's copy of the snooze).
fn stamp(prev: Option<i64>, now: i64) -> i64 {
    prev.map_or(now, |p| now.max(p + 1))
}

fn merge_marks(mine: &mut BTreeMap<String, Mark>, theirs: &BTreeMap<String, Mark>) -> bool {
    let mut changed = false;
    for (k, m) in theirs {
        match mine.get(k) {
            Some(cur) if (cur.at, cur.on) >= (m.at, m.on) => {}
            _ => {
                mine.insert(k.clone(), *m);
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Entry;
    use crate::model::{ArtistRef, ServerId, SourceKind};

    fn song(title: &str, artist: &str) -> Song {
        let mut s = Entry {
            ts: 0,
            server_id: ServerId("s".into()),
            source: SourceKind::YouTubeMusic,
            song_id: title.into(),
            status: crate::history::PlayStatus::Played,
            title: title.into(),
            artists: vec![ArtistRef { id: None, name: artist.into() }],
            album: None,
            album_id: None,
            image: None,
            duration_secs: 200,
            at_s: None,
            frac: None,
            ctx: None,
        }
        .to_song();
        s.title = title.into();
        s
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn snooze_keeps_out_of_automatic_picks_for_30_days_only() {
        let a = song("Song", "A");
        let mut x = Exclusions::default();
        x.snooze(&a, NOW);
        assert!(!x.allows(&a, Ctx::Auto { now: NOW + 29 * DAY }));
        assert!(x.allows(&a, Ctx::Auto { now: NOW + 30 * DAY }));
        assert!(x.allows(&a, Ctx::User));
        assert!(x.allows(&a, Ctx::Playlist { id: "p" }));
        // Every version of the song is covered.
        assert!(!x.allows(&song("Song (Radio Edit)", "A"), Ctx::Auto { now: NOW }));
        assert!(x.allows(&song("Other", "A"), Ctx::Auto { now: NOW }));
        x.unsnooze(&song_key(&a), NOW + DAY);
        assert!(x.allows(&a, Ctx::Auto { now: NOW + DAY }));
    }

    #[test]
    fn not_interested_and_hidden_in_one_playlist() {
        let a = song("Song", "A");
        let k = song_key(&a);
        let mut x = Exclusions::default();
        x.set_not_interested(&k, true, NOW);
        assert!(!x.allows(&a, Ctx::Auto { now: NOW + 9999 * DAY }));
        assert!(x.allows(&a, Ctx::User) && x.allows(&a, Ctx::Playlist { id: "p" }));
        x.set_not_interested(&k, false, NOW + 1);
        assert!(x.allows(&a, Ctx::Auto { now: NOW }));

        x.set_hidden_in("p", &k, true, NOW);
        assert!(!x.allows(&a, Ctx::Playlist { id: "p" }));
        assert!(x.allows(&a, Ctx::Playlist { id: "q" }) && x.allows(&a, Ctx::Auto { now: NOW }));
        assert!(x.is_hidden_in("p", &k));
        assert_eq!(x.hidden_keys("p").collect::<Vec<_>>(), [&k]);
        assert_eq!(x.hidden_keys("q").count(), 0);
    }

    #[test]
    fn blocked_keys_are_running_snoozes_and_not_interested() {
        let mut x = Exclusions::default();
        assert!(!x.any_set(NOW));
        x.snooze(&song("S", "A"), NOW);
        x.set_not_interested("n|a", true, NOW);
        x.set_not_interested("c|a", false, NOW);
        let keys: std::collections::BTreeSet<_> = x.blocked_keys(NOW).cloned().collect();
        assert_eq!(keys, [song_key(&song("S", "A")), "n|a".to_string()].into());
        assert_eq!(x.blocked_keys(NOW + 31 * DAY).count(), 1, "the snooze ran out");
        assert!(x.any_set(NOW));
    }

    #[test]
    fn undo_in_the_same_second_wins_the_next_merge() {
        let song = song("Song", "Band");
        let key = song_key(&song);
        let mut x = Exclusions::default();
        x.snooze(&song, NOW);
        x.set_not_interested(&key, true, NOW);
        x.set_hidden_in("pl", &key, true, NOW);
        let server = x.clone();
        x.unsnooze(&key, NOW);
        x.set_not_interested(&key, false, NOW);
        x.set_hidden_in("pl", &key, false, NOW);
        x.merge(&server);
        assert_eq!(x.blocked_keys(NOW).count(), 0);
        assert!(!x.is_hidden_in("pl", &key));
        // And the server takes the undo from this device.
        let mut s = server;
        s.merge(&x);
        assert_eq!(s.blocked_keys(NOW).count(), 0);
        assert!(!s.is_hidden_in("pl", &key));
    }

    #[test]
    fn merge_takes_the_later_change_per_song() {
        let k = "song|a";
        let mut phone = Exclusions::default();
        let mut pc = Exclusions::default();
        pc.set_not_interested(k, true, NOW);
        phone.set_not_interested(k, false, NOW + 5);
        phone.set_hidden_in("p", k, true, NOW);
        pc.snoozed.insert("x|y".into(), Snooze { until: NOW + 30 * DAY, at: NOW });

        let mut a = pc.clone();
        assert!(a.merge(&phone));
        let mut b = phone.clone();
        b.merge(&pc);
        assert_eq!(a, b, "merge is order independent");
        assert!(!a.is_not_interested(k), "the later clear wins");
        assert!(a.is_hidden_in("p", k));
        assert!(a.snoozed_until("x|y", NOW).is_some());
        assert!(!a.clone().merge(&b), "merging the same state changes nothing");
    }

    #[test]
    fn file_roundtrip_old_files_and_prune() {
        let dir = std::env::temp_dir().join(format!("listen-excl-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        assert_eq!(Exclusions::load(&path), Exclusions::default());
        let mut x = Exclusions::default();
        x.snooze(&song("S", "A"), NOW);
        x.set_not_interested("n|a", true, NOW);
        x.set_not_interested("c|a", false, NOW);
        x.set_hidden_in("p", "h|a", true, NOW);
        x.save(&path).unwrap();
        assert_eq!(Exclusions::load(&path), x);
        // Missing sections and a mark without `on` load.
        let partial: Exclusions = serde_json::from_str(r#"{"not_interested":{"k":{"at":1}}}"#).unwrap();
        assert!(partial.is_not_interested("k") && partial.snoozed.is_empty());

        x.prune(NOW + 400 * DAY, 90);
        assert!(x.snoozed.is_empty() && !x.not_interested.contains_key("c|a"));
        assert!(x.is_not_interested("n|a") && x.is_hidden_in("p", "h|a"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
