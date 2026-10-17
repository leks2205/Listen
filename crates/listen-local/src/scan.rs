//! Walks a folder and reads tags, audio properties and cover-art hints.

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
#[derive(Debug, Clone, Default)]
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
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| list.contains(&e.to_ascii_lowercase().as_str()))
}

/// Reads every audio file below `root`; unreadable files are skipped.
pub fn scan(root: &Path) -> Vec<Scanned> {
    let mut art_cache: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    let mut out = Vec::new();
    for entry in WalkDir::new(root).follow_links(true).sort_by_file_name().into_iter().filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() || !is_audio(entry.path()) {
            continue;
        }
        let path = entry.path().to_path_buf();
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let art = art_cache.entry(dir.clone()).or_insert_with(|| find_folder_art(&dir)).clone();
        match read_file(root, &path) {
            Ok(mut s) => {
                s.folder_art = art;
                out.push(s);
            }
            Err(e) => tracing::debug!("skipping {}: {e}", path.display()),
        }
    }
    out
}

fn find_folder_art(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(usize, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !is_image(&path) {
            continue;
        }
        let stem = path.file_stem()?.to_string_lossy().to_ascii_lowercase();
        if let Some(rank) = COVER_NAMES.iter().position(|n| *n == stem)
            && best.as_ref().is_none_or(|(r, _)| rank < *r)
        {
            best = Some((rank, path));
        }
    }
    best.map(|(_, p)| p)
}

fn read_file(root: &Path, path: &Path) -> Result<Scanned, String> {
    let tagged = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let props = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut s = Scanned {
        rel: path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned(),
        path: path.to_path_buf(),
        modified: std::fs::metadata(path).and_then(|m| m.modified()).ok(),
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
}
