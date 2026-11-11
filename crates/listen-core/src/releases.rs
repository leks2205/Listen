//! New releases from the artists you follow and play most (the release radar). listen-server checks
//! the artist pages once a day and keeps a [`Radar`]; the apps show its [`Radar::feed`] as the Home
//! shelf "New from your artists". YouTube Music's artist pages only give a year per release, so a new
//! release is one whose id was not on the page the last time; deluxe editions, repackages and other
//! versions of a release already known are not new.

use crate::history::{Entry, PlayStatus};
use crate::model::SourceKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A release stays in the feed this long after it was found.
pub const FEED_DAYS: i64 = 60;
/// Each artist page is read again after this long (a daily job with some slack).
pub const RECHECK_SECS: i64 = 20 * 3600;
/// Artists from the history: this many with the most plays.
pub const TOP_HISTORY: usize = 50;
/// Plays older than this do not count for the top artists.
pub const HISTORY_DAYS: i64 = 365;
/// The artists with the most plays whose releases may notify (decision D7).
pub const NOTIFY_TOP: usize = 10;

/// An artist the radar watches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watch {
    /// YouTube Music channel id (`UC…`); `None` until a name from the history was looked up.
    pub id: Option<String>,
    pub name: String,
    /// Plays in the last [`HISTORY_DAYS`].
    pub plays: u32,
    pub subscribed: bool,
    /// One of the [`NOTIFY_TOP`] most played artists.
    pub top: bool,
}

/// A release as an artist page lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    pub id: String,
    pub title: String,
    /// `album` | `single`.
    pub kind: String,
    pub year: Option<i32>,
    pub image: Option<String>,
}

/// A release found new.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// The source's album id (YouTube Music `MPREb_…`).
    pub id: String,
    pub title: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub artist: String,
    pub artist_id: String,
    /// Unix seconds when the radar first saw it.
    pub found: i64,
    /// By one of the most played artists (the phone may notify).
    #[serde(default)]
    pub top: bool,
}

/// What the radar keeps between runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Radar {
    /// Release ids seen on each artist's page (channel id → ids).
    pub known: BTreeMap<String, BTreeSet<String>>,
    /// [`release_key`]s of each artist's releases, so a new version of a known release is not new.
    pub keys: BTreeMap<String, BTreeSet<String>>,
    /// When each artist page was last read.
    pub checked: BTreeMap<String, i64>,
    /// History artist names (folded) looked up on YouTube Music: channel id, or `None` when nothing
    /// matched, with the time of the lookup.
    pub lookups: BTreeMap<String, (Option<String>, i64)>,
    pub releases: Vec<Release>,
    /// The end of the last run.
    pub updated: i64,
    /// The subscribed artists (channel id, name) as last read, and when.
    pub subscribed: Vec<(String, String)>,
    pub subscribed_at: i64,
    /// Requests made on day `.0` (Unix days), to keep the job within its daily budget.
    pub spent: (i64, u32),
}

/// The same key for every edition of a release: the title without bracketed parts, dashed extras and
/// edition words, folded to letters and digits.
pub fn release_key(title: &str) -> String {
    let lower = title.to_lowercase();
    let mut base = String::new();
    let mut depth = 0usize;
    for c in lower.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => base.push(c),
            _ => {}
        }
    }
    for cut in [" - ", " – ", " — "] {
        if let Some(i) = base.find(cut) {
            base.truncate(i);
        }
    }
    const EDITION: [&str; 14] = [
        "deluxe",
        "repackage",
        "special",
        "expanded",
        "edition",
        "remastered",
        "remaster",
        "anniversary",
        "version",
        "ver.",
        "japanese",
        "english",
        "instrumental",
        "sped up",
    ];
    for w in EDITION {
        base = base.replace(w, " ");
    }
    let folded = fold(&base);
    if folded.is_empty() { fold(&lower) } else { folded }
}

/// Kept for callers of the old path; the helper lives in [`crate::text::fold`].
pub use crate::text::fold;

