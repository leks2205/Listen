//! Walks a folder and reads tags, audio properties and cover-art hints.

use crate::cache;
use listen_core::model::GainInfo;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::{Accessor, ItemKey};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

const AUDIO_EXTENSIONS: &[&str] =
    &["mp3", "flac", "ogg", "oga", "opus", "m4a", "m4b", "aac", "wav", "aif", "aiff", "wv", "ape", "mpc"];
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
const COVER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];

/// One audio file as read from disk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scanned {
    /// Path below the scanned root (stable id source).
    pub rel: String,
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub codec: Option<String>,
    pub bit_rate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub bpm: Option<u32>,
    pub gain: GainInfo,
    pub duration: Duration,
    /// An image file in the same folder.
    pub folder_art: Option<PathBuf>,
    /// The file has embedded cover art.
    pub embedded_art: bool,
}

pub fn is_audio(path: &Path) -> bool {
    has_extension(path, AUDIO_EXTENSIONS)
}

pub fn is_image(path: &Path) -> bool {
    has_extension(path, IMAGE_EXTENSIONS)
}

fn has_extension(path: &Path, list: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| list.iter().any(|known| e.eq_ignore_ascii_case(known)))
}

/// Reads every audio file below `root`; unreadable files are skipped.
pub fn scan(root: &Path) -> Vec<Scanned> {
    let mut art = FolderArt::default();
    let mut out = Vec::new();
    for entry in WalkDir::new(root).follow_links(true).sort_by_file_name().into_iter().filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if is_audio(path) {
            match read_file(root, path) {
                Ok(s) => out.push(s),
                Err(e) => tracing::debug!("skipping {}: {e}", path.display()),
            }
        } else if is_image(path) {
            art.add(path);
        }
    }
    for s in &mut out {
        s.folder_art = art.get(&s.path);
    }
    out
}

/// Like [`scan`], but reuses the tags of files whose exact modification time and
/// size did not change. `full` ignores the cache and rewrites it. Only the tag
/// data is reused: the folder art is looked up for every folder again.
pub(crate) fn scan_with_cache(root: &Path, cache_path: Option<&Path>, full: bool) -> Vec<Scanned> {
    let mut cached = if full { None } else { cache_path.and_then(cache::ScanCache::load) };
    let mut dirty = cached.is_none();
    let mut art = FolderArt::default();
    let mut out = Vec::new();
    // `(index in out, len)` for every current file that can be cached; the mtime
    // is read back from `Scanned::modified` when the cache is written.
    let mut written: Vec<(usize, u64)> = Vec::new();
    let mut unreadable: Vec<(String, cache::Stamp, u64)> = Vec::new();

    for entry in WalkDir::new(root).follow_links(true).sort_by_file_name().into_iter().filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if !is_audio(entry.path()) {
            if is_image(entry.path()) {
                art.add(entry.path());
            }
            continue;
        }
        let path = entry.path().to_path_buf();
        let meta = std::fs::metadata(&path).ok();
        let modified = meta.as_ref().and_then(|m| m.modified().ok());
        let len = meta.as_ref().map(std::fs::Metadata::len);
        let now = modified.and_then(cache::stamp);
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();

        // Remove first, so whatever is left in the map after the walk has vanished.
        let previous = cached.as_mut().and_then(|c| c.remove(rel.as_ref()));
        let hit = match (previous, now, len) {
            (Some(e), Some(stamp), Some(len)) if e.mtime == stamp && e.len == len => Some(e),
            _ => None,
        };
        match hit {
            Some(mut e) => match e.scanned.take() {
                Some(mut s) => {
                    s.path = path;
                    s.modified = modified;
                    let idx = out.len();
                    out.push(s);
                    written.push((idx, len.expect("hit has a length")));
                }
                None => {
                    // A cached unreadable file: do not parse it again.
                    unreadable.push((
                        rel.into_owned(),
                        now.expect("hit has a stamp"),
                        len.expect("hit has a length"),
                    ));
                }
            },
            None => {
                dirty = true;
                match read_file_with(root, &path, modified) {
                    Ok(s) => {
                        if let Some(len) = len
                            && s.modified.is_some()
                        {
                            let idx = out.len();
                            out.push(s);
                            written.push((idx, len));
                        } else {
                            // No usable stat key: show the song, but do not cache it.
                            out.push(s);
                        }
                    }
                    Err(e) => {
                        tracing::debug!("skipping {}: {e}", path.display());
                        if let (Some(stamp), Some(len)) = (now, len) {
                            unreadable.push((rel.into_owned(), stamp, len));
                        }
                    }
                }
            }
        }
    }

    if cached.is_some_and(|c| !c.is_empty()) {
        dirty = true;
    }
    if dirty && let Some(path) = cache_path {
        let data = cache::encode(&out, &written, &unreadable);
        if let Err(e) = cache::write_atomic(path, &data) {
            tracing::debug!("could not write scan cache {}: {e}", path.display());
        }
    }
    for s in &mut out {
        s.folder_art = art.get(&s.path);
    }
    out
}

