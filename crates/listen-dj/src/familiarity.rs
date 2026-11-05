//! What the listener already knows, from the listening history: how often each song played, how long
//! ago, and whether a song (or its artist) is new to them. The AI DJ gets this per candidate so it can
//! mix favourites with new songs and introduce the new ones; the history also adds a few candidates of
//! its own (heavy rotation, songs not heard for a while).

use listen_core::history::{self, Entry, MixKind, PlayStatus};
use listen_core::model::Song;
use listen_core::versions::song_key;
use std::collections::{HashMap, HashSet};

const DAY: i64 = 86_400;
/// "New to you" is only said with a history this long (plays and days): a short history can't tell
/// a new song from one the listener played elsewhere.
const MIN_PLAYS: usize = 100;
const MIN_DAYS: i64 = 14;

/// What the listener knows about one song.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Familiarity {
    /// Plays of any version of the song.
    pub plays: u32,
    /// Days since the last play (0 = today); `None` = never played.
    pub last_played_days: Option<u32>,
    /// Never played, and the history is long enough to say so.
    pub new_to_you: bool,
    /// New to you, and no song of its main artist played either.
    pub new_artist: bool,
}

impl Familiarity {
    /// The tag in the AI DJ prompt ("new to you", "plays 12, last played 3 days ago"); `None` when
    /// nothing is known.
    pub fn tag(&self) -> Option<String> {
        if self.new_to_you {
            return Some(if self.new_artist { "new to you, new artist".into() } else { "new to you".into() });
        }
        let days = self.last_played_days?;
        if self.plays == 0 {
            return None;
        }
        let when = match days {
            0 => "today".to_string(),
            1 => "yesterday".to_string(),
            d if d < 60 => format!("{d} days ago"),
            d => format!("not heard for {} months", d / 30),
        };
        let fav = if self.plays >= 10 { "favourite, " } else { "" };
        Some(if days >= 60 {
            format!("{fav}plays {}, {when}", self.plays)
        } else {
            format!("{fav}plays {}, last played {when}", self.plays)
        })
    }
}

/// Plays per song (all versions together) and the artists played, from the history's `Played` entries.
#[derive(Debug, Clone, Default)]
pub struct Listening {
    /// song key → (plays, last play in unix seconds)
    songs: HashMap<String, (u32, i64)>,
    artists: HashSet<String>,
    now: i64,
    /// The history is long enough to call a song new.
    knows_new: bool,
}

/// The artist half of [`song_key`].
fn artist_of(key: &str) -> &str {
    key.rsplit('|').next().unwrap_or(key)
}

impl Listening {
    pub fn new(entries: &[Entry], now: i64) -> Self {
        let mut songs: HashMap<String, (u32, i64)> = HashMap::new();
        let mut artists = HashSet::new();
        let (mut plays, mut first) = (0usize, i64::MAX);
        for e in entries.iter().filter(|e| e.status == PlayStatus::Played && e.ts <= now) {
            let key = song_key(&e.to_song());
            artists.insert(artist_of(&key).to_string());
            let s = songs.entry(key).or_insert((0, e.ts));
            s.0 += 1;
            s.1 = s.1.max(e.ts);
            plays += 1;
            first = first.min(e.ts);
        }
        let knows_new = plays >= MIN_PLAYS && now - first >= MIN_DAYS * DAY;
        Self { songs, artists, now, knows_new }
    }

    /// Whether "new to you" is said at all.
    pub fn knows_new(&self) -> bool {
        self.knows_new
    }

    pub fn of(&self, song: &Song) -> Familiarity {
        let key = song_key(song);
        match self.songs.get(&key) {
            Some(&(plays, last)) => Familiarity {
                plays,
                last_played_days: Some(u32::try_from((self.now - last).max(0) / DAY).unwrap_or(u32::MAX)),
                new_to_you: false,
                new_artist: false,
            },
            None => Familiarity {
                plays: 0,
                last_played_days: None,
                new_to_you: self.knows_new,
                new_artist: self.knows_new && !self.artists.contains(artist_of(&key)),
            },
        }
    }

    /// Tags for these songs by id (only songs with something to say).
    pub fn tags<'a>(&self, songs: impl IntoIterator<Item = &'a Song>) -> HashMap<String, String> {
        songs.into_iter().filter_map(|s| self.of(s).tag().map(|t| (s.id.clone(), t))).collect()
    }
}

/// Songs from the history the AI DJ may add to its pool: the heavy rotation of the last 30 days (not
/// played today) and favourites not heard for two months, most played first. The caller keeps those
/// that fit the moment.
pub fn history_candidates(entries: &[Entry], now: i64, limit: usize) -> (Vec<Song>, Vec<Song>) {
    let played: Vec<Entry> = entries.iter().filter(|e| e.status == PlayStatus::Played).cloned().collect();
    let tz = history::local_offset();
    let heavy = history::mix(&played, MixKind::HeavyRotation, now, tz, limit * 2);
    let mut last: HashMap<String, i64> = HashMap::new();
    for e in &played {
        let l = last.entry(song_key(&e.to_song())).or_insert(e.ts);
        *l = (*l).max(e.ts);
    }
    let rested = |s: &Song| last.get(&song_key(s)).is_none_or(|t| now - t >= DAY);
    let heavy: Vec<Song> = heavy.iter().map(Entry::to_song).filter(rested).take(limit).collect();
    let rediscover =
        history::mix(&played, MixKind::Rediscover, now, tz, limit).iter().map(Entry::to_song).collect();
    (heavy, rediscover)
}

