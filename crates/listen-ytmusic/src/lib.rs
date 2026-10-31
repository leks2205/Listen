//! YouTube Music backend (unofficial API use; YouTube changes break it from time to time).
//!
//! * Library, search, albums, artists, playlists, likes, history, home feed: `ytmapi-rs` (InnerTube),
//!   signed in with cookies from a local browser ([`browser`]) or a cookie export ([`cookies`]).
//! * Audio: `yt-dlp` resolves a stream URL for mpv. Analysed or DJ-mixed songs are fetched into a local
//!   cache ([`audio`]) under a shared request budget ([`budget`]); playback prefers the cached copy.

mod artist;
pub mod audio;
pub mod browser;
pub mod budget;
pub mod channels;
pub mod cookies;
pub mod favorites;
mod home;
mod library;
pub mod query;
pub mod token;
pub mod ytdlp;

use async_trait::async_trait;
use listen_core::model::{
    Album, Artist, ArtistRef, GainInfo, ImageRef, LyricLine, Lyrics, Page, Playlist, SearchResults, ServerId,
    Song, SourceKind,
};
use listen_core::settings::StreamQuality;
use listen_source::{
    AlbumQuery, AlbumSort, AnalysisSource, AudioInput, Capabilities, HomeShelf, ItemKind, ListQuery,
    MusicSource, Result, ScrobbleEvent, SongQuery, SourceError, StreamSource, Urgency, clientside,
};
use rand::seq::SliceRandom;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use ytmapi_rs::YtMusic;
use ytmapi_rs::common::{AlbumID, ArtistChannelID, Explicit, LikeStatus, Thumbnail, VideoID, YoutubeID};
use ytmapi_rs::parse::{ParsedSongAlbum, ParsedSongArtist};

/// Library > Albums.
const LIBRARY_ALBUMS: &str = "FEmusic_liked_albums";
/// Library > Playlists.
const LIBRARY_PLAYLISTS: &str = "FEmusic_liked_playlists";
/// Library lists are fetched completely and reused for this long.
const LIBRARY_TTL: Duration = Duration::from_secs(10 * 60);
/// How long after Listen edits a playlist its reads are not cached.
const EDIT_SETTLE: Duration = Duration::from_secs(90);
/// Browse id of the "Liked Music" auto-playlist.
pub const LIKED_PLAYLIST: &str = "VLLM";
/// Pseudo playlist of the songs the user uploaded (a library page, not a playlist).
pub const UPLOADS_PLAYLIST: &str = "FEmusic_library_privately_owned_tracks";
/// Cookie for signed-out use: only the EU consent cookie (search, albums,
/// artists and radio work; there is no personal library).
pub const GUEST_COOKIE: &str = "SOCS=CAI; SAPISID=guest;";

struct Cached<T> {
    at: Instant,
    items: Vec<T>,
}

/// The liked songs with their search haystacks, built once per library refresh.
struct CachedSongs {
    at: Instant,
    list: std::sync::Arc<clientside::SongList>,
}

/// Runs a ytmapi-rs call: `yt!(self, yt => yt.get_album(id).await)` →
/// `Result<_, SourceError>`.
macro_rules! yt {
    ($self:ident, $yt:ident => $call:expr) => {{
        let $yt = &$self.yt;
        $call.map_err(source_err)
    }};
}

/// Like [`yt!`] for read-only calls, retried twice: YouTube occasionally
/// breaks off a response or answers with a different page layout.
macro_rules! yt_retry {
    ($self:ident, $yt:ident => $call:expr) => {{
        let mut attempt = 0;
        loop {
            let $yt = &$self.yt;
            match $call.map_err(source_err) {
                Err(e) if attempt < 2 && !matches!(e, SourceError::Auth(_)) => {
                    attempt += 1;
                    tracing::debug!("retrying after: {e}");
                    tokio::time::sleep(Duration::from_millis(400 * attempt)).await;
                }
                other => break other,
            }
        }
    }};
}

/// One entry of a playlist as YouTube Music edits it: the song and the id of
/// this place in the playlist (the same song can be in it twice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub video_id: String,
    pub set_video_id: Option<String>,
}

pub struct YouTubeMusic {
    server_id: ServerId,
    yt: YtMusic<token::CookieToken>,
    /// The cookies in use, to read them from the browser again when YouTube rotates the session.
    session: Option<token::Session>,
    ytdlp: ytdlp::YtDlp,
    songs: Mutex<Option<CachedSongs>>,
    albums: Mutex<Option<Cached<Album>>>,
    /// Complete playlists by browse id, and ones being loaded page by page.
    playlist_cache: Mutex<std::collections::HashMap<String, Cached<Song>>>,
    /// When Listen last edited each playlist. YouTube Music shows its own edits with a delay, so a read
    /// shortly after an edit is not cached.
    edited: std::sync::Mutex<std::collections::HashMap<String, Instant>>,
    playlist_partial: Mutex<std::collections::HashMap<String, Vec<Song>>>,
    /// The entries of the playlists read so far, in list order: the ids that
    /// removing and moving address (see [`PlaylistEntry`]).
    playlist_items: std::sync::Mutex<std::collections::HashMap<String, Vec<PlaylistEntry>>>,
    guest: bool,
    /// The last artist page fetched: the page, the albums, the top songs and the
    /// related artists all come from the same response.
    artist_page: std::sync::Mutex<Option<(String, std::time::Instant, std::sync::Arc<artist::ArtistData>)>>,
    /// `StreamQuality` as a `u8` (see `to_u8`).
    quality: std::sync::atomic::AtomicU8,
}

/// Reads an artist page with ytmapi-rs; when that fails (typically a page with
/// a visual header instead of the immersive one) the raw response is read once
/// with Listen's own parser. Auth errors are not retried through the fallback.
async fn load_artist<F1, Fut1, F2, Fut2>(primary: F1, fallback: F2) -> Result<artist::ArtistData>
where
    F1: FnOnce() -> Fut1,
    Fut1: std::future::Future<Output = Result<artist::ArtistData>>,
    F2: FnOnce() -> Fut2,
    Fut2: std::future::Future<Output = Result<artist::ArtistData>>,
{
    match primary().await {
        Err(e) if !matches!(e, SourceError::Auth(_)) => {
            tracing::debug!("artist page: {e}; reading the raw response");
            fallback().await
        }
        other => other,
    }
}

fn source_err(e: ytmapi_rs::Error) -> SourceError {
    let text = e.to_string();
    match e.into_kind() {
        ytmapi_rs::error::ErrorKind::Header | ytmapi_rs::error::ErrorKind::InvalidUserAgent(_) => {
            SourceError::Auth(format!("YouTube Music rejected the sign-in ({text})"))
        }
        ytmapi_rs::error::ErrorKind::OtherErrorCodeInResponse { code: 401 | 403, message } => {
            SourceError::Auth(format!("sign-in expired or invalid: {message}"))
        }
        ytmapi_rs::error::ErrorKind::Web { message } => SourceError::Network(message),
        _ => SourceError::Other(text),
    }
}

/// "3:45" / "1:02:03" → Duration.
pub fn parse_duration(s: &str) -> Duration {
    let secs = s
        .trim()
        .split(':')
        .try_fold(0u64, |acc, part| part.trim().parse::<u64>().ok().map(|n| acc * 60 + n))
        .unwrap_or(0);
    Duration::from_secs(secs)
}

/// Largest thumbnail, as an image reference.
fn thumb(list: &[Thumbnail]) -> Option<ImageRef> {
    list.iter().max_by_key(|t| t.width).map(|t| ImageRef::Url(t.url.clone()))
}

/// Rewrites googleusercontent sizing (`=w120-h120-…`) to the requested size.
pub fn resize_thumbnail(url: &str, size: u32) -> String {
    if let Some(pos) = url.rfind("=w")
        && url.contains("googleusercontent.com")
    {
        return format!("{}=w{size}-h{size}-l90-rj", &url[..pos]);
    }
    url.to_string()
}

fn artist_refs(list: &[ParsedSongArtist]) -> Vec<ArtistRef> {
    list.iter()
        .map(|a| ArtistRef { id: a.id.as_ref().map(|id| id.get_raw().to_string()), name: a.name.clone() })
        .collect()
}

fn name_ref(name: &str) -> Vec<ArtistRef> {
    if name.is_empty() { vec![] } else { vec![ArtistRef { id: None, name: name.to_string() }] }
}

/// Playlist ids need the `VL` prefix for browsing.
fn playlist_browse_id(id: &str) -> String {
    if id.starts_with("VL") || id == UPLOADS_PLAYLIST { id.to_string() } else { format!("VL{id}") }
}

/// Page cursors for [`MusicSource::playlist_page`]: the continuation style + token.
fn encode_cursor(c: &home::Continuation) -> String {
    match c {
        home::Continuation::Body(t) => format!("B|{t}"),
        home::Continuation::Params(t) => format!("P|{t}"),
    }
}

fn decode_cursor(s: &str) -> Option<home::Continuation> {
    match s.split_once('|')? {
        ("B", t) => Some(home::Continuation::Body(t.to_string())),
        ("P", t) => Some(home::Continuation::Params(t.to_string())),
        _ => None,
    }
}