/// The best cover candidate per folder, collected from the walk itself. Nothing
/// is cached: every scan sees a new `cover.jpg` in a folder it already visits.
#[derive(Default)]
struct FolderArt {
    best: HashMap<PathBuf, (usize, PathBuf)>,
}

impl FolderArt {
    fn add(&mut self, path: &Path) {
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()) else { return };
        let Some(rank) = COVER_NAMES.iter().position(|n| *n == stem) else { return };
        let dir = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        match self.best.get_mut(&dir) {
            Some((best_rank, best)) if rank < *best_rank => {
                *best_rank = rank;
                *best = path.to_path_buf();
            }
            Some(_) => {}
            None => {
                self.best.insert(dir, (rank, path.to_path_buf()));
            }
        }
    }

    fn get(&self, audio: &Path) -> Option<PathBuf> {
        let dir = audio.parent().unwrap_or_else(|| Path::new(""));
        self.best.get(dir).map(|(_, path)| path.clone())
    }
}

fn read_file(root: &Path, path: &Path) -> Result<Scanned, String> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    read_file_with(root, path, modified)
}

fn read_file_with(root: &Path, path: &Path, modified: Option<SystemTime>) -> Result<Scanned, String> {
    let tagged = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let props = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut s = Scanned {
        rel: path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned(),
        path: path.to_path_buf(),
        modified,
        title: stem,
        codec: path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()),
        bit_rate: props.audio_bitrate(),
        sample_rate: props.sample_rate(),
        bit_depth: props.bit_depth().map(u32::from),
        duration: props.duration(),
        ..Default::default()
    };
    if let Some(tag) = tag {
        let text =
            |v: Option<std::borrow::Cow<'_, str>>| v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        if let Some(t) = text(tag.title()) {
            s.title = t;
        }
        let mut artists: Vec<String> = tag
            .get_strings(&ItemKey::TrackArtist)
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
        if artists.is_empty()
            && let Some(a) = text(tag.artist())
        {
            artists.push(a);
        }
        s.artists = artists;
        s.album = text(tag.album());
        s.album_artist =
            tag.get_string(&ItemKey::AlbumArtist).map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
        s.track = tag.track();
        s.disc = tag.disk();
        s.year = tag.year().and_then(|y| i32::try_from(y).ok());
        s.genres = tag
            .get_strings(&ItemKey::Genre)
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty())
            .collect();
        s.bpm = tag
            .get_string(&ItemKey::Bpm)
            .and_then(|b| b.trim().parse::<f32>().ok())
            .map(|b| b.round() as u32);
        let gain = |k: ItemKey| tag.get_string(&k).and_then(parse_db);
        s.gain = GainInfo {
            track_gain: gain(ItemKey::ReplayGainTrackGain),
            track_peak: tag.get_string(&ItemKey::ReplayGainTrackPeak).and_then(|v| v.trim().parse().ok()),
            album_gain: gain(ItemKey::ReplayGainAlbumGain),
            album_peak: tag.get_string(&ItemKey::ReplayGainAlbumPeak).and_then(|v| v.trim().parse().ok()),
        };
        s.embedded_art = !tag.pictures().is_empty();
    }
    Ok(s)
}

/// "-6.48 dB" -> -6.48
fn parse_db(v: &str) -> Option<f32> {
    v.trim().trim_end_matches("dB").trim_end_matches("db").trim().parse().ok()
}

