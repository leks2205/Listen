//! Local copies of YouTube Music audio for analysis, DJ decks and playback: a private LRU
//! cache with a size cap in the user's cache dir (only derived features leave the machine).
//! Files are `<videoId>-<itag>-<duration ms>.<ext>` (the [`FileId`] without a request) and
//! are `clen` bytes: downloads are verified before the `.part` is renamed.

use crate::ytdlp::UrlFacts;
use listen_core::analysis::FileId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// Size cap of the cache (about 130 songs at ~1 MB/min).
pub const CAP_BYTES: u64 = 500 * 1024 * 1024;
/// A file handed out or just written is not evicted for this long, so the mix engine
/// can open it without a [`Pin`].
pub const GRACE: Duration = Duration::from_secs(120);
/// Leftover `.part` files older than this are from a crashed fetch.
const STALE_PART: Duration = Duration::from_secs(3600);
/// Range size per request; googlevideo throttles long un-ranged requests (yt-dlp uses ~10 MB).
const CHUNK: u64 = 10 * 1024 * 1024;

/// A cached file and which exact audio it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub file: FileId,
}

pub struct AudioCache {
    dir: PathBuf,
    cap: u64,
    pins: Arc<Mutex<HashMap<String, usize>>>,
    /// One fetch per video at a time (analysis and mix may ask together).
    inflight: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

/// Keeps a video's cached file from being evicted while it lives (e.g. while
/// the song is loaded on a DJ deck). Pins nest.
#[must_use = "the file is only pinned while the guard lives"]
pub struct Pin {
    pins: Arc<Mutex<HashMap<String, usize>>>,
    video_id: String,
}

impl Drop for Pin {
    fn drop(&mut self) {
        release(&self.pins, &self.video_id);
    }
}

/// Drops one pin of `video_id`; unknown ids are ignored.
fn release(pins: &Mutex<HashMap<String, usize>>, video_id: &str) {
    let mut pins = pins.lock().expect("pins");
    if let Some(n) = pins.get_mut(video_id) {
        *n -= 1;
        if *n == 0 {
            pins.remove(video_id);
        }
    }
}

/// Video ids are 11 characters of `[A-Za-z0-9_-]`; anything else could
/// escape the cache dir.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `(videoId, FileId)` from a cache file name; `None` for anything else.
fn parse_name(name: &str) -> Option<(&str, FileId)> {
    let (stem, ext) = name.rsplit_once('.')?;
    let mut parts = stem.rsplitn(3, '-');
    let dur_ms = parts.next()?.parse().ok()?;
    let itag = parts.next()?;
    let id = parts.next()?;
    if !valid_id(id) || itag.is_empty() || !itag.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let suffix = match ext {
        "webm" => "opus",
        "m4a" => "m4a",
        _ => return None,
    };
    Some((
        id,
        FileId { duration_ms: dur_ms, suffix: Some(suffix.into()), format: Some(itag.into()), bytes: None },
    ))
}

/// The cache file name for `video_id` served as `facts`; `None` if the URL
/// can't be fetched as one verified file (HLS has no `clen`).
pub fn file_name(video_id: &str, facts: &UrlFacts) -> Option<String> {
    let itag = facts.itag.as_deref()?;
    facts.clen?;
    let ext = match facts.suffix()? {
        "opus" => "webm",
        _ => "m4a",
    };
    (valid_id(video_id) && itag.bytes().all(|b| b.is_ascii_digit()))
        .then(|| format!("{video_id}-{itag}-{}.{ext}", facts.dur_ms.unwrap_or(0)))
}

impl AudioCache {
    pub fn new(dir: impl Into<PathBuf>, cap: u64) -> Self {
        Self { dir: dir.into(), cap, pins: Arc::default(), inflight: Mutex::default() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The cached file of `video_id`, if any. Marks it as used (LRU, grace).
    pub fn lookup(&self, video_id: &str) -> Option<Entry> {
        if !valid_id(video_id) {
            return None;
        }
        let prefix = format!("{video_id}-");
        let found = std::fs::read_dir(&self.dir).ok()?.flatten().find_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !name.starts_with(&prefix) {
                return None;
            }
            let (id, mut file) = parse_name(&name)?;
            if id != video_id {
                return None;
            }
            file.bytes = Some(e.metadata().ok()?.len());
            Some(Entry { path: e.path(), file })
        })?;
        touch(&found.path);
        Some(found)
    }