/// Whether a library page is YouTube's "nothing here yet" message.
fn empty_message(json: &serde_json::Value) -> bool {
    json.pointer(
        "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents",
    )
    .and_then(serde_json::Value::as_array)
    .is_some_and(|c| {
        c.len() == 1 && c[0].pointer("/itemSectionRenderer/contents/0/messageRenderer").is_some()
    })
}

/// "Alex • 25 tracks" (a library grid tile's subtitle) → the author and the track count.
fn playlist_subtitle(subtitle: &str) -> (Option<String>, Option<u32>) {
    // List rows start with the type ("Playlist • Alex • 25 tracks").
    let parts: Vec<&str> = subtitle
        .split(" • ")
        .map(str::trim)
        .filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("playlist"))
        .collect();
    let count = parts.iter().rev().find_map(|p| track_count(p));
    let author = parts.first().filter(|p| track_count(p).is_none()).map(|p| p.to_string());
    (author, count)
}

fn track_count(label: &str) -> Option<u32> {
    label.split_whitespace().next().and_then(|n| n.replace([',', '.'], "").parse().ok())
}

fn year(s: &str) -> Option<i32> {
    s.trim().parse().ok()
}

impl YouTubeMusic {
    /// Signs in with the stored secret: a browser profile to read cookies from
    /// ([`browser::Profile::to_secret`]) or a music.youtube.com cookie export (JSON, Netscape `cookies.txt`
    /// or a raw `Cookie` header). Empty = guest.
    pub async fn connect(server_id: ServerId, secret: &str) -> Result<Self> {
        if let Some(profile) = browser::Profile::from_secret(secret) {
            // The profile, not just its cookies: YouTube rotates the session token while Listen
            // runs, and the cookies have to be read again when it does (see `token::Session`).
            return Self::connect_browser(server_id, profile, false).await;
        }
        let cookie = secret;
        let guest = cookie.trim().is_empty() || cookie.trim() == GUEST_COOKIE;
        let cookie = if guest {
            GUEST_COOKIE.to_string()
        } else {
            cookies::to_header(cookie).map_err(SourceError::Auth)?
        };
        if !cookie.contains("SAPISID=") {
            return Err(SourceError::Auth(
                "no SAPISID cookie found — export the cookies while signed in to music.youtube.com".into(),
            ));
        }
        Self::connect_cookies(server_id, &cookie, None, guest).await
    }

    /// Signs in with a `Cookie` header, acting as the brand-account channel
    /// `page_id` if given.
    pub async fn connect_cookies(
        server_id: ServerId,
        cookie: &str,
        page_id: Option<String>,
        guest: bool,
    ) -> Result<Self> {
        let token = token::CookieToken::new(cookie, page_id).await.map_err(SourceError::Auth)?;
        Self::with_token(server_id, token, guest)
    }

    /// As [`YouTubeMusic::connect_cookies`], reading the cookies from the browser again while Listen
    /// runs: YouTube rotates the session token, and an old one is answered as signed out.
    pub async fn connect_browser(
        server_id: ServerId,
        profile: browser::Profile,
        guest: bool,
    ) -> Result<Self> {
        let page_id = profile.page_id.clone();
        let token = token::CookieToken::from_profile(profile, page_id).await.map_err(SourceError::Auth)?;
        Self::with_token(server_id, token, guest)
    }

    fn with_token(server_id: ServerId, token: token::CookieToken, guest: bool) -> Result<Self> {
        let session = token.session().can_refresh().then(|| token.session());
        let yt =
            ytmapi_rs::YtMusicBuilder::new_rustls_tls().with_auth_token(token).build().map_err(source_err)?;
        Ok(Self {
            session,
            server_id,
            yt,
            ytdlp: ytdlp::YtDlp::new(),
            quality: std::sync::atomic::AtomicU8::new(0),
            songs: Mutex::default(),
            albums: Mutex::default(),
            playlist_cache: Mutex::default(),
            edited: std::sync::Mutex::default(),
            playlist_partial: Mutex::default(),
            playlist_items: std::sync::Mutex::default(),
            artist_page: std::sync::Mutex::default(),
            guest,
        })
    }

    /// Signed out: no library, likes, playlists or history.
    pub fn is_guest(&self) -> bool {
        self.guest
    }

    pub async fn check_ytdlp(&self) -> std::result::Result<String, String> {
        self.ytdlp.check().await
    }

    /// The song's audio as a local file, fetched into [`audio::cache`] if missing. `Ok(None)`: a background
    /// fetch found no budget slot (try later) or the only format is HLS. A failing edge node gets the
    /// playback retry ladder (backup server, fresh URL, its backup).
    pub async fn fetch_audio(
        &self,
        video_id: &str,
        urgency: Urgency,
    ) -> std::result::Result<Option<audio::Entry>, String> {
        let cache = audio::cache();
        if let Some(e) = cache.lookup(video_id) {
            return Ok(Some(e));
        }
        let lock = cache.fetch_lock(video_id);
        let _fetching = lock.lock().await;
        if let Some(e) = cache.lookup(video_id) {
            return Ok(Some(e));
        }
        if !budget::BUDGET.acquire(urgency, budget::BACKGROUND_MAX_WAIT).await {
            tracing::debug!("no request budget to fetch {video_id} now");
            return Ok(None);
        }
        std::fs::create_dir_all(cache.dir()).map_err(|e| format!("{}: {e}", cache.dir().display()))?;
        let mut last_err = String::from("no URL");
        for attempt in 0..4 {
            let resolved = if attempt < 2 {
                self.ytdlp.resolve_reserved(video_id).await?
            } else {
                if attempt == 2 {
                    self.ytdlp.forget(video_id);
                }
                self.ytdlp.resolve(video_id).await?
            };
            let (Some(name), Some(len)) = (audio::file_name(video_id, &resolved.facts), resolved.facts.clen)
            else {
                tracing::info!("{video_id}: no fetchable audio format ({:?})", resolved.facts.itag);
                return Ok(None);
            };
            let url = if attempt % 2 == 1 {
                match ytdlp::fallback_url(&resolved.url) {
                    Some(u) => u,
                    None => continue,
                }
            } else {
                resolved.url.clone()
            };
            let part = cache.part_path(&name);
            let started = Instant::now();
            match audio::download(audio::http(), &url, &part, len).await {
                Ok(()) => {
                    let path = cache.commit(&name, len).map_err(|e| e.to_string())?;
                    tracing::debug!(
                        "fetched {video_id} (itag {:?}, {len} B) in {:.1}s, attempt {}",
                        resolved.facts.itag,
                        started.elapsed().as_secs_f32(),
                        attempt + 1
                    );
                    return Ok(Some(audio::Entry { path, file: resolved.facts.file_id() }));
                }
                Err(e) => {
                    tracing::warn!("fetching {video_id} failed (attempt {}): {e}", attempt + 1);
                    let _ = std::fs::remove_file(&part);
                    last_err = e;
                }
            }
        }
        Err(format!("could not fetch the audio: {last_err}"))
    }

    // One argument per field that ytmapi-rs's various song structs share.
    #[allow(clippy::too_many_arguments)]
    fn song(
        &self,
        video_id: &VideoID<'_>,
        title: &str,
        artists: Vec<ArtistRef>,
        album: Option<&ParsedSongAlbum>,
        duration: &str,
        thumbnails: &[Thumbnail],
        like: Option<&LikeStatus>,
        explicit: &Explicit,
    ) -> Song {
        Song {
            mbz: Default::default(),
            extra: Default::default(),
            id: video_id.get_raw().to_string(),
            server_id: self.server_id.clone(),
            source: SourceKind::YouTubeMusic,
            title: title.to_string(),
            album_artists: artists.clone(),
            artists,
            album: album.map(|a| a.name.clone()),
            album_id: album.map(|a| a.id.get_raw().to_string()),
            image: thumb(thumbnails),
            duration: parse_duration(duration),
            track_number: None,
            disc_number: None,
            year: None,
            genres: vec![],
            codec: None,
            bit_rate: None,
            sample_rate: None,
            bit_depth: None,
            bpm: None,
            favorite: matches!(like, Some(LikeStatus::Liked)),
            rating: None,
            play_count: None,
            gain: GainInfo::default(),
            explicit: matches!(explicit, Explicit::IsExplicit),
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: None,
            comment: None,
        }
    }

