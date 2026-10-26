//! Navidrome / OpenSubsonic backend: Subsonic REST with token auth (`t = md5(password + salt)`), as
//! Feishin's `subsonic-controller.ts`. URLs carry credentials, so they are redacted before logging.

pub mod details;
pub mod native;
pub mod normalize;
pub mod playlists;
pub mod radio;
pub mod types;

use async_trait::async_trait;
use listen_core::model::{
    Album, Artist, Folder, FolderEntry, Genre, ImageRef, LyricLine, Lyrics, Page, Playlist, SearchResults,
    ServerId, Song, SourceKind,
};
use listen_core::settings::StreamQuality;
use listen_source::{
    AlbumQuery, AlbumSort, ArtistQuery, ArtistSort, Capabilities, FolderNode, ItemKind, ListQuery,
    MusicSource, QueryFilters, Result, ScanStatus, ScrobbleEvent, SmartPlaylistDef, SongQuery, SongSort,
    SortOption, SortOrder, SourceError, StreamSource, clientside, sorting,
};
use md5::{Digest, Md5};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;
use url::Url;

const API_VERSION: &str = "1.16.1";
const CLIENT: &str = "Listen";

pub struct Subsonic {
    server_id: ServerId,
    base: Url,
    username: String,
    /// `u`, `t`, `s` query parameters.
    auth: Vec<(String, String)>,
    http: reqwest::Client,
    ignore_ssl: bool,
    /// `StreamQuality` as a `u8` (see `to_u8`).
    quality: std::sync::atomic::AtomicU8,
    /// Explicit transcoding (overrides `quality` while enabled).
    transcode: std::sync::Mutex<listen_core::settings::Transcode>,
    /// For Navidrome's native login (list pages sort and filter through its own API).
    password: String,
    native: NativeState,
}

/// Whether the server is Navidrome (native API usable), found out by the ping.
#[derive(Default)]
struct NativeState {
    /// 0 unknown, 1 Navidrome, 2 not usable.
    kind: std::sync::atomic::AtomicU8,
    features: std::sync::Mutex<native::Features>,
    token: std::sync::Mutex<Option<String>>,
    /// One login at a time, so parallel loads share a token (Navidrome rate-limits `/auth/login`).
    login: tokio::sync::Mutex<()>,
    /// After a rate-limited login, Subsonic is used until this time.
    pause_until: std::sync::Mutex<Option<std::time::Instant>>,
    genre_ids: std::sync::Mutex<Option<std::collections::HashMap<String, String>>>,
}

/// Per-server sign-in options (Feishin's "legacy authentication", "ignore SSL").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Send `p=enc:<hex password>` instead of token + salt.
    pub legacy_auth: bool,
    /// Accept invalid certificates (this server's client only).
    pub ignore_ssl: bool,
}

