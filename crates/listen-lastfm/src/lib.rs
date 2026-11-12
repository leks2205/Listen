//! Last.fm for Listen (desktop and phone use the same code):
//!
//! - [`Client`]: the web API (desktop sign-in flow: token → browser → session key; now playing; scrobbles;
//!   album covers, [`Client::album_image`]). Every Last.fm call of Listen goes through it (shared
//!   User-Agent, signatures, error mapping): add a method here instead of calling the API from the app.
//! - [`Resolver`]: the most common spelling of a song's artist, title and album on Last.fm, so the same song
//!   from YouTube Music and from a music server lands on one track, not two (cached on disk).
//! - [`Scrobbler`]: scrobbles go into an on-disk queue first and are sent in batches, so plays made offline
//!   or while Last.fm is down arrive later (Last.fm takes plays up to 14 days old).
//!
//! Signatures follow <https://www.last.fm/api/authspec>: md5 over the sorted parameters and the shared secret.

pub mod names;

use listen_core::model::Song;
use listen_core::text::fold;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const API: &str = "https://ws.audioscrobbler.com/2.0/";
/// Where the user allows access (desktop flow).
const AUTH: &str = "https://www.last.fm/api/auth/";
/// Last.fm drops scrobbles older than this.
const MAX_AGE_S: i64 = 14 * 24 * 3600;
/// Names looked up are kept this long before they are asked again.
const CACHE_DAYS: i64 = 30;
const CACHE_MAX: usize = 5000;

/// Last.fm's API error codes (<https://www.last.fm/api/errorcodes>) that change what Listen does.
mod api_code {
    /// The artist, track or album doesn't exist (a lookup answers `None`, not an error).
    pub const NOT_FOUND: i64 = 6;
    pub const OPERATION_FAILED: i64 = 8;
    /// The session key is no longer valid.
    pub const INVALID_SESSION: i64 = 9;
    pub const SERVICE_OFFLINE: i64 = 11;
    /// The sign-in token has not been authorised (yet).
    pub const TOKEN_NOT_AUTHORISED: i64 = 14;
    pub const TEMPORARILY_UNAVAILABLE: i64 = 16;
    pub const RATE_LIMIT: i64 = 29;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Last.fm error {code}: {message}")]
    Api { code: i64, message: String },
    #[error("network: {0}")]
    Http(String),
    #[error("{0}")]
    Bad(String),
}

impl Error {
    /// Worth trying again later (network, Last.fm busy or down, rate limit).
    pub fn transient(&self) -> bool {
        match self {
            Error::Http(_) => true,
            Error::Api { code, .. } => matches!(
                *code,
                api_code::OPERATION_FAILED
                    | api_code::SERVICE_OFFLINE
                    | api_code::TEMPORARILY_UNAVAILABLE
                    | api_code::RATE_LIMIT
            ),
            Error::Bad(_) => false,
        }
    }

    /// The session key is no longer valid: sign in again.
    pub fn signed_out(&self) -> bool {
        matches!(self, Error::Api { code: api_code::INVALID_SESSION, .. })
    }

    /// Sign-in: the user has not allowed access (yet).
    pub fn not_authorized(&self) -> bool {
        matches!(self, Error::Api { code: api_code::TOKEN_NOT_AUTHORISED, .. })
    }
}

/// An API account (key + shared secret, from <https://www.last.fm/api/account/create>).
#[derive(Debug, Clone)]
pub struct Creds {
    pub api_key: String,
    pub secret: String,
}

/// A signed-in user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub key: String,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    creds: Creds,
}