    /// The song radio of `song` with the official audio where YouTube pairs a video with its song:
    /// a music video as the seed is replaced by its song first (a video's radio is made of videos),
    /// and every video entry that has a song version becomes that song (with its album).
    async fn song_radio(&self, song: &Song, count: usize) -> Result<Vec<Song>> {
        let mut seed = song.id.clone();
        if song.album.is_none() {
            let q = library::RawPost::new(
                "next",
                serde_json::json!({"videoId": seed, "isAudioOnly": true, "enablePersistentPlaylistPanel": true}),
            );
            if let Ok(json) = self.raw(&q).await
                && let Some(first) = library::radio_entries(&json).into_iter().next()
                && first.video_id != seed
            {
                tracing::debug!("song radio: {} is a video, starting from its song {}", seed, first.video_id);
                seed = first.video_id;
            }
        }
        if seed == song.id && song.album.is_none() {
            // A video YouTube pairs with no song (its radio would be videos too): find the song.
            for text in listen_core::substitute::queries(song) {
                let Ok(found) = self.search(&text, 10).await else { break };
                if let Some(s) = listen_core::substitute::song_of_video(song, &found.songs) {
                    tracing::debug!("song radio: {} is a video, its song is {} ({})", seed, s.id, s.title);
                    seed = s.id.clone();
                    break;
                }
            }
        }
        let q = library::RawPost::new(
            "next",
            serde_json::json!({
                "videoId": seed,
                "playlistId": format!("RDAMVM{seed}"),
                "isAudioOnly": true,
                "enablePersistentPlaylistPanel": true,
                "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
            }),
        );
        let json = self.raw(&q).await?;
        let mut seen = std::collections::HashSet::new();
        let mut entries: Vec<library::RadioEntry> = library::radio_entries(&json)
            .into_iter()
            .filter(|e| e.video_id != song.id && e.video_id != seed && seen.insert(e.video_id.clone()))
            .collect();
        // Songs first, then official videos; uploads (covers, fancams, clips) only when the radio
        // has too few of the others. A video-only seed (no song version) gets a video radio.
        entries.sort_by_key(|e| e.kind);
        let proper = entries.iter().filter(|e| e.kind < 2).count();
        if proper >= count.min(10) {
            entries.retain(|e| e.kind < 2);
        }
        Ok(entries.into_iter().take(count).map(|e| self.radio_song(e)).collect())
    }

    fn radio_song(&self, e: library::RadioEntry) -> Song {
        let artists: Vec<ArtistRef> =
            e.artists.into_iter().map(|(name, id)| ArtistRef { id, name }).collect();
        Song {
            mbz: Default::default(),
            extra: Default::default(),
            id: e.video_id,
            server_id: self.server_id.clone(),
            source: SourceKind::YouTubeMusic,
            title: e.title,
            album_artists: artists.clone(),
            artists,
            album: e.album.as_ref().map(|a| a.0.clone()),
            album_id: e.album.map(|a| a.1),
            image: thumb(&e.thumbnails),
            duration: parse_duration(&e.length),
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

    /// Drops the cached library (songs, albums, complete and partial playlists
    /// and the open artist page): the next reads are one fresh burst.
    async fn forget_library(&self) {
        *self.songs.lock().await = None;
        *self.albums.lock().await = None;
        self.playlist_cache.lock().await.clear();
        self.playlist_partial.lock().await.clear();
        *self.artist_page.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    async fn library_list(&self) -> Result<std::sync::Arc<clientside::SongList>> {
        if self.guest {
            return Ok(std::sync::Arc::new(clientside::SongList::new(vec![])));
        }
        let mut cache = self.songs.lock().await;
        if let Some(c) = cache.as_ref().filter(|c| c.at.elapsed() < LIBRARY_TTL) {
            return Ok(c.list.clone());
        }
        // YouTube Music's Library > Songs (liked songs are under Favorites = Liked Music).
        let items = self.browse_songs("FEmusic_liked_videos").await?;
        let list = std::sync::Arc::new(clientside::SongList::new(items));
        *cache = Some(CachedSongs { at: Instant::now(), list: list.clone() });
        Ok(list)
    }

    /// The library as plain songs, for the callers that do not query it (random play).
    async fn library_songs(&self) -> Result<Vec<Song>> {
        Ok(self.library_list().await?.songs().to_vec())
    }

    async fn library_albums(&self) -> Result<Vec<Album>> {
        if self.guest {
            return Ok(vec![]);
        }
        let cache = self.albums.lock().await;
        if let Some(c) = cache.as_ref().filter(|c| c.at.elapsed() < LIBRARY_TTL) {
            return Ok(c.items.clone());
        }
        // Library > Albums on YouTube Music, in its order (recently added first).
        let mut items = Vec::new();
        for item in self.browse_grid(LIBRARY_ALBUMS).await? {
            if let home::Item::Album { id, title, artists, year, thumbs } = item {
                let artists = artists.into_iter().map(|n| ArtistRef { id: n.id, name: n.name }).collect();
                let mut album = self.album_stub(&id, &title, artists, &year, &thumbs);
                album.favorite = true;
                items.push(album);
            }
        }
        drop(cache);
        *self.albums.lock().await = Some(Cached { at: Instant::now(), items: items.clone() });
        Ok(items)
    }

    /// The artist page (cached for a minute, so opening an artist is one request).
    async fn artist_data(&self, id: &str) -> Result<std::sync::Arc<artist::ArtistData>> {
        {
            let cache = self.artist_page.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((cached, at, page)) = cache.as_ref()
                && cached == id
                && at.elapsed() < Duration::from_secs(60)
            {
                return Ok(page.clone());
            }
        }
        let page = load_artist(
            || async {
                yt!(self, yt => yt.get_artist(&ArtistChannelID::from_raw(id.to_string())).await)
                    .map(|a| artist::ArtistData::from(&a))
            },
            || async { artist::parse(&self.raw_browse(id).await?).map_err(SourceError::Other) },
        )
        .await
        .inspect_err(|e| tracing::warn!("artist page {id}: {e}"))?;
        let page = std::sync::Arc::new(page);
        *self.artist_page.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((id.to_string(), std::time::Instant::now(), page.clone()));
        Ok(page)
    }

    fn album_stub(
        &self,
        id: &str,
        name: &str,
        artists: Vec<ArtistRef>,
        year_s: &str,
        thumbs: &[Thumbnail],
    ) -> Album {
        Album {
            release_types: Vec::new(),
            id: id.to_string(),
            server_id: self.server_id.clone(),
            source: SourceKind::YouTubeMusic,
            name: name.to_string(),
            album_artists: artists,
            image: thumb(thumbs),
            year: year(year_s),
            release_type: None,
            genres: vec![],
            song_count: None,
            duration: Duration::ZERO,
            favorite: false,
            rating: None,
            play_count: None,
            songs: vec![],
            info: Default::default(),
        }
    }

    /// Albums of recently played songs, most recent first.
    async fn recent_albums(&self) -> Result<Vec<Album>> {
        if self.guest {
            return Ok(vec![]);
        }
        let history = yt!(self, yt => yt.get_history().await)?;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for item in history.iter().flat_map(|p| &p.items) {
            if let ytmapi_rs::parse::HistoryItem::Song(s) = item
                && seen.insert(s.album.id.get_raw().to_string())
            {
                out.push(self.album_stub(
                    s.album.id.get_raw(),
                    &s.album.name,
                    artist_refs(&s.artists),
                    "",
                    &s.thumbnails,
                ));
            }
        }
        Ok(out)
    }

    fn page<T: Clone>(items: &[T], start: usize, limit: usize) -> Page<T> {
        let limit = if limit == 0 { 100 } else { limit };
        Page {
            items: items.iter().skip(start).take(limit).cloned().collect(),
            start_index: start,
            total: Some(items.len()),
        }
    }
}

/// What a YouTube Music connection supports: a guest only browses and plays; an account also edits
/// playlists (add, create, rename, delete, remove and move songs), likes and scrobbles.
pub fn capabilities(guest: bool) -> Capabilities {
    let base = Capabilities::SIMILAR_SONGS
        | Capabilities::RADIO
        | Capabilities::LYRICS
        | Capabilities::ARTIST_INFO
        | Capabilities::ITEM_DETAILS;
    if guest {
        base
    } else {
        base | Capabilities::PLAYLIST_RECOMMENDATIONS
            | Capabilities::PLAYLIST_EDIT
            | Capabilities::PLAYLIST_RENAME
            | Capabilities::PLAYLIST_REMOVE
            | Capabilities::PLAYLIST_REORDER
            | Capabilities::PLAYLIST_UPDATE
            | Capabilities::FAVORITE_ITEMS
            | Capabilities::SCROBBLE
    }
}

#[async_trait]
impl MusicSource for YouTubeMusic {
    fn kind(&self) -> SourceKind {
        SourceKind::YouTubeMusic
    }

    async fn refresh(&self) {
        self.forget_library().await;
    }

    fn capabilities(&self) -> Capabilities {
        capabilities(self.guest)
    }

    async fn ping(&self) -> Result<()> {
        if self.guest {
            return yt!(self, yt => yt.search_songs("music").await).map(|_| ());
        }
        let q = home::BrowseQuery { browse_id: Some(LIBRARY_PLAYLISTS.to_string()), continuation: None };
        yt!(self, yt => yt.raw_json_query::<home::BrowseQuery>(&q).await).map(|_| ())
    }

    fn album_sorts(&self) -> &'static [listen_source::SortOption<AlbumSort>] {
        listen_source::sorting::LIBRARY_ALBUMS
    }

    fn song_sorts(&self) -> &'static [listen_source::SortOption<listen_source::SongSort>] {
        listen_source::sorting::LIBRARY_SONGS
    }

    fn artist_sorts(&self) -> &'static [listen_source::SortOption<listen_source::ArtistSort>] {
        listen_source::sorting::LIBRARY_ARTISTS
    }