/// Cover bytes for a `file://` URL: an image file as is, or the first
/// embedded picture of an audio file.
pub fn load_cover(url: &str) -> Option<Vec<u8>> {
    let path = url::Url::parse(url).ok()?.to_file_path().ok()?;
    if is_image(&path) {
        return std::fs::read(path).ok();
    }
    let tagged = lofty::read_from_path(&path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    tag.pictures().first().map(|p| p.data().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::{TagEdit, TagField};

    #[test]
    fn parses_replaygain() {
        assert_eq!(parse_db("-6.48 dB"), Some(-6.48));
        assert_eq!(parse_db("+1.0"), Some(1.0));
        assert_eq!(parse_db("x"), None);
    }

    #[test]
    fn recognises_extensions() {
        assert!(is_audio(Path::new("a/B.FLAC")));
        assert!(!is_audio(Path::new("a/cover.jpg")));
        assert!(is_image(Path::new("a/Folder.JPG")));
    }

    // ---- scan cache -------------------------------------------------------

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("listen-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 0.1 s of 8-bit mono silence.
    fn wav_bytes() -> Vec<u8> {
        let samples = vec![128u8; 800];
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&8000u32.to_le_bytes());
        w.extend_from_slice(&8000u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&8u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        w.extend_from_slice(&samples);
        w
    }

    /// A WAV with an ID3 title, so a tag edit really changes what `scan` reads.
    fn tagged_wav(dir: &Path, title: &str) -> Vec<u8> {
        let path = dir.join(format!("template-{title}.wav"));
        std::fs::write(&path, wav_bytes()).unwrap();
        let mut edit = TagEdit::default();
        edit.fields.insert(TagField::Title, title.to_string());
        crate::tags::write(&path, &edit).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        bytes
    }

    fn set_mtime(path: &Path, time: SystemTime) {
        std::fs::OpenOptions::new().write(true).open(path).unwrap().set_modified(time).unwrap();
    }

    fn song_titles(scanned: &[Scanned]) -> Vec<&str> {
        scanned.iter().map(|s| s.title.as_str()).collect()
    }

    #[test]
    fn first_cached_scan_matches_the_uncached_scan() {
        let root = temp("first");
        let cache = root.with_extension("cache.bin");
        std::fs::create_dir_all(root.join("Album")).unwrap();
        std::fs::write(root.join("Album/one.wav"), tagged_wav(&root, "One")).unwrap();
        std::fs::write(root.join("Album/two.wav"), tagged_wav(&root, "Two")).unwrap();
        std::fs::write(root.join("Album/cover.jpg"), [0xff, 0xd8, 0xff]).unwrap();

        let uncached = scan(&root);
        let first = scan_with_cache(&root, Some(&cache), false);
        assert_eq!(first, uncached);
        assert!(cache.is_file(), "the first scan writes the cache");

        // Warm: the same records come back from the cache.
        assert_eq!(scan_with_cache(&root, Some(&cache), false), uncached);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tag_edit_with_a_new_mtime_is_read_again() {
        let root = temp("mtime");
        let cache = root.with_extension("cache.bin");
        let song = root.join("song.wav");
        std::fs::write(&song, tagged_wav(&root, "AAA")).unwrap();
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), false)), ["AAA"]);
        let old = std::fs::metadata(&song).unwrap().modified().unwrap();

        std::fs::write(&song, tagged_wav(&root, "BBB")).unwrap();
        set_mtime(&song, old + Duration::from_secs(2));
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), false)), ["BBB"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_same_size_edit_with_the_old_mtime_stays_stale_until_a_full_rescan() {
        let root = temp("stale");
        let cache = root.with_extension("cache.bin");
        let song = root.join("song.wav");
        let before = tagged_wav(&root, "AAA");
        let after = tagged_wav(&root, "BBB");
        assert_eq!(before.len(), after.len(), "the fixture must keep the file size");
        std::fs::write(&song, &before).unwrap();
        let old = std::fs::metadata(&song).unwrap().modified().unwrap();
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), false)), ["AAA"]);

        std::fs::write(&song, &after).unwrap();
        set_mtime(&song, old);
        // Exact mtime + size match: the cheap scan trusts the cache.
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), false)), ["AAA"]);
        // The full rescan is the way out (tag editors that keep the mtime).
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), true)), ["BBB"]);
        // ... and it refreshed the cache.
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), false)), ["BBB"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_is_a_new_key_and_drops_the_old_entry() {
        let root = temp("rename");
        let cache = root.with_extension("cache.bin");
        std::fs::write(root.join("a.wav"), wav_bytes()).unwrap();
        // No tag: the title comes from the file name, so a stale hit would be visible.
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), false)), ["a"]);
        std::fs::rename(root.join("a.wav"), root.join("b.wav")).unwrap();
        let after = scan_with_cache(&root, Some(&cache), false);
        assert_eq!(song_titles(&after), ["b"]);
        assert_eq!(after[0].rel, "b.wav");

        let mut entries = crate::cache::ScanCache::load(&cache).unwrap();
        assert!(entries.remove("a.wav").is_none(), "the old relative path is gone");
        assert!(entries.remove("b.wav").is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deleted_files_are_dropped_from_the_cache() {
        let root = temp("delete");
        let cache = root.with_extension("cache.bin");
        std::fs::write(root.join("one.wav"), wav_bytes()).unwrap();
        std::fs::write(root.join("two.wav"), wav_bytes()).unwrap();
        assert_eq!(scan_with_cache(&root, Some(&cache), false).len(), 2);
        std::fs::remove_file(root.join("one.wav")).unwrap();
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), false)), ["two"]);
        let mut entries = crate::cache::ScanCache::load(&cache).unwrap();
        assert!(entries.remove("one.wav").is_none());
        assert!(entries.remove("two.wav").is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_cover_added_to_an_existing_folder_is_picked_up() {
        let root = temp("cover");
        let cache = root.with_extension("cache.bin");
        std::fs::write(root.join("song.wav"), wav_bytes()).unwrap();
        assert_eq!(scan_with_cache(&root, Some(&cache), false)[0].folder_art, None);
        std::fs::write(root.join("cover.jpg"), [0xff, 0xd8, 0xff]).unwrap();
        let after = scan_with_cache(&root, Some(&cache), false);
        assert_eq!(after[0].folder_art.as_deref(), Some(root.join("cover.jpg").as_path()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unreadable_files_are_cached_and_not_parsed_again() {
        let root = temp("unreadable");
        let cache = root.with_extension("cache.bin");
        std::fs::write(root.join("good.wav"), tagged_wav(&root, "Good")).unwrap();
        let valid = tagged_wav(&root, "Now valid");
        let bad = root.join("bad.wav");
        std::fs::write(&bad, vec![0xee; valid.len()]).unwrap();
        let old = std::fs::metadata(&bad).unwrap().modified().unwrap();
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), false)), ["Good"]);

        // Same length and mtime, now readable: the negative cache entry still wins.
        std::fs::write(&bad, &valid).unwrap();
        set_mtime(&bad, old);
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), false)), ["Good"]);
        assert_eq!(song_titles(&scan_with_cache(&root, Some(&cache), true)), ["Now valid", "Good"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_or_old_cache_files_fall_back_to_a_full_scan() {
        let root = temp("corrupt");
        let cache = root.with_extension("cache.bin");
        std::fs::write(root.join("song.wav"), tagged_wav(&root, "Title")).unwrap();
        let expected = scan(&root);
        assert_eq!(scan_with_cache(&root, Some(&cache), false), expected);

        // Truncated / corrupt body.
        std::fs::write(&cache, b"not a cache").unwrap();
        assert_eq!(scan_with_cache(&root, Some(&cache), false), expected);

        // A cache from an older format version.
        let mut data = std::fs::read(&cache).unwrap();
        data[8..12].copy_from_slice(&0u32.to_le_bytes());
        std::fs::write(&cache, &data).unwrap();
        assert_eq!(scan_with_cache(&root, Some(&cache), false), expected);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_interrupted_write_leaves_the_previous_cache_readable() {
        let root = temp("interrupted");
        let cache = root.with_extension("cache.bin");
        std::fs::write(root.join("song.wav"), tagged_wav(&root, "Title")).unwrap();
        let expected = scan(&root);
        assert_eq!(scan_with_cache(&root, Some(&cache), false), expected);
        // What a killed writer leaves behind: a half-written temp file next to the cache.
        let tmp = cache.with_file_name(format!(
            ".{}.{}.tmp",
            cache.file_name().unwrap().to_string_lossy(),
            999_999
        ));
        std::fs::write(&tmp, b"partial").unwrap();
        assert_eq!(scan_with_cache(&root, Some(&cache), false), expected);
        assert!(cache::ScanCache::load(&cache).is_some(), "the previous cache is still valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn each_server_and_root_gets_its_own_cache_file() {
        let a = Path::new("/music/a");
        let b = Path::new("/music/b");
        let one = crate::cache::cache_file(&listen_core::model::ServerId("one".into()), a).unwrap();
        let two = crate::cache::cache_file(&listen_core::model::ServerId("two".into()), a).unwrap();
        let other_root = crate::cache::cache_file(&listen_core::model::ServerId("one".into()), b).unwrap();
        assert_ne!(one, two);
        assert_ne!(one, other_root);
    }
}