/// The HTTP client builder of a server; `ignore_ssl` accepts invalid certificates.
pub fn client_builder(ignore_ssl: bool) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(concat!("Listen/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .danger_accept_invalid_certs(ignore_ssl)
}

/// Auth query parameters: token + salt, or the hex-encoded password.
fn auth_params(username: &str, password: &str, salt: &str, legacy: bool) -> Vec<(String, String)> {
    if legacy {
        let hex: String = password.bytes().map(|b| format!("{b:02x}")).collect();
        return vec![("u".into(), username.into()), ("p".into(), format!("enc:{hex}"))];
    }
    let token = format!("{:x}", Md5::digest(format!("{password}{salt}").as_bytes()));
    vec![("u".into(), username.into()), ("t".into(), token), ("s".into(), salt.into())]
}

impl Subsonic {
    /// `url` is the server root (e.g. `https://music.example.com`).
    pub fn new(server_id: ServerId, url: &str, username: &str, password: &str) -> Result<Self> {
        Self::with_options(server_id, url, username, password, Options::default())
    }

    pub fn with_options(
        server_id: ServerId,
        url: &str,
        username: &str,
        password: &str,
        options: Options,
    ) -> Result<Self> {
        let mut base = Url::parse(url.trim()).map_err(|e| SourceError::Other(format!("invalid URL: {e}")))?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let salt: String = {
            use rand::Rng;
            let mut rng = rand::rng();
            (0..12).map(|_| format!("{:x}", rng.random_range(0..16u8))).collect()
        };
        let http =
            client_builder(options.ignore_ssl).build().map_err(|e| SourceError::Other(e.to_string()))?;
        Ok(Self {
            server_id,
            base,
            username: username.to_string(),
            auth: auth_params(username, password, &salt, options.legacy_auth),
            http,
            ignore_ssl: options.ignore_ssl,
            quality: std::sync::atomic::AtomicU8::new(0),
            transcode: Default::default(),
            password: password.to_string(),
            native: NativeState::default(),
        })
    }

    /// Reads `type` / `serverVersion` of a ping response.
    fn note_ping(&self, root: &Value) {
        use std::sync::atomic::Ordering;
        let navidrome = root["type"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("navidrome"));
        *self.native.features.lock().unwrap() =
            native::Features::from_version(root["serverVersion"].as_str());
        self.native.kind.store(if navidrome { 1 } else { 2 }, Ordering::Relaxed);
    }

    /// True for a Navidrome server whose native API works (detected on first use).
    async fn navidrome(&self) -> bool {
        use std::sync::atomic::Ordering;
        if self.native.pause_until.lock().unwrap().is_some_and(|t| std::time::Instant::now() < t) {
            return false;
        }
        if self.native.kind.load(Ordering::Relaxed) == 0 {
            match self.ping_info().await {
                Ok(root) => self.note_ping(&root),
                Err(_) => return false,
            }
        }
        self.native.kind.load(Ordering::Relaxed) == 1
    }

    fn native_off(&self, why: &SourceError) {
        if matches!(why, SourceError::RateLimited) {
            // Temporary: try the native API again after a minute.
            tracing::info!("Navidrome login rate-limited, using Subsonic for a minute");
            *self.native.pause_until.lock().unwrap() =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(60));
            return;
        }
        tracing::warn!("Navidrome native API unavailable, using Subsonic: {why}");
        self.native.kind.store(2, std::sync::atomic::Ordering::Relaxed);
    }

    fn features(&self) -> native::Features {
        *self.native.features.lock().unwrap()
    }

    async fn native_login(&self) -> Result<String> {
        let _one = self.native.login.lock().await;
        // Another load may have logged in while this one waited.
        if let Some(token) = self.native.token.lock().unwrap().clone() {
            return Ok(token);
        }
        // ...or was refused for logging in too often.
        if self.native.pause_until.lock().unwrap().is_some_and(|t| std::time::Instant::now() < t) {
            return Err(SourceError::RateLimited);
        }
        let url = self.base.join("auth/login").map_err(|e| SourceError::Other(e.to_string()))?;
        let resp = self
            .http
            .post(url)
            .json(&serde_json::json!({"username": self.username, "password": self.password}))
            .send()
            .await
            .map_err(|e| SourceError::Network(e.without_url().to_string()))?;
        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            *self.native.pause_until.lock().unwrap() =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(60));
            return Err(SourceError::RateLimited);
        }
        if !resp.status().is_success() {
            return Err(SourceError::Auth(format!("HTTP {}", resp.status())));
        }
        let body: Value = resp.json().await.map_err(|e| SourceError::Parse(e.without_url().to_string()))?;
        let token = body["token"].as_str().ok_or_else(|| SourceError::Parse("no token".into()))?.to_string();
        *self.native.token.lock().unwrap() = Some(token.clone());
        Ok(token)
    }

    /// `GET /api/{path}` with the native token; the rows and `x-total-count`.
    async fn native_get(&self, path: &str, params: &native::Params) -> Result<(Vec<Value>, Option<usize>)> {
        for attempt in 0..2 {
            let cached = self.native.token.lock().unwrap().clone();
            let token = match cached {
                Some(t) => t,
                None => self.native_login().await?,
            };
            let mut url =
                self.base.join(&format!("api/{path}")).map_err(|e| SourceError::Other(e.to_string()))?;
            url.query_pairs_mut().extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str())));
            let resp = self
                .http
                .get(url)
                .header("x-nd-authorization", format!("Bearer {token}"))
                .send()
                .await
                .map_err(|e| SourceError::Network(e.without_url().to_string()))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                *self.native.token.lock().unwrap() = None;
                continue;
            }
            if !resp.status().is_success() {
                return Err(SourceError::Server {
                    code: resp.status().as_u16() as i32,
                    message: path.into(),
                });
            }
            let total = resp
                .headers()
                .get("x-total-count")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<usize>().ok());
            let rows: Vec<Value> =
                resp.json().await.map_err(|e| SourceError::Parse(e.without_url().to_string()))?;
            return Ok((rows, total));
        }
        Err(SourceError::Auth("native login refused".into()))
    }

    /// A native request with an optional JSON body; returns the response JSON (`Null` when empty).
    /// Retries once with a new token after a 401.
    async fn native_json(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<Value> {
        for attempt in 0..2 {
            let cached = self.native.token.lock().unwrap().clone();
            let token = match cached {
                Some(t) => t,
                None => self.native_login().await?,
            };
            let url =
                self.base.join(&format!("api/{path}")).map_err(|e| SourceError::Other(e.to_string()))?;
            let mut req = self
                .http
                .request(method.clone(), url)
                .header("x-nd-authorization", format!("Bearer {token}"));
            if let Some(body) = body {
                req = req.json(body);
            }
            let resp = req.send().await.map_err(|e| SourceError::Network(e.without_url().to_string()))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                *self.native.token.lock().unwrap() = None;
                continue;
            }
            if !resp.status().is_success() {
                let code = resp.status().as_u16() as i32;
                let message = resp.text().await.unwrap_or_default();
                let message = message.trim();
                return Err(SourceError::Server {
                    code,
                    message: if message.is_empty() {
                        path.into()
                    } else {
                        message.chars().take(300).collect()
                    },
                });
            }
            let text = resp.text().await.map_err(|e| SourceError::Network(e.without_url().to_string()))?;
            if text.trim().is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&text).map_err(|e| SourceError::Parse(e.to_string()));
        }
        Err(SourceError::Auth("native login refused".into()))
    }

    /// Navidrome genre ids for genre names (the native filters take ids).
    async fn genre_ids(&self, names: &[String]) -> Result<Vec<String>> {
        if names.is_empty() {
            return Ok(vec![]);
        }
        let known = self.native.genre_ids.lock().unwrap().is_some();
        if !known {
            let params: native::Params = vec![("_start", "0".into()), ("_end", "5000".into())];
            let (rows, _) = self.native_get("genre", &params).await?;
            let map = rows
                .iter()
                .filter_map(|g| Some((g["name"].as_str()?.to_lowercase(), g["id"].as_str()?.to_string())))
                .collect();
            *self.native.genre_ids.lock().unwrap() = Some(map);
        }
        let guard = self.native.genre_ids.lock().unwrap();
        let map = guard.as_ref().expect("filled above");
        Ok(names.iter().filter_map(|n| map.get(&n.to_lowercase()).cloned()).collect())
    }

    async fn native_albums(&self, q: &AlbumQuery) -> Result<Page<Album>> {
        let mut names = q.genres.clone();
        names.extend(q.genre.clone());
        let ids = self.genre_ids(&names).await?;
        let params = native::album_params(q, &ids, self.features());
        let (rows, total) = self.native_get("album", &params).await?;
        Ok(Page {
            items: rows.iter().filter_map(|r| native::album(&self.server_id, r)).collect(),
            start_index: q.start,
            total,
        })
    }

    async fn native_songs(&self, q: &SongQuery) -> Result<Page<Song>> {
        let mut names = q.genres.clone();
        names.extend(q.genre.clone());
        let ids = self.genre_ids(&names).await?;
        let params = native::song_params(q, &ids, self.features());
        let (rows, total) = self.native_get("song", &params).await?;
        Ok(Page {
            items: rows.iter().filter_map(|r| native::song(&self.server_id, r)).collect(),
            start_index: q.start,
            total,
        })
    }

    async fn native_artists(&self, q: &ArtistQuery) -> Result<Page<Artist>> {
        let params = native::artist_params(q, self.features());
        let (rows, total) = self.native_get("artist", &params).await?;
        Ok(Page {
            items: rows.iter().filter_map(|r| native::artist(&self.server_id, r)).collect(),
            start_index: q.start,
            total,
        })
    }

    pub fn server_id(&self) -> &ServerId {
        &self.server_id
    }

    /// `{base}rest/{endpoint}?u&t&s&v&c&f=json&{params}`.
    fn url(&self, endpoint: &str, params: &[(&str, String)]) -> Url {
        let mut url = self.base.join(&format!("rest/{endpoint}")).expect("valid endpoint");
        {
            let mut q = url.query_pairs_mut();
            for (k, v) in &self.auth {
                q.append_pair(k, v);
            }
            q.append_pair("v", API_VERSION).append_pair("c", CLIENT).append_pair("f", "json");
            for (k, v) in params {
                q.append_pair(k, v);
            }
        }
        url
    }

    /// Calls an endpoint and extracts `subsonic-response[key]` (or checks status only if `key` is empty).
    async fn call<T: DeserializeOwned + Default>(
        &self,
        endpoint: &str,
        key: &str,
        params: &[(&str, String)],
    ) -> Result<T> {
        let url = self.url(endpoint, params);
        tracing::debug!("GET {}", redact(url.as_str()));
        let resp =
            self.http.get(url).send().await.map_err(|e| SourceError::Network(e.without_url().to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(SourceError::Auth(format!("HTTP {status}")));
        }
        let body: Value = resp.json().await.map_err(|e| SourceError::Parse(e.without_url().to_string()))?;
        parse_response(body, key)
    }

    pub async fn ping_info(&self) -> Result<Value> {
        self.call::<Value>("ping", "", &[]).await
    }

    fn page_params(start: usize, limit: usize) -> (usize, usize) {
        (start, if limit == 0 { 100 } else { limit.min(500) })
    }
}

/// Validates a Subsonic envelope and returns `key`'s payload.
pub fn parse_response<T: DeserializeOwned + Default>(body: Value, key: &str) -> Result<T> {
    let root =
        body.get("subsonic-response").ok_or_else(|| SourceError::Parse("not a Subsonic response".into()))?;
    if root["status"] != "ok" {
        let code = root["error"]["code"].as_i64().unwrap_or(0) as i32;
        let message = root["error"]["message"].as_str().unwrap_or("unknown error").to_string();
        // 40: wrong credentials, 41: token auth unsupported, 50: not authorized.
        return Err(match code {
            40 | 41 | 44 | 50 => SourceError::Auth(message),
            70 => SourceError::NotFound,
            _ => SourceError::Server { code, message },
        });
    }
    if key.is_empty() {
        return serde_json::from_value(root.clone()).map_err(|e| SourceError::Parse(e.to_string()));
    }
    match root.get(key) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| SourceError::Parse(format!("{key}: {e}"))),
        None => Ok(T::default()),
    }
}