    /// Pins `video_id`'s file against eviction until the guard is dropped.
    pub fn pin(&self, video_id: &str) -> Pin {
        self.pin_count(video_id, true);
        Pin { pins: self.pins.clone(), video_id: video_id.to_string() }
    }

    /// Guard-less pinning for callers that track state themselves
    /// ([`MusicSource::pin_mix`](listen_source::MusicSource::pin_mix)):
    /// `true` adds a pin, `false` drops one (ignored if there is none).
    pub fn pin_count(&self, video_id: &str, pinned: bool) {
        if pinned {
            *self.pins.lock().expect("pins").entry(video_id.to_string()).or_insert(0) += 1;
        } else {
            release(&self.pins, video_id);
        }
    }

    fn pinned(&self, video_id: &str) -> bool {
        self.pins.lock().expect("pins").contains_key(video_id)
    }

    /// Deletes `video_id`'s file (it failed to play), unless it is pinned.
    pub fn remove(&self, video_id: &str) {
        if self.pinned(video_id) {
            return;
        }
        if let Some(e) = self.lookup(video_id) {
            let _ = std::fs::remove_file(e.path);
        }
    }

    /// Where a download of `name` is written before it is complete.
    pub fn part_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.part"))
    }

    /// Moves a finished download into the cache after checking its size,
    /// then evicts old files.
    pub fn commit(&self, name: &str, expected_len: u64) -> std::io::Result<PathBuf> {
        let part = self.part_path(name);
        let len = std::fs::metadata(&part)?.len();
        if len != expected_len {
            let _ = std::fs::remove_file(&part);
            return Err(std::io::Error::other(format!("got {len} bytes, expected {expected_len}")));
        }
        let path = self.dir.join(name);
        std::fs::rename(&part, &path)?;
        touch(&path);
        self.evict();
        Ok(path)
    }

    /// Drops least recently used files until the cache fits its cap, sparing
    /// pinned files and ones used within [`GRACE`]; removes stale `.part`s.
    pub fn evict(&self) {
        let now = SystemTime::now();
        let age = |m: &std::fs::Metadata| m.modified().ok().and_then(|t| now.duration_since(t).ok());
        let Ok(dir) = std::fs::read_dir(&self.dir) else { return };
        let mut files = Vec::new();
        for e in dir.flatten() {
            let (Ok(name), Ok(meta)) = (e.file_name().into_string(), e.metadata()) else { continue };
            if name.ends_with(".part") {
                if age(&meta).is_some_and(|a| a > STALE_PART) {
                    let _ = std::fs::remove_file(e.path());
                }
                continue;
            }
            let Some((id, _)) = parse_name(&name) else { continue };
            let modified = meta.modified().unwrap_or(now);
            files.push((modified, meta.len(), id.to_string(), e.path(), age(&meta).unwrap_or_default()));
        }
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        files.sort_by_key(|f| f.0);
        for (_, len, id, path, age) in files {
            if total <= self.cap {
                break;
            }
            if age < GRACE || self.pinned(&id) {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }

    /// The lock serialising fetches of `video_id`.
    pub fn fetch_lock(&self, video_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self.inflight.lock().expect("inflight");
        map.retain(|_, l| Arc::strong_count(l) > 1);
        map.entry(video_id.to_string()).or_default().clone()
    }
}

/// Marks a file as just used (its mtime is the LRU clock).
fn touch(path: &Path) {
    if let Ok(f) = std::fs::File::options().append(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// The process-wide cache in `<cache dir>/ytm-audio`, shared by all accounts.
pub fn cache() -> &'static AudioCache {
    static CACHE: OnceLock<AudioCache> = OnceLock::new();
    CACHE.get_or_init(|| {
        let base = listen_core::config::Paths::discover()
            .map(|p| p.cache_dir)
            .unwrap_or_else(|_| std::env::temp_dir().join("listen"));
        let cache = AudioCache::new(base.join("ytm-audio"), CAP_BYTES);
        let _ = std::fs::create_dir_all(cache.dir());
        cache.evict();
        cache
    })
}

/// Pins a YouTube Music song's cached audio (see [`Pin`]).
pub fn pin(video_id: &str) -> Pin {
    cache().pin(video_id)
}

/// The cached local file of a YouTube Music song, if it is there (no request).
pub fn cached(video_id: &str) -> Option<PathBuf> {
    cache().lookup(video_id).map(|e| e.path)
}

/// HTTP client for googlevideo fetches (no cookies).
pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default()
    })
}