/// Up to `n` history songs for the pool, each bucket already best-first: about three of five from the
/// heavy rotation, the rest forgotten favourites; a short bucket leaves its places to the other.
pub fn mix_buckets(heavy: Vec<Song>, rediscover: Vec<Song>, n: usize) -> Vec<Song> {
    let want_old = (n * 2 / 5).min(rediscover.len());
    let take_heavy = (n - want_old).min(heavy.len());
    let take_old = (n - take_heavy).min(rediscover.len());
    let mut out: Vec<Song> = heavy.into_iter().take(take_heavy).collect();
    let keys: HashSet<String> = out.iter().map(song_key).collect();
    out.extend(rediscover.into_iter().filter(|s| !keys.contains(&song_key(s))).take(take_old));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::{ArtistRef, ServerId, SourceKind};

    fn entry(title: &str, artist: &str, ts: i64) -> Entry {
        Entry {
            ts,
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            song_id: format!("{title}-{artist}"),
            status: PlayStatus::Played,
            title: title.into(),
            artists: vec![ArtistRef { id: None, name: artist.into() }],
            album: None,
            album_id: None,
            image: None,
            duration_secs: 200,
        }
    }

    const NOW: i64 = 1_800_000_000;

    /// 120 plays over 40 days of filler songs, plus the given entries.
    fn long_history(extra: Vec<Entry>) -> Vec<Entry> {
        let mut v: Vec<Entry> =
            (0..120).map(|i| entry(&format!("Filler {i}"), "Somebody", NOW - 40 * DAY + i * 3600)).collect();
        v.extend(extra);
        v
    }

    #[test]
    fn plays_and_days_across_versions() {
        let h = long_history(vec![
            entry("Teardrop", "Massive Attack", NOW - 3 * DAY),
            entry("Teardrop (Remastered 2019)", "Massive Attack", NOW - 10 * DAY),
        ]);
        let l = Listening::new(&h, NOW);
        assert!(l.knows_new());
        let f = l.of(&entry("Teardrop - Radio Edit", "Massive Attack", 0).to_song());
        assert_eq!(
            f,
            Familiarity { plays: 2, last_played_days: Some(3), new_to_you: false, new_artist: false }
        );
        assert_eq!(f.tag().unwrap(), "plays 2, last played 3 days ago");
        let angel = l.of(&entry("Angel", "Massive Attack", 0).to_song());
        assert!(angel.new_to_you && !angel.new_artist);
        assert_eq!(angel.tag().unwrap(), "new to you");
        let new = l.of(&entry("Roads", "Portishead", 0).to_song());
        assert_eq!(new.tag().unwrap(), "new to you, new artist");
    }

    #[test]
    fn favourites_and_long_gaps() {
        let mut extra: Vec<Entry> =
            (0..12).map(|i| entry("Glory Box", "Portishead", NOW - 30 * DAY - i)).collect();
        extra.push(entry("Today", "X", NOW - 100));
        let l = Listening::new(&long_history(extra), NOW);
        assert_eq!(
            l.of(&entry("Glory Box", "Portishead", 0).to_song()).tag().unwrap(),
            "favourite, plays 12, last played 30 days ago"
        );
        assert_eq!(l.of(&entry("Today", "X", 0).to_song()).tag().unwrap(), "plays 1, last played today");
        let f = Familiarity { plays: 3, last_played_days: Some(95), ..Default::default() };
        assert_eq!(f.tag().unwrap(), "plays 3, not heard for 3 months");
    }

    #[test]
    fn short_history_never_says_new() {
        let h = vec![entry("Teardrop", "Massive Attack", NOW - DAY)];
        let l = Listening::new(&h, NOW);
        assert!(!l.knows_new());
        assert_eq!(l.of(&entry("Roads", "Portishead", 0).to_song()).tag(), None);
        assert_eq!(l.tags([&entry("Teardrop", "Massive Attack", 0).to_song()]).len(), 1);
    }

    #[test]
    fn history_buckets() {
        let mut extra: Vec<Entry> = (0..5).map(|i| entry("Hot", "A", NOW - 2 * DAY - i)).collect();
        extra.extend((0..6).map(|i| entry("Today", "B", NOW - 3600 - i)));
        extra.extend((0..3).map(|i| entry("Old", "C", NOW - 100 * DAY - i)));
        let (heavy, rediscover) = history_candidates(&long_history(extra), NOW, 5);
        // Played today is rested; the fillers are older than 30 days.
        assert_eq!(heavy.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Hot"]);
        assert_eq!(rediscover.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Old"]);
        let titles = |v: Vec<Song>| v.into_iter().map(|s| s.title).collect::<Vec<_>>();
        let songs =
            |p: &str, n: usize| (0..n).map(|i| entry(&format!("{p}{i}"), p, 0).to_song()).collect::<Vec<_>>();
        assert_eq!(titles(mix_buckets(songs("H", 9), songs("R", 9), 5)), ["H0", "H1", "H2", "R0", "R1"]);
        assert_eq!(titles(mix_buckets(songs("H", 9), vec![], 5)).len(), 5);
        assert_eq!(titles(mix_buckets(songs("H", 1), songs("R", 9), 5)), ["H0", "R0", "R1", "R2", "R3"]);
        assert!(mix_buckets(vec![], vec![], 5).is_empty());
    }
}