/// Hides auth parameters in a URL for logging.
pub fn redact(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut u) => {
            let pairs: Vec<(String, String)> = u
                .query_pairs()
                .map(|(k, v)| {
                    let v = if matches!(k.as_ref(), "t" | "s" | "p" | "apiKey") {
                        "***".into()
                    } else {
                        v.into_owned()
                    };
                    (k.into_owned(), v)
                })
                .collect();
            u.query_pairs_mut().clear().extend_pairs(pairs);
            u.to_string()
        }
        Err(_) => "<invalid url>".into(),
    }
}

/// Feishin's Subsonic `getAlbumList`: a genre or a year range switches to its own list type.
fn album_list_type(q: &AlbumQuery) -> Vec<(&'static str, String)> {
    if let Some(genre) = q.genre.as_ref().or(q.genres.first()) {
        return vec![("type", "byGenre".into()), ("genre", genre.clone())];
    }
    if q.year_min.is_some() || q.year_max.is_some() {
        let far_future = 2100;
        let (from, to) = (q.year_min.unwrap_or(0), q.year_max.unwrap_or(far_future));
        return vec![("type", "byYear".into()), ("fromYear", from.to_string()), ("toYear", to.to_string())];
    }
    let t = match q.sort {
        AlbumSort::Name | AlbumSort::Id => "alphabeticalByName",
        AlbumSort::Artist | AlbumSort::AlbumArtist => "alphabeticalByArtist",
        AlbumSort::RecentlyAdded => "newest",
        AlbumSort::RecentlyPlayed => "recent",
        AlbumSort::MostPlayed => "frequent",
        AlbumSort::Random => "random",
        AlbumSort::Favorited => "starred",
        // Not offered for Subsonic servers; the name order is the default.
        AlbumSort::Duration
        | AlbumSort::Rating
        | AlbumSort::SongCount
        | AlbumSort::CommunityRating
        | AlbumSort::CriticRating => "alphabeticalByName",
        AlbumSort::Year | AlbumSort::ReleaseDate => {
            let (from, to) = match q.order {
                SortOrder::Ascending => ("0", "9999"),
                SortOrder::Descending => ("9999", "0"),
            };
            return vec![("type", "byYear".into()), ("fromYear", from.into()), ("toYear", to.into())];
        }
    };
    vec![("type", t.into())]
}

/// What a Subsonic/Navidrome server supports.
pub fn capabilities() -> Capabilities {
    Capabilities::FOLDERS
        | Capabilities::RATINGS
        | Capabilities::GENRES
        | Capabilities::PLAYLIST_EDIT
        | Capabilities::PLAYLIST_REMOVE
        | Capabilities::PLAYLIST_REORDER
        | Capabilities::PLAYLIST_RENAME
        | Capabilities::PLAYLIST_UPDATE
        | Capabilities::PLAY_QUEUE_SYNC
        | Capabilities::SCROBBLE
        | Capabilities::LYRICS
        | Capabilities::SIMILAR_SONGS
        | Capabilities::PLAYLIST_RECOMMENDATIONS
        | Capabilities::ARTIST_INFO
        | Capabilities::ALBUM_ARTISTS
        | Capabilities::DOWNLOAD
        | Capabilities::FAVORITE_ITEMS
        | Capabilities::FOLDER_TREE
        | Capabilities::ITEM_DETAILS
        | Capabilities::SHARES
        | Capabilities::INTERNET_RADIO
        | Capabilities::JUKEBOX
}

/// Parameters of `star` / `unstar`: `id` for songs, `albumId` for albums, `artistId` for artists.
pub fn star_params(kind: ItemKind, ids: &[String]) -> Vec<(&'static str, String)> {
    let key = match kind {
        ItemKind::Song => "id",
        ItemKind::Album => "albumId",
        ItemKind::Artist => "artistId",
    };
    ids.iter().map(|i| (key, i.clone())).collect()
}

#[async_trait]
impl MusicSource for Subsonic {
    fn kind(&self) -> SourceKind {
        SourceKind::Subsonic
    }

    fn capabilities(&self) -> Capabilities {
        // Smart playlists and shares are Navidrome-only, known after the ping.
        let mut caps = capabilities();
        match self.native.kind.load(std::sync::atomic::Ordering::Relaxed) {
            1 => {
                caps |= Capabilities::SMART_PLAYLISTS;
                // Navidrome 0.61 can store a cover for a playlist.
                if self.features().playlist_image {
                    caps |= Capabilities::PLAYLIST_IMAGE;
                }
            }
            2 => caps -= Capabilities::SHARES,
            _ => {}
        }
        caps
    }

    async fn ping(&self) -> Result<()> {
        let root = self.ping_info().await?;
        self.note_ping(&root);
        Ok(())
    }

