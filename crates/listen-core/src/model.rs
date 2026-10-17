//! Normalized library types every music source maps its API into (as Feishin's
//! `src/shared/types/domain-types.ts`).

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Which kind of backend an item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Navidrome and any other (Open)Subsonic-compatible server.
    Subsonic,
    Jellyfin,
    YouTubeMusic,
    /// A music folder on this computer.
    Local,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Subsonic => "Navidrome / OpenSubsonic",
            SourceKind::Jellyfin => "Jellyfin",
            SourceKind::YouTubeMusic => "YouTube Music",
            SourceKind::Local => "Local folder",
        }
    }
}

/// Identifies a configured server (a user-chosen UUID-like string).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServerId(pub String);

/// A reference to artwork; resolved to a URL by the owning source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageRef {
    /// Backend cover-art id (Subsonic `coverArt`, Jellyfin item id).
    Id(String),
    /// Absolute URL (YouTube thumbnails).
    Url(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtistRef {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GainInfo {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub id: String,
    pub server_id: ServerId,
    pub source: SourceKind,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub album_artists: Vec<ArtistRef>,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub image: Option<ImageRef>,
    #[serde(with = "duration_secs")]
    pub duration: Duration,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub codec: Option<String>,
    pub bit_rate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub bpm: Option<u32>,
    pub favorite: bool,
    pub rating: Option<u8>,
    pub play_count: Option<u32>,
    pub gain: GainInfo,
    pub explicit: bool,
    /// File path on the server (Subsonic `path`); for the details modal.
    #[serde(default)]
    pub path: Option<String>,
    /// File size in bytes.
    #[serde(default)]
    pub size: Option<u64>,
    /// File extension as the server reports it (`flac`, lower case).
    #[serde(default)]
    pub suffix: Option<String>,
    /// Channel count (2 = stereo).
    #[serde(default)]
    pub channels: Option<u32>,
    /// When the server added it (ISO 8601 as sent).
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    /// MusicBrainz ids the server knows (Discord links).
    #[serde(default)]
    pub mbz: MbzIds,
    /// Composers, dates and last play, for the columns of the song tables.
    #[serde(default)]
    pub extra: SongExtra,
}

/// What only some servers know about a song (table columns Composer, Date,
/// Release date and Last played). All empty where the source has no data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SongExtra {
    pub composers: Vec<String>,
    /// The date tag of the track (`2001-03-12`, or only `2001`).
    pub date: Option<String>,
    /// The date of the release the track is on.
    pub release_date: Option<String>,
    /// When the user last played it (ISO 8601 as sent).
    pub last_played: Option<String>,
}

/// MusicBrainz ids of a song: Subsonic/OpenSubsonic `musicBrainzId` (the
/// recording), Navidrome's native `mbzReleaseTrackId`/`mbzAlbumId`, Jellyfin
/// `ProviderIds` `MusicBrainzTrack`/`MusicBrainzAlbum`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MbzIds {
    pub track: Option<String>,
    pub recording: Option<String>,
    pub album: Option<String>,
}

impl Song {
    /// "Artist A, Artist B" as shown in lists and the player bar.
    pub fn artist_line(&self) -> String {
        join_names(&self.artists)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub server_id: ServerId,
    pub source: SourceKind,
    pub name: String,
    pub album_artists: Vec<ArtistRef>,
    pub image: Option<ImageRef>,
    pub year: Option<i32>,
    pub release_type: Option<String>,
    /// Every release type of the album (Subsonic `releaseTypes`: "album", "live", ...);
    /// `release_type` is the first.
    #[serde(default)]
    pub release_types: Vec<String>,
    pub genres: Vec<String>,
    pub song_count: Option<u32>,
    #[serde(with = "duration_secs")]
    pub duration: Duration,
    pub favorite: bool,
    pub rating: Option<u8>,
    pub play_count: Option<u32>,
    /// Filled by `album_detail`; empty in list responses.
    pub songs: Vec<Song>,
    /// Extra metadata of the album page (version, dates, labels, ...).
    #[serde(default)]
    pub info: AlbumInfo,
}

/// What only the album page shows. Sources fill what their server has.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AlbumInfo {
    /// "Deluxe Edition", "Remastered", ...
    pub version: Option<String>,
    /// ISO date (`2001-03-12`, or only `2001`) of the first release.
    pub original_date: Option<String>,
    /// ISO date of this release.
    pub release_date: Option<String>,
    /// When the user last played it (ISO 8601 as sent).
    #[serde(default)]
    pub last_played: Option<String>,
    pub explicit: bool,
    /// Total size of the tracks in bytes.
    pub size: Option<u64>,
    pub comment: Option<String>,
    pub record_labels: Vec<String>,
    pub moods: Vec<String>,
    pub groupings: Vec<String>,
    /// Further tags as (name, values).
    pub tags: Vec<(String, Vec<String>)>,
    pub musicbrainz_id: Option<String>,
    /// Titles of the discs, by disc number.
    pub disc_titles: Vec<(u32, String)>,
}