fn now_s() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Client {
    pub fn new(creds: Creds) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(listen_core::USER_AGENT)
            .build()
            .unwrap_or_default();
        Client { http, creds }
    }

    fn signature(&self, params: &[(String, String)]) -> String {
        let mut sorted: Vec<&(String, String)> = params.iter().filter(|(k, _)| k != "format").collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let mut h = Md5::new();
        for (k, v) in sorted {
            h.update(k.as_bytes());
            h.update(v.as_bytes());
        }
        h.update(self.creds.secret.as_bytes());
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    async fn call(
        &self,
        method: &str,
        mut params: Vec<(String, String)>,
        signed: bool,
        post: bool,
    ) -> Result<Value, Error> {
        params.push(("method".into(), method.into()));
        params.push(("api_key".into(), self.creds.api_key.clone()));
        if signed {
            let sig = self.signature(&params);
            params.push(("api_sig".into(), sig));
        }
        params.push(("format".into(), "json".into()));
        let req = if post { self.http.post(API).form(&params) } else { self.http.get(API).query(&params) };
        let resp = req.send().await.map_err(network)?;
        let status = resp.status();
        let body: Value =
            resp.json().await.map_err(|e| Error::Http(format!("{status}: {}", e.without_url())))?;
        if let Some(code) = body.get("error").and_then(Value::as_i64) {
            let message = body.get("message").and_then(Value::as_str).unwrap_or("").to_string();
            return Err(Error::Api { code, message });
        }
        if !status.is_success() {
            return Err(Error::Http(status.to_string()));
        }
        Ok(body)
    }

    /// Step 1 of the sign-in: a request token.
    pub async fn get_token(&self) -> Result<String, Error> {
        let v = self.call("auth.getToken", vec![], true, false).await?;
        v.get("token").and_then(Value::as_str).map(String::from).ok_or_else(|| Error::Bad("no token".into()))
    }

    /// Step 2: the page where the user allows access for `token`.
    pub fn auth_url(&self, token: &str) -> String {
        format!("{AUTH}?api_key={}&token={token}", self.creds.api_key)
    }

    /// Step 3: the session, once the user allowed access ([`Error::not_authorized`] until then).
    pub async fn get_session(&self, token: &str) -> Result<Session, Error> {
        let v = self.call("auth.getSession", vec![("token".into(), token.into())], true, false).await?;
        let s = v.get("session").ok_or_else(|| Error::Bad("no session".into()))?;
        let text = |k: &str| s.get(k).and_then(Value::as_str).map(String::from);
        Ok(Session {
            name: text("name").unwrap_or_default(),
            key: text("key").ok_or_else(|| Error::Bad("no session key".into()))?,
        })
    }

    pub async fn now_playing(&self, sk: &str, t: &Track) -> Result<(), Error> {
        let mut p = vec![
            ("sk".into(), sk.into()),
            ("artist".into(), t.artist.clone()),
            ("track".into(), t.title.clone()),
        ];
        if let Some(a) = &t.album {
            p.push(("album".into(), a.clone()));
        }
        if let Some(d) = t.duration_s.filter(|d| *d > 0) {
            p.push(("duration".into(), d.to_string()));
        }
        self.call("track.updateNowPlaying", p, true, true).await.map(|_| ())
    }

    /// Up to 50 plays in one request.
    pub async fn scrobble(&self, sk: &str, plays: &[(Track, i64)]) -> Result<(), Error> {
        let mut p: Vec<(String, String)> = vec![("sk".into(), sk.into())];
        for (i, (t, ts)) in plays.iter().take(50).enumerate() {
            p.push((format!("artist[{i}]"), t.artist.clone()));
            p.push((format!("track[{i}]"), t.title.clone()));
            p.push((format!("timestamp[{i}]"), ts.to_string()));
            if let Some(a) = &t.album {
                p.push((format!("album[{i}]"), a.clone()));
            }
            if let Some(d) = t.duration_s.filter(|d| *d > 0) {
                p.push((format!("duration[{i}]"), d.to_string()));
            }
        }
        self.call("track.scrobble", p, true, true).await.map(|_| ())
    }

    /// An artist as Last.fm knows it (autocorrected).
    async fn artist(&self, name: &str) -> Result<Option<ArtistInfo>, Error> {
        let p = vec![("artist".into(), name.into()), ("autocorrect".into(), "1".into())];
        match self.call("artist.getInfo", p, false, false).await {
            Ok(v) => {
                let a = &v["artist"];
                Ok(a["name"].as_str().map(|n| ArtistInfo {
                    name: n.to_string(),
                    listeners: num(&a["stats"]["listeners"]),
                    mbid: a["mbid"].as_str().unwrap_or("").to_string(),
                    bio: a["bio"]["summary"].as_str().unwrap_or("").to_string(),
                }))
            }
            Err(Error::Api { code: api_code::NOT_FOUND, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Tracks named like `title`, any artist, most listened first: (artist, title, listeners).
    async fn search_track(&self, title: &str) -> Result<Vec<(String, String, u64)>, Error> {
        let p = vec![("track".into(), title.into()), ("limit".into(), "10".into())];
        let v = self.call("track.search", p, false, false).await?;
        let list = v["results"]["trackmatches"]["track"].as_array().cloned().unwrap_or_default();
        Ok(list
            .iter()
            .filter_map(|t| {
                Some((
                    t["artist"].as_str()?.to_string(),
                    t["name"].as_str()?.to_string(),
                    num(&t["listeners"]),
                ))
            })
            .collect())
    }

    /// (corrected artist, corrected title, album title, listeners) of a track.
    async fn track(
        &self,
        artist: &str,
        title: &str,
    ) -> Result<Option<(String, String, Option<String>, u64)>, Error> {
        let p = vec![
            ("artist".into(), artist.into()),
            ("track".into(), title.into()),
            ("autocorrect".into(), "1".into()),
        ];
        match self.call("track.getInfo", p, false, false).await {
            Ok(v) => {
                let t = &v["track"];
                let Some(name) = t["name"].as_str() else { return Ok(None) };
                let artist = t["artist"]["name"].as_str().unwrap_or(artist).to_string();
                let album = t["album"]["title"].as_str().filter(|s| !s.is_empty()).map(String::from);
                Ok(Some((artist, name.to_string(), album, num(&t["listeners"]))))
            }
            Err(Error::Api { code: api_code::NOT_FOUND, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The largest cover image (`extralarge`, https only) Last.fm has for an album, `None` when it
    /// has none or doesn't know the album. Needs only the API key: a [`Creds`] with an empty secret
    /// is enough.
    pub async fn album_image(&self, artist: &str, album: &str) -> Result<Option<String>, Error> {
        let p = vec![("artist".into(), artist.into()), ("album".into(), album.into())];
        match self.call("album.getInfo", p, false, false).await {
            Ok(v) => Ok(album_image_of(&v)),
            Err(Error::Api { code: api_code::NOT_FOUND, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// (corrected title, listeners) of an album.
    async fn album(&self, artist: &str, album: &str) -> Result<Option<(String, u64)>, Error> {
        let p = vec![
            ("artist".into(), artist.into()),
            ("album".into(), album.into()),
            ("autocorrect".into(), "1".into()),
        ];
        match self.call("album.getInfo", p, false, false).await {
            Ok(v) => Ok(v["album"]["name"].as_str().map(|n| (n.to_string(), num(&v["album"]["listeners"])))),
            Err(Error::Api { code: api_code::NOT_FOUND, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[derive(Debug, Clone)]
struct ArtistInfo {
    name: String,
    listeners: u64,
    mbid: String,
    bio: String,
}

/// `name` occurs in `text` as a word of its own ("IU" in "known professionally as IU (아이유)", not in
/// "IUD"). Scripts without spaces (Hangul, kana) match as a plain substring.
fn mentions(text: &str, name: &str) -> bool {
    let (t, n) = (text.to_lowercase(), name.trim().to_lowercase());
    if n.chars().count() < 2 {
        return false;
    }
    t.match_indices(&n).any(|(i, _)| {
        let before = t[..i].chars().next_back();
        let after = t[i + n.len()..].chars().next();
        let edge = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric() || !c.is_ascii());
        edge(before) && edge(after)
    })
}

/// The same artist under two names: same MusicBrainz id, or one's Last.fm bio names the other.
fn same_artist(a: &ArtistInfo, b: &ArtistInfo) -> bool {
    (!a.mbid.is_empty() && a.mbid == b.mbid) || mentions(&a.bio, &b.name) || mentions(&b.bio, &a.name)
}

fn same_title(a: &str, b: &str) -> bool {
    fold(a) == fold(b)
}

/// The `extralarge` (4th) image of an `album.getInfo` answer, if it is an https URL.
fn album_image_of(v: &Value) -> Option<String> {
    let image = v["album"]["image"].as_array()?.get(3)?["#text"].as_str()?;
    image.starts_with("https://").then(|| image.to_string())
}

/// A failed request as an [`Error::Http`] without its URL: a GET carries the API key, the auth token
/// and the signature in its query, and the message reaches logs and the settings page.
fn network(e: reqwest::Error) -> Error {
    Error::Http(e.without_url().to_string())
}

fn num(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

/// What is sent for one play.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Track {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub duration_s: Option<u32>,
    /// The whole artist credit, for the spelling variants ("뉴진스 (NewJeans)", "A & B").
    #[serde(default)]
    pub artist_line: String,
}

impl Track {
    /// A song as its source names it (not yet resolved).
    pub fn of(song: &Song) -> Self {
        let first = song.artists.first().map(|a| a.name.clone()).unwrap_or_default();
        Track {
            artist: if first.is_empty() { song.artist_line() } else { first },
            title: song.title.clone(),
            album: song.album.clone().filter(|a| !a.trim().is_empty()),
            duration_s: u32::try_from(song.duration.as_secs()).ok().filter(|d| *d > 0),
            artist_line: song.artist_line(),
        }
    }

    fn key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.artist_line,
            self.artist,
            self.title,
            self.album.as_deref().unwrap_or("")
        )
        .to_lowercase()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cached {
    track: Track,
    at: i64,
}

/// Picks the most common spelling of every name on Last.fm (most listeners), with an on-disk cache.
pub struct Resolver {
    client: Client,
    path: Option<PathBuf>,
    cache: tokio::sync::Mutex<HashMap<String, Cached>>,
}

impl Resolver {
    pub fn new(client: Client, cache_file: Option<PathBuf>) -> Self {
        let cache = cache_file
            .as_deref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Resolver { client, path: cache_file, cache: tokio::sync::Mutex::new(cache) }
    }

    /// The names to send for `t`. On a network error the error is returned (the play waits in the queue);
    /// names Last.fm doesn't know keep their cleaned spelling.
    pub async fn resolve(&self, t: &Track) -> Result<Track, Error> {
        let key = t.key();
        if let Some(c) = self.cache.lock().await.get(&key)
            && now_s() - c.at < CACHE_DAYS * 86400
        {
            return Ok(c.track.clone());
        }
        let out = self.lookup(t).await?;
        let mut cache = self.cache.lock().await;
        cache.insert(key, Cached { track: out.clone(), at: now_s() });
        if cache.len() > CACHE_MAX {
            let mut by_age: Vec<(i64, String)> = cache.iter().map(|(k, c)| (c.at, k.clone())).collect();
            by_age.sort();
            for (_, k) in by_age.into_iter().take(cache.len() - CACHE_MAX) {
                cache.remove(&k);
            }
        }
        if let Some(p) = &self.path
            && let Ok(bytes) = serde_json::to_vec(&*cache)
        {
            let tmp = p.with_extension("tmp");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, p);
            }
        }
        Ok(out)
    }

    async fn lookup(&self, t: &Track) -> Result<Track, Error> {
        let c = &self.client;
        // Artist: the spelling with the most listeners.
        let mut artist = (names::clean_title(&t.artist), 0u64);
        for a in names::artist_variants(&t.artist, &t.artist_line) {
            if let Some(info) = c.artist(&a).await?
                && info.listeners > artist.1
            {
                artist = (info.name, info.listeners);
            }
        }
        let artist = artist.0;
        // Title: the spelling with the most listeners for that artist.
        let mut best: Option<(String, String, Option<String>, u64)> = None;
        for title in names::title_variants(&t.title) {
            if let Some(found) = c.track(&artist, &title).await?
                && found.3 > 0
                && best.as_ref().is_none_or(|b| found.3 > b.3)
            {
                best = Some(found);
            }
        }
        let (mut artist, mut title, mut track_album, listeners) = match best {
            Some((a, ti, al, l)) => (a, ti, al, l),
            None => (artist, names::clean_title(&t.title), None, 0),
        };
        // The same song under the artist's other name (아이유 / IU): the most listened track of that title
        // whose artist Last.fm itself ties to ours.
        if let Ok(found) = c.search_track(&title).await {
            let ours = c.artist(&artist).await?;
            for (other, name, l) in found {
                if l <= listeners || other.eq_ignore_ascii_case(&artist) || !same_title(&name, &title) {
                    continue;
                }
                let (Some(a), Some(b)) = (ours.as_ref(), c.artist(&other).await?) else { continue };
                if same_artist(a, &b)
                    && let Some(found) = c.track(&b.name, &name).await?
                {
                    tracing::debug!("Last.fm: “{artist}” is better known as “{}”", found.0);
                    (artist, title, track_album) = (found.0, found.1, found.2);
                    break;
                }
            }
        }
        // Album: the one Last.fm files the track under (the same whichever source played it), else the
        // song's own album in its most listened spelling.
        if let Some(a) = track_album {
            return Ok(Track {
                artist,
                title,
                album: Some(a),
                duration_s: t.duration_s,
                artist_line: t.artist_line.clone(),
            });
        }
        let candidates: Vec<String> = t.album.as_deref().map(names::album_variants).unwrap_or_default();
        let mut album: Option<(String, u64)> = None;
        for a in &candidates {
            if let Some((name, listeners)) = c.album(&artist, a).await?
                && listeners > 0
                && album.as_ref().is_none_or(|b| listeners > b.1)
            {
                album = Some((name, listeners));
            }
        }
        let album = album
            .map(|a| a.0)
            .or_else(|| t.album.as_deref().map(names::clean_title))
            .filter(|a| !a.is_empty());
        Ok(Track { artist, title, album, duration_s: t.duration_s, artist_line: t.artist_line.clone() })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pending {
    track: Track,
    timestamp: i64,
}

/// The plays Last.fm still takes at `now` (younger than [`MAX_AGE_S`], with an hour's margin for the
/// time a batch spends in flight).
fn fresh(mut queue: Vec<Pending>, now: i64) -> Vec<Pending> {
    let oldest = now - MAX_AGE_S + 3600;
    queue.retain(|p| p.timestamp >= oldest);
    queue
}

/// Now playing and scrobbles for one signed-in user. Plays wait in `dir/lastfm-queue.json` until Last.fm
/// accepted them.
pub struct Scrobbler {
    client: Client,
    session: Session,
    resolver: Resolver,
    queue_path: PathBuf,
    /// One sender at a time (a play must not go out twice).
    lock: tokio::sync::Mutex<()>,
}

impl Scrobbler {
    pub fn new(creds: Creds, session: Session, dir: &Path) -> Self {
        let client = Client::new(creds);
        let _ = std::fs::create_dir_all(dir);
        Scrobbler {
            resolver: Resolver::new(client.clone(), Some(dir.join("lastfm-names.json"))),
            client,
            session,
            queue_path: dir.join("lastfm-queue.json"),
            lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn user(&self) -> &str {
        &self.session.name
    }

    pub async fn now_playing(&self, song: &Song) -> Result<(), Error> {
        let t = self.resolver.resolve(&Track::of(song)).await?;
        self.client.now_playing(&self.session.key, &t).await
    }

    /// Queues a play that started at `timestamp` (Unix seconds) and sends everything waiting. Returns how
    /// many plays Last.fm took now.
    pub async fn scrobble(&self, song: &Song, timestamp: i64) -> Result<usize, Error> {
        let _g = self.lock.lock().await;
        let mut queue = self.load();
        queue.push(Pending { track: Track::of(song), timestamp });
        self.save(&queue);
        self.send(queue).await
    }

    /// Sends what waits in the queue (call now and then, e.g. on start-up).
    pub async fn flush(&self) -> Result<usize, Error> {
        let _g = self.lock.lock().await;
        let queue = self.load();
        if queue.is_empty() {
            return Ok(0);
        }
        self.send(queue).await
    }

    /// Plays waiting to be sent (plays too old for Last.fm are not counted: they are never sent).
    pub fn waiting(&self) -> usize {
        fresh(self.load(), now_s()).len()
    }

    async fn send(&self, queue: Vec<Pending>) -> Result<usize, Error> {
        let before = queue.len();
        let mut queue = fresh(queue, now_s());
        if queue.len() != before {
            // Expired plays leave the file now, or `waiting` would count them forever.
            self.save(&queue);
        }
        let mut sent = 0;
        while !queue.is_empty() {
            let n = queue.len().min(50);
            let mut batch = Vec::with_capacity(n);
            for p in &queue[..n] {
                batch.push((self.resolver.resolve(&p.track).await?, p.timestamp));
            }
            match self.client.scrobble(&self.session.key, &batch).await {
                Ok(()) => {
                    queue.drain(..n);
                    sent += n;
                    self.save(&queue);
                }
                // Rejected for good (not a network or service problem): don't block the queue forever.
                Err(e) if !e.transient() && !e.signed_out() => {
                    tracing::warn!("Last.fm refused {n} scrobbles: {e}");
                    queue.drain(..n);
                    self.save(&queue);
                }
                Err(e) => {
                    self.save(&queue);
                    return Err(e);
                }
            }
        }
        Ok(sent)
    }

    fn load(&self) -> Vec<Pending> {
        std::fs::read(&self.queue_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save(&self, queue: &[Pending]) {
        if queue.is_empty() {
            let _ = std::fs::remove_file(&self.queue_path);
            return;
        }
        if let Ok(bytes) = serde_json::to_vec(queue) {
            let tmp = self.queue_path.with_extension("tmp");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &self.queue_path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn network_errors_never_carry_the_request_url() {
        // A port nobody listens on: the request fails locally, nothing leaves the machine.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}/2.0/?api_key=KEY123&token=TOK456&api_sig=SIG789");
        let e = reqwest::Client::new().get(url).send().await.unwrap_err();
        let text = network(e).to_string();
        for secret in ["KEY123", "TOK456", "SIG789"] {
            assert!(!text.contains(secret), "{text}");
        }
    }

    #[test]
    fn names_in_a_bio() {
        let bio = "Lee Ji-eun (이지은), known professionally as IU  (아이유) is a South Korean singer";
        assert!(mentions(bio, "IU"));
        assert!(mentions(bio, "아이유"));
        assert!(!mentions("an IUD", "IU"));
        assert!(!mentions(bio, "BXB"));
        assert!(same_title("좋은 날", "좋은날"));
        let info = serde_json::json!({"album": {"image": [
            {"#text": "https://a/s.png"}, {"#text": "https://a/m.png"}, {"#text": "https://a/l.png"},
            {"#text": "https://a/xl.png"}]}});
        assert_eq!(album_image_of(&info).as_deref(), Some("https://a/xl.png"));
        assert_eq!(album_image_of(&serde_json::json!({"album": {"image": [{"#text": ""}]}})), None);
        assert!(!same_title("좋은 날", "좋은 날 (Instrumental)"));
    }

    #[test]
    fn signature_matches_the_spec() {
        // md5("api_keyxxmethodauth.getSessiontokenyy" + "secret"), format excluded.
        let c = Client::new(Creds { api_key: "xx".into(), secret: "secret".into() });
        let params = vec![
            ("token".to_string(), "yy".to_string()),
            ("method".to_string(), "auth.getSession".to_string()),
            ("api_key".to_string(), "xx".to_string()),
            ("format".to_string(), "json".to_string()),
        ];
        let mut h = Md5::new();
        h.update(b"api_keyxxmethodauth.getSessiontokenyysecret");
        let want: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(c.signature(&params), want);
    }

    #[tokio::test]
    async fn the_queue_survives_and_old_plays_are_dropped() {
        let dir = std::env::temp_dir().join(format!("listen-lastfm-test-{}", std::process::id()));
        let s = Scrobbler::new(
            Creds { api_key: "k".into(), secret: "s".into() },
            Session { name: "me".into(), key: "sk".into() },
            &dir,
        );
        let t = Track {
            artist: "A".into(),
            title: "T".into(),
            album: None,
            duration_s: Some(200),
            artist_line: "A".into(),
        };
        s.save(&[
            Pending { track: t.clone(), timestamp: 1 },
            Pending { track: t.clone(), timestamp: now_s() },
        ]);
        assert_eq!(s.load().len(), 2, "the queue file keeps both");
        assert_eq!(s.waiting(), 1, "a play older than 14 days is not waiting: it will never be sent");
        s.save(&[]);
        assert_eq!(s.waiting(), 0);
        // Only expired plays: flushing sends nothing (no network) and clears the file.
        s.save(&[Pending { track: t, timestamp: 1 }]);
        assert_eq!(s.flush().await.unwrap(), 0);
        assert!(s.load().is_empty(), "expired plays leave the queue file");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
