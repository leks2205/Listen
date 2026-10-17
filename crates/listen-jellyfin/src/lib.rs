//! Jellyfin backend (cf. Feishin's `jellyfin-controller.ts`). Auth: `POST /Users/AuthenticateByName` gives
//! an access token sent as `Authorization: MediaBrowser … Token="…"`. Streams are direct play
//! (`/Items/{id}/Download`), since mpv handles every codec.

pub mod details;
pub mod discovery;
pub mod quick_connect;

use async_trait::async_trait;
use listen_core::model::{
    Album, Artist, ArtistRef, GainInfo, Genre, ImageRef, LyricLine, Lyrics, MbzIds, Page, Playlist,
    SearchResults, ServerId, Song, SongExtra, SourceKind,
};
use listen_source::{
    AlbumQuery, AlbumSort, ArtistQuery, ArtistSort, Capabilities, ItemKind, ListQuery, MusicSource,
    QueryFilters, Result, ScrobbleEvent, SongQuery, SongSort, SortOption, SortOrder, SourceError,
    StreamSource, sorting,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;

const CLIENT: &str = "Listen";
/// Jellyfin durations are in 100 ns ticks.
const TICKS_PER_SEC: u64 = 10_000_000;
const ITEM_FIELDS: &str =
    "Genres,DateCreated,ChildCount,ParentId,Overview,AudioInfo,MediaSources,ProviderIds,People";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct NameId {
    pub name: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct UserData {
    pub is_favorite: bool,
    pub play_count: Option<u32>,
    pub last_played_date: Option<String>,
}

/// A credit of an item (`People`): `kind` is "Composer", "Artist", ...
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Person {
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaStream {
    #[serde(rename = "Type")]
    pub kind: String,
    pub codec: Option<String>,
    pub bit_rate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaSource {
    pub container: Option<String>,
    pub media_streams: Vec<MediaStream>,
}

/// A `BaseItemDto` (only the fields Listen uses).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Item {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: String,
    pub album: Option<String>,
    pub album_id: Option<String>,
    pub album_artist: Option<String>,
    pub album_artists: Vec<NameId>,
    pub artist_items: Vec<NameId>,
    pub production_year: Option<i32>,
    pub index_number: Option<u32>,
    pub parent_index_number: Option<u32>,
    pub run_time_ticks: Option<u64>,
    pub child_count: Option<u32>,
    pub song_count: Option<u32>,
    pub album_count: Option<u32>,
    pub genres: Vec<String>,
    pub overview: Option<String>,
    /// `MusicBrainzArtist`, `MusicBrainzAlbum`, ... → id.
    pub provider_ids: std::collections::HashMap<String, String>,
    pub image_tags: std::collections::HashMap<String, String>,
    pub album_primary_image_tag: Option<String>,
    pub user_data: Option<UserData>,
    pub media_sources: Vec<MediaSource>,
    pub normalization_gain: Option<f32>,
    pub premiere_date: Option<String>,
    pub date_created: Option<String>,
    pub people: Vec<Person>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Items {
    pub items: Vec<Item>,
    pub total_record_count: Option<usize>,
}

/// Per-server options (Feishin's "ignore SSL" and "prefer instant mix").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Accept invalid certificates (this server's client only).
    pub ignore_ssl: bool,
    /// Radio from InstantMix without asking `/Similar` first.
    pub prefer_instant_mix: bool,
}

/// Prefix of a stored secret that is an access token (Quick Connect sign-in)
/// instead of a password.
pub const TOKEN_PREFIX: &str = "jf-token:";

/// The HTTP client builder of a server; `ignore_ssl` accepts invalid certificates.
pub fn client_builder(ignore_ssl: bool) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(concat!("Listen/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .danger_accept_invalid_certs(ignore_ssl)
}

fn http_client(ignore_ssl: bool) -> Result<reqwest::Client> {
    client_builder(ignore_ssl).build().map_err(|e| SourceError::Other(e.to_string()))
}

fn base_url(url: &str) -> Result<Url> {
    let mut base = Url::parse(url.trim()).map_err(|e| SourceError::Other(format!("invalid URL: {e}")))?;
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    Ok(base)
}

pub struct Jellyfin {
    server_id: ServerId,
    base: Url,
    token: String,
    user_id: String,
    device_id: String,
    http: reqwest::Client,
    /// Explicit transcoding (Feishin's "Transcode" settings).
    transcode: std::sync::Mutex<listen_core::settings::Transcode>,
    prefer_instant_mix: bool,
    ignore_ssl: bool,
}

/// Jellyfin lowers the sample rate itself only for these codecs; Feishin caps them at 48 kHz.
fn clamp_sample_rate(rate: u32, codec: &str) -> u32 {
    if rate > 0 && ["aac", "mp3", "ogg", "opus", "vorbis"].contains(&codec) { rate.min(48_000) } else { rate }
}

/// Path and query of the universal audio endpoint for transcoding (Feishin `getStreamUrl`): MP3 when no
/// format is set (ffmpeg crashes without one), bit rate in bit/s, optional sample-rate cap. The play session
/// id carries every parameter so Jellyfin doesn't reuse a running job with other ones.
fn transcode_request(
    id: &str,
    user_id: &str,
    token: &str,
    t: &listen_core::settings::Transcode,
) -> (String, Vec<(&'static str, String)>) {
    let format = t.format().unwrap_or_else(|| "mp3".into());
    let rate = clamp_sample_rate(t.max_sample_rate, &format);
    let mut params = vec![
        ("userId", user_id.to_string()),
        ("deviceId", String::new()),
        ("audioCodec", format.clone()),
        ("apiKey", token.to_string()),
        ("playSessionId", format!("listen-{id}-{format}-{rate}")),
        ("container", format.clone()),
        ("transcodingProtocol", "http".into()),
        ("transcodingContainer", format),
    ];
    if t.bitrate > 0 {
        params.push(("maxStreamingBitrate", (u64::from(t.bitrate) * 1000).to_string()));
    }
    if rate > 0 {
        params.push(("maxAudioSampleRate", rate.to_string()));
    }
    (format!("audio/{id}/universal"), params)
}

fn auth_header(device_id: &str, token: Option<&str>) -> String {
    let mut h = format!(
        r#"MediaBrowser Client="{CLIENT}", Device="{}", DeviceId="{device_id}", Version="{}""#,
        std::env::consts::OS,
        env!("CARGO_PKG_VERSION")
    );
    if let Some(t) = token {
        h.push_str(&format!(r#", Token="{t}""#));
    }
    h
}

fn net(e: reqwest::Error) -> SourceError {
    SourceError::Network(e.without_url().to_string())
}

/// "2001-03-12T00:00:00.0000000Z" to "2001-03-12".
fn iso_date(s: &str) -> String {
    s.split('T').next().unwrap_or(s).to_string()
}

fn secs(ticks: Option<u64>) -> Duration {
    Duration::from_secs(ticks.unwrap_or(0) / TICKS_PER_SEC)
}

impl Jellyfin {
    /// Signs in with username/password, or with a stored access token
    /// (`jf-token:…`, from Quick Connect).
    pub async fn connect(server_id: ServerId, url: &str, username: &str, password: &str) -> Result<Self> {
        Self::connect_with(server_id, url, username, password, Options::default()).await
    }

    pub async fn connect_with(
        server_id: ServerId,
        url: &str,
        username: &str,
        password: &str,
        options: Options,
    ) -> Result<Self> {
        let base = base_url(url)?;
        let http = http_client(options.ignore_ssl)?;
        let device_id = format!("listen-{}", server_id.0);
        if let Some(token) = password.strip_prefix(TOKEN_PREFIX) {
            // The session of a Quick Connect sign-in: ask whose it is.
            let resp = http
                .get(base.join("Users/Me").expect("valid path"))
                .header("Authorization", auth_header(&device_id, Some(token)))
                .send()
                .await
                .map_err(net)?;
            if matches!(resp.status().as_u16(), 401 | 403) {
                return Err(SourceError::Auth("session expired".into()));
            }
            if !resp.status().is_success() {
                return Err(SourceError::Server {
                    code: resp.status().as_u16().into(),
                    message: "sign-in failed".into(),
                });
            }
            let me: Value = resp.json().await.map_err(|e| SourceError::Parse(e.to_string()))?;
            let user_id = me["Id"].as_str().ok_or_else(|| SourceError::Parse("no user id".into()))?;
            return Ok(Self {
                server_id,
                base,
                token: token.into(),
                user_id: user_id.into(),
                device_id,
                http,
                transcode: Default::default(),
                prefer_instant_mix: options.prefer_instant_mix,
                ignore_ssl: options.ignore_ssl,
            });
        }
        let resp = http
            .post(base.join("Users/AuthenticateByName").expect("valid path"))
            .header("Authorization", auth_header(&device_id, None))
            .json(&json!({ "Username": username, "Pw": password }))
            .send()
            .await
            .map_err(net)?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(SourceError::Auth("wrong username or password".into()));
        }
        if !resp.status().is_success() {
            return Err(SourceError::Server {
                code: resp.status().as_u16().into(),
                message: "sign-in failed".into(),
            });
        }
        let body: Value = resp.json().await.map_err(|e| SourceError::Parse(e.to_string()))?;
        let token =
            body["AccessToken"].as_str().ok_or_else(|| SourceError::Parse("no AccessToken".into()))?;
        let user_id = body["User"]["Id"].as_str().ok_or_else(|| SourceError::Parse("no user id".into()))?;
        Ok(Self {
            server_id,
            base,
            token: token.into(),
            user_id: user_id.into(),
            device_id,
            http,
            transcode: Default::default(),
            prefer_instant_mix: options.prefer_instant_mix,
            ignore_ssl: options.ignore_ssl,
        })
    }

    /// A client that was never connected, for tests of the mapping.
    #[cfg(test)]
    fn for_tests() -> Self {
        Jellyfin {
            server_id: ServerId("jf".into()),
            base: Url::parse("http://localhost:8096/").unwrap(),
            token: String::new(),
            user_id: "u".into(),
            device_id: "d".into(),
            http: reqwest::Client::new(),
            transcode: Default::default(),
            prefer_instant_mix: false,
            ignore_ssl: false,
        }
    }

    fn url(&self, path: &str, query: &[(&str, String)]) -> Url {
        let mut u = self.base.join(path.trim_start_matches('/')).expect("valid path");
        {
            let mut q = u.query_pairs_mut();
            for (k, v) in query {
                q.append_pair(k, v);
            }
        }
        u
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let resp = self
            .http
            .get(self.url(path, query))
            .header("Authorization", auth_header(&self.device_id, Some(&self.token)))
            .send()
            .await
            .map_err(net)?;
        match resp.status() {
            s if s == reqwest::StatusCode::UNAUTHORIZED => {
                return Err(SourceError::Auth("session expired".into()));
            }
            s if s == reqwest::StatusCode::NOT_FOUND => return Err(SourceError::NotFound),
            s if !s.is_success() => {
                return Err(SourceError::Server { code: s.as_u16().into(), message: path.to_string() });
            }
            _ => {}
        }
        resp.json().await.map_err(|e| SourceError::Parse(format!("{path}: {e}")))
    }

    async fn send(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<()> {
        let mut req = self
            .http
            .request(method, self.url(path, &[]))
            .header("Authorization", auth_header(&self.device_id, Some(&self.token)));
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.map_err(net)?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(SourceError::Server { code: resp.status().as_u16().into(), message: path.to_string() })
        }
    }

    async fn items(&self, query: Vec<(&str, String)>) -> Result<Items> {
        let mut q = vec![("Recursive", "true".to_string()), ("Fields", ITEM_FIELDS.to_string())];
        q.extend(query);
        self.get(&format!("Users/{}/Items", self.user_id), &q).await
    }

    fn song(&self, i: &Item) -> Song {
        let stream = i.media_sources.first().and_then(|m| m.media_streams.iter().find(|s| s.kind == "Audio"));
        let artists: Vec<ArtistRef> =
            i.artist_items.iter().map(|a| ArtistRef { id: a.id.clone(), name: a.name.clone() }).collect();
        let album_artists: Vec<ArtistRef> =
            i.album_artists.iter().map(|a| ArtistRef { id: a.id.clone(), name: a.name.clone() }).collect();
        let image = if i.image_tags.contains_key("Primary") {
            Some(ImageRef::Id(i.id.clone()))
        } else {
            i.album_primary_image_tag.as_ref().and(i.album_id.clone()).map(ImageRef::Id)
        };
        Song {
            id: i.id.clone(),
            server_id: self.server_id.clone(),
            source: SourceKind::Jellyfin,
            title: i.name.clone(),
            album_artists: if album_artists.is_empty() { artists.clone() } else { album_artists },
            artists,
            album: i.album.clone(),
            album_id: i.album_id.clone(),
            image,
            duration: secs(i.run_time_ticks),
            track_number: i.index_number,
            disc_number: i.parent_index_number,
            year: i.production_year,
            genres: i.genres.clone(),
            codec: stream
                .and_then(|s| s.codec.clone())
                .or_else(|| i.media_sources.first().and_then(|m| m.container.clone()))
                .map(|c| c.to_uppercase()),
            bit_rate: stream.and_then(|s| s.bit_rate).map(|b| b / 1000),
            sample_rate: stream.and_then(|s| s.sample_rate),
            bit_depth: stream.and_then(|s| s.bit_depth),
            bpm: None,
            favorite: i.user_data.as_ref().is_some_and(|u| u.is_favorite),
            rating: None,
            play_count: i.user_data.as_ref().and_then(|u| u.play_count),
            gain: GainInfo { track_gain: i.normalization_gain, ..Default::default() },
            explicit: false,
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: i.date_created.clone(),
            comment: None,
            mbz: MbzIds {
                track: i.provider_ids.get("MusicBrainzTrack").cloned().filter(|s| !s.is_empty()),
                recording: None,
                album: i.provider_ids.get("MusicBrainzAlbum").cloned().filter(|s| !s.is_empty()),
            },
            extra: SongExtra {
                composers: i
                    .people
                    .iter()
                    .filter(|p| p.kind.eq_ignore_ascii_case("composer") && !p.name.is_empty())
                    .map(|p| p.name.clone())
                    .collect(),
                date: i.premiere_date.clone().map(|d| iso_date(&d)),
                release_date: i.premiere_date.clone().map(|d| iso_date(&d)),
                last_played: i.user_data.as_ref().and_then(|u| u.last_played_date.clone()),
            },
        }
    }

    fn album(&self, i: &Item) -> Album {
        let artists: Vec<ArtistRef> = if i.album_artists.is_empty() {
            i.album_artist.iter().map(|n| ArtistRef { id: None, name: n.clone() }).collect()
        } else {
            i.album_artists.iter().map(|a| ArtistRef { id: a.id.clone(), name: a.name.clone() }).collect()
        };
        Album {
            id: i.id.clone(),
            server_id: self.server_id.clone(),
            source: SourceKind::Jellyfin,
            name: i.name.clone(),
            album_artists: artists,
            image: i.image_tags.contains_key("Primary").then(|| ImageRef::Id(i.id.clone())),
            year: i.production_year,
            release_type: None,
            release_types: Vec::new(),
            genres: i.genres.clone(),
            song_count: i.child_count.or(i.song_count),
            duration: secs(i.run_time_ticks),
            favorite: i.user_data.as_ref().is_some_and(|u| u.is_favorite),
            rating: None,
            play_count: i.user_data.as_ref().and_then(|u| u.play_count),
            songs: vec![],
            info: listen_core::model::AlbumInfo {
                release_date: i.premiere_date.clone().map(|d| iso_date(&d)),
                last_played: i.user_data.as_ref().and_then(|u| u.last_played_date.clone()),
                ..Default::default()
            },
        }
    }

    fn artist(&self, i: &Item) -> Artist {
        Artist {
            id: i.id.clone(),
            server_id: self.server_id.clone(),
            source: SourceKind::Jellyfin,
            name: i.name.clone(),
            image: i.image_tags.contains_key("Primary").then(|| ImageRef::Id(i.id.clone())),
            album_count: i.album_count.or(i.child_count),
            song_count: i.song_count,
            favorite: i.user_data.as_ref().is_some_and(|u| u.is_favorite),
            rating: None,
            biography: i.overview.clone().filter(|o| !o.trim().is_empty()),
            similar: vec![],
        }
    }

    fn playlist(&self, i: &Item) -> Playlist {
        Playlist {
            id: i.id.clone(),
            server_id: self.server_id.clone(),
            source: SourceKind::Jellyfin,
            name: i.name.clone(),
            description: i.overview.clone(),
            owner: None,
            image: i.image_tags.contains_key("Primary").then(|| ImageRef::Id(i.id.clone())),
            song_count: i.child_count,
            duration: secs(i.run_time_ticks),
            public: false,
            editable: true,
        }
    }
}

fn page_params(start: usize, limit: usize) -> Vec<(&'static str, String)> {
    vec![("StartIndex", start.to_string()), ("Limit", (if limit == 0 { 100 } else { limit }).to_string())]
}

fn order(o: SortOrder) -> String {
    match o {
        SortOrder::Ascending => "Ascending".into(),
        SortOrder::Descending => "Descending".into(),
    }
}

type Request = Vec<(&'static str, String)>;

/// Jellyfin `SortBy` of an album sort (`JFAlbumListSort`).
pub fn album_sort(s: AlbumSort) -> &'static str {
    match s {
        AlbumSort::AlbumArtist | AlbumSort::Artist => "AlbumArtist,SortName",
        AlbumSort::CommunityRating => "CommunityRating,SortName",
        AlbumSort::CriticRating => "CriticRating,SortName",
        AlbumSort::MostPlayed => "PlayCount",
        AlbumSort::Random => "Random,SortName",
        AlbumSort::RecentlyAdded => "DateCreated,SortName",
        AlbumSort::RecentlyPlayed => "DatePlayed",
        AlbumSort::ReleaseDate | AlbumSort::Year => "ProductionYear,PremiereDate,SortName",
        AlbumSort::Name
        | AlbumSort::Id
        | AlbumSort::Favorited
        | AlbumSort::Duration
        | AlbumSort::Rating
        | AlbumSort::SongCount => "SortName",
    }
}

/// Jellyfin `SortBy` of a song sort (`JFSongListSort`).
pub fn song_sort(s: SongSort) -> &'static str {
    match s {
        SongSort::Album => "Album,SortName",
        SongSort::AlbumArtist => "AlbumArtist,Album,SortName",
        SongSort::Artist => "Artist,Album,SortName",
        SongSort::Duration => "Runtime,AlbumArtist,Album,SortName",
        SongSort::MostPlayed => "PlayCount,SortName",
        SongSort::Name => "Name",
        SongSort::Random => "Random,SortName",
        SongSort::RecentlyAdded => "DateCreated,SortName",
        SongSort::RecentlyPlayed => "DatePlayed,SortName",
        SongSort::ReleaseDate | SongSort::ReleaseYear | SongSort::Year => {
            "PremiereDate,AlbumArtist,Album,SortName"
        }
        _ => "Album,SortName",
    }
}

fn artist_sort(s: ArtistSort) -> &'static str {
    match s {
        ArtistSort::Album => "Album,SortName",
        ArtistSort::Duration => "Runtime,AlbumArtist,Album,SortName",
        ArtistSort::Random => "Random,SortName",
        ArtistSort::RecentlyAdded => "DateCreated,SortName",
        _ => "SortName,Name",
    }
}

/// `Years=1990,1991,...` for a range (Jellyfin has no range filter), at most 200 years.
fn years(min: Option<i32>, max: Option<i32>) -> Option<String> {
    if min.is_none() && max.is_none() {
        return None;
    }
    let (lo, hi) = (min.unwrap_or(1900), max.unwrap_or(2030));
    if lo > hi {
        return None;
    }
    Some((lo..=hi).take(200).map(|y| y.to_string()).collect::<Vec<_>>().join(","))
}

fn push_bool(req: &mut Request, key: &'static str, v: Option<bool>) {
    if let Some(v) = v {
        req.push((key, v.to_string()));
    }
}

/// The `Users/{id}/Items` parameters of an album list (Feishin `getAlbumList`).
pub fn album_request(q: &AlbumQuery) -> Request {
    let mut req: Request = vec![
        ("IncludeItemTypes", "MusicAlbum".to_string()),
        ("SortBy", album_sort(q.sort).to_string()),
        ("SortOrder", order(q.order)),
    ];
    req.extend(page_params(q.start, q.limit));
    let mut genres = q.genres.clone();
    genres.extend(q.genre.clone());
    if !genres.is_empty() {
        req.push(("Genres", genres.join("|")));
    }
    let mut artists = q.artist_ids.clone();
    artists.extend(q.artist_id.clone());
    if !artists.is_empty() {
        // Compilations credit the artist as a contributor; otherwise album artist (as Feishin).
        let key = match q.compilation {
            Some(true) => "ContributingArtistIds",
            Some(false) => "AlbumArtistIds",
            None => "ArtistIds",
        };
        req.push((key, artists.join(",")));
    }
    if let Some(t) = q.search.as_ref().filter(|t| !t.is_empty()) {
        req.push(("SearchTerm", t.clone()));
    }
    push_bool(&mut req, "IsFavorite", q.favorite_filter());
    if let Some(y) = years(q.year_min, q.year_max) {
        req.push(("Years", y));
    }
    if q.sort == AlbumSort::RecentlyPlayed {
        req.push(("Filters", "IsPlayed".into()));
    }
    req
}

/// The `Users/{id}/Items` parameters of a song list (Feishin `getSongList`).
pub fn song_request(q: &SongQuery) -> Request {
    let mut req: Request = vec![
        ("IncludeItemTypes", "Audio".to_string()),
        ("SortBy", song_sort(q.sort).to_string()),
        ("SortOrder", order(q.order)),
    ];
    req.extend(page_params(q.start, q.limit));
    let mut genres = q.genres.clone();
    genres.extend(q.genre.clone());
    if !genres.is_empty() {
        req.push(("Genres", genres.join("|")));
    }
    if !q.artist_ids.is_empty() {
        req.push(("ArtistIds", q.artist_ids.join(",")));
    }
    if let Some(t) = q.search.as_ref().filter(|t| !t.is_empty()) {
        req.push(("SearchTerm", t.clone()));
    }
    push_bool(&mut req, "IsFavorite", q.favorite_filter());
    if let Some(y) = years(q.year_min, q.year_max) {
        req.push(("Years", y));
    }
    req
}

/// Parameters of `Artists/AlbumArtists` for an [`ArtistQuery`].
pub fn artist_request(q: &ArtistQuery) -> Request {
    let mut req: Request = vec![("SortBy", artist_sort(q.sort).to_string()), ("SortOrder", order(q.order))];
    req.extend(page_params(q.start, q.limit));
    if let Some(t) = q.search.as_ref().filter(|t| !t.is_empty()) {
        req.push(("SearchTerm", t.clone()));
    }
    push_bool(&mut req, "IsFavorite", if q.favorites_only { Some(true) } else { q.favorite });
    req
}

/// What Jellyfin supports (playlists: only adding songs to existing ones).
pub fn capabilities() -> Capabilities {
    Capabilities::GENRES
            // Only `add_to_playlist` is implemented: no create, delete, remove or rename.
            | Capabilities::PLAYLIST_ADD
            | Capabilities::SCROBBLE
            | Capabilities::LYRICS
            | Capabilities::SIMILAR_SONGS
            | Capabilities::ARTIST_INFO
            | Capabilities::ALBUM_ARTISTS
            | Capabilities::FAVORITE_ITEMS
            | Capabilities::ITEM_DETAILS
}

#[async_trait]
impl MusicSource for Jellyfin {
    fn kind(&self) -> SourceKind {
        SourceKind::Jellyfin
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    async fn ping(&self) -> Result<()> {
        self.get::<Value>("System/Info", &[]).await.map(|_| ())
    }

    async fn albums(&self, q: &AlbumQuery) -> Result<Page<Album>> {
        let items = self.items(album_request(q)).await?;
        Ok(Page {
            items: items.items.iter().map(|i| self.album(i)).collect(),
            start_index: q.start,
            total: items.total_record_count,
        })
    }

    async fn album(&self, id: &str) -> Result<Album> {
        let item: Item = self.get(&format!("Users/{}/Items/{id}", self.user_id), &[]).await?;
        let tracks = self
            .items(vec![
                ("ParentId", id.to_string()),
                ("IncludeItemTypes", "Audio".into()),
                ("SortBy", "ParentIndexNumber,IndexNumber,SortName".into()),
            ])
            .await?;
        let mut album = self.album(&item);
        album.songs = tracks.items.iter().map(|i| self.song(i)).collect();
        if album.duration.is_zero() {
            album.duration = album.songs.iter().map(|s| s.duration).sum();
        }
        album.song_count = Some(album.songs.len() as u32);
        Ok(album)
    }

    async fn artists(&self, q: &ListQuery) -> Result<Page<Artist>> {
        let mut query = vec![
            ("userId", self.user_id.clone()),
            ("Fields", "Overview,ChildCount".to_string()),
            ("SortBy", "SortName".to_string()),
        ];
        query.extend(page_params(q.start, q.limit));
        if let Some(t) = q.search.as_ref().filter(|t| !t.is_empty()) {
            query.push(("SearchTerm", t.clone()));
        }
        let items: Items = self.get("Artists/AlbumArtists", &query).await?;
        Ok(Page {
            items: items.items.iter().map(|i| self.artist(i)).collect(),
            start_index: q.start,
            total: items.total_record_count,
        })
    }

    async fn artist(&self, id: &str) -> Result<Artist> {
        let item: Item = self.get(&format!("Users/{}/Items/{id}", self.user_id), &[]).await?;
        Ok(self.artist(&item))
    }

    async fn artist_albums(&self, id: &str) -> Result<Vec<Album>> {
        let items = self
            .items(vec![
                ("IncludeItemTypes", "MusicAlbum".into()),
                ("AlbumArtistIds", id.to_string()),
                ("SortBy", "ProductionYear,SortName".into()),
                ("SortOrder", "Descending".into()),
            ])
            .await?;
        Ok(items.items.iter().map(|i| self.album(i)).collect())
    }

    async fn top_songs(&self, artist: &Artist, count: usize) -> Result<Vec<Song>> {
        let items = self
            .items(vec![
                ("IncludeItemTypes", "Audio".into()),
                ("ArtistIds", artist.id.clone()),
                ("SortBy", "PlayCount,SortName".into()),
                ("SortOrder", "Descending".into()),
                ("Limit", count.to_string()),
            ])
            .await?;
        Ok(items.items.iter().map(|i| self.song(i)).collect())
    }

    async fn songs(&self, q: &SongQuery) -> Result<Page<Song>> {
        let items = self.items(song_request(q)).await?;
        Ok(Page {
            items: items.items.iter().map(|i| self.song(i)).collect(),
            start_index: q.start,
            total: items.total_record_count,
        })
    }

    async fn artists_query(&self, q: &ArtistQuery) -> Result<Page<Artist>> {
        let mut query = vec![("userId", self.user_id.clone()), ("Fields", "Overview,ChildCount".to_string())];
        query.extend(artist_request(q));
        let items: Items = self.get("Artists/AlbumArtists", &query).await?;
        Ok(Page {
            items: items.items.iter().map(|i| self.artist(i)).collect(),
            start_index: q.start,
            total: items.total_record_count,
        })
    }

    fn album_sorts(&self) -> &'static [SortOption<AlbumSort>] {
        sorting::JELLYFIN_ALBUMS
    }

    fn song_sorts(&self) -> &'static [SortOption<SongSort>] {
        sorting::JELLYFIN_SONGS
    }

    fn artist_sorts(&self) -> &'static [SortOption<ArtistSort>] {
        sorting::JELLYFIN_ARTISTS
    }

    fn album_filters(&self) -> QueryFilters {
        QueryFilters::FAVORITE
            | QueryFilters::COMPILATION
            | QueryFilters::GENRES
            | QueryFilters::ARTISTS
            | QueryFilters::YEAR_RANGE
            | QueryFilters::MULTI
    }

    fn song_filters(&self) -> QueryFilters {
        QueryFilters::FAVORITE
            | QueryFilters::GENRES
            | QueryFilters::ARTISTS
            | QueryFilters::YEAR_RANGE
            | QueryFilters::MULTI
    }

    fn artist_filters(&self) -> QueryFilters {
        QueryFilters::FAVORITE
    }

    async fn random_songs(&self, count: usize, genre: Option<&str>) -> Result<Vec<Song>> {
        let mut query = vec![
            ("IncludeItemTypes", "Audio".to_string()),
            ("SortBy", "Random".to_string()),
            ("Limit", count.to_string()),
        ];
        if let Some(g) = genre {
            query.push(("Genres", g.to_string()));
        }
        Ok(self.items(query).await?.items.iter().map(|i| self.song(i)).collect())
    }

    async fn artist_info(&self, id: &str) -> Result<listen_source::ArtistInfo> {
        let item: Item = self
            .get(
                &format!("Users/{}/Items/{id}", self.user_id),
                &[("Fields", "Overview,Genres,ProviderIds".to_string())],
            )
            .await?;
        let similar = self
            .get::<Items>(
                &format!("Artists/{id}/Similar"),
                &[("userId", self.user_id.clone()), ("Limit", "12".to_string())],
            )
            .await;
        let similar: Vec<Artist> =
            similar.map(|s| s.items.iter().map(|i| self.artist(i)).collect()).unwrap_or_default();
        let musicbrainz_id = item.provider_ids.get("MusicBrainzArtist").cloned().filter(|m| !m.is_empty());
        Ok(listen_source::ArtistInfo {
            biography: item.overview.clone().filter(|o| !o.trim().is_empty()),
            links: listen_source::ExternalLinks {
                musicbrainz_url: musicbrainz_id
                    .as_ref()
                    .map(|m| format!("https://musicbrainz.org/artist/{m}")),
                lastfm_url: None,
            },
            musicbrainz_id,
            similar: similar
                .iter()
                .map(|a| ArtistRef { id: Some(a.id.clone()), name: a.name.clone() })
                .collect(),
            similar_artists: similar,
            genres: item.genres.clone(),
        })
    }

    async fn appears_on(&self, artist: &Artist) -> Result<Vec<Album>> {
        let items = self
            .items(vec![
                ("IncludeItemTypes", "MusicAlbum".into()),
                ("ContributingArtistIds", artist.id.clone()),
                ("SortBy", "ProductionYear,SortName".into()),
                ("SortOrder", "Descending".into()),
            ])
            .await?;
        Ok(items
            .items
            .iter()
            .filter(|i| !i.album_artists.iter().any(|a| a.id.as_deref() == Some(artist.id.as_str())))
            .map(|i| self.album(i))
            .collect())
    }

    async fn artist_radio(&self, artist: &Artist, count: usize) -> Result<Vec<Song>> {
        let items: Items = self
            .get(
                &format!("Artists/{}/InstantMix", artist.id),
                &[
                    ("userId", self.user_id.clone()),
                    ("Limit", count.to_string()),
                    ("Fields", ITEM_FIELDS.to_string()),
                ],
            )
            .await?;
        if items.items.is_empty() {
            return Err(SourceError::NotFound);
        }
        Ok(items.items.iter().take(count).map(|i| self.song(i)).collect())
    }

    async fn similar_songs(&self, song: &Song, count: usize) -> Result<Vec<Song>> {
        if !self.prefer_instant_mix {
            // Feishin: `/Similar` first (it can beat InstantMix when the server
            // has the data); InstantMix when it has nothing.
            let similar: Result<Items> = self
                .get(
                    &format!("Items/{}/Similar", song.id),
                    &[
                        ("userId", self.user_id.clone()),
                        ("Limit", (count + 1).to_string()),
                        ("Fields", ITEM_FIELDS.to_string()),
                    ],
                )
                .await;
            if let Ok(items) = similar {
                let songs: Vec<Song> = items
                    .items
                    .iter()
                    .filter(|i| i.id != song.id)
                    .take(count)
                    .map(|i| self.song(i))
                    .collect();
                if !songs.is_empty() {
                    return Ok(songs);
                }
            }
        }
        let items: Items = self
            .get(
                &format!("Items/{}/InstantMix", song.id),
                &[
                    ("userId", self.user_id.clone()),
                    ("Limit", (count + 1).to_string()),
                    ("Fields", ITEM_FIELDS.to_string()),
                ],
            )
            .await?;
        Ok(items.items.iter().filter(|i| i.id != song.id).take(count).map(|i| self.song(i)).collect())
    }

    async fn genres(&self) -> Result<Vec<Genre>> {
        let items: Items = self
            .get(
                "MusicGenres",
                &[
                    ("userId", self.user_id.clone()),
                    ("SortBy", "SortName".into()),
                    ("Fields", "ChildCount".into()),
                ],
            )
            .await?;
        Ok(items
            .items
            .iter()
            .map(|g| Genre {
                id: g.name.clone(),
                name: g.name.clone(),
                album_count: g.album_count,
                song_count: g.song_count,
            })
            .collect())
    }

    async fn search(&self, text: &str, limit: usize) -> Result<SearchResults> {
        let search = |types: &'static str| {
            self.items(vec![
                ("SearchTerm", text.to_string()),
                ("IncludeItemTypes", types.into()),
                ("Limit", limit.to_string()),
            ])
        };
        let (songs, albums, playlists) =
            tokio::join!(search("Audio"), search("MusicAlbum"), search("Playlist"));
        let artists: Items = self
            .get(
                "Artists/AlbumArtists",
                &[
                    ("userId", self.user_id.clone()),
                    ("SearchTerm", text.to_string()),
                    ("Limit", limit.to_string()),
                ],
            )
            .await
            .unwrap_or_default();
        Ok(SearchResults {
            songs: songs?.items.iter().map(|i| self.song(i)).collect(),
            albums: albums.unwrap_or_default().items.iter().map(|i| self.album(i)).collect(),
            artists: artists.items.iter().map(|i| self.artist(i)).collect(),
            playlists: playlists.unwrap_or_default().items.iter().map(|i| self.playlist(i)).collect(),
        })
    }

    async fn playlists(&self) -> Result<Vec<Playlist>> {
        let items =
            self.items(vec![("IncludeItemTypes", "Playlist".into()), ("SortBy", "SortName".into())]).await?;
        Ok(items.items.iter().map(|i| self.playlist(i)).collect())
    }

    async fn playlist_songs(&self, id: &str) -> Result<Vec<Song>> {
        let items: Items = self
            .get(
                &format!("Playlists/{id}/Items"),
                &[("userId", self.user_id.clone()), ("Fields", ITEM_FIELDS.to_string())],
            )
            .await?;
        Ok(items.items.iter().map(|i| self.song(i)).collect())
    }

    async fn add_to_playlist(&self, id: &str, songs: &[String]) -> Result<()> {
        let ids = songs.join(",");
        let path = format!("Playlists/{id}/Items?ids={ids}&userId={}", self.user_id);
        self.send(reqwest::Method::POST, &path, None).await
    }

    async fn set_favorite(&self, song_ids: &[String], favorite: bool) -> Result<()> {
        let method = if favorite { reqwest::Method::POST } else { reqwest::Method::DELETE };
        for id in song_ids {
            self.send(method.clone(), &format!("Users/{}/FavoriteItems/{id}", self.user_id), None).await?;
        }
        Ok(())
    }

    /// Any item (song, album, artist) is favourited through `FavoriteItems/{id}`.
    async fn set_favorite_items(&self, _kind: ItemKind, ids: &[String], favorite: bool) -> Result<()> {
        self.set_favorite(ids, favorite).await
    }

    async fn favorite_albums(&self) -> Result<Vec<Album>> {
        let items = self
            .items(vec![
                ("IncludeItemTypes", "MusicAlbum".into()),
                ("Filters", "IsFavorite".into()),
                ("SortBy", "SortName".into()),
            ])
            .await?;
        Ok(items.items.iter().map(|i| self.album(i)).collect())
    }

    async fn favorite_artists(&self) -> Result<Vec<Artist>> {
        let items: Items = self
            .get(
                "Artists/AlbumArtists",
                &[
                    ("userId", self.user_id.clone()),
                    ("isFavorite", "true".into()),
                    ("Fields", "Overview,ChildCount".into()),
                    ("SortBy", "SortName".into()),
                ],
            )
            .await?;
        Ok(items.items.iter().map(|i| self.artist(i)).collect())
    }

    async fn scrobble(&self, song: &Song, event: ScrobbleEvent) -> Result<()> {
        match event {
            ScrobbleEvent::NowPlaying => {
                self.send(
                    reqwest::Method::POST,
                    "Sessions/Playing",
                    Some(json!({ "ItemId": song.id, "CanSeek": true })),
                )
                .await
            }
            ScrobbleEvent::Submission => {
                self.send(
                    reqwest::Method::POST,
                    &format!("Users/{}/PlayedItems/{}", self.user_id, song.id),
                    None,
                )
                .await
            }
        }
    }

    async fn stream(&self, song: &Song) -> Result<StreamSource> {
        let transcode = self.transcode.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if transcode.enabled {
            let (path, params) = transcode_request(&song.id, &self.user_id, &self.token, &transcode);
            return Ok(
                StreamSource::direct(self.url(&path, &params).to_string()).insecure_tls(self.ignore_ssl)
            );
        }
        Ok(StreamSource::direct(
            self.url(&format!("Items/{}/Download", song.id), &[("ApiKey", self.token.clone())]).to_string(),
        )
        .insecure_tls(self.ignore_ssl))
    }

    fn set_transcode(&self, transcode: &listen_core::settings::Transcode) {
        *self.transcode.lock().unwrap_or_else(|e| e.into_inner()) = transcode.clone();
    }

    /// The original file (`Items/{id}/Download`).
    fn download_url(&self, song: &Song) -> Option<String> {
        Some(self.url(&format!("Items/{}/Download", song.id), &[("ApiKey", self.token.clone())]).to_string())
    }

    async fn item_details(&self, _kind: ItemKind, id: &str) -> Result<listen_source::ItemDetails> {
        let item: Value = self.get(&format!("Users/{}/Items/{id}", self.user_id), &[]).await?;
        Ok(details::item(&item))
    }

    fn image_url(&self, image: &ImageRef, size: u32) -> Option<String> {
        match image {
            ImageRef::Id(id) => Some(
                self.url(
                    &format!("Items/{id}/Images/Primary"),
                    &[
                        ("fillHeight", size.to_string()),
                        ("fillWidth", size.to_string()),
                        ("quality", "90".into()),
                    ],
                )
                .to_string(),
            ),
            ImageRef::Url(u) => Some(u.clone()),
        }
    }

    async fn lyrics(&self, song: &Song) -> Result<Option<Lyrics>> {
        let Ok(body) = self.get::<Value>(&format!("Audio/{}/Lyrics", song.id), &[]).await else {
            return Ok(None);
        };
        let lines: Vec<LyricLine> = body["Lyrics"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|l| {
                        LyricLine::new(
                            l["Start"].as_u64().map(|t| Duration::from_millis(t / 10_000)),
                            l["Text"].as_str().unwrap_or_default(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        if lines.is_empty() {
            return Ok(None);
        }
        let synced = lines.iter().all(|l| l.start.is_some());
        Ok(Some(Lyrics { provider: "server".into(), synced, lines, matched: None }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(r: &'a Request, k: &str) -> Option<&'a str> {
        r.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn song_keeps_provider_music_brainz_ids() {
        let item: Item = serde_json::from_value(json!({
            "Id": "1", "Name": "T",
            "ProviderIds": {"MusicBrainzTrack": "trk", "MusicBrainzAlbum": "rel", "Other": "x"}
        }))
        .unwrap();
        let b = Jellyfin {
            server_id: ServerId("j".into()),
            base: Url::parse("http://localhost:8096/").unwrap(),
            token: String::new(),
            user_id: String::new(),
            device_id: String::new(),
            http: reqwest::Client::new(),
            transcode: Default::default(),
            prefer_instant_mix: false,
            ignore_ssl: false,
        };
        let s = b.song(&item);
        assert_eq!(s.mbz.track.as_deref(), Some("trk"));
        assert_eq!(s.mbz.album.as_deref(), Some("rel"));
        assert_eq!(s.mbz.recording, None);
    }

    #[test]
    fn songs_and_albums_get_composers_dates_and_the_last_play() {
        let item: Item = serde_json::from_str(
            r#"{"Id":"i1","Name":"T","Type":"Audio","PremiereDate":"2001-03-12T00:00:00.0000000Z",
                "DateCreated":"2024-05-01T10:00:00Z",
                "People":[{"Name":"Lena Voss","Type":"Composer"},{"Name":"Someone","Type":"Artist"}],
                "UserData":{"IsFavorite":true,"PlayCount":3,"LastPlayedDate":"2026-10-14T21:30:00Z"}}"#,
        )
        .unwrap();
        let client = Jellyfin::for_tests();
        let song = client.song(&item);
        assert_eq!(song.extra.composers, ["Lena Voss"]);
        assert_eq!(song.extra.release_date.as_deref(), Some("2001-03-12"));
        assert_eq!(song.extra.last_played.as_deref(), Some("2026-10-14T21:30:00Z"));
        assert_eq!(song.created.as_deref(), Some("2024-05-01T10:00:00Z"));
        let album = client.album(&item);
        assert_eq!(album.info.release_date.as_deref(), Some("2001-03-12"));
        assert_eq!(album.info.last_played.as_deref(), Some("2026-10-14T21:30:00Z"));
    }

    #[test]
    fn album_request_follows_feishin() {
        let q = AlbumQuery {
            sort: AlbumSort::ReleaseDate,
            order: SortOrder::Descending,
            favorite: Some(true),
            genres: vec!["Rock".into(), "Pop".into()],
            artist_ids: vec!["a1".into(), "a2".into()],
            compilation: Some(true),
            year_min: Some(1999),
            year_max: Some(2001),
            search: Some("x".into()),
            start: 50,
            limit: 25,
            ..Default::default()
        };
        let r = album_request(&q);
        assert_eq!(get(&r, "SortBy"), Some("ProductionYear,PremiereDate,SortName"));
        assert_eq!(get(&r, "SortOrder"), Some("Descending"));
        assert_eq!(get(&r, "Genres"), Some("Rock|Pop"));
        assert_eq!(get(&r, "ContributingArtistIds"), Some("a1,a2"));
        assert_eq!(get(&r, "IsFavorite"), Some("true"));
        assert_eq!(get(&r, "Years"), Some("1999,2000,2001"));
        assert_eq!((get(&r, "StartIndex"), get(&r, "Limit")), (Some("50"), Some("25")));
        let r = album_request(&AlbumQuery {
            artist_ids: vec!["a".into()],
            compilation: Some(false),
            ..Default::default()
        });
        assert_eq!(get(&r, "AlbumArtistIds"), Some("a"));
        let r = album_request(&AlbumQuery { artist_ids: vec!["a".into()], ..Default::default() });
        assert_eq!(get(&r, "ArtistIds"), Some("a"));
        assert_eq!(get(&album_request(&AlbumQuery::default()), "SortBy"), Some("SortName"));
    }

    #[test]
    fn song_and_artist_requests() {
        let q = SongQuery { sort: SongSort::RecentlyPlayed, favorites_only: true, ..Default::default() };
        let r = song_request(&q);
        assert_eq!(get(&r, "SortBy"), Some("DatePlayed,SortName"));
        assert_eq!(get(&r, "IsFavorite"), Some("true"));
        let r = artist_request(&ArtistQuery {
            sort: ArtistSort::Random,
            favorite: Some(false),
            ..Default::default()
        });
        assert_eq!(get(&r, "SortBy"), Some("Random,SortName"));
        assert_eq!(get(&r, "IsFavorite"), Some("false"));
    }

    #[test]
    fn auth_header_format() {
        let h = auth_header("dev1", Some("tok"));
        assert!(h.starts_with(r#"MediaBrowser Client="Listen""#));
        assert!(h.contains(r#"DeviceId="dev1""#) && h.ends_with(r#"Token="tok""#));
    }

    #[test]
    fn ssl_option_sets_the_certificate_check_of_that_client_only() {
        assert!(format!("{:?}", client_builder(true)).contains("danger_accept_invalid_certs: true"));
        assert!(!format!("{:?}", client_builder(false)).contains("danger_accept_invalid_certs"));
    }

    #[test]
    fn parses_items() {
        let json = r#"{"Items":[{"Id":"a1","Name":"Song","Type":"Audio","Album":"Alb","AlbumId":"al","RunTimeTicks":2140000000,
            "IndexNumber":3,"ParentIndexNumber":1,"ArtistItems":[{"Name":"Art","Id":"ar"}],"ImageTags":{},
            "AlbumPrimaryImageTag":"x","UserData":{"IsFavorite":true,"PlayCount":2},
            "MediaSources":[{"Container":"flac","MediaStreams":[{"Type":"Audio","Codec":"flac","BitRate":900000,"SampleRate":44100,"BitDepth":16}]}]}],
            "TotalRecordCount":1}"#;
        let items: Items = serde_json::from_str(json).unwrap();
        let jf = Jellyfin {
            server_id: ServerId("s".into()),
            base: Url::parse("http://x/").unwrap(),
            token: "t".into(),
            user_id: "u".into(),
            device_id: "d".into(),
            http: reqwest::Client::new(),
            transcode: Default::default(),
            prefer_instant_mix: false,
            ignore_ssl: false,
        };
        let s = jf.song(&items.items[0]);
        assert_eq!(s.duration, Duration::from_secs(214));
        assert_eq!(s.artist_line(), "Art");
        assert_eq!(s.image, Some(ImageRef::Id("al".into())));
        assert!(s.favorite);
        assert_eq!(s.codec.as_deref(), Some("FLAC"));
        assert_eq!(s.bit_rate, Some(900));
        let url = jf.stream_url_for_test("a1");
        assert!(url.contains("/Items/a1/Download?ApiKey=t"));
    }

    #[test]
    fn transcoding_uses_the_universal_endpoint_with_caps() {
        use listen_core::settings::Transcode;
        let t = Transcode { enabled: true, format: "Opus".into(), bitrate: 128, max_sample_rate: 96000 };
        let (path, params) = transcode_request("a1", "u1", "tok", &t);
        assert_eq!(path, "audio/a1/universal");
        let get = |k: &str| params.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("audioCodec"), Some("opus"));
        assert_eq!(get("transcodingContainer"), Some("opus"));
        assert_eq!(get("maxStreamingBitrate"), Some("128000"));
        // Lossy codecs cap at 48 kHz, as in Feishin.
        assert_eq!(get("maxAudioSampleRate"), Some("48000"));
        assert_eq!(get("playSessionId"), Some("listen-a1-opus-48000"));
        // No format: MP3; FLAC keeps the requested rate; no caps when unset.
        let (_, p) = transcode_request("a1", "u1", "tok", &Transcode { enabled: true, ..Default::default() });
        assert!(p.contains(&("audioCodec", "mp3".to_string())));
        assert!(p.iter().all(|(k, _)| *k != "maxStreamingBitrate" && *k != "maxAudioSampleRate"));
        let flac = Transcode { enabled: true, format: "flac".into(), max_sample_rate: 96000, bitrate: 0 };
        let (_, p) = transcode_request("a1", "u1", "tok", &flac);
        assert!(p.contains(&("maxAudioSampleRate", "96000".to_string())));
    }

    impl Jellyfin {
        fn stream_url_for_test(&self, id: &str) -> String {
            self.url(&format!("Items/{id}/Download"), &[("ApiKey", self.token.clone())]).to_string()
        }
    }
}
