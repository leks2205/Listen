//! The [`MusicSource`] trait (Feishin's `ControllerEndpoint`), implemented by every backend on top
//! of `listen_core::model` types.

use async_trait::async_trait;
use bitflags::bitflags;
use listen_core::analysis::FileId;
use listen_core::model::{
    Album, Artist, Folder, Genre, Lyrics, Page, Playlist, SearchResults, Song, SourceKind,
};
use rand::seq::SliceRandom;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// How many playlist songs the default [`MusicSource::playlist_recommendations`] seeds from.
pub const RECOMMENDATION_SEEDS: usize = 3;

pub mod clientside;
pub mod nsp;
pub mod sorting;
pub use sorting::{QueryFilters, SortOption};

bitflags! {
    /// Optional features; the UI hides what a source does not support.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Capabilities: u32 {
        const FOLDERS         = 1 << 0;
        const RATINGS         = 1 << 1;
        const GENRES          = 1 << 2;
        /// [`Self::PLAYLIST_ADD`] + [`Self::PLAYLIST_CREATE`] + [`Self::PLAYLIST_DELETE`]; not removing
        /// songs or renaming. The UI checks the finer flags.
        const PLAYLIST_EDIT   = Self::PLAYLIST_ADD.bits() | Self::PLAYLIST_CREATE.bits() | Self::PLAYLIST_DELETE.bits();
        const PLAY_QUEUE_SYNC = 1 << 4;
        const SCROBBLE        = 1 << 5;
        const LYRICS          = 1 << 6;
        const SIMILAR_SONGS   = 1 << 7;
        const RADIO           = 1 << 8;
        const HOME_FEED       = 1 << 9;
        const ALBUM_ARTISTS   = 1 << 10;
        /// Songs of a playlist can be reordered.
        const PLAYLIST_REORDER = 1 << 11;
        /// Playlists can be renamed.
        const PLAYLIST_RENAME = 1 << 12;
        const DOWNLOAD        = 1 << 13;
        /// Internet radio stations (list/create/update/delete).
        const INTERNET_RADIO  = 1 << 14;
        /// Share links ([`MusicSource::create_share`] / [`MusicSource::shares`]).
        const SHARES          = 1 << 15;
        /// [`MusicSource::item_details`] (the "Get info" data).
        const ITEM_DETAILS    = 1 << 16;
        /// [`MusicSource::artist_info`] (bio, external links).
        const ARTIST_INFO     = 1 << 17;
        /// Smart (rule based) playlists.
        const SMART_PLAYLISTS = 1 << 18;
        /// [`MusicSource::upload_playlist_image`].
        const PLAYLIST_IMAGE  = 1 << 19;
        /// [`MusicSource::update_playlist`] (comment, public flag).
        const PLAYLIST_UPDATE = 1 << 20;
        /// Albums and artists can be favourited (`set_favorite_items` with a non-song kind).
        const FAVORITE_ITEMS  = 1 << 21;
        /// [`MusicSource::folder_tree`].
        const FOLDER_TREE     = 1 << 22;
        /// Songs can be added to an existing playlist.
        const PLAYLIST_ADD    = 1 << 23;
        /// New playlists can be created.
        const PLAYLIST_CREATE = 1 << 24;
        /// Playlists can be deleted.
        const PLAYLIST_DELETE = 1 << 25;
        /// Songs can be removed from a playlist ([`MusicSource::remove_from_playlist`]).
        const PLAYLIST_REMOVE = 1 << 26;
        /// The server speaks `jukeboxControl`; it may still refuse, so probe with [`MusicSource::jukebox`].
        const JUKEBOX         = 1 << 27;
        /// [`MusicSource::playlist_recommendations`]: songs that fit a playlist, for the
        /// "Recommended" strip under its rows.
        const PLAYLIST_RECOMMENDATIONS = 1 << 28;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("server error {code}: {message}")]
    Server { code: i32, message: String },
    #[error("unexpected response: {0}")]
    Parse(String),
    #[error("not found")]
    NotFound,
    #[error("not supported by this source")]
    Unsupported,
    /// The server asked us to slow down (HTTP 429); try again later.
    #[error("too many requests, try again in a minute")]
    RateLimited,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, SourceError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlbumSort {
    #[default]
    Name,
    Artist,
    Year,
    RecentlyAdded,
    RecentlyPlayed,
    MostPlayed,
    Random,
    Favorited,
    /// Album artist (Navidrome `album_artist`, Subsonic `alphabeticalByArtist`).
    AlbumArtist,
    /// The server's own order (no sort given).
    Id,
    Duration,
    Rating,
    SongCount,
    CommunityRating,
    CriticRating,
    ReleaseDate,
}