    // Library albums carry no likes, genres or artist ids: only the year range filters.
    fn album_filters(&self) -> listen_source::QueryFilters {
        listen_source::QueryFilters::YEAR_RANGE
    }

    fn song_filters(&self) -> listen_source::QueryFilters {
        listen_source::QueryFilters::FAVORITE
    }

    fn artist_filters(&self) -> listen_source::QueryFilters {
        listen_source::QueryFilters::empty()
    }

    async fn albums(&self, q: &AlbumQuery) -> Result<Page<Album>> {
        // Search, sort and filters work on the library (Library > Albums), like the list pages
        // of the other sources; the catalogue search is the search page.
        let items = match q.sort {
            AlbumSort::RecentlyPlayed => self.recent_albums().await?,
            _ => self.library_albums().await?,
        };
        let items = query::albums_in(items, q);
        Ok(Self::page(&items, q.start, q.limit))
    }

    async fn album(&self, id: &str) -> Result<Album> {
        let a = yt_retry!(self, yt => yt.get_album(AlbumID::from_raw(id.to_string())).await)?;
        let artists = artist_refs(&a.artists);
        let album_ref = ParsedSongAlbum { name: a.title.clone(), id: AlbumID::from_raw(id.to_string()) };
        let songs: Vec<Song> = a
            .tracks
            .iter()
            .map(|t| {
                let mut s = self.song(
                    &t.video_id,
                    &t.title,
                    artists.clone(),
                    Some(&album_ref),
                    &t.duration,
                    &a.thumbnails,
                    Some(&t.like_status),
                    &t.explicit,
                );
                s.track_number = Some(t.track_no as u32);
                s.year = year(&a.year);
                s
            })
            .collect();
        let mut album = self.album_stub(id, &a.title, artists, &a.year, &a.thumbnails);
        album.duration = songs.iter().map(|s| s.duration).sum();
        album.song_count = Some(songs.len() as u32);
        album.release_type = Some(format!("{:?}", a.category));
        album.info.comment = a.description.clone().filter(|d| !d.trim().is_empty());
        album.info.release_date = year(&a.year).map(|y| y.to_string());
        album.songs = songs;
        Ok(album)
    }