/// The artists to watch: everyone subscribed (channel id, name) plus the [`TOP_HISTORY`] most played
/// in the history since `now - HISTORY_DAYS`. A history artist takes the channel id from a YouTube
/// Music play or a subscription with the same name; others come back with `id: None` (look them up).
pub fn watch_list(subscribed: &[(String, String)], history: &[Entry], now: i64) -> Vec<Watch> {
    let since = now - HISTORY_DAYS * 86_400;
    // Folded name → (display name, channel id, plays).
    type Count = (String, Option<String>, u32);
    let mut plays: HashMap<String, Count> = HashMap::new();
    for e in history.iter().filter(|e| e.status == PlayStatus::Played && e.ts >= since) {
        // The main artist only: features would flood the list.
        let Some(a) = e.artists.first() else { continue };
        let key = fold(&a.name);
        if key.is_empty() {
            continue;
        }
        let slot = plays.entry(key).or_insert_with(|| (a.name.clone(), None, 0));
        slot.2 += 1;
        if slot.1.is_none()
            && e.source == SourceKind::YouTubeMusic
            && let Some(id) = a.id.as_deref().filter(|i| i.starts_with("UC"))
        {
            slot.1 = Some(id.to_string());
        }
    }
    let mut top: Vec<(String, Count)> = plays.into_iter().collect();
    top.sort_by(|a, b| b.1.2.cmp(&a.1.2).then_with(|| a.0.cmp(&b.0)));
    top.truncate(TOP_HISTORY);
    let notify: BTreeSet<String> = top.iter().take(NOTIFY_TOP).map(|(k, _)| k.clone()).collect();

    let mut out: Vec<Watch> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for (k, (name, id, n)) in top {
        by_key.insert(k.clone(), out.len());
        out.push(Watch { id, name, plays: n, subscribed: false, top: notify.contains(&k) });
    }
    for (id, name) in subscribed {
        let k = fold(name);
        match by_key.get(&k) {
            Some(&i) => {
                out[i].subscribed = true;
                out[i].id = Some(id.clone());
            }
            None => {
                by_key.insert(k, out.len());
                out.push(Watch {
                    id: Some(id.clone()),
                    name: name.clone(),
                    plays: 0,
                    subscribed: true,
                    top: false,
                });
            }
        }
    }
    // The same channel twice (two spellings, e.g. a subscription named "IVE (아이브)" and plays of "IVE"):
    // one watch, with the first (most played) entry's name and plays and the flags of every duplicate.
    let mut first: HashMap<String, usize> = HashMap::new();
    let mut merged: Vec<Watch> = Vec::with_capacity(out.len());
    for w in out {
        match w.id.as_ref().and_then(|id| first.get(id)).copied() {
            Some(i) => {
                merged[i].subscribed |= w.subscribed;
                merged[i].top |= w.top;
            }
            None => {
                if let Some(id) = &w.id {
                    first.insert(id.clone(), merged.len());
                }
                merged.push(w);
            }
        }
    }
    merged
}

impl Listed {
    /// An album from an artist's page (`MusicSource::artist_albums`).
    pub fn from_album(a: &crate::model::Album) -> Self {
        Listed {
            id: a.id.clone(),
            title: a.name.clone(),
            kind: a.release_type.clone().unwrap_or_else(|| "album".into()),
            year: a.year,
            image: match &a.image {
                Some(crate::model::ImageRef::Url(u)) => Some(u.clone()),
                _ => None,
            },
        }
    }
}

