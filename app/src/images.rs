//! Cover-art cache: decoded handles in memory, original bytes on disk
//! (`~/.cache/Listen/covers`). Fetches are deduplicated. Only bytes that decode as an image are cached,
//! written atomically; a cached file that is not an image counts as a miss.

use iced::widget::image::Handle;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

/// Cache key `{server}/{image id or url}@{size}`; never the credential-bearing URL.
pub type Key = String;

pub fn key(server: &str, image: &listen_core::model::ImageRef, size: u32) -> Key {
    let id = match image {
        listen_core::model::ImageRef::Id(id) => id.as_str(),
        listen_core::model::ImageRef::Url(url) => url.as_str(),
    };
    format!("{server}/{id}@{size}")
}

#[derive(Default)]
pub struct Images {
    handles: HashMap<Key, Handle>,
    /// Pre-blurred small versions for header backgrounds.
    blurred: HashMap<Key, Handle>,
    blur_pending: HashSet<Key>,
    /// Tiny blurred covers shown instead of explicit ones.
    explicit: HashMap<Key, Handle>,
    explicit_pending: HashSet<Key>,
    pending: HashSet<Key>,
    failed: HashSet<Key>,
    dir: Option<PathBuf>,
    /// Decoded ThumbHash previews of covers that loaded before.
    previews: HashMap<Key, Handle>,
    /// Width / height of the loaded covers.
    ratios: HashMap<Key, f32>,
}

impl Images {
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        Self { dir: cache_dir.map(|d| d.join("covers")), ..Default::default() }
    }

    pub fn get(&self, key: &str) -> Option<&Handle> {
        self.handles.get(key)
    }

    /// Marks `key` as requested; returns false if it is cached, in flight or failed.
    pub fn should_fetch(&mut self, key: &str) -> bool {
        if self.handles.contains_key(key) || self.failed.contains(key) {
            return false;
        }
        self.pending.insert(key.to_string())
    }

    pub fn loaded(&mut self, key: Key, bytes: Option<Arc<Vec<u8>>>) {
        self.pending.remove(&key);
        // Bytes that are no image would stay a blank cover for good: they count as failed.
        match bytes.and_then(|b| Some((aspect_ratio(&b)?, b))) {
            Some((r, bytes)) => {
                self.ratios.insert(key.clone(), r);
                self.handles.insert(key, Handle::from_bytes(bytes.as_ref().clone()));
            }
            None => {
                self.failed.insert(key);
            }
        }
    }

    /// Width / height of a loaded cover.
    pub fn ratio(&self, key: &str) -> Option<f32> {
        self.ratios.get(key).copied()
    }

    /// The ThumbHash preview of a cover (see [`Images::prime_preview`]).
    pub fn preview(&self, key: &str) -> Option<&Handle> {
        self.previews.get(key)
    }

    /// Loads the ThumbHash saved next to the cached cover, for a preview while the image loads.
    pub fn prime_preview(&mut self, key: &str) {
        if self.previews.contains_key(key) {
            return;
        }
        let Some(path) = self.disk_path(key).map(|p| thumbhash_path(&p)) else { return };
        if let Ok(hash) = std::fs::read(path)
            && let Some(handle) = preview_handle(&hash)
        {
            self.previews.insert(key.to_string(), handle);
        }
    }

    pub fn blurred(&self, key: &str) -> Option<&Handle> {
        self.blurred.get(key)
    }

    /// Marks a blur job as started; false if done or already running.
    pub fn should_blur(&mut self, key: &str) -> bool {
        !self.blurred.contains_key(key) && self.blur_pending.insert(key.to_string())
    }

    pub fn blur_loaded(&mut self, key: Key, handle: Option<Handle>) {
        self.blur_pending.remove(&key);
        if let Some(h) = handle {
            self.blurred.insert(key, h);
        }
    }

    /// Forgets every decoded cover and every failed lookup (the disk cache stays).
    pub fn clear_memory(&mut self) {
        self.handles.clear();
        self.blurred.clear();
        self.explicit.clear();
        self.ratios.clear();
        self.previews.clear();
        self.blur_pending.clear();
        self.explicit_pending.clear();
        self.pending.clear();
        self.failed.clear();
    }

    pub fn clear_blurred(&mut self) {
        self.blurred.clear();
        self.blur_pending.clear();
    }

    /// The blurred stand-in of a cover (explicit songs), once made.
    pub fn explicit(&self, key: &str) -> Option<&Handle> {
        self.explicit.get(key)
    }

    pub fn should_blur_explicit(&mut self, key: &str) -> bool {
        !self.explicit.contains_key(key) && self.explicit_pending.insert(key.to_string())
    }

    pub fn explicit_loaded(&mut self, key: Key, handle: Option<Handle>) {
        self.explicit_pending.remove(&key);
        if let Some(h) = handle {
            self.explicit.insert(key, h);
        }
    }

    /// Bytes of the covers held in memory (for jobs started after the fact).
    pub fn cached_bytes(&self) -> Vec<(Key, Vec<u8>)> {
        self.handles
            .iter()
            .filter_map(|(k, h)| match h {
                Handle::Bytes(_, b) => Some((k.clone(), b.to_vec())),
                _ => None,
            })
            .collect()
    }

    pub fn disk_path(&self, key: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("{:016x}", fnv1a(key.as_bytes()))))
    }
}