#[derive(Debug, Clone, Default)]
pub struct AlbumQuery {
    pub sort: AlbumSort,
    pub order: SortOrder,
    pub genre: Option<String>,
    pub artist_id: Option<String>,
    pub favorites_only: bool,
    /// Rated at least this many stars (1-5).
    pub min_rating: Option<u8>,
    /// Release year range, inclusive.
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    /// Any of these genres (in addition to `genre`).
    pub genres: Vec<String>,
    /// Any of these artists (in addition to `artist_id`).
    pub artist_ids: Vec<String>,
    /// Favourited (`Some(true)`) or not (`Some(false)`); `favorites_only` is the same as `Some(true)`.
    pub favorite: Option<bool>,
    /// Has a rating / has none (Navidrome `has_rating`).
    pub has_rating: Option<bool>,
    /// Is / is not a compilation.
    pub compilation: Option<bool>,
    /// Only albums that were played (Navidrome `recently_played`).
    pub recently_played: bool,
    /// Custom tag filters: tag name and the picked value ids (Navidrome `/api/tag`).
    pub tags: Vec<(String, Vec<String>)>,
    pub search: Option<String>,
    pub start: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SongSort {
    #[default]
    Name,
    Album,
    Artist,
    Year,
    RecentlyAdded,
    MostPlayed,
    Random,
    AlbumArtist,
    Bpm,
    Channels,
    Comment,
    Duration,
    Favorited,
    Genre,
    Id,
    Rating,
    RecentlyPlayed,
    ReleaseYear,
    ReleaseDate,
}

#[derive(Debug, Clone, Default)]
pub struct SongQuery {
    pub sort: SongSort,
    pub order: SortOrder,
    pub genre: Option<String>,
    pub favorites_only: bool,
    /// Rated at least this many stars (1-5).
    pub min_rating: Option<u8>,
    /// Release year range, inclusive.
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    /// Any of these genres (in addition to `genre`).
    pub genres: Vec<String>,
    /// Any of these artists.
    pub artist_ids: Vec<String>,
    /// Favourited (`Some(true)`) or not (`Some(false)`); `favorites_only` is the same as `Some(true)`.
    pub favorite: Option<bool>,
    pub has_rating: Option<bool>,
    /// Custom tag filters: tag name and the picked value ids.
    pub tags: Vec<(String, Vec<String>)>,
    pub search: Option<String>,
    pub start: usize,
    pub limit: usize,
}

/// A tag the server can filter by (Navidrome mood, release type, ...) and its values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagFilter {
    /// The tag as the server names it (the query parameter).
    pub name: String,
    /// (value id, value) sorted by value.
    pub options: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArtistSort {
    #[default]
    Name,
    AlbumCount,
    SongCount,
    Favorited,
    Rating,
    RecentlyAdded,
    Random,
    MostPlayed,
    Duration,
    /// Number of albums the artist is on (Jellyfin sorts by album name).
    Album,
}

/// Filters and sorting for [`MusicSource::artists_query`].
#[derive(Debug, Clone, Default)]
pub struct ArtistQuery {
    pub sort: ArtistSort,
    pub order: SortOrder,
    pub favorites_only: bool,
    pub favorite: Option<bool>,
    pub min_rating: Option<u8>,
    pub genres: Vec<String>,
    pub search: Option<String>,
    pub start: usize,
    pub limit: usize,
}

impl AlbumQuery {
    /// The favourite filter, folding in `favorites_only`.
    pub fn favorite_filter(&self) -> Option<bool> {
        if self.favorites_only { Some(true) } else { self.favorite }
    }
}

impl SongQuery {
    /// The favourite filter, folding in `favorites_only`.
    pub fn favorite_filter(&self) -> Option<bool> {
        if self.favorites_only { Some(true) } else { self.favorite }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ListQuery {
    pub search: Option<String>,
    pub start: usize,
    pub limit: usize,
}

/// Pseudo-header (never sent) marking a stream whose invalid certificate is accepted ("ignore SSL").
pub const INSECURE_TLS_HEADER: &str = "x-listen-insecure-tls";

/// What the player should open for a song.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamSource {
    pub url: String,
    /// Extra HTTP headers the stream needs (YouTube).
    pub headers: HashMap<String, String>,
    /// When the URL stops working (YouTube URLs expire after hours).
    pub expires_at: Option<SystemTime>,
}

impl StreamSource {
    /// Marks the stream as served with an invalid certificate that is accepted.
    pub fn insecure_tls(mut self, insecure: bool) -> Self {
        if insecure {
            self.headers.insert(INSECURE_TLS_HEADER.into(), "1".into());
        }
        self
    }

    pub fn direct(url: impl Into<String>) -> Self {
        Self { url: url.into(), headers: HashMap::new(), expires_at: None }
    }

