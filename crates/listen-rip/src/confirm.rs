//! Does a record exist? Asked when several gnudb entries fit a disc's TOC equally well (the TOC
//! can't tell them apart): an entry is confirmed when the iTunes store has its album with the
//! disc's track count and every track within [`TOLERANCE_MS`] of the disc's; Deezer (track count
//! only) is the second opinion when iTunes has no album of that name.
//!
//! Names are matched like the iTunes cover ([`crate::cover::fits`]), on the album title
//! without its bracketed parts: gnudb's "PULSE (미니4집)" is "PULSE - EP" on iTunes, and a search
//! for the whole title finds nothing.

use crate::cover::fits;
use crate::gnudb::Net;
use crate::toc::Toc;
use listen_core::text::{fold, without_brackets};
use serde_json::Value;

/// How far a store's track length may be off the disc's (stores round down, a CD's track
/// includes the gap before the next one).
pub const TOLERANCE_MS: u64 = 3000;
/// iTunes albums of the right name looked up for their track lengths, at most.
const LOOKUPS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The record exists with this disc's tracks; what was found, for the log.
    Confirmed(String),
    /// Why not.
    Unconfirmed(String),
}

impl Verdict {
    pub fn confirmed(&self) -> bool {
        matches!(self, Verdict::Confirmed(_))
    }
}

/// The album title to search and match: without bracketed parts ("PULSE (미니4집)" → "PULSE"),
/// unless nothing is left of it.
pub fn search_title(album: &str) -> String {
    let bare = without_brackets(album).split_whitespace().collect::<Vec<_>>().join(" ");
    if fold(&bare).is_empty() { album.trim().to_string() } else { bare }
}

/// An album a store lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub id: u64,
    pub artist: String,
    pub album: String,
    pub tracks: usize,
}

impl Found {
    fn name(&self) -> String {
        format!("{} – {}", self.artist, self.album)
    }
}

/// The albums of an iTunes search answer that fit artist + album.
pub fn itunes_albums(body: &Value, artist: &str, album: &str) -> Vec<Found> {
    let rows = body["results"].as_array().map(Vec::as_slice).unwrap_or_default();
    rows.iter()
        .filter_map(|r| {
            let found = Found {
                id: r["collectionId"].as_u64()?,
                artist: r["artistName"].as_str().unwrap_or("").to_string(),
                album: r["collectionName"].as_str().unwrap_or("").to_string(),
                tracks: r["trackCount"].as_u64().unwrap_or(0) as usize,
            };
            fits(artist, album, &found.artist, &found.album).then_some(found)
        })
        .collect()
}