/// Fetches bytes from the disk cache or the network (and fills the disk cache). `None` when the source
/// gives no image: an error page served with HTTP 200 is neither returned nor cached.
pub async fn fetch(http: reqwest::Client, url: String, disk: Option<PathBuf>) -> Option<Arc<Vec<u8>>> {
    let bytes = fetch_bytes(http, url, disk.clone()).await?;
    if let Some(path) = disk {
        // After the first load, remember a ThumbHash for the next time.
        let for_hash = bytes.clone();
        let _ = tokio::task::spawn_blocking(move || save_thumbhash(&path, &for_hash)).await;
    }
    Some(bytes)
}

async fn fetch_bytes(http: reqwest::Client, url: String, disk: Option<PathBuf>) -> Option<Arc<Vec<u8>>> {
    if let Some(path) = &disk
        && let Ok(bytes) = tokio::fs::read(path).await
    {
        if aspect_ratio(&bytes).is_some() {
            return Some(Arc::new(bytes));
        }
        // Not an image (e.g. an error page an older version cached): a miss, fetched again below.
        let _ = tokio::fs::remove_file(path).await;
        let _ = tokio::fs::remove_file(thumbhash_path(path)).await;
    }
    let bytes = if url.starts_with("file://") {
        tokio::task::spawn_blocking(move || listen_local::load_cover(&url)).await.ok()??
    } else {
        let resp = http.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            tracing::debug!("cover fetch failed: HTTP {}", resp.status());
            return None;
        }
        resp.bytes().await.ok()?.to_vec()
    };
    let (bytes, decodes) = tokio::task::spawn_blocking(move || {
        let ok = image::load_from_memory(&bytes).is_ok();
        (bytes, ok)
    })
    .await
    .ok()?;
    if !decodes {
        tracing::debug!("cover fetch returned no decodable image");
        return None;
    }
    if let Some(path) = disk {
        store(&path, &bytes).await;
    }
    Some(Arc::new(bytes))
}

/// Writes a cover into the disk cache through a temporary file and a rename, so a quit mid-write
/// never leaves a cut-off file at `path` that later starts would take for the cover.
async fn store(path: &std::path::Path, bytes: &[u8]) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}-{n}.tmp", std::process::id()));
    let stored = tokio::fs::write(&tmp, bytes).await.is_ok() && tokio::fs::rename(&tmp, path).await.is_ok();
    if !stored {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
}

/// A small, heavily blurred version of a cover, like Feishin's `filter: blur(<n>rem)` header art.
pub fn blur(bytes: &[u8], sigma: f32) -> Option<Handle> {
    let img = image::load_from_memory(bytes).ok()?;
    // 4:1 centre crop to match the header strip, drawn with `Fill` (`Cover` overflows its bounds).
    let (w, h) = (img.width(), img.height());
    let crop_h = (w / 4).clamp(1, h);
    let small = img.crop_imm(0, (h - crop_h) / 2, w, crop_h).resize_exact(
        192,
        48,
        image::imageops::FilterType::Triangle,
    );
    let blurred = image::imageops::blur(&small.to_rgba8(), sigma.max(0.5));
    let (w, h) = blurred.dimensions();
    Some(Handle::from_rgba(w, h, blurred.into_raw()))
}

/// A small, heavily blurred square version of a cover (for explicit songs).
pub fn blur_cover(bytes: &[u8]) -> Option<Handle> {
    let img = image::load_from_memory(bytes).ok()?;
    let small = img.resize_to_fill(32, 32, image::imageops::FilterType::Triangle);
    let blurred = image::imageops::blur(&small.to_rgba8(), 2.5);
    let (w, h) = blurred.dimensions();
    Some(Handle::from_rgba(w, h, blurred.into_raw()))
}