    /// True if the URL expires within `margin` (or already has).
    pub fn expires_within(&self, margin: Duration) -> bool {
        self.expires_at
            .is_some_and(|t| t.duration_since(SystemTime::now()).map_or(true, |left| left < margin))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrobbleEvent {
    NowPlaying,
    Submission,
}

/// Audio for the analyser or a DJ deck.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioInput {
    Url(String),
    File(std::path::PathBuf),
}

impl AudioInput {
    /// What mpv is given (URL or path).
    pub fn as_mpv_arg(&self) -> String {
        match self {
            AudioInput::Url(u) => u.clone(),
            AudioInput::File(p) => p.to_string_lossy().into_owned(),
        }
    }
}

/// The audio to analyse and which exact file it is (stored in the record).
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisSource {
    pub input: AudioInput,
    pub file: FileId,
}

/// How soon an analysis is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// Playing, next or about to be picked: a transition depends on it.
    Now,
    /// Library or candidate-pool work that can wait.
    Background,
}

/// What a favourite, rating, share or details call is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    Song,
    Album,
    Artist,
}

/// Server data about an item beyond the list model, for the "Get info" modal; all optional.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemDetails {
    pub title: String,
    pub path: Option<String>,
    pub size: Option<u64>,
    pub suffix: Option<String>,
    pub bit_rate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub channels: Option<u32>,
    pub duration: Option<Duration>,
    pub bpm: Option<u32>,
    /// When the server added it (as sent).
    pub created: Option<String>,
    /// When the file or entry last changed (as sent).
    pub updated: Option<String>,
    pub last_played: Option<String>,
    pub play_count: Option<u32>,
    pub comment: Option<String>,
    /// The main MusicBrainz id of the item (recording / release / artist).
    pub musicbrainz_id: Option<String>,
    /// Further MusicBrainz ids as (label, id): "Track", "Album", "Artist", ...
    pub mbids: Vec<(String, String)>,
    pub gain: listen_core::model::GainInfo,
    pub explicit: Option<bool>,
    pub compilation: Option<bool>,
    /// The song has lyrics (embedded or on the server).
    pub has_lyrics: Option<bool>,
    pub record_labels: Vec<String>,
    pub release_type: Option<String>,
    pub version: Option<String>,
    /// Additional participants as (role, name).
    pub participants: Vec<(String, String)>,
    /// Further tag name/value pairs as the server has them.
    pub tags: Vec<(String, String)>,
}

/// Links to an artist on other sites.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalLinks {
    pub musicbrainz_url: Option<String>,
    pub lastfm_url: Option<String>,
}

/// Artist biography and external links ([`MusicSource::artist_info`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArtistInfo {
    /// May contain HTML links (Last.fm).
    pub biography: Option<String>,
    pub links: ExternalLinks,
    /// MusicBrainz id, where the server knows one.
    pub musicbrainz_id: Option<String>,
    pub similar: Vec<listen_core::model::ArtistRef>,
    /// The similar artists that are in the library, with their images.
    pub similar_artists: Vec<Artist>,
    /// Genres of the artist, if the server has them (else the page derives them from the albums).
    pub genres: Vec<String>,
}

/// A saved play queue to sync between clients (Subsonic `getPlayQueue`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayQueue {
    pub songs: Vec<Song>,
    /// The song that was playing.
    pub current: Option<String>,
    pub position: Duration,
    /// Which client saved it.
    pub changed_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Share {
    pub id: String,
    pub url: String,
    pub description: Option<String>,
    pub expires: Option<SystemTime>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InternetRadio {
    /// Empty when creating.
    pub id: String,
    pub name: String,
    pub stream_url: String,
    pub home_page_url: Option<String>,
    /// The station's picture: the server's `coverArt` id (Subsonic).
    pub image_id: Option<String>,
}

/// A `jukeboxControl` request (server-side playback, Subsonic API).
#[derive(Debug, Clone, PartialEq)]
pub enum JukeboxCmd {
    /// Only the status (also the support probe).
    Status,
    /// Replaces the jukebox queue with these song ids.
    Set(Vec<String>),
    Add(Vec<String>),
    Clear,
    Start,
    Stop,
    /// Jumps to queue entry `index`, `offset_secs` into it.
    Skip {
        index: usize,
        offset_secs: u32,
    },
    /// Volume, 0.0 to 1.0.
    SetGain(f32),
}

/// What the jukebox is doing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JukeboxStatus {
    /// Index in the jukebox queue; -1 when nothing is queued.
    pub current_index: i64,
    pub playing: bool,
    pub gain: f32,
    pub position_secs: u32,
}

/// Changes to a playlist's metadata; `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistUpdate {
    pub name: Option<String>,
    pub comment: Option<String>,
    pub public: Option<bool>,
}

/// What a new playlist is created with (Feishin's create form: name, description, public).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewPlaylist {
    pub name: String,
    pub comment: String,
    pub public: bool,
}

/// `(from, to)` moves turning `0..n` into `order` (`order[i]`: current index of the song for
/// position `i`). Each move applies after the previous ones, as [`MusicSource::move_in_playlist`].
pub fn reorder_moves(order: &[usize]) -> Vec<(usize, usize)> {
    let mut current: Vec<usize> = (0..order.len()).collect();
    let mut moves = Vec::new();
    for (to, want) in order.iter().enumerate() {
        let Some(from) = current.iter().position(|c| c == want) else { continue };
        if from != to {
            let item = current.remove(from);
            current.insert(to, item);
            moves.push((from, to));
        }
    }
    moves
}

/// A Navidrome smart playlist as the editor saves and loads it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SmartPlaylistDef {
    pub name: String,
    pub comment: String,
    pub public: bool,
    /// Navidrome re-evaluates the rules whenever the playlist is read ("auto-refresh").
    pub sync: bool,
    pub rules: nsp::Nsp,
}

/// One node of the folder tree ([`MusicSource::folder_tree`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderNode {
    pub id: String,
    pub name: String,
    pub children: Vec<FolderNode>,
}

/// A labelled row of items on the home page (YouTube Music shelves, "Recently added", ...).
#[derive(Debug, Clone)]
pub struct HomeShelf {
    pub title: String,
    pub albums: Vec<Album>,
    pub playlists: Vec<Playlist>,
    pub songs: Vec<Song>,
}

#[async_trait]
pub trait MusicSource: Send + Sync {
    fn kind(&self) -> SourceKind;
    fn capabilities(&self) -> Capabilities;