/// Downloads `url` into `dest` in ranged chunks and checks it is `len` bytes.
pub async fn download(client: &reqwest::Client, url: &str, dest: &Path, len: u64) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let mut out = tokio::fs::File::create(dest).await.map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut done = 0u64;
    while done < len {
        let end = (done + CHUNK).min(len) - 1;
        let resp = client
            .get(url)
            .header(reqwest::header::RANGE, format!("bytes={done}-{end}"))
            .send()
            .await
            .map_err(|e| format!("request failed: {}", e.without_url()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        let mut resp = resp;
        while let Some(chunk) =
            resp.chunk().await.map_err(|e| format!("download broke off: {}", e.without_url()))?
        {
            out.write_all(&chunk).await.map_err(|e| e.to_string())?;
            done += chunk.len() as u64;
        }
        if done <= end {
            return Err(format!("short read: {done} of {len} bytes"));
        }
    }
    out.flush().await.map_err(|e| e.to_string())?;
    if done != len {
        return Err(format!("got {done} bytes, expected {len}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache(cap: u64) -> AudioCache {
        let dir = std::env::temp_dir().join(format!("listen-ytm-audio-test-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        AudioCache::new(dir, cap)
    }

    fn put(c: &AudioCache, name: &str, len: usize, age: Duration) -> PathBuf {
        let p = c.dir().join(name);
        std::fs::write(&p, vec![0u8; len]).unwrap();
        let f = std::fs::File::options().append(true).open(&p).unwrap();
        f.set_modified(SystemTime::now() - age).unwrap();
        p
    }

    fn facts(itag: &str, clen: u64) -> UrlFacts {
        UrlFacts { itag: Some(itag.into()), clen: Some(clen), dur_ms: Some(213_581), mime: None }
    }

    #[test]
    fn names_round_trip() {
        let name = file_name("a-b_cDEF123", &facts("251", 10)).unwrap();
        assert_eq!(name, "a-b_cDEF123-251-213581.webm");
        let (id, f) = parse_name(&name).unwrap();
        assert_eq!(id, "a-b_cDEF123");
        assert_eq!(
            (f.format.as_deref(), f.suffix.as_deref(), f.duration_ms),
            (Some("251"), Some("opus"), 213_581)
        );
        assert_eq!(file_name("x", &facts("140", 1)).as_deref(), Some("x-140-213581.m4a"));
        assert!(file_name("../etc", &facts("251", 1)).is_none());
        assert!(file_name("x", &UrlFacts { clen: None, ..facts("251", 1) }).is_none());
        assert!(parse_name("x-251-1.webm.part").is_none());
        assert!(parse_name("notes.txt").is_none());
    }

    #[test]
    fn lookup_commit_and_part_files() {
        let c = temp_cache(u64::MAX);
        let name = file_name("vid", &facts("251", 5)).unwrap();
        // A download in progress is invisible.
        std::fs::write(c.part_path(&name), b"abc").unwrap();
        assert!(c.lookup("vid").is_none());
        // Wrong size: rejected and removed.
        assert!(c.commit(&name, 5).is_err());
        assert!(!c.part_path(&name).exists());
        std::fs::write(c.part_path(&name), b"abcde").unwrap();
        let path = c.commit(&name, 5).unwrap();
        let e = c.lookup("vid").unwrap();
        assert_eq!(e.path, path);
        assert_eq!(e.file.bytes, Some(5));
        assert!(c.lookup("vi").is_none());
        // Stale .part files are cleaned up, fresh ones kept.
        let old = put(&c, "old-251-1.webm.part", 1, STALE_PART * 2);
        let fresh = put(&c, "new-251-1.webm.part", 1, Duration::ZERO);
        c.evict();
        assert!(!old.exists() && fresh.exists());
        c.remove("vid");
        assert!(c.lookup("vid").is_none());
        std::fs::remove_dir_all(c.dir()).unwrap();
    }

    #[test]
    fn evicts_least_recently_used_but_not_pinned_or_recent() {
        let c = temp_cache(250);
        let hour = Duration::from_secs(3600);
        let a = put(&c, "aaa-251-1.webm", 100, 4 * hour);
        let b = put(&c, "bbb-251-1.webm", 100, 3 * hour);
        let d = put(&c, "ddd-251-1.webm", 100, 2 * hour);
        let e = put(&c, "eee-251-1.webm", 100, Duration::ZERO);
        let pin = c.pin("aaa");
        c.evict();
        // 400 > 250: "aaa" is pinned, so "bbb" and "ddd" go; "eee" is recent.
        assert!(a.exists() && !b.exists() && !d.exists() && e.exists());
        drop(pin);
        assert!(!c.pinned("aaa"));
        // Still 200 ≤ 250: nothing more to do.
        c.evict();
        assert!(a.exists());
        // Using a file moves it to the front.
        let f = put(&c, "fff-251-1.webm", 100, 5 * hour);
        assert!(c.lookup("fff").is_some());
        c.evict();
        assert!(!a.exists() && f.exists());
        std::fs::remove_dir_all(c.dir()).unwrap();
    }

    /// `LISTEN_TEST_URL=<googlevideo url> cargo test -p listen-ytmusic -- --ignored download_url`
    #[tokio::test]
    #[ignore = "network"]
    async fn download_url() {
        // The URL itself, or a file holding it.
        let mut url = std::env::var("LISTEN_TEST_URL").expect("LISTEN_TEST_URL");
        if url.starts_with('/') {
            url = std::fs::read_to_string(&url).unwrap().trim().to_string();
        }
        let len = crate::ytdlp::url_facts(&url).clen.expect("clen");
        let dest = std::env::temp_dir().join("listen-download-test.bin");
        let r = download(http(), &url, &dest, len).await;
        let _ = std::fs::remove_file(&dest);
        r.unwrap();
    }

    #[test]
    fn pins_nest() {
        let c = temp_cache(0);
        let p1 = c.pin("x");
        let p2 = c.pin("x");
        drop(p1);
        assert!(c.pinned("x"));
        drop(p2);
        assert!(!c.pinned("x"));
        // Guard-less pins balance too; extra releases are harmless.
        c.pin_count("x", false);
        c.pin_count("x", true);
        c.pin_count("x", true);
        c.pin_count("x", false);
        assert!(c.pinned("x"));
        c.pin_count("x", false);
        c.pin_count("x", false);
        assert!(!c.pinned("x"));
        // Pinned files survive remove().
        let path = put(&c, "y-251-1.webm", 1, Duration::ZERO);
        let _p = c.pin("y");
        c.remove("y");
        assert!(path.exists());
        std::fs::remove_dir_all(c.dir()).unwrap();
    }
}
