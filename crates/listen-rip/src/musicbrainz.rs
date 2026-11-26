//! MusicBrainz lookup by disc ID (with the TOC for a fuzzy fallback), the
//! releases it returns, and which one fits best.

use crate::toc::Toc;
use serde_json::Value;
use std::time::Duration;

pub const USER_AGENT: &str = listen_core::MUSICBRAINZ_USER_AGENT;
const API: &str = "https://musicbrainz.org/ws/2";
/// Countries preferred when several releases fit equally (Germany, Europe, worldwide).
const PREFERRED_COUNTRIES: [&str; 3] = ["DE", "XE", "XW"];
/// How far a track of a release may be from the disc's track of the same
/// number before it is a different disc. MusicBrainz's fuzzy TOC search
/// compares the disc as a whole, so it also answers with albums that merely
/// happen to be as long; tagging a CD from one of those files it under a
/// wrong album.
const TRACK_TOLERANCE_MS: i64 = 3_000;
/// MusicBrainz's "Various Artists".
pub const VARIOUS_ARTISTS_ID: &str = "89ad4ac3-39f7-470e-963a-56509c546377";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Credit {
    /// As credited on the release.
    pub name: String,
    pub id: String,
    pub sort_name: String,
    pub joinphrase: String,
}

/// "Calvin Harris & Dua Lipa".
pub fn credit_line(credits: &[Credit]) -> String {
    credits.iter().map(|c| format!("{}{}", c.name, c.joinphrase)).collect::<String>().trim().to_string()
}