    async fn artists(&self, q: &ListQuery) -> Result<Page<Artist>> {
        if self.guest {
            return Ok(Self::page(&[], 0, 0));
        }
        // The artists you subscribed to (Library > Artists > Subscriptions),
        // music artist channels only.
        let mut found = Vec::new();
        let mut query = library::RawPost::new(
            "browse",
            serde_json::json!({"browseId": "FEmusic_library_corpus_artists"}),
        );
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let json = self.raw(&query).await?;
            let (page, next) = library::artist_page(&json);
            found.extend(page);
            match next {
                Some(c) if seen.insert(c.token().to_string()) => query.continuation = Some(c),
                _ => break,
            }
        }
        // Without page types (a layout change) keep everything rather than nothing.
        if found.iter().any(|a| a.artist) {
            found.retain(|a| a.artist);
        }
        let mut all: Vec<Artist> = found
            .into_iter()
            .map(|a| Artist {
                id: a.id,
                server_id: self.server_id.clone(),
                source: SourceKind::YouTubeMusic,
                name: a.name,
                image: thumb(&a.thumbs),
                album_count: None,
                song_count: None,
                favorite: true,
                rating: None,
                biography: None,
                similar: vec![],
            })
            .collect();
        let needle = q.search.as_deref().unwrap_or("").to_lowercase();
        all.retain(|a| needle.is_empty() || a.name.to_lowercase().contains(&needle));
        all.sort_by_key(|a| a.name.to_lowercase());
        Ok(Self::page(&all, q.start, q.limit))
    }

    async fn home(&self) -> Result<Vec<HomeShelf>> {
        // The first answer after a connect sometimes arrives without shelves (an older layout or
        // the signed-out page), which parses to an empty feed: ask once more instead of leaving
        // the page empty.
        let mut shelves = Vec::new();
        for attempt in 1..=2 {
            if attempt > 1 {
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
            shelves = self.home_pages().await?;
            if !shelves.is_empty() {
                return Ok(shelves);
            }
            tracing::info!("home feed: no shelves in the answer (attempt {attempt})");
        }
        Ok(shelves)
    }

    async fn artist(&self, id: &str) -> Result<Artist> {
        let a = self.artist_data(id).await?;
        Ok(Artist {
            id: id.to_string(),
            server_id: self.server_id.clone(),
            source: SourceKind::YouTubeMusic,
            name: a.name.clone(),
            image: thumb(&a.thumbnails),
            album_count: Some(a.albums.len() as u32).filter(|n| *n > 0),
            song_count: None,
            favorite: a.subscribed,
            rating: None,
            biography: a.description.clone(),
            similar: a.related.iter().map(|s| ArtistRef { id: s.id.clone(), name: s.name.clone() }).collect(),
        })
    }

    async fn artist_info(&self, id: &str) -> Result<listen_source::ArtistInfo> {
        let a = self.artist_data(id).await?;
        let related: Vec<(ArtistRef, String)> = a
            .related
            .iter()
            .filter_map(|s| {
                let id = s.id.clone()?;
                Some((ArtistRef { id: Some(id.clone()), name: s.name.clone() }, id))
            })
            .collect();
        Ok(listen_source::ArtistInfo {
            biography: a.description.clone().filter(|b| !b.trim().is_empty()),
            similar: related.iter().map(|(r, _)| r.clone()).collect(),
            // The related shelf has no pictures; the cards show the placeholder.
            similar_artists: related
                .into_iter()
                .map(|(r, id)| Artist {
                    id,
                    server_id: self.server_id.clone(),
                    source: SourceKind::YouTubeMusic,
                    name: r.name,
                    image: None,
                    album_count: None,
                    song_count: None,
                    favorite: false,
                    biography: None,
                    similar: vec![],
                    rating: None,
                })
                .collect(),
            ..Default::default()
        })
    }

    /// The music videos on the artist's page, as songs (their video ids play like any song).
    async fn artist_videos(&self, artist: &Artist) -> Result<Vec<Song>> {
        let a = self.artist_data(&artist.id).await?;
        Ok(a.videos
            .iter()
            .map(|v| {
                self.song(
                    &VideoID::from_raw(v.video_id.clone()),
                    &v.title,
                    vec![ArtistRef { id: Some(artist.id.clone()), name: v.channel.clone() }],
                    None,
                    &v.length,
                    &v.thumbs,
                    None,
                    &Explicit::NotExplicit,
                )
            })
            .collect())
    }

    async fn artist_songs(&self, artist: &Artist) -> Result<Vec<Song>> {
        // The artist page's top songs: every album would cost a request each, and YouTube limits those.
        self.top_songs(artist, 100).await
    }

    async fn artist_albums(&self, id: &str) -> Result<Vec<Album>> {
        let a = self.artist_data(id).await?;
        let artists = vec![ArtistRef { id: Some(id.to_string()), name: a.name.clone() }];
        let mut out = Vec::new();
        for (group, kind) in [(&a.albums, "album"), (&a.singles, "single")] {
            for r in group {
                let mut album = self.album_stub(&r.id, &r.title, artists.clone(), &r.year, &r.thumbs);
                album.release_type = Some(kind.into());
                out.push(album);
            }
        }
        Ok(out)
    }

    async fn top_songs(&self, artist: &Artist, count: usize) -> Result<Vec<Song>> {
        let a = self.artist_data(&artist.id).await?;
        Ok(a.songs
            .iter()
            .take(count)
            .map(|s| {
                let album = s.album.as_ref().and_then(|al| {
                    Some(ParsedSongAlbum { name: al.name.clone(), id: AlbumID::from_raw(al.id.clone()?) })
                });
                let artists =
                    s.artists.iter().map(|a| ArtistRef { id: a.id.clone(), name: a.name.clone() }).collect();
                let like = s.liked.map(|l| if l { LikeStatus::Liked } else { LikeStatus::Indifferent });
                let explicit = if s.explicit { Explicit::IsExplicit } else { Explicit::NotExplicit };
                let mut song = self.song(
                    &VideoID::from_raw(s.video_id.clone()),
                    &s.title,
                    artists,
                    album.as_ref(),
                    "",
                    &[],
                    like.as_ref(),
                    &explicit,
                );
                song.image = artist.image.clone();
                song
            })
            .collect())
    }

    async fn songs(&self, q: &SongQuery) -> Result<Page<Song>> {
        // The liked playlist is fetched per page anyway, so a one-shot query is
        // cheaper than building and discarding a haystack per song.
        let items = match (q.favorite_filter(), self.guest) {
            (_, true) => query::songs_in(vec![], q),
            (Some(true), false) => {
                let liked = self.playlist_songs(LIKED_PLAYLIST).await?;
                query::songs_in(liked, q)
            }
            (_, false) => {
                let list = self.library_list().await?;
                query::songs_in_list(&list, q)
            }
        };
        Ok(Self::page(&items, q.start, q.limit))
    }

    async fn random_songs(&self, count: usize, _genre: Option<&str>) -> Result<Vec<Song>> {
        let mut songs = self.library_songs().await?;
        songs.shuffle(&mut rand::rng());
        songs.truncate(count);
        Ok(songs)
    }

    async fn similar_songs(&self, song: &Song, count: usize) -> Result<Vec<Song>> {
        match self.song_radio(song, count).await {
            Ok(songs) if !songs.is_empty() => return Ok(songs),
            Ok(_) => tracing::debug!("song radio for {} was empty; using the watch playlist", song.id),
            Err(e) => tracing::debug!("song radio for {}: {e}; using the watch playlist", song.id),
        }
        let radio =
            yt!(self, yt => yt.get_watch_playlist_from_video_id(VideoID::from_raw(song.id.clone())).await)?;
        Ok(radio
            .iter()
            .filter(|t| t.video_id.get_raw() != song.id)
            .take(count)
            .map(|t| {
                self.song(
                    &t.video_id,
                    &t.title,
                    name_ref(&t.author),
                    None,
                    &t.duration,
                    &t.thumbnails,
                    None,
                    &Explicit::NotExplicit,
                )
            })
            .collect())
    }

    async fn search(&self, text: &str, limit: usize) -> Result<SearchResults> {
        let (songs, albums, artists, playlists) = tokio::join!(
            async { yt!(self, yt => yt.search_songs(text).await) },
            async { yt!(self, yt => yt.search_albums(text).await) },
            async { yt!(self, yt => yt.search_artists(text).await) },
            async { yt!(self, yt => yt.search_playlists(text).await) },
        );
        let songs = songs?;
        Ok(SearchResults {
            songs: songs
                .iter()
                .take(limit)
                .map(|s| {
                    self.song(
                        &s.video_id,
                        &s.title,
                        name_ref(&s.artist),
                        s.album.as_ref(),
                        &s.duration,
                        &s.thumbnails,
                        None,
                        &s.explicit,
                    )
                })
                .collect(),
            albums: albums
                .unwrap_or_default()
                .iter()
                .take(limit)
                .map(|a| {
                    self.album_stub(
                        a.album_id.get_raw(),
                        &a.title,
                        name_ref(&a.artist),
                        &a.year,
                        &a.thumbnails,
                    )
                })
                .collect(),
            artists: artists
                .unwrap_or_default()
                .iter()
                .take(limit)
                .map(|a| Artist {
                    id: a.browse_id.get_raw().to_string(),
                    server_id: self.server_id.clone(),
                    source: SourceKind::YouTubeMusic,
                    name: a.artist.clone(),
                    image: thumb(&a.thumbnails),
                    album_count: None,
                    song_count: None,
                    favorite: false,
                    rating: None,
                    biography: None,
                    similar: vec![],
                })
                .collect(),
            playlists: playlists
                .unwrap_or_default()
                .iter()
                .take(limit)
                .filter_map(|p| {
                    use ytmapi_rs::parse::SearchResultPlaylist as P;
                    let (id, title, author, thumbs) = match p {
                        P::Featured(f) => (&f.playlist_id, &f.title, &f.author, &f.thumbnails),
                        P::Community(c) => (&c.playlist_id, &c.title, &c.author, &c.thumbnails),
                        _ => return None,
                    };
                    Some(self.playlist_stub(id.get_raw(), title, Some(author.clone()), thumbs, None))
                })
                .collect(),
        })
    }

    async fn playlists(&self) -> Result<Vec<Playlist>> {
        if self.guest {
            return Ok(vec![]);
        }
        let grid = self.browse_grid(LIBRARY_PLAYLISTS).await?;
        let mut out = vec![self.playlist_stub(LIKED_PLAYLIST, "Liked Music", None, &[], None)];
        out[0].editable = false;
        // Uploaded music shows as a list of its own, only if there is any.
        match self.browse_page(UPLOADS_PLAYLIST, None).await {
            Ok((songs, _)) if !songs.is_empty() => {
                let mut uploads = self.playlist_stub(UPLOADS_PLAYLIST, "Uploads", None, &[], None);
                uploads.editable = false;
                out.push(uploads);
            }
            Ok(_) => tracing::info!("uploaded music: the library page has 0 songs, no Uploads entry"),
            Err(e) => tracing::warn!("uploaded music: {e}"),
        }
        for item in grid {
            let home::Item::Playlist { id, title, subtitle, thumbs } = item else { continue };
            // The grid's ids are browse ids ("VLPL…"); Liked Music is already first.
            let id = id.strip_prefix("VL").unwrap_or(&id);
            if id == "LM" {
                continue;
            }
            let (author, count) = playlist_subtitle(&subtitle);
            out.push(self.playlist_stub(id, &title, author, &thumbs, count));
        }
        Ok(out)
    }

    async fn playlist_songs(&self, id: &str) -> Result<Vec<Song>> {
        let browse_id = playlist_browse_id(id);
        if let Some(songs) = self.cached_playlist(&browse_id).await {
            return Ok(songs);
        }
        let songs = self.browse_songs(&browse_id).await?;
        if self.cacheable(&browse_id) {
            self.playlist_cache
                .lock()
                .await
                .insert(browse_id, Cached { at: Instant::now(), items: songs.clone() });
        }
        Ok(songs)
    }

    async fn playlist_page(&self, id: &str, cursor: Option<&str>) -> Result<(Vec<Song>, Option<String>)> {
        let browse_id = playlist_browse_id(id);
        if cursor.is_none()
            && let Some(songs) = self.cached_playlist(&browse_id).await
        {
            return Ok((songs, None));
        }
        let continuation = cursor.and_then(decode_cursor);
        let first = continuation.is_none();
        let (songs, next) = self.browse_page(&browse_id, continuation).await?;
        let mut partial = self.playlist_partial.lock().await;
        let so_far = partial.entry(browse_id.clone()).or_default();
        if first {
            so_far.clear();
        }
        so_far.extend(songs.iter().cloned());
        if next.is_none() {
            // Complete: later opens (and Tracks/Favorites for Liked Music) use the cache.
            let all = partial.remove(&browse_id).unwrap_or_default();
            if self.cacheable(&browse_id) {
                self.playlist_cache.lock().await.insert(browse_id, Cached { at: Instant::now(), items: all });
            }
        }
        Ok((songs, next.as_ref().map(encode_cursor)))
    }

    async fn add_to_playlist(&self, id: &str, songs: &[String]) -> Result<()> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        let raw = id.strip_prefix("VL").unwrap_or(id);
        let actions: Vec<serde_json::Value> = songs
            .iter()
            .map(|v| serde_json::json!({"action": "ACTION_ADD_VIDEO", "addedVideoId": v}))
            .collect();
        self.forget_playlist(&playlist_browse_id(id)).await;
        let q = library::RawPost::new(
            "browse/edit_playlist",
            serde_json::json!({"playlistId": raw, "actions": actions}),
        );
        match library::edit_failure(&self.raw(&q).await?) {
            None => Ok(()),
            Some(why) => Err(SourceError::Other(why)),
        }
    }

    async fn rename_playlist(&self, id: &str, name: &str) -> Result<()> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        let title: String = name.chars().filter(|c| !matches!(c, '<' | '>')).collect();
        let raw = id.strip_prefix("VL").unwrap_or(id);
        let q = library::RawPost::new(
            "browse/edit_playlist",
            serde_json::json!({"playlistId": raw, "actions": [{"action": "ACTION_SET_PLAYLIST_NAME", "playlistName": title}]}),
        );
        match library::edit_failure(&self.raw(&q).await?) {
            None => Ok(()),
            Some(why) => Err(SourceError::Other(why)),
        }
    }

    async fn delete_playlist(&self, id: &str) -> Result<()> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        let raw = id.strip_prefix("VL").unwrap_or(id);
        self.forget_playlist(&playlist_browse_id(id)).await;
        let q = library::RawPost::new("playlist/delete", serde_json::json!({"playlistId": raw}));
        match library::delete_failure(&self.raw(&q).await?) {
            None => Ok(()),
            Some(why) => Err(SourceError::Other(why)),
        }
    }

    async fn create_playlist(&self, name: &str, songs: &[String]) -> Result<Playlist> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        self.create_playlist_full(
            &listen_source::NewPlaylist { name: name.into(), ..Default::default() },
            songs,
        )
        .await
    }

    async fn create_playlist_full(
        &self,
        new: &listen_source::NewPlaylist,
        songs: &[String],
    ) -> Result<Playlist> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        let q = library::RawPost::new(
            "playlist/create",
            library::create_body(&new.name, &new.comment, new.public, songs),
        );
        let json = self.raw(&q).await?;
        let id = json
            .get("playlistId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| SourceError::Other("YouTube Music did not create the playlist".into()))?;
        let mut created =
            self.playlist_stub(id, &library::clean_title(&new.name), None, &[], Some(songs.len() as u32));
        created.description = Some(new.comment.clone()).filter(|c| !c.trim().is_empty());
        created.public = new.public;
        Ok(created)
    }

    async fn remove_from_playlist(&self, id: &str, indices: &[usize]) -> Result<()> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        let Some(need) = indices.iter().max().map(|m| m + 1) else { return Ok(()) };
        let entries = self.entries_of(id, need).await?;
        let mut chosen: Vec<(&str, &str)> = Vec::new();
        for &i in indices {
            let e = &entries[i];
            let set = e
                .set_video_id
                .as_deref()
                .ok_or_else(|| SourceError::Other("YouTube Music did not say which entry this is".into()))?;
            chosen.push((e.video_id.as_str(), set));
        }
        let browse_id = playlist_browse_id(id);
        self.forget_cache(&browse_id).await;
        self.edit_playlist(id, library::remove_actions(&chosen)).await?;
        // The entries that are left keep their ids.
        let mut all = self.playlist_items.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(list) = all.get_mut(&browse_id) {
            let mut drop: Vec<usize> = indices.to_vec();
            drop.sort_unstable_by(|a, b| b.cmp(a));
            drop.dedup();
            for i in drop {
                if i < list.len() {
                    list.remove(i);
                }
            }
        }
        Ok(())
    }

    async fn move_in_playlist(&self, id: &str, from: usize, to: usize) -> Result<()> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        if from == to {
            return Ok(());
        }
        let entries = self.entries_of(id, from.max(to) + 1).await?;
        let set = |i: usize| entries[i].set_video_id.clone();
        let moved = set(from)
            .ok_or_else(|| SourceError::Other("YouTube Music did not say which entry this is".into()))?;
        // The entry goes before the one now at `to` (moving up), or before the one after it (down).
        let before =
            if from > to { set(to) } else { entries.get(to + 1).and_then(|e| e.set_video_id.clone()) };
        let browse_id = playlist_browse_id(id);
        self.forget_cache(&browse_id).await;
        self.edit_playlist(id, vec![library::move_action(&moved, before.as_deref())]).await?;
        let mut all = self.playlist_items.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(list) = all.get_mut(&browse_id).filter(|l| from < l.len() && to < l.len()) {
            let entry = list.remove(from);
            list.insert(to, entry);
        }
        Ok(())
    }

    async fn update_playlist(&self, id: &str, update: &listen_source::PlaylistUpdate) -> Result<()> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        let actions =
            library::metadata_actions(update.name.as_deref(), update.comment.as_deref(), update.public);
        if actions.is_empty() {
            return Ok(());
        }
        self.forget_cache(&playlist_browse_id(id)).await;
        self.edit_playlist(id, actions).await
    }

    async fn playlists_for_adding(&self, songs: &[String]) -> Result<Vec<Playlist>> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        // What YouTube Music's own "Save to playlist" offers: the playlists you can edit.
        // YouTube sometimes answers with an empty or different page, so an empty
        // answer is asked again before falling back to the library.
        let ids: Vec<&String> = songs.iter().take(1).collect();
        for attempt in 0..if ids.is_empty() { 0 } else { 2 } {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
            let q = library::RawPost::new(
                "playlist/get_add_to_playlist",
                serde_json::json!({"videoIds": ids, "excludeWatchLater": true}),
            );
            match self.raw(&q).await.map(|json| library::add_options(&json)) {
                Ok(options) if !options.is_empty() => {
                    return Ok(options
                        .into_iter()
                        .map(|o| {
                            let mut p = self.playlist_stub(&o.id, &o.title, None, &[], None);
                            // Already holds the song: shown, but not offered again.
                            p.editable = !o.contains;
                            p
                        })
                        .collect());
                }
                Ok(_) => tracing::warn!("save-to-playlist list was empty (attempt {})", attempt + 1),
                Err(e) => tracing::warn!("save-to-playlist list: {e} (attempt {})", attempt + 1),
            }
        }
        // The library page can also come back in a layout ytmapi-rs reads as
        // "no playlists" (only Liked Music, which isn't offered): ask again.
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(400 * attempt)).await;
            }
            let list: Vec<_> = self.playlists().await?.into_iter().filter(|p| p.editable).collect();
            if !list.is_empty() {
                return Ok(list);
            }
            tracing::warn!("library playlists came back empty (attempt {})", attempt + 1);
        }
        // Never a silent "no playlists": a new one can still be created by name.
        Err(SourceError::Other("YouTube Music sent no playlists. Try again.".into()))
    }

    fn share_url(&self, song: &Song) -> Option<String> {
        Some(format!("https://music.youtube.com/watch?v={}", song.id))
    }

    /// YouTube Music has no file facts: the modal shows the model's fields
    /// and, from here, the link to the item.
    async fn item_details(&self, kind: ItemKind, id: &str) -> Result<listen_source::ItemDetails> {
        let link = match kind {
            ItemKind::Song => format!("https://music.youtube.com/watch?v={id}"),
            ItemKind::Album => format!("https://music.youtube.com/browse/{id}"),
            ItemKind::Artist => format!("https://music.youtube.com/channel/{id}"),
        };
        Ok(listen_source::ItemDetails {
            tags: vec![("YouTube Music link".into(), link)],
            ..Default::default()
        })
    }

    async fn set_favorite(&self, song_ids: &[String], favorite: bool) -> Result<()> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        let status = if favorite { LikeStatus::Liked } else { LikeStatus::Indifferent };
        // Liked Music and the Tracks list change.
        self.forget_playlist(LIKED_PLAYLIST).await;
        *self.songs.lock().await = None;
        for id in song_ids {
            yt!(self, yt => yt.rate_song(VideoID::from_raw(id.clone()), status.clone()).await)?;
        }
        Ok(())
    }

    /// Songs are liked; albums are saved to the library (their audio playlist
    /// is liked); artists are subscribed to.
    async fn set_favorite_items(&self, kind: ItemKind, ids: &[String], favorite: bool) -> Result<()> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        match kind {
            ItemKind::Song => self.set_favorite(ids, favorite).await,
            ItemKind::Album => {
                for id in ids {
                    let album = yt_retry!(self, yt => yt.get_album(AlbumID::from_raw(id.clone())).await)?;
                    let playlist = album
                        .audio_playlist_id
                        .as_ref()
                        .ok_or_else(|| SourceError::Other("this album has no library playlist".into()))?;
                    let query = favorites::album_library(playlist.get_raw(), favorite);
                    yt!(self, yt => yt.query::<ytmapi_rs::query::RatePlaylistQuery>(query).await)?;
                }
                *self.albums.lock().await = None;
                Ok(())
            }
            ItemKind::Artist => {
                if favorite {
                    for id in ids {
                        let query = favorites::subscribe(id);
                        yt!(self, yt => yt.query::<ytmapi_rs::query::SubscribeArtistQuery>(query).await)?;
                    }
                } else {
                    let query = favorites::unsubscribe(ids);
                    yt!(self, yt => yt.query::<ytmapi_rs::query::UnsubscribeArtistsQuery>(query).await)?;
                }
                Ok(())
            }
        }
    }

    /// The albums saved to the library.
    async fn favorite_albums(&self) -> Result<Vec<Album>> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        self.library_albums().await
    }

    /// The artists subscribed to.
    async fn favorite_artists(&self) -> Result<Vec<Artist>> {
        if self.guest {
            return Err(SourceError::Unsupported);
        }
        Ok(self.artists(&ListQuery { search: None, start: 0, limit: usize::MAX }).await?.items)
    }

    async fn scrobble(&self, song: &Song, event: ScrobbleEvent) -> Result<()> {
        // Report plays so YouTube Music history and recommendations stay current.
        if event == ScrobbleEvent::NowPlaying && !self.guest {
            let url = yt!(self, yt => yt.get_song_tracking_url(VideoID::from_raw(song.id.clone())).await)?;
            yt!(self, yt => yt.add_history_item(url.clone()).await)?;
        }
        Ok(())
    }

    fn forget_stream(&self, song: &Song) {
        self.ytdlp.forget(&song.id);
        // If it was the local copy that failed, the next try streams.
        audio::cache().remove(&song.id);
    }

    fn alternate_url(&self, url: &str) -> Option<String> {
        ytdlp::fallback_url(url)
    }

    async fn stream(&self, song: &Song) -> Result<StreamSource> {
        // The local copy is the exact audio that was analysed, seeks are
        // instant and no edge node can refuse it.
        if let Some(path) = audio::cached(&song.id) {
            return Ok(StreamSource::direct(path.to_string_lossy()));
        }
        let quality = StreamQuality::from_u8(self.quality.load(std::sync::atomic::Ordering::Relaxed));
        let r = self.ytdlp.resolve_quality(&song.id, quality).await.map_err(SourceError::Other)?;
        Ok(StreamSource { url: r.url, headers: Default::default(), expires_at: r.expires_at })
    }

    fn set_stream_quality(&self, quality: StreamQuality) {
        self.quality.store(quality.to_u8(), std::sync::atomic::Ordering::Relaxed);
    }

    async fn local_copy(&self, song: &Song) -> Result<Option<std::path::PathBuf>> {
        // The app downloads one song at a time with a pause in between (no bursts).
        let entry = self.fetch_audio(&song.id, Urgency::Now).await.map_err(SourceError::Network)?;
        Ok(entry.map(|e| e.path))
    }

    async fn video_stream(&self, song: &Song) -> Result<Option<StreamSource>> {
        if self.guest {
            return Ok(None);
        }
        // A song's own video id is only the cover art; its music video is the counterpart.
        let q = library::RawPost::new(
            "next",
            serde_json::json!({"videoId": song.id, "isAudioOnly": true, "enablePersistentPlaylistPanel": true}),
        );
        let json = self.raw(&q).await?;
        tracing::info!("video: {} is {:?}", song.id, library::video_type(&json));
        let Some(id) = library::music_video_id(&json, &song.id) else {
            return Ok(None);
        };
        tracing::info!("video: playing the music video {id}");
        let id = id.as_str();
        let url = self.ytdlp.resolve_video(id).await.map_err(SourceError::Other)?;
        Ok(Some(StreamSource { expires_at: ytdlp::expiry(&url), url, headers: Default::default() }))
    }

    fn analysable(&self) -> bool {
        true
    }

    fn pin_mix(&self, song: &Song, pinned: bool) {
        audio::cache().pin_count(&song.id, pinned);
    }

    /// Only a copy that is already in the audio cache (fetched for analysis, a
    /// DJ mix or offline): a waveform never causes a request.
    fn waveform_input(&self, song: &Song) -> Option<AudioInput> {
        audio::cached(&song.id).map(AudioInput::File)
    }

    async fn analysis_source(&self, song: &Song, urgency: Urgency) -> Result<Option<AnalysisSource>> {
        let entry = self.fetch_audio(&song.id, urgency).await.map_err(SourceError::Network)?;
        Ok(entry.map(|e| AnalysisSource { input: AudioInput::File(e.path), file: e.file }))
    }

    async fn mix_input(&self, song: &Song) -> Result<Option<AudioInput>> {
        match self.fetch_audio(&song.id, Urgency::Now).await {
            Ok(entry) => Ok(entry.map(|e| AudioInput::File(e.path))),
            Err(e) => {
                tracing::warn!("no local audio to mix “{}” [{}]: {e}", song.title, song.id);
                Ok(None)
            }
        }
    }

    fn image_url(&self, image: &ImageRef, size: u32) -> Option<String> {
        match image {
            ImageRef::Url(url) => Some(resize_thumbnail(url, size)),
            ImageRef::Id(_) => None,
        }
    }

    async fn lyrics(&self, song: &Song) -> Result<Option<Lyrics>> {
        let Ok(id) = yt!(self, yt => yt.get_lyrics_id(VideoID::from_raw(song.id.clone())).await) else {
            return Ok(None);
        };
        let Ok(l) = yt!(self, yt => yt.get_lyrics(id.clone()).await) else { return Ok(None) };
        Ok(Some(Lyrics {
            provider: l.source.clone(),
            synced: false,
            lines: l.lyrics.lines().map(|t| LyricLine::new(None, t)).collect(),
            matched: None,
        }))
    }
}