    fn album_sorts(&self) -> &'static [SortOption<AlbumSort>] {
        if self.native.kind.load(std::sync::atomic::Ordering::Relaxed) == 1 {
            sorting::NAVIDROME_ALBUMS
        } else {
            sorting::SUBSONIC_ALBUMS
        }
    }

    fn song_sorts(&self) -> &'static [SortOption<SongSort>] {
        if self.native.kind.load(std::sync::atomic::Ordering::Relaxed) == 1 {
            sorting::NAVIDROME_SONGS
        } else {
            sorting::SUBSONIC_SONGS
        }
    }

    fn artist_sorts(&self) -> &'static [SortOption<ArtistSort>] {
        if self.native.kind.load(std::sync::atomic::Ordering::Relaxed) == 1 {
            sorting::NAVIDROME_ARTISTS
        } else {
            sorting::SUBSONIC_ARTISTS
        }
    }

    fn album_filters(&self) -> QueryFilters {
        if self.native.kind.load(std::sync::atomic::Ordering::Relaxed) == 1 {
            let tags = if self.features().bfr { QueryFilters::TAGS } else { QueryFilters::empty() };
            QueryFilters::FAVORITE
                | QueryFilters::RATED
                | QueryFilters::COMPILATION
                | QueryFilters::RECENTLY_PLAYED
                | QueryFilters::GENRES
                | QueryFilters::ARTISTS
                | QueryFilters::YEAR
                | QueryFilters::MULTI
                | tags
        } else {
            // getAlbumList2 takes one kind of list: one of these filters at a time.
            QueryFilters::FAVORITE
                | QueryFilters::GENRES
                | QueryFilters::ARTISTS
                | QueryFilters::YEAR_RANGE
                | QueryFilters::EXCLUSIVE
        }
    }

    async fn tag_filters(&self, kind: ItemKind) -> Result<Vec<listen_source::TagFilter>> {
        if !self.navidrome().await || !self.features().bfr {
            return Err(SourceError::Unsupported);
        }
        let params: native::Params = vec![("_start", "0".into()), ("_end", "-1".into())];
        let (rows, _) = self.native_get("tag", &params).await?;
        Ok(native::tag_filters(&rows, kind == ItemKind::Song))
    }

    fn song_filters(&self) -> QueryFilters {
        if self.native.kind.load(std::sync::atomic::Ordering::Relaxed) == 1 {
            let tags = if self.features().bfr { QueryFilters::TAGS } else { QueryFilters::empty() };
            QueryFilters::FAVORITE
                | QueryFilters::RATED
                | QueryFilters::GENRES
                | QueryFilters::ARTISTS
                | QueryFilters::YEAR
                | QueryFilters::MULTI
                | tags
        } else {
            QueryFilters::FAVORITE | QueryFilters::GENRES
        }
    }

    fn artist_filters(&self) -> QueryFilters {
        QueryFilters::FAVORITE
    }

    async fn albums(&self, q: &AlbumQuery) -> Result<Page<Album>> {
        if self.navidrome().await {
            match self.native_albums(q).await {
                Ok(page) => return Ok(page),
                Err(e @ SourceError::Network(_)) => return Err(e),
                Err(e) => self.native_off(&e),
            }
        }
        let (start, limit) = Self::page_params(q.start, q.limit);
        // As in Feishin: artist and favourite filters read those lists, then sort and page locally.
        let mut artists = q.artist_ids.clone();
        artists.extend(q.artist_id.clone());
        if !artists.is_empty() || q.favorite_filter() == Some(true) {
            let mut items: Vec<Album> = Vec::new();
            if artists.is_empty() {
                let starred: types::SearchResult3 = self.call("getStarred2", "starred2", &[]).await?;
                items.extend(starred.album.iter().map(|a| normalize::album(&self.server_id, a)));
            } else {
                for id in &artists {
                    let a: types::ArtistId3 = self.call("getArtist", "artist", &[("id", id.clone())]).await?;
                    items.extend(a.album.iter().map(|al| normalize::album(&self.server_id, al)));
                }
            }
            let rest = AlbumQuery { artist_ids: vec![], artist_id: None, search: None, ..q.clone() };
            let needle = q.search.clone();
            let items = clientside::albums(items, &AlbumQuery { search: needle, ..rest });
            return Ok(clientside::page(&items, q.start, q.limit));
        }
        if let Some(text) = q.search.as_deref().filter(|t| !t.is_empty()) {
            let r: types::SearchResult3 = self
                .call(
                    "search3",
                    "searchResult3",
                    &[
                        ("query", text.into()),
                        ("albumCount", limit.to_string()),
                        ("albumOffset", start.to_string()),
                        ("songCount", "0".into()),
                        ("artistCount", "0".into()),
                    ],
                )
                .await?;
            let items: Vec<Album> = r.album.iter().map(|a| normalize::album(&self.server_id, a)).collect();
            return Ok(Page { items, start_index: start, total: None });
        }
        let mut params = album_list_type(q);
        params.push(("size", limit.to_string()));
        params.push(("offset", start.to_string()));
        let list: types::AlbumList = self.call("getAlbumList2", "albumList2", &params).await?;
        let mut items: Vec<Album> = list.album.iter().map(|a| normalize::album(&self.server_id, a)).collect();
        if q.order == SortOrder::Descending && matches!(q.sort, AlbumSort::Name | AlbumSort::Artist) {
            // Subsonic has no descending alphabetical list; reverse the page.
            items.reverse();
        }
        Ok(Page { items, start_index: start, total: None })
    }

    async fn album(&self, id: &str) -> Result<Album> {
        let a: types::AlbumId3 = self.call("getAlbum", "album", &[("id", id.into())]).await?;
        if a.id.is_empty() {
            return Err(SourceError::NotFound);
        }
        Ok(normalize::album(&self.server_id, &a))
    }

    async fn artists(&self, q: &ListQuery) -> Result<Page<Artist>> {
        let all: types::Artists = self.call("getArtists", "artists", &[]).await?;
        let needle = q.search.as_deref().unwrap_or("").to_lowercase();
        let matching: Vec<&types::ArtistId3> = all
            .index
            .iter()
            .flat_map(|i| &i.artist)
            .filter(|a| needle.is_empty() || a.name.to_lowercase().contains(&needle))
            .collect();
        let total = matching.len();
        let (start, limit) = Self::page_params(q.start, q.limit);
        let items = matching
            .into_iter()
            .skip(start)
            .take(limit)
            .map(|a| normalize::artist(&self.server_id, a, None))
            .collect();
        Ok(Page { items, start_index: start, total: Some(total) })
    }

    async fn artists_query(&self, q: &ArtistQuery) -> Result<Page<Artist>> {
        if self.navidrome().await {
            match self.native_artists(q).await {
                Ok(page) => return Ok(page),
                Err(e @ SourceError::Network(_)) => return Err(e),
                Err(e) => self.native_off(&e),
            }
        }
        let all: types::Artists = self.call("getArtists", "artists", &[]).await?;
        let items: Vec<Artist> = all
            .index
            .iter()
            .flat_map(|i| &i.artist)
            .map(|a| normalize::artist(&self.server_id, a, None))
            .collect();
        let items = clientside::artists(items, q);
        Ok(clientside::page(&items, q.start, q.limit))
    }

    async fn artist(&self, id: &str) -> Result<Artist> {
        let a: types::ArtistId3 = self.call("getArtist", "artist", &[("id", id.into())]).await?;
        if a.id.is_empty() {
            return Err(SourceError::NotFound);
        }
        let info: types::ArtistInfo = self
            .call("getArtistInfo2", "artistInfo2", &[("id", id.into()), ("count", "10".into())])
            .await
            .unwrap_or_default();
        Ok(normalize::artist(&self.server_id, &a, Some(&info)))
    }

    async fn artist_albums(&self, id: &str) -> Result<Vec<Album>> {
        let a: types::ArtistId3 = self.call("getArtist", "artist", &[("id", id.into())]).await?;
        Ok(a.album.iter().map(|al| normalize::album(&self.server_id, al)).collect())
    }

    async fn top_songs(&self, artist: &Artist, count: usize) -> Result<Vec<Song>> {
        let r: types::Songs = self
            .call("getTopSongs", "topSongs", &[("artist", artist.name.clone()), ("count", count.to_string())])
            .await?;
        Ok(r.song.iter().map(|c| normalize::song(&self.server_id, c)).collect())
    }

    async fn item_details(&self, kind: ItemKind, id: &str) -> Result<listen_source::ItemDetails> {
        let params = [("id", id.to_string())];
        Ok(match kind {
            ItemKind::Song => details::song(&self.call::<Value>("getSong", "song", &params).await?),
            ItemKind::Album => details::album(&self.call::<Value>("getAlbum", "album", &params).await?),
            ItemKind::Artist => {
                let mut d = details::artist(&self.call::<Value>("getArtist", "artist", &params).await?);
                if d.musicbrainz_id.is_none() {
                    d.musicbrainz_id = self.artist_info(id).await.ok().and_then(|i| i.musicbrainz_id);
                }
                d
            }
        })
    }

    async fn create_share(
        &self,
        _kind: ItemKind,
        ids: &[String],
        description: Option<&str>,
        expires: Option<std::time::SystemTime>,
    ) -> Result<listen_source::Share> {
        // Any song, album, artist or playlist id works; the server looks the kind up.
        let params = details::share_params(ids, description, expires);
        let shares: Value = self.call("createShare", "shares", &params).await?;
        shares["share"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| details::share(s, expires))
            .ok_or_else(|| SourceError::Parse("createShare returned no share".into()))
    }

    async fn internet_radios(&self) -> Result<Vec<listen_source::InternetRadio>> {
        let v: Value = self.call("getInternetRadioStations", "internetRadioStations", &[]).await?;
        Ok(radio::stations(&v))
    }

    async fn create_internet_radio(
        &self,
        r: &listen_source::InternetRadio,
    ) -> Result<listen_source::InternetRadio> {
        self.call::<Value>("createInternetRadioStation", "", &radio::create_params(r)).await?;
        // The answer carries no station: find the new one by its stream address.
        let all = self.internet_radios().await.unwrap_or_default();
        Ok(all
            .into_iter()
            .rev()
            .find(|s| s.stream_url == r.stream_url && s.name == r.name)
            .unwrap_or_else(|| listen_source::InternetRadio { id: String::new(), ..r.clone() }))
    }

    async fn update_internet_radio(&self, r: &listen_source::InternetRadio) -> Result<()> {
        self.call::<Value>("updateInternetRadioStation", "", &radio::update_params(r)).await.map(|_| ())
    }

    async fn delete_internet_radio(&self, id: &str) -> Result<()> {
        self.call::<Value>("deleteInternetRadioStation", "", &[("id", id.to_string())]).await.map(|_| ())
    }

    async fn jukebox(&self, cmd: &listen_source::JukeboxCmd) -> Result<listen_source::JukeboxStatus> {
        let v: Value = self.call("jukeboxControl", "", &radio::jukebox_params(cmd)).await?;
        // `jukeboxStatus` for most actions, `jukeboxPlaylist` for `get`.
        let status = if v["jukeboxStatus"].is_object() { &v["jukeboxStatus"] } else { &v["jukeboxPlaylist"] };
        Ok(radio::jukebox_status(status))
    }

    async fn shares(&self) -> Result<Vec<listen_source::Share>> {
        let shares: Value = self.call("getShares", "shares", &[]).await?;
        Ok(shares["share"]
            .as_array()
            .map(|a| a.iter().filter_map(|s| details::share(s, None)).collect())
            .unwrap_or_default())
    }

    async fn artist_info(&self, id: &str) -> Result<listen_source::ArtistInfo> {
        let info: types::ArtistInfo =
            self.call("getArtistInfo2", "artistInfo2", &[("id", id.into()), ("count", "12".into())]).await?;
        Ok(normalize::artist_info(&self.server_id, &info))
    }

    /// Albums with a song by the artist under another album artist, found via a song search.
    async fn appears_on(&self, artist: &Artist) -> Result<Vec<Album>> {
        let found: types::SearchResult3 = self
            .call(
                "search3",
                "searchResult3",
                &[
                    ("query", artist.name.clone()),
                    ("songCount", "300".into()),
                    ("albumCount", "0".into()),
                    ("artistCount", "0".into()),
                ],
            )
            .await?;
        let mut ids: Vec<String> = Vec::new();
        for c in &found.song {
            let song = normalize::song(&self.server_id, c);
            let album_artist = song.album_artists.iter().any(|a| a.id.as_deref() == Some(artist.id.as_str()));
            if let Some(album) = &song.album_id
                && listen_source::is_by(&song, artist)
                && !album_artist
                && !ids.contains(album)
            {
                ids.push(album.clone());
            }
        }
        ids.truncate(40);
        let mut out = Vec::new();
        for id in ids {
            if let Ok(a) = self.call::<types::AlbumId3>("getAlbum", "album", &[("id", id)]).await {
                out.push(normalize::album(&self.server_id, &a));
            }
        }
        Ok(out)
    }

    async fn artist_radio(&self, artist: &Artist, count: usize) -> Result<Vec<Song>> {
        let r: types::Songs = self
            .call(
                "getSimilarSongs2",
                "similarSongs2",
                &[("id", artist.id.clone()), ("count", count.to_string())],
            )
            .await?;
        let mut songs: Vec<Song> = r.song.iter().map(|c| normalize::song(&self.server_id, c)).collect();
        if songs.is_empty() {
            return Err(SourceError::NotFound);
        }
        songs.truncate(count.max(1));
        Ok(songs)
    }

    async fn songs(&self, q: &SongQuery) -> Result<Page<Song>> {
        if self.navidrome().await {
            match self.native_songs(q).await {
                Ok(page) => return Ok(page),
                Err(e @ SourceError::Network(_)) => return Err(e),
                Err(e) => self.native_off(&e),
            }
        }
        let (start, limit) = Self::page_params(q.start, q.limit);
        let genre = q.genre.as_ref().or(q.genres.first());
        if let (Some(genre), None, false) =
            (genre, q.search.as_deref().filter(|t| !t.is_empty()), q.favorites_only)
        {
            let r: types::Songs = self
                .call(
                    "getSongsByGenre",
                    "songsByGenre",
                    &[("genre", genre.clone()), ("count", limit.to_string()), ("offset", start.to_string())],
                )
                .await?;
            return Ok(Page {
                items: r.song.iter().map(|c| normalize::song(&self.server_id, c)).collect(),
                start_index: start,
                total: None,
            });
        }
        if q.favorites_only || q.favorite == Some(true) {
            let starred: types::SearchResult3 = self.call("getStarred2", "starred2", &[]).await?;
            let items: Vec<Song> = starred
                .song
                .iter()
                .skip(start)
                .take(limit)
                .map(|c| normalize::song(&self.server_id, c))
                .collect();
            return Ok(Page { items, start_index: start, total: Some(starred.song.len()) });
        }
        if let Some(genre) = q.genre.as_deref().filter(|g| !g.is_empty()) {
            let r: types::Songs = self
                .call(
                    "getSongsByGenre",
                    "songsByGenre",
                    &[("genre", genre.into()), ("count", limit.to_string()), ("offset", start.to_string())],
                )
                .await?;
            let items = r.song.iter().map(|c| normalize::song(&self.server_id, c)).collect();
            return Ok(Page { items, start_index: start, total: None });
        }
        // An empty search3 query lists all songs on Navidrome (and most OpenSubsonic servers).
        let text = q.search.clone().unwrap_or_default();
        let r: types::SearchResult3 = self
            .call(
                "search3",
                "searchResult3",
                &[
                    ("query", text),
                    ("songCount", limit.to_string()),
                    ("songOffset", start.to_string()),
                    ("albumCount", "0".into()),
                    ("artistCount", "0".into()),
                ],
            )
            .await?;
        Ok(Page {
            items: r.song.iter().map(|c| normalize::song(&self.server_id, c)).collect(),
            start_index: start,
            total: None,
        })
    }

    async fn random_songs(&self, count: usize, genre: Option<&str>) -> Result<Vec<Song>> {
        let mut params = vec![("size", count.min(500).to_string())];
        if let Some(g) = genre {
            params.push(("genre", g.to_string()));
        }
        let r: types::Songs = self.call("getRandomSongs", "randomSongs", &params).await?;
        Ok(r.song.iter().map(|c| normalize::song(&self.server_id, c)).collect())
    }

    async fn similar_songs(&self, song: &Song, count: usize) -> Result<Vec<Song>> {
        let r: types::Songs = self
            .call(
                "getSimilarSongs2",
                "similarSongs2",
                &[("id", song.id.clone()), ("count", count.to_string())],
            )
            .await?;
        Ok(r.song.iter().map(|c| normalize::song(&self.server_id, c)).collect())
    }

    async fn genres(&self) -> Result<Vec<Genre>> {
        let r: types::Genres = self.call("getGenres", "genres", &[]).await?;
        let mut out: Vec<Genre> = r.genre.iter().map(normalize::genre).collect();
        out.sort_by_key(|g| g.name.to_lowercase());
        Ok(out)
    }

    async fn folder(&self, id: Option<&str>) -> Result<Folder> {
        match id {
            None => {
                let folders: types::MusicFolders = self.call("getMusicFolders", "musicFolders", &[]).await?;
                let mut entries = Vec::new();
                for f in &folders.music_folder {
                    let fid = match &f.id {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let idx: Value = self.call("getIndexes", "indexes", &[("musicFolderId", fid)]).await?;
                    for index in idx["index"].as_array().into_iter().flatten() {
                        for a in index["artist"].as_array().into_iter().flatten() {
                            if let (Some(id), Some(name)) = (a["id"].as_str(), a["name"].as_str()) {
                                entries.push(FolderEntry { id: id.into(), name: name.into() });
                            }
                        }
                    }
                }
                Ok(Folder {
                    id: String::new(),
                    name: "Folders".into(),
                    parent_id: None,
                    folders: entries,
                    songs: vec![],
                })
            }
            Some(id) => {
                let d: types::Directory =
                    self.call("getMusicDirectory", "directory", &[("id", id.into())]).await?;
                let (dirs, files): (Vec<_>, Vec<_>) = d.child.iter().partition(|c| c.is_dir);
                Ok(Folder {
                    id: d.id,
                    name: d.name,
                    parent_id: d.parent,
                    folders: dirs
                        .iter()
                        .map(|c| FolderEntry { id: c.id.clone(), name: c.title.clone() })
                        .collect(),
                    songs: files.iter().map(|c| normalize::song(&self.server_id, c)).collect(),
                })
            }
        }
    }

    async fn search(&self, text: &str, limit: usize) -> Result<SearchResults> {
        let n = limit.to_string();
        let r: types::SearchResult3 = self
            .call(
                "search3",
                "searchResult3",
                &[
                    ("query", text.into()),
                    ("songCount", n.clone()),
                    ("albumCount", n.clone()),
                    ("artistCount", n),
                ],
            )
            .await?;
        Ok(SearchResults {
            songs: r.song.iter().map(|c| normalize::song(&self.server_id, c)).collect(),
            albums: r.album.iter().map(|a| normalize::album(&self.server_id, a)).collect(),
            artists: r.artist.iter().map(|a| normalize::artist(&self.server_id, a, None)).collect(),
            playlists: vec![],
        })
    }

    async fn playlists(&self) -> Result<Vec<Playlist>> {
        let r: types::Playlists = self.call("getPlaylists", "playlists", &[]).await?;
        Ok(r.playlist.iter().map(|p| normalize::playlist(&self.server_id, p, &self.username)).collect())
    }

    async fn playlist_songs(&self, id: &str) -> Result<Vec<Song>> {
        let p: types::Playlist = self.call("getPlaylist", "playlist", &[("id", id.into())]).await?;
        Ok(p.entry.iter().map(|c| normalize::song(&self.server_id, c)).collect())
    }

    async fn create_playlist(&self, name: &str, songs: &[String]) -> Result<Playlist> {
        let mut params = vec![("name", name.to_string())];
        params.extend(songs.iter().map(|s| ("songId", s.clone())));
        let p: types::Playlist = self.call("createPlaylist", "playlist", &params).await?;
        Ok(normalize::playlist(&self.server_id, &p, &self.username))
    }

    async fn smart_playlist(&self, id: &str) -> Result<Option<SmartPlaylistDef>> {
        if !self.navidrome().await {
            return Err(SourceError::Unsupported);
        }
        let row = self.native_json(reqwest::Method::GET, &format!("playlist/{id}"), None).await?;
        Ok(native::smart_def(&row))
    }

    async fn smart_playlist_ids(&self) -> Result<Vec<String>> {
        if !self.navidrome().await {
            return Ok(vec![]);
        }
        let params: native::Params = vec![("_start", "0".into()), ("_end", "5000".into())];
        let (rows, _) = self.native_get("playlist", &params).await?;
        Ok(native::smart_ids(&rows))
    }

    async fn create_smart_playlist(&self, def: &SmartPlaylistDef) -> Result<Playlist> {
        if !self.navidrome().await {
            return Err(SourceError::Unsupported);
        }
        let created =
            self.native_json(reqwest::Method::POST, "playlist", Some(&native::playlist_body(def))).await?;
        let id = created["id"].as_str().ok_or_else(|| SourceError::Parse("no playlist id".into()))?;
        Ok(Playlist {
            id: id.to_string(),
            server_id: self.server_id.clone(),
            source: SourceKind::Subsonic,
            name: def.name.clone(),
            description: Some(def.comment.clone()).filter(|c| !c.is_empty()),
            owner: Some(self.username.clone()),
            image: None,
            song_count: None,
            duration: Duration::ZERO,
            public: def.public,
            editable: true,
        })
    }

    async fn update_smart_playlist(&self, id: &str, def: &SmartPlaylistDef) -> Result<()> {
        if !self.navidrome().await {
            return Err(SourceError::Unsupported);
        }
        self.native_json(reqwest::Method::PUT, &format!("playlist/{id}"), Some(&native::playlist_body(def)))
            .await
            .map(|_| ())
    }

    async fn add_to_playlist(&self, id: &str, songs: &[String]) -> Result<()> {
        let mut params = vec![("playlistId", id.to_string())];
        params.extend(songs.iter().map(|s| ("songIdToAdd", s.clone())));
        self.call::<Value>("updatePlaylist", "", &params).await.map(|_| ())
    }

    async fn remove_from_playlist(&self, id: &str, indices: &[usize]) -> Result<()> {
        let params = playlists::remove_params(id, indices);
        self.call::<Value>("updatePlaylist", "", &params).await.map(|_| ())
    }

    async fn update_playlist(&self, id: &str, update: &listen_source::PlaylistUpdate) -> Result<()> {
        self.update_metadata(id, update).await
    }

    async fn set_playlist_order(&self, id: &str, order: &[usize]) -> Result<()> {
        let ids: Vec<String> = self.playlist_songs(id).await?.into_iter().map(|s| s.id).collect();
        let ordered = playlists::apply_order(&ids, order)
            .ok_or_else(|| SourceError::Other("the playlist changed on the server; open it again".into()))?;
        self.call::<Value>("createPlaylist", "", &playlists::replace_params(id, &ordered)).await.map(|_| ())
    }

    async fn upload_playlist_image(&self, id: &str, image: &[u8], mime: &str) -> Result<()> {
        self.upload_image(id, image, mime).await
    }

    async fn delete_playlist_image(&self, id: &str) -> Result<()> {
        self.delete_image(id).await
    }

    async fn delete_playlist(&self, id: &str) -> Result<()> {
        self.call::<Value>("deletePlaylist", "", &[("id", id.into())]).await.map(|_| ())
    }

    async fn rename_playlist(&self, id: &str, name: &str) -> Result<()> {
        let params = [("playlistId", id.to_string()), ("name", name.to_string())];
        self.call::<Value>("updatePlaylist", "", &params).await.map(|_| ())
    }

    async fn move_in_playlist(&self, id: &str, from: usize, to: usize) -> Result<()> {
        // Subsonic has no "move": createPlaylist with an existing id replaces its songs.
        let mut ids: Vec<String> = self.playlist_songs(id).await?.into_iter().map(|s| s.id).collect();
        let order = reorder(&mut ids, from, to);
        if !order {
            return Ok(());
        }
        let mut params = vec![("playlistId", id.to_string())];
        params.extend(ids.into_iter().map(|s| ("songId", s)));
        self.call::<Value>("createPlaylist", "", &params).await.map(|_| ())
    }

    async fn set_favorite(&self, ids: &[String], favorite: bool) -> Result<()> {
        let params: Vec<(&str, String)> = ids.iter().map(|i| ("id", i.clone())).collect();
        self.call::<Value>(if favorite { "star" } else { "unstar" }, "", &params).await.map(|_| ())
    }

    async fn set_favorite_items(&self, kind: ItemKind, ids: &[String], favorite: bool) -> Result<()> {
        let params = star_params(kind, ids);
        self.call::<Value>(if favorite { "star" } else { "unstar" }, "", &params).await.map(|_| ())
    }

    async fn favorite_albums(&self) -> Result<Vec<Album>> {
        let starred: types::SearchResult3 = self.call("getStarred2", "starred2", &[]).await?;
        Ok(starred.album.iter().map(|a| normalize::album(&self.server_id, a)).collect())
    }

    async fn favorite_artists(&self) -> Result<Vec<Artist>> {
        let starred: types::SearchResult3 = self.call("getStarred2", "starred2", &[]).await?;
        Ok(starred.artist.iter().map(|a| normalize::artist(&self.server_id, a, None)).collect())
    }

    /// The top level of the folder hierarchy; children load on open via `folder(id)`.
    async fn folder_tree(&self) -> Result<Vec<FolderNode>> {
        let root = self.folder(None).await?;
        Ok(root
            .folders
            .into_iter()
            .map(|e| FolderNode { id: e.id, name: e.name, children: vec![] })
            .collect())
    }

    async fn set_rating(&self, id: &str, rating: u8) -> Result<()> {
        self.call::<Value>("setRating", "", &[("id", id.into()), ("rating", rating.min(5).to_string())])
            .await
            .map(|_| ())
    }

    /// `setRating` takes any song, album or artist id.
    async fn set_item_rating(&self, _kind: ItemKind, id: &str, rating: u8) -> Result<()> {
        self.set_rating(id, rating).await
    }

    /// `getPlayQueue`: the queue another client saved (`Ok(None)` when none is saved).
    async fn play_queue(&self) -> Result<Option<listen_source::PlayQueue>> {
        let q: Value = self.call("getPlayQueue", "playQueue", &[]).await?;
        let entries: Vec<types::Child> = serde_json::from_value(q["entry"].clone()).unwrap_or_default();
        if entries.is_empty() {
            return Ok(None);
        }
        Ok(Some(listen_source::PlayQueue {
            songs: entries
                .iter()
                .filter(|c| !c.is_dir)
                .map(|c| normalize::song(&self.server_id, c))
                .collect(),
            current: q["current"]
                .as_str()
                .map(str::to_string)
                .or_else(|| q["current"].as_i64().map(|n| n.to_string())),
            position: Duration::from_millis(q["position"].as_u64().unwrap_or(0)),
            changed_by: q["changedBy"].as_str().map(str::to_string),
        }))
    }

    /// `savePlayQueue`: every song id, the playing one and how far it got (ms).
    async fn save_play_queue(&self, queue: &listen_source::PlayQueue) -> Result<()> {
        let mut params: Vec<(&str, String)> = queue.songs.iter().map(|s| ("id", s.id.clone())).collect();
        if let Some(current) = &queue.current {
            params.push(("current", current.clone()));
            params.push(("position", queue.position.as_millis().to_string()));
        }
        self.call::<Value>("savePlayQueue", "", &params).await.map(|_| ())
    }

    async fn scrobble(&self, song: &Song, event: ScrobbleEvent) -> Result<()> {
        let submission = matches!(event, ScrobbleEvent::Submission);
        self.call::<Value>("scrobble", "", &[("id", song.id.clone()), ("submission", submission.to_string())])
            .await
            .map(|_| ())
    }

    async fn stream(&self, song: &Song) -> Result<StreamSource> {
        // The original file (Feishin: skipAutoTranscode) unless the user capped the quality.
        let quality = StreamQuality::from_u8(self.quality.load(std::sync::atomic::Ordering::Relaxed));
        let transcode = self.transcode.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let params = if transcode.enabled {
            transcode_params(&song.id, &transcode)
        } else {
            stream_params(&song.id, quality)
        };
        Ok(StreamSource::direct(self.url("stream", &params).to_string()).insecure_tls(self.ignore_ssl))
    }

    fn set_stream_quality(&self, quality: StreamQuality) {
        self.quality.store(quality.to_u8(), std::sync::atomic::Ordering::Relaxed);
    }

    fn set_transcode(&self, transcode: &listen_core::settings::Transcode) {
        *self.transcode.lock().unwrap_or_else(|e| e.into_inner()) = transcode.clone();
    }

    fn download_url(&self, song: &Song) -> Option<String> {
        Some(self.url("download", &[("id", song.id.clone())]).to_string())
    }

    fn analysable(&self) -> bool {
        true
    }

    fn analysis_url(&self, song: &Song) -> Option<String> {
        // `format=raw`: never a transcode (a server-side transcoding rule would shift beat times).
        Some(self.url("stream", &[("id", song.id.clone()), ("format", "raw".into())]).to_string())
    }

    fn image_url(&self, image: &ImageRef, size: u32) -> Option<String> {
        match image {
            ImageRef::Id(id) => {
                Some(self.url("getCoverArt", &[("id", id.clone()), ("size", size.to_string())]).to_string())
            }
            ImageRef::Url(url) => Some(url.clone()),
        }
    }

    async fn lyrics(&self, song: &Song) -> Result<Option<Lyrics>> {
        let list: types::LyricsList = self
            .call("getLyricsBySongId", "lyricsList", &[("id", song.id.clone())])
            .await
            .unwrap_or_default();
        let Some(best) = list.structured_lyrics.iter().max_by_key(|l| l.synced) else {
            return Ok(None);
        };
        Ok(Some(Lyrics {
            provider: "server".into(),
            synced: best.synced,
            lines: best
                .line
                .iter()
                .map(|l| {
                    LyricLine::new(
                        best.synced.then(|| Duration::from_millis(l.start.unwrap_or(0))),
                        l.value.clone(),
                    )
                })
                .collect(),
            matched: None,
        }))
    }

    async fn start_scan(&self) -> Result<ScanStatus> {
        // Not a full scan: Navidrome picks up new and changed folders.
        let v: Value = self.call("startScan", "scanStatus", &[("fullScan", "false".into())]).await?;
        Ok(scan_status(&v))
    }

    async fn scan_status(&self) -> Result<ScanStatus> {
        let v: Value = self.call("getScanStatus", "scanStatus", &[]).await?;
        Ok(scan_status(&v))
    }
}