    /// Checks the credentials; called once when a server is added or selected.
    async fn ping(&self) -> Result<()>;

    // What the list headers offer (sort options and filters) for this server.
    fn album_sorts(&self) -> &'static [SortOption<AlbumSort>] {
        sorting::SUBSONIC_ALBUMS
    }
    fn song_sorts(&self) -> &'static [SortOption<SongSort>] {
        sorting::SUBSONIC_SONGS
    }
    fn artist_sorts(&self) -> &'static [SortOption<ArtistSort>] {
        sorting::SUBSONIC_ARTISTS
    }
    fn album_filters(&self) -> QueryFilters {
        QueryFilters::FAVORITE | QueryFilters::GENRES | QueryFilters::ARTISTS | QueryFilters::YEAR_RANGE
    }
    fn song_filters(&self) -> QueryFilters {
        QueryFilters::empty()
    }
    fn artist_filters(&self) -> QueryFilters {
        QueryFilters::FAVORITE
    }

    /// Drops cached library data so the next reads refetch it (the list header's Refresh).
    async fn refresh(&self) {}

    /// Tags a list can be filtered by ([`QueryFilters::TAGS`]); `kind` is `Album` or `Song`.
    async fn tag_filters(&self, kind: ItemKind) -> Result<Vec<TagFilter>> {
        let _ = kind;
        Err(SourceError::Unsupported)
    }

    // Albums
    async fn albums(&self, query: &AlbumQuery) -> Result<Page<Album>>;
    /// Album with its `songs` filled in.
    async fn album(&self, id: &str) -> Result<Album>;

    // Artists
    async fn artists(&self, query: &ListQuery) -> Result<Page<Artist>>;
    async fn artist(&self, id: &str) -> Result<Artist>;
    async fn artist_albums(&self, id: &str) -> Result<Vec<Album>>;
    async fn top_songs(&self, artist: &Artist, count: usize) -> Result<Vec<Song>>;

    // Songs
    async fn songs(&self, query: &SongQuery) -> Result<Page<Song>>;
    async fn random_songs(&self, count: usize, genre: Option<&str>) -> Result<Vec<Song>>;
    async fn similar_songs(&self, song: &Song, count: usize) -> Result<Vec<Song>> {
        let _ = (song, count);
        Err(SourceError::Unsupported)
    }

    // Browse
    async fn genres(&self) -> Result<Vec<Genre>> {
        Err(SourceError::Unsupported)
    }
    async fn folder(&self, id: Option<&str>) -> Result<Folder> {
        let _ = id;
        Err(SourceError::Unsupported)
    }
    async fn search(&self, text: &str, limit: usize) -> Result<SearchResults>;
    async fn home(&self) -> Result<Vec<HomeShelf>> {
        Err(SourceError::Unsupported)
    }

    // Playlists
    async fn playlists(&self) -> Result<Vec<Playlist>>;
    async fn playlist_songs(&self, id: &str) -> Result<Vec<Song>>;
    /// One page of a playlist and the cursor of the next; sources without paging return everything.
    async fn playlist_page(&self, id: &str, cursor: Option<&str>) -> Result<(Vec<Song>, Option<String>)> {
        match cursor {
            None => Ok((self.playlist_songs(id).await?, None)),
            Some(_) => Ok((vec![], None)),
        }
    }
    /// Songs that fit a playlist, for the "Recommended" strip under its rows
    /// ([`Capabilities::PLAYLIST_RECOMMENDATIONS`]). `songs` are the rows the page shows (the
    /// playlist's own songs); anything among them is left out. The default seeds the source's own
    /// similarity with a few of them and picks at random, so asking again reshuffles.
    async fn playlist_recommendations(&self, songs: &[Song], limit: usize) -> Result<Vec<Song>> {
        if !self.capabilities().contains(Capabilities::PLAYLIST_RECOMMENDATIONS) {
            return Err(SourceError::Unsupported);
        }
        if songs.is_empty() || limit == 0 {
            return Ok(vec![]);
        }
        // Seeds spread over the playlist: its last, first and middle song.
        let mut seeds: Vec<&Song> = Vec::new();
        for at in [songs.len() - 1, 0, songs.len() / 2] {
            let Some(song) = songs.get(at) else { continue };
            if seeds.len() < RECOMMENDATION_SEEDS && !seeds.iter().any(|s| s.id == song.id) {
                seeds.push(song);
            }
        }
        seeds.shuffle(&mut rand::rng());
        let have: std::collections::HashSet<&str> = songs.iter().map(|s| s.id.as_str()).collect();
        let mut pool: Vec<Song> = Vec::new();
        for seed in seeds {
            // One similarity request per seed, and only until there are enough candidates.
            for song in self.similar_songs(seed, limit * 3).await.unwrap_or_default() {
                if have.contains(song.id.as_str()) || pool.iter().any(|p| p.id == song.id) {
                    continue;
                }
                pool.push(song);
            }
            if pool.len() >= limit * 2 {
                break;
            }
        }
        pool.shuffle(&mut rand::rng());
        pool.truncate(limit);
        Ok(pool)
    }
    async fn create_playlist(&self, name: &str, songs: &[String]) -> Result<Playlist> {
        let _ = (name, songs);
        Err(SourceError::Unsupported)
    }
    async fn add_to_playlist(&self, id: &str, songs: &[String]) -> Result<()> {
        let _ = (id, songs);
        Err(SourceError::Unsupported)
    }
    /// Removes entries by their position in the playlist.
    async fn remove_from_playlist(&self, id: &str, indices: &[usize]) -> Result<()> {
        let _ = (id, indices);
        Err(SourceError::Unsupported)
    }
    /// The rules of a smart playlist (`Ok(None)`: a plain playlist). Navidrome only.
    async fn smart_playlist(&self, id: &str) -> Result<Option<SmartPlaylistDef>> {
        let _ = id;
        Err(SourceError::Unsupported)
    }
    /// Ids of the smart playlists, for the "smart" badge. Empty where there are none.
    async fn smart_playlist_ids(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
    async fn create_smart_playlist(&self, def: &SmartPlaylistDef) -> Result<Playlist> {
        let _ = def;
        Err(SourceError::Unsupported)
    }
    async fn update_smart_playlist(&self, id: &str, def: &SmartPlaylistDef) -> Result<()> {
        let _ = (id, def);
        Err(SourceError::Unsupported)
    }
    /// Playlists `songs` can be added to ("Add to playlist"): the user's own.
    async fn playlists_for_adding(&self, songs: &[String]) -> Result<Vec<Playlist>> {
        let _ = songs;
        Ok(self.playlists().await?.into_iter().filter(|p| p.editable).collect())
    }
    /// Streaming quality for songs played from now on; ignored by sources without a choice.
    fn set_stream_quality(&self, quality: listen_core::settings::StreamQuality) {
        let _ = quality;
    }
    /// Explicit transcoding for songs played from now on, overriding the stream quality while
    /// enabled; ignored by sources that can't transcode.
    fn set_transcode(&self, transcode: &listen_core::settings::Transcode) {
        let _ = transcode;
    }
    /// URL of the song's original file for analysis and DJ mixing. `None`: no direct URL
    /// (YouTube Music uses [`Self::analysis_source`]).
    fn analysis_url(&self, song: &Song) -> Option<String> {
        let _ = song;
        None
    }
    /// Audio to draw a waveform from without a new request (local file, cached copy, direct URL).
    /// `None`: no waveform.
    fn waveform_input(&self, song: &Song) -> Option<AudioInput> {
        self.local_path(song).map(AudioInput::File).or_else(|| self.analysis_url(song).map(AudioInput::Url))
    }
    /// Songs of this source can be analysed and DJ-mixed.
    fn analysable(&self) -> bool {
        false
    }
    /// Where the analyser reads the song's audio, and which file it is. Budgeted sources may delay
    /// or refuse `Background` requests; `Ok(None)`: not analysable.
    async fn analysis_source(&self, song: &Song, urgency: Urgency) -> Result<Option<AnalysisSource>> {
        let _ = urgency;
        Ok(self
            .analysis_url(song)
            .map(|url| AnalysisSource { input: AudioInput::Url(url), file: FileId::of(song) }))
    }
    /// What the second deck plays when mixing into `song`: the analysed audio, a prefetched local
    /// copy for YouTube. `Ok(None)`: no mix.
    async fn mix_input(&self, song: &Song) -> Result<Option<AudioInput>> {
        Ok(self.analysis_url(song).map(AudioInput::Url))
    }
    /// Pins or releases the local copy from [`Self::mix_input`], so cache eviction can't remove it
    /// while a deck plays it.
    fn pin_mix(&self, song: &Song, pinned: bool) {
        let _ = (song, pinned);
    }
    /// Download URL of the song's original file. `None`: use [`Self::local_copy`] if the source has
    /// [`Capabilities::DOWNLOAD`].
    fn download_url(&self, song: &Song) -> Option<String> {
        let _ = song;
        None
    }
    /// A finished local file of the song's audio for the caller to copy; `Ok(None)`: none.
    async fn local_copy(&self, song: &Song) -> Result<Option<std::path::PathBuf>> {
        let _ = song;
        Ok(None)
    }
    /// The song's music video stream; `None` if it has none.
    async fn video_stream(&self, song: &Song) -> Result<Option<StreamSource>> {
        let _ = song;
        Ok(None)
    }
    /// The song's local file, for "Show in file manager" and the tag editor.
    fn local_path(&self, song: &Song) -> Option<std::path::PathBuf> {
        let _ = song;
        None
    }
    /// A link to the song to share, if the source has public ones.
    fn share_url(&self, song: &Song) -> Option<String> {
        let _ = song;
        None
    }
    async fn delete_playlist(&self, id: &str) -> Result<()> {
        let _ = id;
        Err(SourceError::Unsupported)
    }
    async fn rename_playlist(&self, id: &str, name: &str) -> Result<()> {
        let _ = (id, name);
        Err(SourceError::Unsupported)
    }
    /// Creates a playlist with description and visibility. Default: [`Self::create_playlist`], then
    /// [`Self::update_playlist`] if needed.
    async fn create_playlist_full(&self, new: &NewPlaylist, songs: &[String]) -> Result<Playlist> {
        let mut created = self.create_playlist(&new.name, songs).await?;
        if !new.comment.trim().is_empty() || new.public {
            let update = PlaylistUpdate {
                comment: Some(new.comment.clone()).filter(|c| !c.trim().is_empty()),
                public: new.public.then_some(true),
                ..Default::default()
            };
            match self.update_playlist(&created.id, &update).await {
                Ok(()) | Err(SourceError::Unsupported) => {}
                Err(e) => return Err(e),
            }
            created.description = Some(new.comment.clone()).filter(|c| !c.trim().is_empty());
            created.public = new.public;
        }
        Ok(created)
    }
    /// Saves a new order (`order[i]`: current index of the song for position `i`). Default: one
    /// [`Self::move_in_playlist`] per moved song.
    async fn set_playlist_order(&self, id: &str, order: &[usize]) -> Result<()> {
        for (from, to) in reorder_moves(order) {
            self.move_in_playlist(id, from, to).await?;
        }
        Ok(())
    }
    /// Removes the uploaded cover ([`Capabilities::PLAYLIST_IMAGE`]).
    async fn delete_playlist_image(&self, id: &str) -> Result<()> {
        let _ = id;
        Err(SourceError::Unsupported)
    }
    /// Moves the song at `from` to `to` (indices into the playlist as it is now).
    async fn move_in_playlist(&self, id: &str, from: usize, to: usize) -> Result<()> {
        let _ = (id, from, to);
        Err(SourceError::Unsupported)
    }

    // User data
    async fn set_favorite(&self, song_ids: &[String], favorite: bool) -> Result<()>;
    /// Favourites songs, albums or artists; non-song kinds need [`Capabilities::FAVORITE_ITEMS`].
    async fn set_favorite_items(&self, kind: ItemKind, ids: &[String], favorite: bool) -> Result<()> {
        match kind {
            ItemKind::Song => self.set_favorite(ids, favorite).await,
            ItemKind::Album | ItemKind::Artist => Err(SourceError::Unsupported),
        }
    }
    /// Every favourite album ([`Capabilities::FAVORITE_ITEMS`]); saved albums on YouTube Music.
    async fn favorite_albums(&self) -> Result<Vec<Album>> {
        Err(SourceError::Unsupported)
    }
    /// Every favourite artist ([`Capabilities::FAVORITE_ITEMS`]); subscriptions on YouTube Music.
    async fn favorite_artists(&self) -> Result<Vec<Artist>> {
        Err(SourceError::Unsupported)
    }
    async fn set_rating(&self, id: &str, rating: u8) -> Result<()> {
        let _ = (id, rating);
        Err(SourceError::Unsupported)
    }
    /// Rates a song, album or artist (0 clears). Songs go through [`Self::set_rating`].
    async fn set_item_rating(&self, kind: ItemKind, id: &str, rating: u8) -> Result<()> {
        match kind {
            ItemKind::Song => self.set_rating(id, rating).await,
            ItemKind::Album | ItemKind::Artist => Err(SourceError::Unsupported),
        }
    }
    /// The play queue saved on the server ([`Capabilities::PLAY_QUEUE_SYNC`]); `Ok(None)`: none saved.
    async fn play_queue(&self) -> Result<Option<PlayQueue>> {
        Err(SourceError::Unsupported)
    }
    async fn save_play_queue(&self, queue: &PlayQueue) -> Result<()> {
        let _ = queue;
        Err(SourceError::Unsupported)
    }

    // Sharing ([`Capabilities::SHARES`])
    /// A public link to the items, optionally expiring.
    async fn create_share(
        &self,
        kind: ItemKind,
        ids: &[String],
        description: Option<&str>,
        expires: Option<SystemTime>,
    ) -> Result<Share> {
        let _ = (kind, ids, description, expires);
        Err(SourceError::Unsupported)
    }
    async fn shares(&self) -> Result<Vec<Share>> {
        Err(SourceError::Unsupported)
    }

    // Internet radio ([`Capabilities::INTERNET_RADIO`])
    async fn internet_radios(&self) -> Result<Vec<InternetRadio>> {
        Err(SourceError::Unsupported)
    }
    async fn create_internet_radio(&self, radio: &InternetRadio) -> Result<InternetRadio> {
        let _ = radio;
        Err(SourceError::Unsupported)
    }
    async fn update_internet_radio(&self, radio: &InternetRadio) -> Result<()> {
        let _ = radio;
        Err(SourceError::Unsupported)
    }
    async fn delete_internet_radio(&self, id: &str) -> Result<()> {
        let _ = id;
        Err(SourceError::Unsupported)
    }

    /// Server-side playback ([`Capabilities::JUKEBOX`]).
    async fn jukebox(&self, cmd: &JukeboxCmd) -> Result<JukeboxStatus> {
        let _ = cmd;
        Err(SourceError::Unsupported)
    }

    // Details ([`Capabilities::ITEM_DETAILS`], [`Capabilities::ARTIST_INFO`])
    async fn item_details(&self, kind: ItemKind, id: &str) -> Result<ItemDetails> {
        let _ = (kind, id);
        Err(SourceError::Unsupported)
    }
    /// Biography, similar artists and links to MusicBrainz / Last.fm.
    async fn artist_info(&self, id: &str) -> Result<ArtistInfo> {
        let _ = id;
        Err(SourceError::Unsupported)
    }
    /// Albums the artist appears on without being the album artist; empty if unknown.
    async fn appears_on(&self, artist: &Artist) -> Result<Vec<Album>> {
        let _ = artist;
        Ok(Vec::new())
    }
    /// Music videos of the artist (YouTube Music's Videos shelf), as songs.
    async fn artist_videos(&self, artist: &Artist) -> Result<Vec<Song>> {
        let _ = artist;
        Ok(Vec::new())
    }
    /// Every song of the artist, album by album, as in Feishin. Override where cheaper.
    async fn artist_songs(&self, artist: &Artist) -> Result<Vec<Song>> {
        // Newest first like the page's default order, then the albums the artist guests on.
        let mut albums = self.artist_albums(&artist.id).await?;
        albums.sort_by_key(|a| std::cmp::Reverse(a.year.unwrap_or(0)));
        albums.extend(self.appears_on(artist).await.unwrap_or_default());
        use futures::StreamExt;
        let ids: Vec<String> = albums.iter().map(|a| a.id.clone()).collect();
        let loaded: Vec<_> = futures::stream::iter(ids)
            .map(|id| async move { self.album(&id).await })
            .buffered(6)
            .collect()
            .await;
        let mut songs = Vec::new();
        for album in loaded.into_iter().flatten() {
            songs.extend(album.songs.into_iter().filter(|s| is_by(s, artist)));
        }
        Ok(songs)
    }
    /// Songs for "Artist radio": the artist's top song, then similar ones.
    async fn artist_radio(&self, artist: &Artist, count: usize) -> Result<Vec<Song>> {
        let seed = self.top_songs(artist, 1).await?.into_iter().next().ok_or(SourceError::NotFound)?;
        let mut songs = vec![seed.clone()];
        songs.extend(self.similar_songs(&seed, count.saturating_sub(1).max(1)).await?);
        songs.truncate(count.max(1));
        Ok(songs)
    }
    /// Artists with filters and sorting; the default applies only search and paging.
    async fn artists_query(&self, query: &ArtistQuery) -> Result<Page<Artist>> {
        self.artists(&ListQuery { search: query.search.clone(), start: query.start, limit: query.limit })
            .await
    }
    /// The whole folder hierarchy ([`Capabilities::FOLDER_TREE`]).
    async fn folder_tree(&self) -> Result<Vec<FolderNode>> {
        Err(SourceError::Unsupported)
    }

    // Playlist metadata ([`Capabilities::PLAYLIST_UPDATE`], [`Capabilities::PLAYLIST_IMAGE`])
    async fn update_playlist(&self, id: &str, update: &PlaylistUpdate) -> Result<()> {
        let _ = (id, update);
        Err(SourceError::Unsupported)
    }
    /// Sets the playlist cover from an image file's bytes (`mime` e.g. `image/png`).
    async fn upload_playlist_image(&self, id: &str, image: &[u8], mime: &str) -> Result<()> {
        let _ = (id, image, mime);
        Err(SourceError::Unsupported)
    }
    async fn scrobble(&self, song: &Song, event: ScrobbleEvent) -> Result<()> {
        let _ = (song, event);
        Ok(())
    }

    // Media
    async fn stream(&self, song: &Song) -> Result<StreamSource>;
    /// Drops a cached stream URL that failed, so the next [`MusicSource::stream`] resolves a new one.
    fn forget_stream(&self, _song: &Song) {}
    /// The same stream from a backup server, if any (YouTube edge nodes sometimes refuse).
    fn alternate_url(&self, _url: &str) -> Option<String> {
        None
    }
    /// URL for artwork at roughly `size` pixels (the image cache fetches it).
    fn image_url(&self, image: &listen_core::model::ImageRef, size: u32) -> Option<String>;
    async fn lyrics(&self, song: &Song) -> Result<Option<Lyrics>> {
        let _ = song;
        Ok(None)
    }

    // Library maintenance
    /// Starts a library scan on the server (Navidrome: admins only).
    async fn start_scan(&self) -> Result<ScanStatus> {
        Err(SourceError::Unsupported)
    }
    async fn scan_status(&self) -> Result<ScanStatus> {
        Err(SourceError::Unsupported)
    }
}