impl YouTubeMusic {
    fn item_song(&self, item: home::Item) -> Option<Song> {
        let home::Item::Song { video_id, title, artists, album, thumbs, duration, .. } = item else {
            return None;
        };
        let refs: Vec<ArtistRef> =
            artists.iter().map(|n| ArtistRef { id: n.id.clone(), name: n.name.clone() }).collect();
        let album = album.and_then(|a| Some(ParsedSongAlbum { name: a.name, id: AlbumID::from_raw(a.id?) }));
        Some(self.song(
            &VideoID::from_raw(video_id),
            &title,
            refs,
            album.as_ref(),
            &duration,
            &thumbs,
            None,
            &Explicit::NotExplicit,
        ))
    }

    async fn home_pages(&self) -> Result<Vec<HomeShelf>> {
        let mut shelves = Vec::new();
        let mut continuation = None;
        // The first response has ~6 shelves; two more pages cover the usual feed.
        for _ in 0..3 {
            let q = home::HomeQuery { continuation: continuation.take() };
            let raw = self.raw_json(&q).await?;
            let json: serde_json::Value =
                serde_json::from_str(&raw).map_err(|e| SourceError::Other(format!("home feed: {e}")))?;
            let (page, next) = home::parse(&json);
            shelves.extend(page.into_iter().map(|s| self.shelf(s)));
            match next {
                Some(n) => continuation = Some(n),
                None => break,
            }
        }
        Ok(shelves)
    }