pub fn credit_sort_line(credits: &[Credit]) -> String {
    credits
        .iter()
        .map(|c| format!("{}{}", if c.sort_name.is_empty() { &c.name } else { &c.sort_name }, c.joinphrase))
        .collect::<String>()
        .trim()
        .to_string()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    pub position: u32,
    pub title: String,
    pub length_ms: Option<u64>,
    /// Release track id (`MUSICBRAINZ_RELEASETRACKID`).
    pub id: String,
    pub recording_id: String,
    pub credits: Vec<Credit>,
    pub isrcs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Medium {
    pub position: u32,
    pub format: String,
    pub title: String,
    pub disc_ids: Vec<String>,
    pub tracks: Vec<Track>,
}

impl Medium {
    pub fn length_ms(&self) -> u64 {
        self.tracks.iter().filter_map(|t| t.length_ms).sum()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Release {
    pub id: String,
    pub title: String,
    pub disambiguation: String,
    pub credits: Vec<Credit>,
    pub date: String,
    pub country: String,
    pub barcode: String,
    pub status: String,
    pub packaging: String,
    pub script: String,
    pub labels: Vec<String>,
    pub catalog_numbers: Vec<String>,
    pub release_group_id: String,
    /// Primary type and secondary types, lowercase ("album", "compilation").
    pub release_types: Vec<String>,
    pub original_date: String,
    pub has_front_cover: bool,
    pub media: Vec<Medium>,
}

impl Release {
    pub fn artist(&self) -> String {
        credit_line(&self.credits)
    }

    /// The medium this disc is: by disc ID, else the first CD with as many
    /// tracks as the disc has audio tracks.
    pub fn medium_for(&self, disc_id: &str, audio_tracks: usize) -> Option<usize> {
        self.media
            .iter()
            .position(|m| m.disc_ids.iter().any(|d| d == disc_id))
            .or_else(|| self.media.iter().position(|m| m.tracks.len() == audio_tracks))
    }

    /// Is this disc listed on the release? Then MusicBrainz knows its disc ID
    /// and this release *is* the disc.
    pub fn has_disc_id(&self, disc_id: &str) -> bool {
        self.media.iter().any(|m| m.disc_ids.iter().any(|d| d == disc_id))
    }

    /// Could this release be this disc: as many tracks, each within
    /// [`TRACK_TOLERANCE_MS`] of the disc's. Tracks MusicBrainz has no length
    /// for don't count against it.
    pub fn fits_toc(&self, disc_id: &str, toc: &Toc) -> bool {
        let lengths = toc.track_lengths_ms();
        let Some(m) = self.medium_for(disc_id, lengths.len()) else {
            return false;
        };
        let tracks = &self.media[m].tracks;
        tracks.len() == lengths.len()
            && tracks.iter().zip(&lengths).all(|(t, &want)| match t.length_ms {
                Some(have) => (have as i64 - want as i64).abs() <= TRACK_TOLERANCE_MS,
                None => true,
            })
    }

    pub fn year(&self) -> &str {
        self.date.get(..4).unwrap_or("")
    }

    /// One line for the picker: "DIGIPACK ver. · KR · 2026 · Digipak · 8804775459771".
    pub fn details(&self) -> String {
        [
            self.disambiguation.as_str(),
            self.country.as_str(),
            self.date.as_str(),
            self.packaging.as_str(),
            self.barcode.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty() && *s != "None")
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn credits(v: &Value) -> Vec<Credit> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|c| Credit {
                    name: s(&c["name"]),
                    id: s(&c["artist"]["id"]),
                    sort_name: s(&c["artist"]["sort-name"]),
                    joinphrase: s(&c["joinphrase"]),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One release object from the web service.
pub fn parse_release(r: &Value) -> Release {
    let rg = &r["release-group"];
    let mut release_types = Vec::new();
    if let Some(p) = rg["primary-type"].as_str() {
        release_types.push(p.to_lowercase());
    }
    if let Some(sec) = rg["secondary-types"].as_array() {
        release_types.extend(sec.iter().filter_map(|t| t.as_str()).map(str::to_lowercase));
    }
    let labels = r["label-info"].as_array().cloned().unwrap_or_default();
    Release {
        id: s(&r["id"]),
        title: s(&r["title"]),
        disambiguation: s(&r["disambiguation"]),
        credits: credits(&r["artist-credit"]),
        date: s(&r["date"]),
        country: s(&r["country"]),
        barcode: s(&r["barcode"]),
        status: s(&r["status"]),
        packaging: s(&r["packaging"]),
        script: s(&r["text-representation"]["script"]),
        labels: labels.iter().filter_map(|l| l["label"]["name"].as_str()).map(String::from).collect(),
        catalog_numbers: labels
            .iter()
            .filter_map(|l| l["catalog-number"].as_str())
            .filter(|c| !c.is_empty())
            .map(String::from)
            .collect(),
        release_group_id: s(&rg["id"]),
        release_types,
        original_date: s(&rg["first-release-date"]),
        has_front_cover: r["cover-art-archive"]["front"].as_bool().unwrap_or(false),
        media: r["media"]
            .as_array()
            .map(|ms| {
                ms.iter()
                    .map(|m| Medium {
                        position: m["position"].as_u64().unwrap_or(1) as u32,
                        format: s(&m["format"]),
                        title: s(&m["title"]),
                        disc_ids: m["discs"]
                            .as_array()
                            .map(|d| d.iter().map(|d| s(&d["id"])).collect())
                            .unwrap_or_default(),
                        tracks: m["tracks"]
                            .as_array()
                            .map(|ts| {
                                ts.iter()
                                    .map(|t| {
                                        let rec = &t["recording"];
                                        let tc = credits(&t["artist-credit"]);
                                        Track {
                                            position: t["position"].as_u64().unwrap_or(0) as u32,
                                            title: if t["title"].is_string() {
                                                s(&t["title"])
                                            } else {
                                                s(&rec["title"])
                                            },
                                            length_ms: t["length"]
                                                .as_u64()
                                                .or_else(|| rec["length"].as_u64()),
                                            id: s(&t["id"]),
                                            recording_id: s(&rec["id"]),
                                            credits: if tc.is_empty() {
                                                credits(&rec["artist-credit"])
                                            } else {
                                                tc
                                            },
                                            isrcs: rec["isrcs"]
                                                .as_array()
                                                .map(|a| {
                                                    a.iter()
                                                        .filter_map(|i| i.as_str())
                                                        .map(String::from)
                                                        .collect()
                                                })
                                                .unwrap_or_default(),
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// What [`lookup`] found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    /// In MusicBrainz's order.
    pub releases: Vec<Release>,
    /// MusicBrainz knows this disc ID, so these releases are this disc.
    /// `false`: they come from its fuzzy TOC search and are candidates that
    /// still have to fit ([`Release::fits_toc`]).
    pub exact: bool,
}

/// `/discid/…` answer: `{"releases": [...]}` (or a single release object).
pub fn parse_lookup(body: &Value) -> Vec<Release> {
    match body["releases"].as_array() {
        Some(rs) => rs.iter().map(parse_release).collect(),
        None if body["media"].is_array() => vec![parse_release(body)],
        None => Vec::new(),
    }
}

/// Indices of `releases`, best first: closest in length to the disc, has a
/// front cover, official, a preferred country, earliest date, then
/// MusicBrainz's order.
pub fn rank(releases: &[Release], toc: &Toc) -> Vec<usize> {
    let disc_id = toc.musicbrainz_id();
    let disc_ms = toc.duration_ms() as i64;
    let audio = toc.audio_count();
    let mut idx: Vec<usize> = (0..releases.len()).collect();
    let key = |i: &usize| {
        let r = &releases[*i];
        let delta = r
            .medium_for(&disc_id, audio)
            .map(|m| (r.media[m].length_ms() as i64 - disc_ms).abs())
            .unwrap_or(i64::MAX);
        // Differences under 2 s are noise (pregaps, rounding).
        let delta_bucket = if delta == i64::MAX { i64::MAX } else { delta / 2000 };
        let country =
            PREFERRED_COUNTRIES.iter().position(|c| *c == r.country).unwrap_or(PREFERRED_COUNTRIES.len());
        let date = if r.date.is_empty() { "9999".to_string() } else { r.date.clone() };
        (delta_bucket, !r.has_front_cover, r.status != "Official", country, date, *i)
    };
    idx.sort_by_key(key);
    idx
}

#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    #[error("MusicBrainz: {0}")]
    Network(String),
    #[error("MusicBrainz answered {0}")]
    Status(u16),
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("http client")
}

/// Releases for this disc (empty: MusicBrainz doesn't know it). The TOC in the
/// query makes MusicBrainz fall back to its fuzzy search when the disc ID is
/// unknown, which is why the answer says whether the disc ID itself matched.
/// Retries MusicBrainz's rate limiting (503) a few times.
pub async fn lookup(http: &reqwest::Client, toc: &Toc) -> Result<Found, LookupError> {
    let disc_id = toc.musicbrainz_id();
    let url = format!(
        "{API}/discid/{disc_id}?toc={}&cdstubs=no&inc=artist-credits+labels+recordings+release-groups+isrcs&fmt=json",
        toc.toc_string().replace(' ', "+")
    );
    let mut last = LookupError::Status(0);
    for attempt in 0..4 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(1500 * attempt)).await;
        }
        let resp = match http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                last = LookupError::Network(e.without_url().to_string());
                continue;
            }
        };
        match resp.status().as_u16() {
            200 => {
                let body: Value =
                    resp.json().await.map_err(|e| LookupError::Network(e.without_url().to_string()))?;
                let releases = parse_lookup(&body);
                let exact = releases.iter().any(|r| r.has_disc_id(&disc_id));
                return Ok(Found { releases, exact });
            }
            404 => return Ok(Found::default()),
            503 | 502 | 500 => last = LookupError::Status(resp.status().as_u16()),
            code => return Err(LookupError::Status(code)),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revive_toc() -> Toc {
        Toc::from_offsets(
            1,
            154_800,
            &[150, 14625, 27975, 40575, 53175, 66750, 79425, 92325, 105150, 117975, 130200, 139500],
        )
    }

    fn revive() -> Vec<Release> {
        parse_lookup(&serde_json::from_str(include_str!("../tests/fixtures/discid_revive.json")).unwrap())
    }

    #[test]
    fn parses_a_disc_id_answer() {
        let rs = revive();
        assert_eq!(rs.len(), 5);
        let r = &rs[0];
        assert_eq!(r.title, "REVIVE+");
        assert_eq!(r.artist(), "IVE");
        assert_eq!(r.release_types, vec!["album"]);
        assert_eq!(r.labels, vec!["STARSHIP Entertainment"]);
        assert!(r.year() == "2026");
        let m = r.medium_for("7rEapOzTbtsuZwvusWs2r_d8duc-", 12).unwrap();
        assert_eq!(r.media[m].tracks.len(), 12);
        assert_eq!(r.media[m].tracks[2].title, "숨바꼭질 (Hush)");
        assert!(!r.media[m].tracks[0].recording_id.is_empty());
        assert!(rs.iter().any(|r| r.disambiguation == "DIGIPACK ver."));
    }

    #[test]
    fn ranks_equal_versions_stably() {
        let rs = revive();
        let order = rank(&rs, &revive_toc());
        assert_eq!(order.len(), 5);
        // All five are the same length, KR, no cover: earliest date, then MusicBrainz's order.
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn prefers_cover_then_country() {
        let mut rs = revive();
        rs[3].has_front_cover = true;
        assert_eq!(rank(&rs, &revive_toc())[0], 3);
        rs[3].has_front_cover = false;
        rs[4].country = "DE".into();
        assert_eq!(rank(&rs, &revive_toc())[0], 4);
    }

    #[test]
    fn parses_a_multi_disc_compilation() {
        let v: Value = serde_json::from_str(include_str!("../tests/fixtures/release_now100.json")).unwrap();
        let rs = parse_lookup(&v);
        assert_eq!(rs.len(), 1);
        let r = &rs[0];
        assert_eq!(r.media.len(), 2);
        assert_eq!(r.media[1].title, "NOW Classics");
        assert_eq!(r.credits[0].id, VARIOUS_ARTISTS_ID);
        assert_eq!(r.release_types, vec!["album", "compilation"]);
        assert!(r.has_front_cover);
        assert_eq!(credit_line(&r.media[0].tracks[0].credits), "Calvin Harris & Dua Lipa");
        assert_eq!(r.medium_for("g5YUIa02dIrQzXqsX__dciVdFDo-", 0), Some(1));
    }

    /// A 6-track CD (15:53) that MusicBrainz does not know.
    fn unknown_toc() -> Toc {
        Toc::from_offsets(1, 71625, &[150, 12675, 22950, 35175, 46875, 60225])
    }

    /// The one release MusicBrainz's fuzzy TOC search answers with for that
    /// disc: as many tracks and the same total length to within a second, but
    /// two of its tracks are 5.5 s and 8.2 s off, so it is a different album.
    fn same_length_other_album(lengths: &[u64]) -> Release {
        Release {
            id: "e4e0b05a-01df-4e5d-af4e-d479cfc83780".into(),
            title: "ORIGINAL SOUND TRACK".into(),
            credits: vec![Credit { name: "Various Artists".into(), ..Default::default() }],
            media: vec![Medium {
                position: 1,
                format: "CD".into(),
                disc_ids: vec!["d2sA8pwMLzvc3mgfypRWeEcvXUQ-".into()],
                tracks: lengths
                    .iter()
                    .enumerate()
                    .map(|(i, &ms)| Track {
                        position: i as u32 + 1,
                        length_ms: Some(ms),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_fuzzy_match_of_the_same_length_but_other_tracks_does_not_fit() {
        let toc = unknown_toc();
        let r = same_length_other_album(&[172_520, 137_786, 162_093, 155_280, 169_786, 154_533]);
        assert!(!r.has_disc_id(&toc.musicbrainz_id()));
        // The whole disc fits within a second, which is why ranking alone took it.
        let whole = r.media[0].length_ms() as i64 - toc.duration_ms() as i64;
        assert!(whole.abs() < 2_000, "{whole} ms");
        assert!(!r.fits_toc(&toc.musicbrainz_id(), &toc));
    }

    #[test]
    fn a_fuzzy_match_whose_tracks_all_fit_is_kept() {
        let toc = unknown_toc();
        let mut lengths = toc.track_lengths_ms();
        // Another pressing: gaps shift the tracks by a moment, not by seconds.
        lengths[0] += 900;
        lengths[4] -= 1_100;
        let r = same_length_other_album(&lengths);
        assert!(r.fits_toc(&toc.musicbrainz_id(), &toc));
    }

    #[test]
    fn a_fuzzy_match_with_another_track_count_does_not_fit() {
        let toc = unknown_toc();
        let mut lengths = toc.track_lengths_ms();
        lengths.pop();
        assert!(!same_length_other_album(&lengths).fits_toc(&toc.musicbrainz_id(), &toc));
    }

    /// The disc ID match stays authoritative: its tracks are not second-guessed.
    #[test]
    fn an_exact_disc_id_answer_is_exact() {
        let rs = revive();
        let toc = revive_toc();
        assert!(rs.iter().any(|r| r.has_disc_id(&toc.musicbrainz_id())));
    }

    /// Live: the REVIVE+ disc on musicbrainz.org (run with --ignored).
    #[tokio::test]
    #[ignore = "live: musicbrainz.org"]
    async fn live_lookup() {
        let found = lookup(&http_client(), &revive_toc()).await.unwrap();
        assert!(found.exact, "the disc ID is in MusicBrainz");
        assert!(found.releases.len() >= 5, "{} releases", found.releases.len());
        assert!(found.releases.iter().all(|r| r.title == "REVIVE+"));
    }
}
