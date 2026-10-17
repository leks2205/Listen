//! What an album is called and how its FLAC files are tagged: from a
//! MusicBrainz release (Picard's tag names, so Navidrome groups albums and
//! artists by MusicBrainz IDs), from CD-Text, or "Unknown …" (flagged for
//! fixing).

use crate::musicbrainz::{Credit, Release, VARIOUS_ARTISTS_ID, credit_line, credit_sort_line};
use crate::toc::Toc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaSource {
    MusicBrainz,
    CdText,
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMeta {
    pub number: u32,
    pub title: String,
    pub artist: String,
    pub artist_sort: String,
    /// Each credited artist (multi-valued `ARTISTS`).
    pub artists: Vec<String>,
    pub artist_ids: Vec<String>,
    pub recording_id: String,
    pub release_track_id: String,
    pub isrcs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlbumMeta {
    pub source: MetaSource,
    pub album: String,
    pub album_artist: String,
    pub album_artist_sort: String,
    pub album_artist_ids: Vec<String>,
    pub date: String,
    pub original_date: String,
    pub release_types: Vec<String>,
    pub status: String,
    pub country: String,
    pub labels: Vec<String>,
    pub catalog_numbers: Vec<String>,
    pub barcode: String,
    pub script: String,
    pub release_id: String,
    pub release_group_id: String,
    pub disc_id: String,
    pub disc_number: u32,
    pub disc_total: u32,
    pub disc_subtitle: String,
    pub compilation: bool,
    /// Not identified by MusicBrainz: tagged `LISTEN_NEEDS_FIX=1`.
    pub needs_fix: bool,
    pub tracks: Vec<TrackMeta>,
}

fn names(c: &[Credit]) -> Vec<String> {
    c.iter().map(|c| c.name.clone()).filter(|n| !n.is_empty()).collect()
}
fn ids(c: &[Credit]) -> Vec<String> {
    c.iter().map(|c| c.id.clone()).filter(|n| !n.is_empty()).collect()
}

impl AlbumMeta {
    fn empty(source: MetaSource, toc: &Toc) -> Self {
        Self {
            source,
            album: String::new(),
            album_artist: String::new(),
            album_artist_sort: String::new(),
            album_artist_ids: Vec::new(),
            date: String::new(),
            original_date: String::new(),
            release_types: Vec::new(),
            status: String::new(),
            country: String::new(),
            labels: Vec::new(),
            catalog_numbers: Vec::new(),
            barcode: String::new(),
            script: String::new(),
            release_id: String::new(),
            release_group_id: String::new(),
            disc_id: toc.musicbrainz_id(),
            disc_number: 1,
            disc_total: 1,
            disc_subtitle: String::new(),
            compilation: false,
            needs_fix: source != MetaSource::MusicBrainz,
            tracks: Vec::new(),
        }
    }

    /// From a MusicBrainz release; `medium` is this disc's medium index.
    pub fn from_release(release: &Release, medium: usize, toc: &Toc) -> Self {
        let m = &release.media[medium];
        let mut meta = Self::empty(MetaSource::MusicBrainz, toc);
        meta.album = release.title.clone();
        meta.album_artist = credit_line(&release.credits);
        meta.album_artist_sort = credit_sort_line(&release.credits);
        meta.album_artist_ids = ids(&release.credits);
        meta.date = release.date.clone();
        meta.original_date = release.original_date.clone();
        meta.release_types = release.release_types.clone();
        meta.status = release.status.to_lowercase();
        meta.country = release.country.clone();
        meta.labels = release.labels.clone();
        meta.catalog_numbers = release.catalog_numbers.clone();
        meta.barcode = release.barcode.clone();
        meta.script = release.script.clone();
        meta.release_id = release.id.clone();
        meta.release_group_id = release.release_group_id.clone();
        meta.disc_number = m.position;
        meta.disc_total = release.media.len() as u32;
        meta.disc_subtitle = m.title.clone();
        meta.compilation = meta.album_artist_ids.iter().any(|i| i == VARIOUS_ARTISTS_ID)
            || meta.release_types.iter().any(|t| t == "compilation");
        let count = toc.audio_count().max(m.tracks.len());
        meta.tracks = (0..count)
            .map(|i| match m.tracks.get(i) {
                Some(t) => TrackMeta {
                    number: i as u32 + 1,
                    title: t.title.clone(),
                    artist: credit_line(&t.credits),
                    artist_sort: credit_sort_line(&t.credits),
                    artists: names(&t.credits),
                    artist_ids: ids(&t.credits),
                    recording_id: t.recording_id.clone(),
                    release_track_id: t.id.clone(),
                    isrcs: t.isrcs.clone(),
                },
                None => TrackMeta {
                    number: i as u32 + 1,
                    title: format!("Track {:02}", i + 1),
                    artist: meta.album_artist.clone(),
                    ..Default::default()
                },
            })
            .collect();
        meta
    }

    /// From CD-Text (album + track titles/performers); flagged for fixing.
    pub fn from_cdtext(text: &CdText, toc: &Toc, stamp: &str) -> Self {
        let mut meta = Self::unknown(toc, stamp);
        meta.source = MetaSource::CdText;
        if !text.title.is_empty() {
            meta.album = text.title.clone();
        }
        if !text.performer.is_empty() {
            meta.album_artist = text.performer.clone();
        }
        for (i, t) in meta.tracks.iter_mut().enumerate() {
            let (title, performer) = text.tracks.get(i).cloned().unwrap_or_default();
            if !title.is_empty() {
                t.title = title;
            }
            t.artist = if performer.is_empty() { meta.album_artist.clone() } else { performer };
            t.artists = vec![t.artist.clone()];
        }
        meta
    }

    /// "Unknown Artist" / "Unknown Album (2026-10-01 14.32)"; flagged for fixing.
    pub fn unknown(toc: &Toc, stamp: &str) -> Self {
        let mut meta = Self::empty(MetaSource::Unknown, toc);
        meta.album = format!("Unknown Album ({stamp})");
        meta.album_artist = "Unknown Artist".into();
        meta.tracks = (1..=toc.audio_count() as u32)
            .map(|n| TrackMeta {
                number: n,
                title: format!("Track {n:02}"),
                artist: "Unknown Artist".into(),
                artists: vec!["Unknown Artist".into()],
                ..Default::default()
            })
            .collect();
        meta
    }

    pub fn year(&self) -> &str {
        self.date.get(..4).unwrap_or("")
    }

    /// Vorbis comments for one track (Picard's names).
    pub fn vorbis_comments(&self, track: &TrackMeta) -> Vec<(String, String)> {
        let mut c: Vec<(String, String)> = Vec::new();
        let mut put = |k: &str, v: &str| {
            let v = v.replace(['\n', '\r'], " ");
            let v = v.trim();
            if !v.is_empty() {
                c.push((k.to_string(), v.to_string()));
            }
        };
        put("TITLE", &track.title);
        put("ARTIST", &track.artist);
        put("ARTISTSORT", &track.artist_sort);
        for a in &track.artists {
            put("ARTISTS", a);
        }
        put("ALBUM", &self.album);
        put("ALBUMARTIST", &self.album_artist);
        put("ALBUMARTISTSORT", &self.album_artist_sort);
        put("TRACKNUMBER", &track.number.to_string());
        put("TRACKTOTAL", &self.tracks.len().to_string());
        put("DISCNUMBER", &self.disc_number.to_string());
        put("DISCTOTAL", &self.disc_total.to_string());
        put("DISCSUBTITLE", &self.disc_subtitle);
        put("DATE", &self.date);
        put("ORIGINALDATE", &self.original_date);
        put("ORIGINALYEAR", self.original_date.get(..4).unwrap_or(""));
        for t in &self.release_types {
            put("RELEASETYPE", t);
        }
        put("RELEASESTATUS", &self.status);
        put("RELEASECOUNTRY", &self.country);
        for l in &self.labels {
            put("LABEL", l);
        }
        for n in &self.catalog_numbers {
            put("CATALOGNUMBER", n);
        }
        put("BARCODE", &self.barcode);
        put("SCRIPT", &self.script);
        put("MEDIA", "CD");
        for i in &track.isrcs {
            put("ISRC", i);
        }
        if self.compilation {
            put("COMPILATION", "1");
        }
        put("MUSICBRAINZ_ALBUMID", &self.release_id);
        put("MUSICBRAINZ_RELEASEGROUPID", &self.release_group_id);
        put("MUSICBRAINZ_TRACKID", &track.recording_id);
        put("MUSICBRAINZ_RELEASETRACKID", &track.release_track_id);
        for a in &track.artist_ids {
            put("MUSICBRAINZ_ARTISTID", a);
        }
        for a in &self.album_artist_ids {
            put("MUSICBRAINZ_ALBUMARTISTID", a);
        }
        put("MUSICBRAINZ_DISCID", &self.disc_id);
        put("ENCODEDBY", concat!("Listen ", env!("CARGO_PKG_VERSION"), " (whipper, FLAC -8)"));
        if self.needs_fix {
            put("LISTEN_NEEDS_FIX", "1");
        }
        c
    }
}

/// CD-Text as cdrdao writes it into the rip's `.toc` file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CdText {
    pub title: String,
    pub performer: String,
    /// (title, performer) per track.
    pub tracks: Vec<(String, String)>,
}

impl CdText {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.performer.is_empty() && self.tracks.iter().all(|(t, _)| t.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::musicbrainz::parse_lookup;

    fn revive_toc() -> Toc {
        Toc::from_offsets(
            1,
            154_800,
            &[150, 14625, 27975, 40575, 53175, 66750, 79425, 92325, 105150, 117975, 130200, 139500],
        )
    }

    fn get<'a>(c: &'a [(String, String)], k: &str) -> Vec<&'a str> {
        c.iter().filter(|(n, _)| n == k).map(|(_, v)| v.as_str()).collect()
    }

    #[test]
    fn single_disc_album() {
        let rs = parse_lookup(
            &serde_json::from_str(include_str!("../tests/fixtures/discid_revive.json")).unwrap(),
        );
        let toc = revive_toc();
        let r = &rs[0];
        let meta = AlbumMeta::from_release(r, r.medium_for(&toc.musicbrainz_id(), 12).unwrap(), &toc);
        assert_eq!(meta.tracks.len(), 12);
        assert!(!meta.needs_fix && !meta.compilation);
        let c = meta.vorbis_comments(&meta.tracks[2]);
        assert_eq!(get(&c, "TITLE"), ["숨바꼭질 (Hush)"]);
        assert_eq!(get(&c, "ALBUMARTIST"), ["IVE"]);
        assert_eq!(get(&c, "TRACKNUMBER"), ["3"]);
        assert_eq!(get(&c, "TRACKTOTAL"), ["12"]);
        assert_eq!(get(&c, "DISCNUMBER"), ["1"]);
        assert_eq!(get(&c, "DISCTOTAL"), ["1"]);
        assert_eq!(get(&c, "MUSICBRAINZ_DISCID"), ["7rEapOzTbtsuZwvusWs2r_d8duc-"]);
        assert_eq!(get(&c, "MUSICBRAINZ_ALBUMID"), [r.id.as_str()]);
        assert_eq!(get(&c, "MEDIA"), ["CD"]);
        assert!(get(&c, "DATE")[0].starts_with("2026"));
        assert_eq!(get(&c, "MUSICBRAINZ_TRACKID").len(), 1);
        assert!(get(&c, "LISTEN_NEEDS_FIX").is_empty());
    }

    #[test]
    fn second_disc_of_a_compilation() {
        let v = serde_json::from_str(include_str!("../tests/fixtures/release_now100.json")).unwrap();
        let r = &parse_lookup(&v)[0];
        // A fake TOC with disc 2's track count; the medium is found by disc ID.
        let toc = Toc::from_offsets(1, 300_000, &(0..21).map(|i| 150 + i * 14_000).collect::<Vec<_>>());
        let meta = AlbumMeta::from_release(r, 1, &toc);
        assert_eq!(meta.disc_number, 2);
        assert_eq!(meta.disc_total, 2);
        assert_eq!(meta.disc_subtitle, "NOW Classics");
        assert!(meta.compilation);
        assert_eq!(meta.album_artist, "Various Artists");
        let c = meta.vorbis_comments(&meta.tracks[0]);
        assert_eq!(get(&c, "COMPILATION"), ["1"]);
        assert_eq!(get(&c, "RELEASETYPE"), ["album", "compilation"]);
        assert_eq!(get(&c, "MUSICBRAINZ_ALBUMARTISTID"), [VARIOUS_ARTISTS_ID]);
        // Disc 1, track 1: two credited artists.
        let d1 = AlbumMeta::from_release(r, 0, &toc);
        let c1 = d1.vorbis_comments(&d1.tracks[0]);
        assert_eq!(get(&c1, "ARTIST"), ["Calvin Harris & Dua Lipa"]);
        assert_eq!(get(&c1, "ARTISTS"), ["Calvin Harris", "Dua Lipa"]);
        assert_eq!(get(&c1, "MUSICBRAINZ_ARTISTID").len(), 2);
    }

    #[test]
    fn unknown_and_cdtext() {
        let toc = revive_toc();
        let u = AlbumMeta::unknown(&toc, "2026-10-01 14.32");
        assert_eq!(u.album, "Unknown Album (2026-10-01 14.32)");
        assert_eq!(u.album_artist, "Unknown Artist");
        assert_eq!(u.tracks[11].title, "Track 12");
        assert_eq!(get(&u.vorbis_comments(&u.tracks[0]), "LISTEN_NEEDS_FIX"), ["1"]);

        let text = CdText {
            title: "Mixtape".into(),
            performer: "Band".into(),
            tracks: vec![("Intro".into(), String::new()), ("Guest Song".into(), "Guest".into())],
        };
        let m = AlbumMeta::from_cdtext(&text, &toc, "x");
        assert_eq!(m.source, MetaSource::CdText);
        assert_eq!((m.album.as_str(), m.album_artist.as_str()), ("Mixtape", "Band"));
        assert_eq!((m.tracks[0].title.as_str(), m.tracks[0].artist.as_str()), ("Intro", "Band"));
        assert_eq!(m.tracks[1].artist, "Guest");
        assert_eq!(m.tracks[2].title, "Track 03");
        assert!(m.needs_fix);
    }
}
