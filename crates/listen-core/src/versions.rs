//! Telling versions of the same song apart. YouTube Music often has a song
//! several times (album track, single, "Radio Edit", "Remastered 2011",
//! the music video, a VEVO upload); the DJs treat them as one song.

use crate::model::Song;
use std::collections::HashMap;

/// The same key for every version of a song: its title without bracketed
/// or dashed extras, plus its main artist, both folded to letters and digits.
pub fn song_key(song: &Song) -> String {
    let artist = song.artists.first().map(|a| a.name.clone()).unwrap_or_else(|| song.artist_line());
    format!("{}|{}", title_key(&song.title), artist_key(&artist))
}

fn title_key(title: &str) -> String {
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
    // "Song - Radio Edit", "Song – Remastered 2011", "Song feat. X".
    for cut in [" - ", " – ", " — ", " feat.", " feat ", " ft.", " ft ", " featuring "] {
        if let Some(i) = base.find(cut) {
            base.truncate(i);
        }
    }
    let folded = fold(&base);
    if folded.is_empty() { fold(&lower) } else { folded }
}

fn artist_key(artist: &str) -> String {
    let mut a = artist.to_lowercase();
    // The first of several credited artists.
    for cut in [",", " & ", " x ", " feat", " ft.", " and ", " und "] {
        if let Some(i) = a.find(cut)
            && i > 0
        {
            a.truncate(i);
        }
    }
    fold(a.trim_end_matches(" - topic").trim().trim_end_matches("vevo"))
}

/// Letters and digits only (any script), so punctuation and spacing don't matter.
fn fold(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// A radio edit (or single/radio version) of a song.
pub fn is_radio_edit(song: &Song) -> bool {
    let t = song.title.to_lowercase();
    ["radio edit", "radio version", "radio mix", "radio cut", "single edit", "single version"]
        .iter()
        .any(|m| t.contains(m))
}

/// Which version to keep when several are on offer: radio edits first, then
/// the plain song, then extended/live/remixed and other special versions.
pub fn version_rank(song: &Song) -> u8 {
    if is_radio_edit(song) {
        return 0;
    }
    let t = song.title.to_lowercase();
    const SPECIAL: [&str; 25] = [
        // Music videos and performance clips: the song itself is the version to play.
        " mv",
        "m/v",
        "music video",
        "official video",
        "performance video",
        "lyric video",
        "stage",
        "@ ",
        "fancam",
        "dance practice",
        "special clip",
        "cover",
        "teaser",
        "extended",
        "live",
        "remix",
        "club mix",
        "instrumental",
        "acoustic",
        "sped up",
        "slowed",
        "karaoke",
        "demo",
        "a cappella",
        "acapella",
    ];
    if SPECIAL.iter().any(|m| t.contains(m)) { 2 } else { 1 }
}

/// One version per song, keeping the list's order: where several versions
/// are on offer, the best-ranked one (a radio edit if there is one) takes
/// the place of the first.
pub fn dedupe_versions(songs: Vec<Song>) -> Vec<Song> {
    let mut slot: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Song> = Vec::with_capacity(songs.len());
    for song in songs {
        let key = song_key(&song);
        match slot.get(&key) {
            Some(&i) => {
                // Equal rank: the one with an album (the song, not a video upload) wins.
                if (version_rank(&song), song.album.is_none())
                    < (version_rank(&out[i]), out[i].album.is_none())
                {
                    out[i] = song;
                }
            }
            None => {
                slot.insert(key, out.len());
                out.push(song);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArtistRef, GainInfo, ServerId, SourceKind};
    use std::time::Duration;

    fn song(title: &str, artist: &str) -> Song {
        Song {
            mbz: Default::default(),
            extra: Default::default(),
            id: format!("{title}/{artist}"),
            server_id: ServerId("s".into()),
            source: SourceKind::YouTubeMusic,
            title: title.into(),
            artists: vec![ArtistRef { id: None, name: artist.into() }],
            album_artists: vec![],
            album: None,
            album_id: None,
            image: None,
            duration: Duration::from_secs(200),
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
            gain: GainInfo::default(),
            explicit: false,
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: None,
            comment: None,
        }
    }

    #[test]
    fn versions_share_a_key() {
        let base = song_key(&song("Blinding Lights", "The Weeknd"));
        for (title, artist) in [
            ("Blinding Lights (Radio Edit)", "The Weeknd"),
            ("Blinding Lights - Remastered 2020", "The Weeknd"),
            ("Blinding Lights [Official Video]", "TheWeekndVEVO"),
            ("blinding lights", "The Weeknd - Topic"),
            ("Blinding Lights (feat. Somebody)", "The Weeknd, Somebody"),
            ("Blinding Lights", "The Weeknd & Rosalía"),
        ] {
            assert_eq!(song_key(&song(title, artist)), base, "{title} / {artist}");
        }
        assert_ne!(song_key(&song("Save Your Tears", "The Weeknd")), base);
        assert_ne!(song_key(&song("Blinding Lights", "Someone Else")), base);
        // Titles that are only brackets keep their text.
        assert_eq!(song_key(&song("(Intro)", "X")), "intro|x");
        // Korean titles survive the folding.
        assert_eq!(song_key(&song("봄날 (Spring Day)", "BTS")), "봄날|bts");
    }

    #[test]
    fn radio_edits_win() {
        let list = vec![
            song("Song A (Extended Mix)", "X"),
            song("Song B", "Y"),
            song("Song A", "X"),
            song("Song A (Radio Edit)", "X"),
            song("Song B (Live)", "Y"),
        ];
        let titles: Vec<String> = dedupe_versions(list).into_iter().map(|s| s.title).collect();
        assert_eq!(titles, ["Song A (Radio Edit)", "Song B"]);
        assert!(is_radio_edit(&song("Hit - Radio Version", "X")));
        assert!(!is_radio_edit(&song("Radio Ga Ga", "Queen")));
    }
}