/// Whether the artist is one of the song's artists or album artists.
pub fn is_by(song: &Song, artist: &Artist) -> bool {
    song.artists.iter().chain(&song.album_artists).any(|a| match &a.id {
        Some(id) => *id == artist.id,
        None => a.name.eq_ignore_ascii_case(&artist.name),
    })
}

/// A server's library scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanStatus {
    pub scanning: bool,
    /// Files seen so far (Navidrome's `count`).
    pub count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_moves_turn_the_playlist_into_the_order() {
        let apply = |order: &[usize]| {
            let mut v: Vec<usize> = (0..order.len()).collect();
            for (from, to) in reorder_moves(order) {
                let x = v.remove(from);
                v.insert(to, x);
            }
            v
        };
        for order in [vec![0, 1, 2, 3], vec![3, 2, 1, 0], vec![1, 2, 3, 0], vec![2, 0, 3, 1], vec![]] {
            assert_eq!(apply(&order), order);
        }
        assert!(reorder_moves(&[0, 1, 2]).is_empty());
        // A single song moved to the front is one move.
        assert_eq!(reorder_moves(&[2, 0, 1]), vec![(2, 0)]);
    }

    #[test]
    fn stream_expiry() {
        let s = StreamSource::direct("http://x");
        assert!(!s.expires_within(Duration::from_secs(60)));
        let soon = StreamSource {
            expires_at: Some(SystemTime::now() + Duration::from_secs(30)),
            ..StreamSource::direct("http://x")
        };
        assert!(soon.expires_within(Duration::from_secs(60)));
        assert!(!soon.expires_within(Duration::from_secs(5)));
    }

    use listen_core::model::ImageRef;
    use std::sync::Mutex;

    /// Implements only the required methods; records what `set_favorite` / `set_rating` got.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl MusicSource for Fake {
        fn kind(&self) -> SourceKind {
            SourceKind::Subsonic
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::empty()
        }
        async fn ping(&self) -> Result<()> {
            Ok(())
        }
        async fn albums(&self, _: &AlbumQuery) -> Result<Page<Album>> {
            Ok(Page { items: vec![], start_index: 0, total: Some(0) })
        }
        async fn album(&self, _: &str) -> Result<Album> {
            Err(SourceError::NotFound)
        }
        async fn artists(&self, q: &ListQuery) -> Result<Page<Artist>> {
            self.calls.lock().unwrap().push(format!("artists search={:?}", q.search));
            Ok(Page { items: vec![], start_index: q.start, total: Some(0) })
        }
        async fn artist(&self, _: &str) -> Result<Artist> {
            Err(SourceError::NotFound)
        }
        async fn artist_albums(&self, _: &str) -> Result<Vec<Album>> {
            Ok(vec![])
        }
        async fn top_songs(&self, _: &Artist, _: usize) -> Result<Vec<Song>> {
            Ok(vec![])
        }
        async fn songs(&self, _: &SongQuery) -> Result<Page<Song>> {
            Ok(Page { items: vec![], start_index: 0, total: Some(0) })
        }
        async fn random_songs(&self, _: usize, _: Option<&str>) -> Result<Vec<Song>> {
            Ok(vec![])
        }
        async fn search(&self, _: &str, _: usize) -> Result<SearchResults> {
            Ok(SearchResults::default())
        }
        async fn playlists(&self) -> Result<Vec<Playlist>> {
            Ok(vec![])
        }
        async fn playlist_songs(&self, _: &str) -> Result<Vec<Song>> {
            Ok(vec![])
        }
        async fn set_favorite(&self, ids: &[String], favorite: bool) -> Result<()> {
            self.calls.lock().unwrap().push(format!("favorite {ids:?} {favorite}"));
            Ok(())
        }
        async fn set_rating(&self, id: &str, rating: u8) -> Result<()> {
            self.calls.lock().unwrap().push(format!("rating {id} {rating}"));
            Ok(())
        }
        async fn stream(&self, _: &Song) -> Result<StreamSource> {
            Err(SourceError::Unsupported)
        }
        fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
            None
        }
    }

    #[test]
    fn playlist_edit_is_add_create_delete_only() {
        let e = Capabilities::PLAYLIST_EDIT;
        assert!(e.contains(
            Capabilities::PLAYLIST_ADD | Capabilities::PLAYLIST_CREATE | Capabilities::PLAYLIST_DELETE
        ));
        assert!(!e.contains(Capabilities::PLAYLIST_REMOVE));
        assert!(!Capabilities::PLAYLIST_ADD.contains(Capabilities::PLAYLIST_EDIT));
    }

    #[tokio::test]
    async fn song_favorites_and_ratings_delegate_to_the_old_methods() {
        let f = Fake::default();
        f.set_favorite_items(ItemKind::Song, &["a".into()], true).await.unwrap();
        f.set_item_rating(ItemKind::Song, "a", 4).await.unwrap();
        assert_eq!(*f.calls.lock().unwrap(), vec!["favorite [\"a\"] true", "rating a 4"]);
        assert!(matches!(
            f.set_favorite_items(ItemKind::Album, &["x".into()], true).await,
            Err(SourceError::Unsupported)
        ));
        assert!(matches!(f.set_item_rating(ItemKind::Artist, "x", 3).await, Err(SourceError::Unsupported)));
    }

    #[tokio::test]
    async fn new_methods_are_unsupported_by_default() {
        let f = Fake::default();
        assert!(matches!(f.play_queue().await, Err(SourceError::Unsupported)));
        assert!(matches!(f.save_play_queue(&PlayQueue::default()).await, Err(SourceError::Unsupported)));
        assert!(matches!(f.shares().await, Err(SourceError::Unsupported)));
        assert!(matches!(f.internet_radios().await, Err(SourceError::Unsupported)));
        assert!(matches!(f.item_details(ItemKind::Song, "a").await, Err(SourceError::Unsupported)));
        assert!(matches!(f.artist_info("a").await, Err(SourceError::Unsupported)));
        assert!(matches!(f.folder_tree().await, Err(SourceError::Unsupported)));
        assert!(matches!(
            f.update_playlist("p", &PlaylistUpdate::default()).await,
            Err(SourceError::Unsupported)
        ));
        assert!(matches!(
            f.upload_playlist_image("p", &[], "image/png").await,
            Err(SourceError::Unsupported)
        ));
    }

    #[tokio::test]
    async fn artist_query_falls_back_to_the_plain_artist_list() {
        let f = Fake::default();
        let q = ArtistQuery { search: Some("abba".into()), ..Default::default() };
        f.artists_query(&q).await.unwrap();
        assert_eq!(*f.calls.lock().unwrap(), vec!["artists search=Some(\"abba\")"]);
    }
}