    /// Raw InnerTube JSON. YouTube answers *signed out* once it has rotated the session cookie in
    /// the browser: the cookies are then read again and the request repeated, because the page
    /// itself parses into an empty library rather than an error.
    async fn raw_json<Q>(&self, q: &Q) -> Result<String>
    where
        Q: ytmapi_rs::query::Query<token::CookieToken>,
    {
        let mut retries = 0;
        let mut refreshed = false;
        loop {
            match self.yt.raw_json_query::<Q>(q).await.map_err(source_err) {
                Ok(text) if !token::signed_out_page(&text) => {
                    if let Some(session) = &self.session {
                        session.clear_stale();
                    }
                    return Ok(text);
                }
                Ok(_) => {
                    let Some(session) = &self.session else {
                        return Err(SourceError::Auth("YouTube Music is signed out".into()));
                    };
                    if refreshed {
                        return Err(SourceError::Auth(
                            "YouTube Music is signed out, even after reading the browser cookies again"
                                .into(),
                        ));
                    }
                    tracing::warn!("YouTube Music answered signed out — reading the browser cookies again");
                    session.mark_stale();
                    refreshed = true;
                }
                Err(SourceError::Auth(e)) => return Err(SourceError::Auth(e)),
                Err(e) if retries < 2 => {
                    retries += 1;
                    tracing::debug!("retrying after: {e}");
                    tokio::time::sleep(Duration::from_millis(400 * retries)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Sends a raw InnerTube request and parses the answer.
    async fn raw(&self, q: &library::RawPost) -> Result<serde_json::Value> {
        let text = self.raw_json(q).await?;
        serde_json::from_str(&text).map_err(|e| SourceError::Other(format!("{}: {e}", q.path)))
    }

    /// The raw browse response for `browse_id` (diagnostics).
    pub async fn raw_browse(&self, browse_id: &str) -> Result<serde_json::Value> {
        let q = home::BrowseQuery { browse_id: Some(browse_id.to_string()), continuation: None };
        let raw = self.raw_json(&q).await?;
        serde_json::from_str(&raw).map_err(|e| SourceError::Other(e.to_string()))
    }

    /// All items of a library grid (Library > Albums, > Playlists), page by page.
    /// Read with Listen's own parser: ytmapi-rs fails when the grid isn't the
    /// section list's first entry (an empty library, a wrapped grid).
    async fn browse_grid(&self, browse_id: &str) -> Result<Vec<home::Item>> {
        let mut items = Vec::new();
        let mut cursor = None;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            let q = home::BrowseQuery { browse_id: Some(browse_id.to_string()), continuation: cursor.take() };
            let raw = self.raw_json(&q).await?;
            let json: serde_json::Value =
                serde_json::from_str(&raw).map_err(|e| SourceError::Other(format!("{browse_id}: {e}")))?;
            let (page, next) = home::grid_page(&json);
            if items.is_empty()
                && page.is_empty()
                && cursor.is_none()
                && next.is_none()
                && !empty_message(&json)
            {
                tracing::info!(
                    "{browse_id}: no albums or playlists found; response outline:\n{}",
                    home::outline(&json, 80)
                );
            }
            items.extend(page);
            match next {
                Some(c) if seen.insert(c.token().to_string()) => cursor = Some(c),
                _ => break,
            }
        }
        Ok(items)
    }

    /// All songs of a song list (library songs, a playlist), page by page.
    /// Unreadable entries are skipped rather than failing the list.
    async fn browse_songs(&self, browse_id: &str) -> Result<Vec<Song>> {
        let mut songs = Vec::new();
        let mut cursor = None;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            let (page, next) = self.browse_page(browse_id, cursor.take()).await?;
            songs.extend(page);
            match next {
                Some(c) if seen.insert(c.token().to_string()) => cursor = Some(c),
                _ => break,
            }
        }
        Ok(songs)
    }

    /// One page of a song list.
    async fn browse_page(
        &self,
        browse_id: &str,
        continuation: Option<home::Continuation>,
    ) -> Result<(Vec<Song>, Option<home::Continuation>)> {
        let first = continuation.is_none();
        let q = home::BrowseQuery { browse_id: Some(browse_id.to_string()), continuation };
        let raw = self.raw_json(&q).await?;
        let json: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| SourceError::Other(format!("{browse_id}: {e}")))?;
        let (page, next) = home::song_page(&json);
        self.note_entries(browse_id, first, &page);
        if page.is_empty() && browse_id == UPLOADS_PLAYLIST {
            let keys: Vec<&str> =
                json.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
            tracing::info!(
                "uploaded music: parser found no songs ({} B response, top-level keys {keys:?})",
                raw.len()
            );
        }
        Ok((page.into_iter().filter_map(|i| self.item_song(i)).collect(), next))
    }

    /// Whether a playlist read now may be cached (not right after an edit).
    fn cacheable(&self, browse_id: &str) -> bool {
        self.edited
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(browse_id)
            .is_none_or(|at| at.elapsed() > EDIT_SETTLE)
    }

    async fn cached_playlist(&self, browse_id: &str) -> Option<Vec<Song>> {
        let cache = self.playlist_cache.lock().await;
        cache.get(browse_id).filter(|c| c.at.elapsed() < LIBRARY_TTL).map(|c| c.items.clone())
    }

    async fn forget_playlist(&self, browse_id: &str) {
        self.forget_cache(browse_id).await;
        self.playlist_items.lock().unwrap_or_else(|e| e.into_inner()).remove(browse_id);
    }

    /// The playlist was edited: its next read is not cached (the entries
    /// kept for editing are not touched).
    async fn forget_cache(&self, browse_id: &str) {
        self.edited.lock().unwrap_or_else(|e| e.into_inner()).insert(browse_id.to_string(), Instant::now());
        self.playlist_cache.lock().await.remove(browse_id);
    }

    /// Keeps the entry ids of a page of a playlist (the first page starts the list again).
    fn note_entries(&self, browse_id: &str, first: bool, page: &[home::Item]) {
        let entries = page.iter().filter_map(|i| match i {
            home::Item::Song { video_id, set_video_id, .. } => {
                Some(PlaylistEntry { video_id: video_id.clone(), set_video_id: set_video_id.clone() })
            }
            _ => None,
        });
        let mut all = self.playlist_items.lock().unwrap_or_else(|e| e.into_inner());
        let list = all.entry(browse_id.to_string()).or_default();
        if first {
            list.clear();
        }
        list.extend(entries);
    }

    /// The entries of a playlist, at least `need` of them: those read earlier, else the
    /// playlist is read again.
    async fn entries_of(&self, id: &str, need: usize) -> Result<Vec<PlaylistEntry>> {
        let browse_id = playlist_browse_id(id);
        let known = self.playlist_items.lock().unwrap_or_else(|e| e.into_inner()).get(&browse_id).cloned();
        match known {
            Some(list) if list.len() >= need => Ok(list),
            _ => {
                self.browse_songs(&browse_id).await?;
                let list = self
                    .playlist_items
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&browse_id)
                    .cloned()
                    .unwrap_or_default();
                if list.len() < need {
                    return Err(SourceError::Other("the playlist changed; open it again".into()));
                }
                Ok(list)
            }
        }
    }

    /// Sends `actions` to `browse/edit_playlist`.
    async fn edit_playlist(&self, id: &str, actions: Vec<serde_json::Value>) -> Result<()> {
        let q = library::RawPost::new("browse/edit_playlist", library::edit_body(id, actions));
        match library::edit_failure(&self.raw(&q).await?) {
            None => Ok(()),
            Some(why) => Err(SourceError::Other(why)),
        }
    }

    fn shelf(&self, shelf: home::Shelf) -> HomeShelf {
        let refs = |list: &[home::Named]| -> Vec<ArtistRef> {
            list.iter().map(|n| ArtistRef { id: n.id.clone(), name: n.name.clone() }).collect()
        };
        let mut out = HomeShelf { title: shelf.title, albums: vec![], playlists: vec![], songs: vec![] };
        for item in shelf.items {
            match item {
                home::Item::Album { id, title, artists, year, thumbs } => {
                    out.albums.push(self.album_stub(&id, &title, refs(&artists), &year, &thumbs));
                }
                home::Item::Playlist { id, title, subtitle, thumbs } => {
                    let mut p = self.playlist_stub(&id, &title, Some(subtitle), &thumbs, None);
                    p.editable = false;
                    out.playlists.push(p);
                }
                song @ home::Item::Song { .. } => out.songs.extend(self.item_song(song)),
            }
        }
        out
    }

    fn playlist_stub(
        &self,
        id: &str,
        name: &str,
        owner: Option<String>,
        thumbs: &[Thumbnail],
        count: Option<u32>,
    ) -> Playlist {
        Playlist {
            id: id.to_string(),
            server_id: self.server_id.clone(),
            source: SourceKind::YouTubeMusic,
            name: name.to_string(),
            description: None,
            owner,
            image: thumb(thumbs),
            song_count: count,
            duration: Duration::ZERO,
            public: false,
            editable: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_playlist_tile_subtitle() {
        assert_eq!(playlist_subtitle("Alex • 1,234 tracks"), (Some("Alex".into()), Some(1234)));
        assert_eq!(playlist_subtitle("Playlist • YouTube Music"), (Some("YouTube Music".into()), None));
        assert_eq!(playlist_subtitle("12 songs"), (None, Some(12)));
        assert_eq!(playlist_subtitle("Playlist • Alex • 42 tracks"), (Some("Alex".into()), Some(42)));
        assert_eq!(playlist_subtitle(""), (None, None));
    }

    #[tokio::test]
    async fn refresh_forgets_the_library_caches() {
        // A guest session needs no network to be built.
        let yt =
            YouTubeMusic::connect(ServerId("yt".into()), "").await.expect("a guest session builds offline");
        *yt.songs.lock().await = Some(CachedSongs {
            at: Instant::now(),
            list: std::sync::Arc::new(clientside::SongList::new(vec![])),
        });
        *yt.albums.lock().await = Some(Cached { at: Instant::now(), items: vec![] });
        yt.playlist_cache.lock().await.insert("PL1".into(), Cached { at: Instant::now(), items: vec![] });
        yt.playlist_partial.lock().await.insert("PL2".into(), vec![]);
        yt.refresh().await;
        assert!(yt.songs.lock().await.is_none() && yt.albums.lock().await.is_none());
        assert!(yt.playlist_cache.lock().await.is_empty() && yt.playlist_partial.lock().await.is_empty());
    }

    #[test]
    fn signed_in_account_keeps_every_playlist_and_like_capability() {
        let c = capabilities(false);
        for flag in [
            Capabilities::PLAYLIST_ADD,
            Capabilities::PLAYLIST_CREATE,
            Capabilities::PLAYLIST_DELETE,
            Capabilities::PLAYLIST_RENAME,
            Capabilities::SCROBBLE,
            Capabilities::RADIO,
            Capabilities::FAVORITE_ITEMS,
        ] {
            assert!(c.contains(flag), "{flag:?}");
        }
        for flag in
            [Capabilities::PLAYLIST_REMOVE, Capabilities::PLAYLIST_REORDER, Capabilities::PLAYLIST_UPDATE]
        {
            assert!(c.contains(flag), "{flag:?}");
        }
        // Offline downloads are not offered for YouTube Music.
        assert!(!c.contains(Capabilities::DOWNLOAD));
        assert!(!capabilities(true).intersects(
            Capabilities::PLAYLIST_EDIT
                | Capabilities::PLAYLIST_RENAME
                | Capabilities::PLAYLIST_REMOVE
                | Capabilities::PLAYLIST_REORDER
                | Capabilities::PLAYLIST_UPDATE
                | Capabilities::FAVORITE_ITEMS
        ));
    }

    #[test]
    fn playlist_track_counts() {
        assert_eq!(track_count("1,234 songs"), Some(1234));
        assert_eq!(track_count("1.234 Titel"), Some(1234));
        assert_eq!(track_count("12 tracks"), Some(12));
        assert_eq!(track_count(""), None);
        assert_eq!(track_count("Auto playlist"), None);
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("3:45"), Duration::from_secs(225));
        assert_eq!(parse_duration("1:02:03"), Duration::from_secs(3723));
        assert_eq!(parse_duration(""), Duration::ZERO);
        assert_eq!(parse_duration("n/a"), Duration::ZERO);
    }

    #[test]
    fn thumbnail_resizing() {
        let u = "https://lh3.googleusercontent.com/abc=w120-h120-l90-rj";
        assert_eq!(resize_thumbnail(u, 300), "https://lh3.googleusercontent.com/abc=w300-h300-l90-rj");
        let v = "https://i.ytimg.com/vi/x/hqdefault.jpg";
        assert_eq!(resize_thumbnail(v, 300), v);
    }
}

#[cfg(test)]
mod artist_fallback_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn data() -> artist::ArtistData {
        artist::ArtistData { name: "x".into(), ..Default::default() }
    }

    #[tokio::test]
    async fn failing_primary_parse_falls_back_exactly_once() {
        let (p, f) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let got = load_artist(
            || async {
                p.fetch_add(1, Ordering::SeqCst);
                Err(SourceError::Other("Key /header/musicImmersiveHeaderRenderer not found".into()))
            },
            || async {
                f.fetch_add(1, Ordering::SeqCst);
                Ok(data())
            },
        )
        .await
        .unwrap();
        assert_eq!(got.name, "x");
        assert_eq!((p.load(Ordering::SeqCst), f.load(Ordering::SeqCst)), (1, 1));
    }

    #[tokio::test]
    async fn working_primary_skips_the_fallback_and_auth_errors_stay() {
        let f = AtomicUsize::new(0);
        load_artist(
            || async { Ok(data()) },
            || async {
                f.fetch_add(1, Ordering::SeqCst);
                Ok(data())
            },
        )
        .await
        .unwrap();
        let err = load_artist(
            || async { Err(SourceError::Auth("no".into())) },
            || async {
                f.fetch_add(1, Ordering::SeqCst);
                Ok(data())
            },
        )
        .await;
        assert!(matches!(err, Err(SourceError::Auth(_))));
        assert_eq!(f.load(Ordering::SeqCst), 0);
    }
}