/// The track lengths (ms) of an iTunes lookup answer, in disc and track order.
pub fn itunes_lengths(body: &Value) -> Vec<u64> {
    let rows = body["results"].as_array().map(Vec::as_slice).unwrap_or_default();
    let mut tracks: Vec<(u64, u64, u64)> = rows
        .iter()
        .filter(|r| r["wrapperType"] == "track")
        .map(|r| {
            (
                r["discNumber"].as_u64().unwrap_or(1),
                r["trackNumber"].as_u64().unwrap_or(0),
                r["trackTimeMillis"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    tracks.sort_unstable();
    tracks.into_iter().map(|(_, _, ms)| ms).collect()
}

/// `Ok` when the store's lengths are the disc's, track for track, within [`TOLERANCE_MS`].
pub fn lengths_fit(store: &[u64], disc: &[u64]) -> Result<(), String> {
    if store.len() != disc.len() {
        return Err(format!("{} tracks, the disc has {}", store.len(), disc.len()));
    }
    match store.iter().zip(disc).position(|(s, d)| s.abs_diff(*d) > TOLERANCE_MS) {
        None => Ok(()),
        Some(i) => Err(format!("track {} is {} s, on the disc {} s", i + 1, store[i] / 1000, disc[i] / 1000)),
    }
}

/// The albums of a Deezer album search answer that fit artist + album.
pub fn deezer_albums(body: &Value, artist: &str, album: &str) -> Vec<Found> {
    let rows = body["data"].as_array().map(Vec::as_slice).unwrap_or_default();
    rows.iter()
        .filter_map(|r| {
            let found = Found {
                id: r["id"].as_u64()?,
                artist: r["artist"]["name"].as_str().unwrap_or("").to_string(),
                album: r["title"].as_str().unwrap_or("").to_string(),
                tracks: r["nb_tracks"].as_u64().unwrap_or(0) as usize,
            };
            fits(artist, album, &found.artist, &found.album).then_some(found)
        })
        .collect()
}

/// Whether `artist` – `album` exists as this disc (see the module docs).
pub async fn confirm(net: &impl Net, artist: &str, album: &str, toc: &Toc) -> Verdict {
    let title = search_title(album);
    let term = format!("{artist} {title}");
    let disc = toc.track_lengths_ms();
    let itunes = net
        .json("https://itunes.apple.com/search", &[("term", &term), ("entity", "album"), ("limit", "10")])
        .await;
    let named = itunes.as_ref().map(|b| itunes_albums(b, artist, &title)).unwrap_or_default();
    if let Some(first) = named.first() {
        let mut why =
            format!("iTunes: {} has {} tracks, the disc {}", first.name(), first.tracks, disc.len());
        for found in named.iter().filter(|f| f.tracks == disc.len()).take(LOOKUPS) {
            let id = found.id.to_string();
            let Some(body) =
                net.json("https://itunes.apple.com/lookup", &[("id", &id), ("entity", "song")]).await
            else {
                why = format!("iTunes: the tracks of {} could not be fetched", found.name());
                continue;
            };
            match lengths_fit(&itunes_lengths(&body), &disc) {
                Ok(()) => {
                    return Verdict::Confirmed(format!(
                        "iTunes: {}, {} tracks, each within {} s of the disc",
                        found.name(),
                        disc.len(),
                        TOLERANCE_MS / 1000
                    ));
                }
                Err(e) => why = format!("iTunes: {}: {e}", found.name()),
            }
        }
        return Verdict::Unconfirmed(why);
    }
    // iTunes has no album of that name (or didn't answer): Deezer's word, by track count.
    let deezer = net.json("https://api.deezer.com/search/album", &[("q", &term)]).await;
    let named = deezer.as_ref().map(|b| deezer_albums(b, artist, &title)).unwrap_or_default();
    match named.iter().find(|f| f.tracks == disc.len()) {
        Some(f) => Verdict::Confirmed(format!("Deezer: {}, {} tracks (not on iTunes)", f.name(), f.tracks)),
        None => Verdict::Unconfirmed(match named.first() {
            Some(f) => format!(
                "not on iTunes; Deezer: {} has {} tracks, the disc {}",
                f.name(),
                f.tracks,
                disc.len()
            ),
            None => "found neither on iTunes nor on Deezer".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).unwrap()
    }

    /// RESCENE – PULSE (미니4집): LBAs 0 / 10575 / 24975 / 40875 / 54525, lead-out 71025.
    fn pulse() -> Toc {
        Toc::from_offsets(1, 71_175, &[150, 10_725, 25_125, 41_025, 54_675])
    }

    #[test]
    fn titles_lose_their_brackets_for_the_search() {
        assert_eq!(search_title("PULSE (미니4집)"), "PULSE");
        assert_eq!(search_title("Fadladder Play Erik Satie"), "Fadladder Play Erik Satie");
        assert_eq!(search_title("(What's the Story) Morning Glory?"), "Morning Glory?");
        assert_eq!(search_title("[1999]"), "[1999]", "nothing would be left");
    }

    #[test]
    fn the_recorded_pulse_ep_fits_the_disc() {
        let albums = itunes_albums(&fixture("itunes_search_pulse.json"), "RESCENE", "PULSE");
        assert_eq!(albums.len(), 1);
        assert_eq!((albums[0].id, albums[0].tracks), (6_828_335_122, 5));
        assert_eq!(albums[0].album, "PULSE - EP");
        let lengths = itunes_lengths(&fixture("itunes_lookup_pulse.json"));
        assert_eq!(lengths, [140_000, 191_000, 211_000, 181_000, 220_000]);
        assert_eq!(pulse().track_lengths_ms(), [141_000, 192_000, 212_000, 182_000, 220_000]);
        assert_eq!(lengths_fit(&lengths, &pulse().track_lengths_ms()), Ok(()));
        assert!(itunes_albums(&fixture("itunes_search_pulse.json"), "Fadladder", "PULSE").is_empty());
        assert!(
            itunes_albums(&fixture("itunes_search_fadladder.json"), "Fadladder", "Fadladder Play Erik Satie")
                .is_empty()
        );
    }

    #[test]
    fn lengths_must_all_fit() {
        let disc = [141_000, 192_000];
        assert_eq!(lengths_fit(&[138_000, 195_000], &disc), Ok(()));
        assert_eq!(
            lengths_fit(&[137_999, 192_000], &disc),
            Err("track 1 is 137 s, on the disc 141 s".into())
        );
        assert_eq!(lengths_fit(&[141_000], &disc), Err("1 tracks, the disc has 2".into()));
    }

    #[test]
    fn deezer_matches_artist_title_and_counts() {
        let pulse = deezer_albums(&fixture("deezer_search_pulse.json"), "RESCENE", "PULSE");
        assert_eq!(pulse.len(), 1);
        assert_eq!((pulse[0].album.as_str(), pulse[0].tracks), ("PULSE", 5));
        // The Fadladder search finds an Erik Satie record by someone else: not this album.
        assert!(
            deezer_albums(&fixture("deezer_search_fadladder.json"), "Fadladder", "Fadladder Play Erik Satie")
                .is_empty()
        );
    }
}