impl Radar {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// History artists without a channel id that are due a lookup (never, or a miss older than 30 days).
    pub fn lookups_due<'a>(&self, watch: &'a [Watch], now: i64) -> Vec<&'a Watch> {
        watch
            .iter()
            .filter(|w| w.id.is_none())
            .filter(|w| match self.lookups.get(&fold(&w.name)) {
                None => true,
                Some((None, at)) => now - at > 30 * 86_400,
                Some((Some(_), _)) => false,
            })
            .collect()
    }

    /// Fills in channel ids found earlier.
    pub fn resolve(&self, watch: &mut [Watch]) {
        for w in watch.iter_mut().filter(|w| w.id.is_none()) {
            if let Some((Some(id), _)) = self.lookups.get(&fold(&w.name)) {
                w.id = Some(id.clone());
            }
        }
    }

    /// The artists whose page is due, most played first (subscriptions without plays last), at most `budget`.
    pub fn due<'a>(&self, watch: &'a [Watch], now: i64, budget: usize) -> Vec<&'a Watch> {
        let mut due: Vec<&Watch> = watch
            .iter()
            .filter(|w| {
                w.id.as_ref().is_some_and(|id| self.checked.get(id).is_none_or(|t| now - t >= RECHECK_SECS))
            })
            .collect();
        // Never read pages first, then the oldest check, then the most played.
        due.sort_by_key(|w| {
            let at = w.id.as_ref().and_then(|id| self.checked.get(id)).copied().unwrap_or(i64::MIN);
            (at, std::cmp::Reverse(w.plays))
        });
        due.truncate(budget);
        due
    }

    /// Takes one artist page. The first time an artist is read, only releases from `this_year` count as
    /// new (the rest is the back catalogue); afterwards every id not seen before does, unless it is
    /// another edition of a release already known. Returns the releases added to the feed.
    pub fn ingest(&mut self, w: &Watch, listed: &[Listed], now: i64, this_year: i32) -> Vec<Release> {
        let Some(artist_id) = w.id.clone() else { return vec![] };
        let first = !self.known.contains_key(&artist_id);
        let known = self.known.entry(artist_id.clone()).or_default();
        let keys = self.keys.entry(artist_id.clone()).or_default();
        let mut added = Vec::new();
        // Of several editions the plain one (title = key) stands for the release, else the shortest
        // title: a "Sped Up Edition" or "(Remixes)" listed before the original is a version of it.
        let mut order: Vec<&Listed> = listed.iter().collect();
        order.sort_by_cached_key(|r| (fold(&r.title) != release_key(&r.title), r.title.chars().count()));
        for r in order {
            if !known.insert(r.id.clone()) {
                continue;
            }
            let key = release_key(&r.title);
            let fresh_key = keys.insert(key);
            let new = if first { r.year.is_some_and(|y| y >= this_year) && fresh_key } else { fresh_key };
            if new && !self.releases.iter().any(|x| x.id == r.id) {
                added.push(Release {
                    id: r.id.clone(),
                    title: r.title.clone(),
                    kind: r.kind.clone(),
                    year: r.year,
                    image: r.image.clone(),
                    artist: w.name.clone(),
                    artist_id: artist_id.clone(),
                    found: now,
                    top: w.top,
                });
            }
        }
        self.checked.insert(artist_id, now);
        self.releases.extend(added.iter().cloned());
        added
    }

    /// Requests left today under `per_day`.
    pub fn left_today(&self, now: i64, per_day: u32) -> u32 {
        if self.spent.0 == now.div_euclid(86_400) { per_day.saturating_sub(self.spent.1) } else { per_day }
    }

    /// Counts one request.
    pub fn spend(&mut self, now: i64) {
        let day = now.div_euclid(86_400);
        if self.spent.0 != day {
            self.spent = (day, 0);
        }
        self.spent.1 += 1;
    }

    /// Drops releases older than [`FEED_DAYS`] from the feed.
    pub fn prune(&mut self, now: i64) {
        self.releases.retain(|r| now - r.found <= FEED_DAYS * 86_400);
    }

    /// The feed, newest find first; within a find, newest year first (releases without a year last), then
    /// albums before singles, then by artist.
    pub fn feed(&self, now: i64) -> Vec<Release> {
        let mut out: Vec<Release> =
            self.releases.iter().filter(|r| now - r.found <= FEED_DAYS * 86_400).cloned().collect();
        out.sort_by(|a, b| {
            b.found
                .cmp(&a.found)
                .then_with(|| b.year.cmp(&a.year))
                .then_with(|| (a.kind != "album").cmp(&(b.kind != "album")))
                .then_with(|| a.artist.cmp(&b.artist))
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArtistRef, ServerId};

    fn play(ts: i64, artist: &str, id: Option<&str>, source: SourceKind) -> Entry {
        Entry {
            ts,
            server_id: ServerId("s".into()),
            source,
            song_id: format!("{artist}{ts}"),
            status: PlayStatus::Played,
            title: "t".into(),
            artists: vec![ArtistRef { id: id.map(str::to_string), name: artist.into() }],
            album: None,
            album_id: None,
            image: None,
            duration_secs: 200,
            at_s: None,
            frac: None,
            ctx: None,
        }
    }

    fn listed(id: &str, title: &str, year: i32) -> Listed {
        Listed { id: id.into(), title: title.into(), kind: "album".into(), year: Some(year), image: None }
    }

    fn watch(id: &str) -> Watch {
        Watch { id: Some(id.into()), name: id.into(), plays: 5, subscribed: true, top: true }
    }

    #[test]
    fn editions_share_a_key() {
        let k = release_key("Love Dive");
        assert_eq!(release_key("LOVE DIVE (Deluxe Edition)"), k);
        assert_eq!(release_key("Love Dive - Repackage"), k);
        assert_eq!(release_key("Love Dive [Japanese Ver.]"), k);
        assert_ne!(release_key("After Like"), k);
        assert_eq!(release_key("나의 이름"), "나의이름");
        // A title that is only edition words keeps a key.
        assert!(!release_key("(Deluxe)").is_empty());
    }

    #[test]
    fn first_read_counts_this_years_releases_only() {
        let mut r = Radar::default();
        let w = watch("UCa");
        let page = [listed("n1", "New Single", 2026), listed("o1", "Old Album", 2023)];
        let added = r.ingest(&w, &page, 1000, 2026);
        assert_eq!(added.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["n1"]);
        assert!(added[0].top);
        // The same page again: nothing new.
        assert!(r.ingest(&w, &page, 2000, 2026).is_empty());
        // A new id on a later read is new whatever its year says.
        let page2 = [
            listed("n2", "Comeback", 2026),
            listed("n1", "New Single", 2026),
            listed("o1", "Old Album", 2023),
        ];
        assert_eq!(r.ingest(&w, &page2, 3000, 2026)[0].id, "n2");
        assert_eq!(r.feed(3000).iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["n2", "n1"]);
    }

    #[test]
    fn a_deluxe_edition_of_a_known_album_is_not_new() {
        let mut r = Radar::default();
        let w = watch("UCa");
        r.ingest(&w, &[listed("a", "Born Pink", 2022)], 1, 2026);
        let added =
            r.ingest(&w, &[listed("b", "Born Pink (Deluxe)", 2026), listed("a", "Born Pink", 2022)], 2, 2026);
        assert!(added.is_empty());
        // Two editions new at once: one entry.
        let added =
            r.ingest(&w, &[listed("c", "Ivy", 2026), listed("d", "IVY (Japanese Ver.)", 2026)], 3, 2026);
        assert_eq!(added.len(), 1);
        assert_eq!(r.releases.len(), 1);
        // The plain edition stands for the release even when a version is listed first.
        let mut r = Radar::default();
        let page = [listed("s", "Hit (Sped Up Edition)", 2026), listed("h", "Hit", 2026)];
        assert_eq!(r.ingest(&w, &page, 1, 2026).iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["h"]);
        // Remixes of last year's single are no new release on the first read either.
        let mut r = Radar::default();
        let page = [listed("x", "Boom (Remixes)", 2026), listed("b", "Boom", 2025)];
        assert!(r.ingest(&w, &page, 1, 2026).is_empty());
    }

    #[test]
    fn the_feed_drops_old_finds() {
        let mut r = Radar::default();
        r.ingest(&watch("UCa"), &[listed("n", "N", 2026)], 0, 2026);
        assert_eq!(r.feed(FEED_DAYS * 86_400).len(), 1);
        assert!(r.feed(FEED_DAYS * 86_400 + 1).is_empty());
        r.prune(FEED_DAYS * 86_400 + 1);
        assert!(r.releases.is_empty());
    }

    #[test]
    fn the_daily_budget_resets_each_day() {
        let mut r = Radar::default();
        assert_eq!(r.left_today(10, 3), 3);
        r.spend(10);
        r.spend(20);
        assert_eq!(r.left_today(30, 3), 1);
        r.spend(30);
        r.spend(40);
        assert_eq!(r.left_today(50, 3), 0);
        assert_eq!(r.left_today(86_400, 3), 3);
    }

    #[test]
    fn watch_list_merges_history_and_subscriptions() {
        let now = 400 * 86_400;
        let mut h = Vec::new();
        for i in 0..5 {
            h.push(play(now - i, "IVE", Some("UCive"), SourceKind::YouTubeMusic));
        }
        for i in 0..3 {
            h.push(play(now - i, "NewJeans", None, SourceKind::Subsonic));
        }
        // Too old to count.
        h.push(play(now - 380 * 86_400, "Old Band", None, SourceKind::Subsonic));
        let mut skipped = play(now, "Skipper", None, SourceKind::Subsonic);
        skipped.status = PlayStatus::Skipped;
        h.push(skipped);
        let subs = [
            ("UCnj".to_string(), "newjeans".to_string()),
            ("UCx".to_string(), "Only Subscribed".to_string()),
        ];
        let w = watch_list(&subs, &h, now);
        let names: Vec<_> =
            w.iter().map(|w| (w.name.as_str(), w.id.as_deref(), w.plays, w.subscribed)).collect();
        assert_eq!(
            names,
            [
                ("IVE", Some("UCive"), 5, false),
                ("NewJeans", Some("UCnj"), 3, true),
                ("Only Subscribed", Some("UCx"), 0, true)
            ]
        );
        assert!(w[0].top && w[1].top && !w[2].top);
    }

    #[test]
    fn feed_orders_by_find_then_year_then_albums_first() {
        let rel = |id: &str, kind: &str, year: Option<i32>, found: i64| Release {
            id: id.into(),
            title: id.into(),
            kind: kind.into(),
            year,
            image: None,
            artist: "A".into(),
            artist_id: "UCa".into(),
            found,
            top: false,
        };
        let r = Radar {
            releases: vec![
                rel("album25", "album", Some(2025), 10),
                rel("noyear", "album", None, 10),
                rel("single26", "single", Some(2026), 10),
                rel("album26", "album", Some(2026), 10),
                rel("later", "single", Some(2020), 20),
            ],
            ..Radar::default()
        };
        let ids: Vec<_> = r.feed(30).into_iter().map(|x| x.id).collect();
        assert_eq!(ids, ["later", "album26", "single26", "album25", "noyear"]);
    }

    #[test]
    fn same_channel_under_two_names_stays_subscribed() {
        let now = 400 * 86_400;
        let h: Vec<_> =
            (0..4).map(|i| play(now - i, "IVE", Some("UCive"), SourceKind::YouTubeMusic)).collect();
        let subs = [("UCive".to_string(), "IVE (아이브)".to_string())];
        let w = watch_list(&subs, &h, now);
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!((w[0].name.as_str(), w[0].id.as_deref(), w[0].plays), ("IVE", Some("UCive"), 4));
        assert!(w[0].subscribed && w[0].top);
    }

    #[test]
    fn due_reads_new_artists_first_within_the_budget() {
        let mut r = Radar::default();
        let mut a = watch("UCa");
        a.plays = 10;
        let mut b = watch("UCb");
        b.plays = 1;
        let c = Watch { id: None, name: "Nobody".into(), plays: 3, subscribed: false, top: false };
        r.checked.insert("UCa".into(), 0);
        let list = [a, b, c];
        let due = r.due(&list, RECHECK_SECS, 5);
        assert_eq!(due.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(), ["UCb", "UCa"]);
        assert_eq!(r.due(&list, RECHECK_SECS - 1, 5).len(), 1);
        assert_eq!(r.due(&list, RECHECK_SECS, 1).len(), 1);
        // Lookups: the name without an id, again after a 30-day-old miss.
        assert_eq!(r.lookups_due(&list, 0).len(), 1);
        r.lookups.insert("nobody".into(), (None, 0));
        assert!(r.lookups_due(&list, 86_400).is_empty());
        assert_eq!(r.lookups_due(&list, 31 * 86_400).len(), 1);
        r.lookups.insert("nobody".into(), (Some("UCn".into()), 0));
        let mut list = list.to_vec();
        r.resolve(&mut list);
        assert_eq!(list[2].id.as_deref(), Some("UCn"));
    }
}