impl Album {
    pub fn artist_line(&self) -> String {
        join_names(&self.album_artists)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub server_id: ServerId,
    pub source: SourceKind,
    pub name: String,
    pub image: Option<ImageRef>,
    pub album_count: Option<u32>,
    pub song_count: Option<u32>,
    pub favorite: bool,
    /// The user's rating, 1-5 (Subsonic `userRating`).
    #[serde(default)]
    pub rating: Option<u8>,
    pub biography: Option<String>,
    pub similar: Vec<ArtistRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub server_id: ServerId,
    pub source: SourceKind,
    pub name: String,
    pub description: Option<String>,
    pub owner: Option<String>,
    pub image: Option<ImageRef>,
    pub song_count: Option<u32>,
    #[serde(with = "duration_secs")]
    pub duration: Duration,
    pub public: bool,
    /// Whether the current user may modify it.
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Genre {
    pub id: String,
    pub name: String,
    pub album_count: Option<u32>,
    pub song_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub folders: Vec<FolderEntry>,
    pub songs: Vec<Song>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderEntry {
    pub id: String,
    pub name: String,
}

/// One word of a word-synced (karaoke) line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordCue {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
}

/// One line of lyrics; `start` is `None` for unsynced lyrics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LyricLine {
    pub start: Option<Duration>,
    pub text: String,
    /// Word timings (enhanced LRC); empty for line-synced lyrics.
    #[serde(default)]
    pub words: Vec<WordCue>,
    /// A translation the provider or LRC file carries (NetEase `tlyric`).
    #[serde(default)]
    pub translation: Option<String>,
}

impl LyricLine {
    pub fn new(start: Option<Duration>, text: impl Into<String>) -> Self {
        Self { start, text: text.into(), ..Self::default() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lyrics {
    pub provider: String,
    pub synced: bool,
    pub lines: Vec<LyricLine>,
    /// What a remote provider matched, "title — artist" (Feishin's "show match").
    #[serde(default)]
    pub matched: Option<String>,
}

/// A page of results plus the total count when the backend knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub start_index: usize,
    pub total: Option<usize>,
}

impl<T> Page<T> {
    pub fn has_more(&self) -> bool {
        match self.total {
            Some(total) => self.start_index + self.items.len() < total,
            None => !self.items.is_empty(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchResults {
    pub songs: Vec<Song>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
}

pub fn join_names(refs: &[ArtistRef]) -> String {
    refs.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
}

/// Formats a duration like Feishin: `m:ss`, or `h:mm:ss` from one hour on.
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

mod duration_secs {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(d.as_secs_f64())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_secs_f64(f64::deserialize(d)?.max(0.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration(Duration::from_secs(5)), "0:05");
        assert_eq!(format_duration(Duration::from_secs(245)), "4:05");
        assert_eq!(format_duration(Duration::from_secs(3725)), "1:02:05");
    }

    #[test]
    fn page_has_more() {
        let page = Page { items: vec![1, 2], start_index: 0, total: Some(3) };
        assert!(page.has_more());
        let page = Page { items: vec![3], start_index: 2, total: Some(3) };
        assert!(!page.has_more());
        let page: Page<u8> = Page { items: vec![], start_index: 4, total: None };
        assert!(!page.has_more());
    }

    #[test]
    fn songs_and_albums_saved_before_the_new_fields_still_load() {
        // A queue or history entry written by an older version has no `extra`, no
        // `release_types` and no `last_played`.
        let song = Song {
            mbz: Default::default(),
            id: "1".into(),
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            title: "T".into(),
            artists: vec![],
            album_artists: vec![],
            album: None,
            album_id: None,
            image: None,
            duration: Duration::from_secs(1),
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
            extra: SongExtra { composers: vec!["C".into()], ..Default::default() },
        };
        let mut json: serde_json::Value = serde_json::to_value(&song).unwrap();
        json.as_object_mut().unwrap().remove("extra");
        let old: Song = serde_json::from_value(json).unwrap();
        assert_eq!(old.extra, SongExtra::default());
        let info: AlbumInfo = serde_json::from_str("{\"explicit\": true}").unwrap_or_default();
        assert!(info.last_played.is_none());
    }
}
