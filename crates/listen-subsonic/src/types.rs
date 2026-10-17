//! Raw (Open)Subsonic JSON types. Everything optional — servers differ a lot.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IdName {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Named {
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReplayGain {
    pub track_gain: Option<f32>,
    pub album_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_peak: Option<f32>,
}

/// A song (`Child` in the Subsonic schema).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Child {
    pub id: String,
    pub is_dir: bool,
    pub title: String,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub artist: Option<String>,
    pub artist_id: Option<String>,
    pub track: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub genres: Vec<Named>,
    pub cover_art: Option<String>,
    pub duration: Option<u64>,
    pub bit_rate: Option<u32>,
    pub sampling_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub bpm: Option<u32>,
    pub suffix: Option<String>,
    pub path: Option<String>,
    pub size: Option<u64>,
    pub channel_count: Option<u32>,
    pub created: Option<String>,
    pub comment: Option<String>,
    /// OpenSubsonic: the recording's MusicBrainz id.
    pub music_brainz_id: Option<String>,
    pub starred: Option<String>,
    pub user_rating: Option<u8>,
    pub play_count: Option<u32>,
    pub artists: Vec<IdName>,
    pub album_artists: Vec<IdName>,
    pub display_artist: Option<String>,
    pub display_album_artist: Option<String>,
    pub replay_gain: Option<ReplayGain>,
    pub explicit_status: Option<String>,
    /// OpenSubsonic: when the user last played it.
    pub played: Option<String>,
    /// OpenSubsonic: credits (`role` "composer", ...).
    pub contributors: Vec<Contributor>,
    pub release_date: Option<ItemDate>,
    pub original_release_date: Option<ItemDate>,
    /// Directory entries (`getMusicDirectory`) use `title` or `name`.
    pub name: Option<String>,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlbumId3 {
    pub id: String,
    pub name: String,
    pub artist: Option<String>,
    pub artist_id: Option<String>,
    pub artists: Vec<IdName>,
    pub display_artist: Option<String>,
    pub cover_art: Option<String>,
    pub song_count: Option<u32>,
    pub duration: Option<u64>,
    pub play_count: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub genres: Vec<Named>,
    pub starred: Option<String>,
    pub user_rating: Option<u8>,
    pub release_types: Vec<String>,
    /// OpenSubsonic: when the user last played it.
    pub played: Option<String>,
    pub version: Option<String>,
    pub original_release_date: Option<ItemDate>,
    pub release_date: Option<ItemDate>,
    pub explicit_status: Option<String>,
    pub record_labels: Vec<Named>,
    pub moods: Vec<String>,
    pub music_brainz_id: Option<String>,
    pub disc_titles: Vec<DiscTitle>,
    pub song: Vec<Child>,
}

/// OpenSubsonic `Contributor`: a credit with its role.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Contributor {
    pub role: String,
    pub artist: IdName,
}

/// OpenSubsonic `ItemDate`: any part may be missing.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemDate {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscTitle {
    pub disc: u32,
    pub title: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistId3 {
    pub id: String,
    pub name: String,
    pub cover_art: Option<String>,
    pub artist_image_url: Option<String>,
    pub album_count: Option<u32>,
    pub starred: Option<String>,
    pub user_rating: Option<u8>,
    pub album: Vec<AlbumId3>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistInfo {
    pub biography: Option<String>,
    pub music_brainz_id: Option<String>,
    pub last_fm_url: Option<String>,
    pub similar_artist: Vec<ArtistId3>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Index {
    pub name: String,
    pub artist: Vec<ArtistId3>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Artists {
    pub index: Vec<Index>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlbumList {
    pub album: Vec<AlbumId3>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Songs {
    pub song: Vec<Child>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResult3 {
    pub artist: Vec<ArtistId3>,
    pub album: Vec<AlbumId3>,
    pub song: Vec<Child>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub comment: Option<String>,
    pub owner: Option<String>,
    pub public: Option<bool>,
    pub readonly: Option<bool>,
    pub song_count: Option<u32>,
    pub duration: Option<u64>,
    pub cover_art: Option<String>,
    pub entry: Vec<Child>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Playlists {
    pub playlist: Vec<Playlist>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Genre {
    pub value: String,
    pub song_count: Option<u32>,
    pub album_count: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Genres {
    pub genre: Vec<Genre>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Directory {
    pub id: String,
    pub parent: Option<String>,
    pub name: String,
    pub child: Vec<Child>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MusicFolder {
    pub id: serde_json::Value,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MusicFolders {
    pub music_folder: Vec<MusicFolder>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LyricLine {
    /// Milliseconds.
    pub start: Option<u64>,
    pub value: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StructuredLyrics {
    pub synced: bool,
    pub line: Vec<LyricLine>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LyricsList {
    pub structured_lyrics: Vec<StructuredLyrics>,
}