/// `scanStatus` payload → [`ScanStatus`].
pub fn scan_status(v: &Value) -> ScanStatus {
    ScanStatus { scanning: v["scanning"].as_bool().unwrap_or(false), count: v["count"].as_u64().unwrap_or(0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> Subsonic {
        Subsonic::new(ServerId("t".into()), "https://music.example.com/nd", "alice", "secret").unwrap()
    }

    #[test]
    fn builds_token_auth_urls() {
        let c = client();
        let url = c.url("ping", &[]);
        assert!(url.as_str().starts_with("https://music.example.com/nd/rest/ping?u=alice&t="));
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        let expected = format!("{:x}", Md5::digest(format!("secret{}", pairs["s"]).as_bytes()));
        assert_eq!(pairs["t"], expected);
        assert_eq!(pairs["f"], "json");
        assert_eq!(pairs["c"], "Listen");
        assert!(!url.as_str().contains("secret"));
    }

    #[test]
    fn legacy_auth_sends_the_hex_encoded_password() {
        let c = Subsonic::with_options(
            ServerId("t".into()),
            "https://music.example.com",
            "alice",
            "secret",
            Options { legacy_auth: true, ignore_ssl: false },
        )
        .unwrap();
        let url = c.url("ping", &[]);
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs["u"], "alice");
        assert_eq!(pairs["p"], "enc:736563726574");
        assert!(!pairs.contains_key("t") && !pairs.contains_key("s"));
        assert!(redact(url.as_str()).contains("p=***"));
    }

    #[test]
    fn ssl_option_sets_the_certificate_check_of_that_client_only() {
        assert!(format!("{:?}", client_builder(true)).contains("danger_accept_invalid_certs: true"));
        assert!(!format!("{:?}", client_builder(false)).contains("danger_accept_invalid_certs"));
    }

    #[test]
    fn redacts_credentials() {
        let c = client();
        let r = redact(c.url("stream", &[("id", "42".into())]).as_str());
        assert!(r.contains("t=***") && r.contains("s=***") && r.contains("id=42"));
    }

    #[test]
    fn parses_errors() {
        let body = serde_json::json!({"subsonic-response": {"status": "failed", "error": {"code": 40, "message": "Wrong username or password"}}});
        assert!(matches!(parse_response::<Value>(body, "x"), Err(SourceError::Auth(_))));
        let body = serde_json::json!({"subsonic-response": {"status": "failed", "error": {"code": 70, "message": "nope"}}});
        assert!(matches!(parse_response::<Value>(body, "x"), Err(SourceError::NotFound)));
    }

    #[test]
    fn maps_album_list_types() {
        let q = AlbumQuery { sort: AlbumSort::RecentlyAdded, ..Default::default() };
        assert_eq!(album_list_type(&q), vec![("type", "newest".to_string())]);
        let q = AlbumQuery { genre: Some("Rock".into()), ..Default::default() };
        assert_eq!(album_list_type(&q)[0], ("type", "byGenre".to_string()));
        let q = AlbumQuery { genres: vec!["Jazz".into()], ..Default::default() };
        assert_eq!(album_list_type(&q)[1], ("genre", "Jazz".to_string()));
        // A year range wins over the sort (Feishin).
        let q = AlbumQuery { year_min: Some(1990), year_max: Some(1999), ..Default::default() };
        assert_eq!(
            album_list_type(&q),
            vec![("type", "byYear".into()), ("fromYear", "1990".into()), ("toYear", "1999".into())]
        );
        let q = AlbumQuery { sort: AlbumSort::AlbumArtist, ..Default::default() };
        assert_eq!(album_list_type(&q), vec![("type", "alphabeticalByArtist".to_string())]);
    }

    #[test]
    fn normalizes_album_fixture() {
        let body: Value = serde_json::from_str(include_str!("../tests/fixtures/get_album.json")).unwrap();
        let raw: types::AlbumId3 = parse_response(body, "album").unwrap();
        let album = normalize::album(&ServerId("t".into()), &raw);
        assert_eq!(album.name, "Night Drive");
        assert_eq!(album.artist_line(), "Neon Coast");
        assert_eq!(album.songs.len(), 2);
        let s = &album.songs[0];
        assert_eq!(s.title, "Midnight Signal");
        assert_eq!(s.duration, Duration::from_secs(214));
        assert!(s.favorite);
        assert_eq!(s.codec.as_deref(), Some("FLAC"));
        assert_eq!(s.gain.track_gain, Some(-6.5));
        assert_eq!(s.genres, ["Synthwave", "Electronic"]);
        assert_eq!(album.songs[1].artist_line(), "Neon Coast, Guest");
    }
}

/// Query parameters of a `stream` request for `quality`.
fn stream_params(id: &str, quality: StreamQuality) -> Vec<(&'static str, String)> {
    let mut params = vec![("id", id.to_string())];
    if let Some(kbps) = quality.max_kbps() {
        params.push(("maxBitRate", kbps.to_string()));
        params.push(("format", "mp3".into()));
    }
    params
}

/// `stream` parameters for Feishin-style transcoding: `format` and `maxBitRate` (kbit/s) only when
/// set, else the server's default.
fn transcode_params(id: &str, t: &listen_core::settings::Transcode) -> Vec<(&'static str, String)> {
    let mut params = vec![("id", id.to_string())];
    if let Some(format) = t.format() {
        params.push(("format", format));
    }
    if t.bitrate > 0 {
        params.push(("maxBitRate", t.bitrate.to_string()));
    }
    params
}

#[cfg(test)]
mod quality_tests {
    use super::{StreamQuality, stream_params, transcode_params};
    use listen_core::settings::Transcode;

    #[test]
    fn transcoding_sends_the_chosen_format_and_bit_rate() {
        let t = Transcode { enabled: true, format: " Opus ".into(), bitrate: 128, max_sample_rate: 48000 };
        assert_eq!(
            transcode_params("s1", &t),
            [("id", "s1".to_string()), ("format", "opus".to_string()), ("maxBitRate", "128".to_string())]
        );
        // Bit rate 0: the server picks; no format: the server's default.
        let t = Transcode { enabled: true, ..Default::default() };
        assert_eq!(transcode_params("s1", &t), [("id", "s1".to_string())]);
    }

    #[test]
    fn original_quality_sends_no_transcoding_parameters() {
        assert_eq!(stream_params("s1", StreamQuality::Original), [("id", "s1".to_string())]);
    }

    #[test]
    fn capped_quality_asks_the_server_to_transcode() {
        let p = stream_params("s1", StreamQuality::Low);
        assert!(p.contains(&("maxBitRate", "96".to_string())));
        assert!(p.contains(&("format", "mp3".to_string())));
        assert!(p.iter().all(|(k, _)| *k != "estimateContentLength"));
        assert!(stream_params("s1", StreamQuality::High).contains(&("maxBitRate", "320".to_string())));
    }
}

/// Moves `items[from]` to `to`; `false` when nothing changes or an index is out of range.
fn reorder<T>(items: &mut Vec<T>, from: usize, to: usize) -> bool {
    if from == to || from >= items.len() || to >= items.len() {
        return false;
    }
    let item = items.remove(from);
    items.insert(to, item);
    true
}

#[cfg(test)]
mod reorder_tests {
    use super::reorder;

    #[test]
    fn moves_down_and_up() {
        let mut v = vec!["a", "b", "c", "d"];
        assert!(reorder(&mut v, 0, 2));
        assert_eq!(v, ["b", "c", "a", "d"]);
        assert!(reorder(&mut v, 3, 0));
        assert_eq!(v, ["d", "b", "c", "a"]);
    }

    #[test]
    fn consecutive_drags_apply_in_order() {
        let mut v = vec!["a", "b", "c", "d"];
        assert!(reorder(&mut v, 0, 3));
        assert!(reorder(&mut v, 1, 0));
        assert_eq!(v, ["c", "b", "d", "a"]);
    }

    #[test]
    fn rejects_noops_and_bad_indices() {
        let mut v = vec![1, 2];
        assert!(!reorder(&mut v, 1, 1));
        assert!(!reorder(&mut v, 0, 5));
        assert!(!reorder(&mut v, 5, 0));
        assert_eq!(v, [1, 2]);
    }
}