/// Width / height of an encoded image, from its header only.
pub fn aspect_ratio(bytes: &[u8]) -> Option<f32> {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    (w > 0 && h > 0).then(|| w as f32 / h as f32)
}

/// The ThumbHash of an encoded image (about 25 bytes).
pub fn make_thumbhash(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?.thumbnail(100, 100).to_rgba8();
    let (w, h) = img.dimensions();
    Some(thumbhash::rgba_to_thumb_hash(w as usize, h as usize, img.as_raw()))
}

/// A small image decoded from a ThumbHash.
pub fn preview_handle(hash: &[u8]) -> Option<Handle> {
    let (w, h, rgba) = thumbhash::thumb_hash_to_rgba(hash).ok()?;
    Some(Handle::from_rgba(w as u32, h as u32, rgba))
}

fn thumbhash_path(cover: &std::path::Path) -> PathBuf {
    cover.with_extension("th")
}

/// Writes the ThumbHash next to the cached cover unless it is there already.
fn save_thumbhash(cover: &std::path::Path, bytes: &[u8]) {
    let path = thumbhash_path(cover);
    if path.exists() {
        return;
    }
    if let Some(hash) = make_thumbhash(bytes) {
        let _ = std::fs::write(path, hash);
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, _| image::Rgba([(x * 255 / w) as u8, 40, 200, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn aspect_ratio_comes_from_the_header() {
        assert_eq!(aspect_ratio(&png(200, 100)), Some(2.0));
        assert_eq!(aspect_ratio(&png(50, 100)), Some(0.5));
        assert_eq!(aspect_ratio(b"not an image"), None);
    }

    fn run<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    /// A local cover (file:// URL, never the network) and the cache file `fetch_bytes` keeps for it.
    fn local_cover(name: &str, bytes: &[u8]) -> (PathBuf, String, PathBuf) {
        let dir = std::env::temp_dir().join(format!("listen-cover-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("cover.png");
        std::fs::write(&src, bytes).unwrap();
        let url = url::Url::from_file_path(&src).unwrap().to_string();
        (dir.clone(), url, dir.join("cache").join("0123456789abcdef"))
    }

    #[test]
    fn a_cached_cover_that_is_no_image_is_fetched_again() {
        let (dir, url, cached) = local_cover("corrupt", &png(8, 4));
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, b"<html>captive portal</html>").unwrap();
        let bytes = run(fetch_bytes(reqwest::Client::new(), url, Some(cached.clone()))).unwrap();
        assert_eq!(aspect_ratio(&bytes), Some(2.0));
        assert_eq!(std::fs::read(&cached).unwrap(), *bytes, "the cache now holds the image");
        let left: Vec<_> =
            std::fs::read_dir(cached.parent().unwrap()).unwrap().flatten().map(|e| e.path()).collect();
        assert_eq!(left, [cached], "no temporary file is left behind");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn bytes_that_are_no_image_are_never_cached() {
        let (dir, url, cached) = local_cover("html", b"<html>not a cover</html>");
        assert!(run(fetch_bytes(reqwest::Client::new(), url, Some(cached.clone()))).is_none());
        assert!(!cached.exists());
        let mut images = Images::default();
        images.loaded("k".into(), Some(Arc::new(b"<html>".to_vec())));
        assert!(images.get("k").is_none() && !images.should_fetch("k"), "no image counts as failed");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn thumbhash_round_trips_to_a_small_preview() {
        let hash = make_thumbhash(&png(300, 300)).unwrap();
        assert!(hash.len() < 40, "{}", hash.len());
        assert!(preview_handle(&hash).is_some());
        assert!(preview_handle(&[1, 2]).is_none());
    }

    #[test]
    fn the_hash_is_saved_after_the_first_load_and_found_next_time() {
        let dir = std::env::temp_dir().join(format!("listen-th-{}", std::process::id()));
        let mut images = Images::new(Some(dir.clone()));
        let key = "srv/abc@300";
        let cover = images.disk_path(key).unwrap();
        std::fs::create_dir_all(cover.parent().unwrap()).unwrap();
        images.prime_preview(key);
        assert!(images.preview(key).is_none(), "nothing before the first load");
        save_thumbhash(&cover, &png(64, 64));
        assert!(cover.with_extension("th").exists());
        images.prime_preview(key);
        assert!(images.preview(key).is_some());
        std::fs::remove_dir_all(dir).ok();
    }
}
