//! Reads and writes common audio tags with `lofty` (the local folder source's tag editor).
//! Writes go straight into the file; callers rescan afterwards.

use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, ItemKey, ItemValue, Tag, TagItem, TagType};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Separates several artists / genres in one text field.
pub const MULTI_SEPARATOR: &str = "; ";

/// A tag the editor can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TagField {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Track,
    Disc,
    Year,
    Genre,
    Comment,
    Lyrics,
}

impl TagField {
    pub const ALL: [TagField; 10] = [
        TagField::Title,
        TagField::Artist,
        TagField::Album,
        TagField::AlbumArtist,
        TagField::Track,
        TagField::Disc,
        TagField::Year,
        TagField::Genre,
        TagField::Comment,
        TagField::Lyrics,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TagField::Title => "Title",
            TagField::Artist => "Artist",
            TagField::Album => "Album",
            TagField::AlbumArtist => "Album artist",
            TagField::Track => "Track",
            TagField::Disc => "Disc",
            TagField::Year => "Year",
            TagField::Genre => "Genre",
            TagField::Comment => "Comment",
            TagField::Lyrics => "Lyrics",
        }
    }

    pub fn numeric(self) -> bool {
        matches!(self, TagField::Track | TagField::Disc | TagField::Year)
    }

    /// Several values, separated by [`MULTI_SEPARATOR`].
    pub fn multi(self) -> bool {
        matches!(self, TagField::Artist | TagField::AlbumArtist | TagField::Genre)
    }

    fn key(self) -> Option<ItemKey> {
        Some(match self {
            TagField::Artist => ItemKey::TrackArtist,
            TagField::AlbumArtist => ItemKey::AlbumArtist,
            TagField::Genre => ItemKey::Genre,
            TagField::Lyrics => ItemKey::Lyrics,
            _ => return None,
        })
    }
}

/// What the cover art of a file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverInfo {
    pub mime: String,
    pub bytes: usize,
}

/// The tags of one file (absent tags are missing from `fields`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileTags {
    pub fields: BTreeMap<TagField, String>,
    pub cover: Option<CoverInfo>,
}

impl FileTags {
    pub fn get(&self, field: TagField) -> &str {
        self.fields.get(&field).map_or("", String::as_str)
    }
}

/// What to do with the cover art.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CoverEdit {
    #[default]
    Keep,
    Remove,
    /// Replace the front cover with this image (PNG or JPEG).
    Set(Vec<u8>),
}

/// The changes to write: only the fields present are touched (an empty text removes the tag).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagEdit {
    pub fields: BTreeMap<TagField, String>,
    pub cover: CoverEdit,
}

impl TagEdit {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.cover == CoverEdit::Keep
    }
}

/// One line of the preview: `field` goes from `old` to `new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub field: String,
    pub old: String,
    pub new: String,
}

fn open(path: &Path) -> Result<TaggedFile, String> {
    lofty::read_from_path(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn joined(tag: &Tag, key: &ItemKey) -> String {
    tag.get_strings(key).map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(MULTI_SEPARATOR)
}

/// Reads the editable tags of `path`.
pub fn read(path: &Path) -> Result<FileTags, String> {
    let file = open(path)?;
    let mut out = FileTags::default();
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else { return Ok(out) };
    let mut put = |f: TagField, v: String| {
        if !v.is_empty() {
            out.fields.insert(f, v);
        }
    };
    put(TagField::Title, tag.title().map(|v| v.trim().to_string()).unwrap_or_default());
    let artist = joined(tag, &ItemKey::TrackArtist);
    put(
        TagField::Artist,
        if artist.is_empty() { tag.artist().map(|v| v.into_owned()).unwrap_or_default() } else { artist },
    );
    put(TagField::Album, tag.album().map(|v| v.trim().to_string()).unwrap_or_default());
    put(TagField::AlbumArtist, joined(tag, &ItemKey::AlbumArtist));
    put(TagField::Track, tag.track().map(|n| n.to_string()).unwrap_or_default());
    put(TagField::Disc, tag.disk().map(|n| n.to_string()).unwrap_or_default());
    put(TagField::Year, tag.year().map(|n| n.to_string()).unwrap_or_default());
    put(TagField::Genre, joined(tag, &ItemKey::Genre));
    put(TagField::Comment, tag.comment().map(|v| v.trim().to_string()).unwrap_or_default());
    put(TagField::Lyrics, tag.get_string(&ItemKey::Lyrics).unwrap_or_default().to_string());
    out.cover = tag.pictures().first().map(|p| CoverInfo {
        mime: p.mime_type().map_or_else(|| "image".to_string(), |m| m.to_string()),
        bytes: p.data().len(),
    });
    Ok(out)
}

/// Why `edit` can't be written, if so (a non-numeric number, a cover that isn't PNG or JPEG).
pub fn validate(edit: &TagEdit) -> Result<(), String> {
    for (field, text) in &edit.fields {
        if field.numeric() && !text.trim().is_empty() && parse_number(text).is_none() {
            return Err(format!("{} must be a number.", field.label()));
        }
    }
    if let CoverEdit::Set(data) = &edit.cover
        && image_mime(data).is_none()
    {
        return Err("The cover must be a PNG or JPEG image.".into());
    }
    Ok(())
}

/// "7" and "7/12" are 7.
fn parse_number(text: &str) -> Option<u32> {
    text.trim().split('/').next()?.trim().parse().ok()
}

fn image_mime(data: &[u8]) -> Option<MimeType> {
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(MimeType::Png)
    } else if data.starts_with(&[0xFF, 0xD8]) {
        Some(MimeType::Jpeg)
    } else {
        None
    }
}

fn normalised(field: TagField, text: &str) -> String {
    let text = text.trim();
    if field.numeric() {
        return parse_number(text).map(|n| n.to_string()).unwrap_or_default();
    }
    if field.multi() {
        return text
            .split(';')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(MULTI_SEPARATOR);
    }
    text.to_string()
}

/// The preview of writing `edit` over `before`: only what really changes.
pub fn changes(before: &FileTags, edit: &TagEdit) -> Vec<Change> {
    let mut out = Vec::new();
    for field in TagField::ALL {
        let Some(new) = edit.fields.get(&field) else { continue };
        let (old, new) = (before.get(field).to_string(), normalised(field, new));
        if old != new {
            out.push(Change { field: field.label().into(), old, new });
        }
    }
    let cover = |c: &Option<CoverInfo>| {
        c.as_ref().map_or_else(String::new, |c| format!("{} ({} KB)", c.mime, c.bytes.div_ceil(1024)))
    };
    match &edit.cover {
        CoverEdit::Keep => {}
        CoverEdit::Remove if before.cover.is_some() => {
            out.push(Change { field: "Cover".into(), old: cover(&before.cover), new: String::new() })
        }
        CoverEdit::Remove => {}
        CoverEdit::Set(data) => out.push(Change {
            field: "Cover".into(),
            old: cover(&before.cover),
            new: format!("new image ({} KB)", data.len().div_ceil(1024)),
        }),
    }
    out
}

/// Writes `edit` into the file at `path`.
pub fn write(path: &Path, edit: &TagEdit) -> Result<(), String> {
    validate(edit)?;
    if edit.is_empty() {
        return Ok(());
    }
    let mut file = open(path)?;
    if file.primary_tag().is_none() && file.first_tag().is_none() {
        let tag_type = file.primary_tag_type();
        file.insert_tag(Tag::new(tag_type));
    }
    let tag = match file.primary_tag_mut() {
        Some(t) => t,
        None => file.first_tag_mut().ok_or_else(|| format!("{}: no tag to write", path.display()))?,
    };
    for (field, text) in &edit.fields {
        let text = normalised(*field, text);
        apply(tag, *field, &text);
    }
    match &edit.cover {
        CoverEdit::Keep => {}
        CoverEdit::Remove => {
            tag.remove_picture_type(PictureType::CoverFront);
            tag.remove_picture_type(PictureType::Other);
        }
        CoverEdit::Set(data) => {
            tag.remove_picture_type(PictureType::CoverFront);
            tag.push_picture(Picture::new_unchecked(
                PictureType::CoverFront,
                image_mime(data),
                None,
                data.clone(),
            ));
        }
    }
    file.save_to_path(path, WriteOptions::default()).map_err(|e| format!("{}: {e}", path.display()))
}

fn apply(tag: &mut Tag, field: TagField, text: &str) {
    let number = || text.parse::<u32>().ok();
    match field {
        TagField::Title if text.is_empty() => tag.remove_title(),
        TagField::Title => tag.set_title(text.to_string()),
        TagField::Album if text.is_empty() => tag.remove_album(),
        TagField::Album => tag.set_album(text.to_string()),
        TagField::Comment if text.is_empty() => tag.remove_comment(),
        TagField::Comment => tag.set_comment(text.to_string()),
        TagField::Track => match number() {
            Some(n) => tag.set_track(n),
            None => tag.remove_track(),
        },
        TagField::Disc => match number() {
            Some(n) => tag.set_disk(n),
            None => tag.remove_disk(),
        },
        TagField::Year => match number() {
            Some(n) => tag.set_year(n),
            None => tag.remove_year(),
        },
        TagField::Artist | TagField::AlbumArtist | TagField::Genre | TagField::Lyrics => {
            let key = field.key().expect("keyed field");
            if field == TagField::Artist {
                tag.remove_artist();
            }
            tag.remove_key(&key);
            if field.multi() && tag.tag_type() == TagType::Id3v2 {
                // One frame with NUL-separated values (ID3v2.4); separate items would overwrite each other.
                tag.insert_text(key, text.replace(MULTI_SEPARATOR, "\0"));
            } else if field.multi() {
                for part in text.split(MULTI_SEPARATOR).filter(|p| !p.is_empty()) {
                    tag.push_unchecked(TagItem::new(key.clone(), ItemValue::Text(part.to_string())));
                }
            } else if !text.is_empty() {
                tag.insert_text(key, text.to_string());
            }
        }
    }
}

/// "2026-10-16T12:30:00Z" for a file time.
pub fn iso_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970 (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::path::{Path, PathBuf};

    /// A short silent MP3 (MPEG-1 layer III, 128 kbit/s, 44.1 kHz stereo).
    pub fn mp3() -> Vec<u8> {
        let mut out = Vec::new();
        for _ in 0..12 {
            out.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
            out.extend(std::iter::repeat_n(0u8, 413));
        }
        out
    }

    /// A FLAC file with a STREAMINFO and a padding block (44.1 kHz, 16 bit, stereo, 1 s).
    pub fn flac() -> Vec<u8> {
        let mut out = b"fLaC".to_vec();
        out.extend_from_slice(&[0x00, 0, 0, 34]); // STREAMINFO, 34 bytes
        out.extend_from_slice(&[0x10, 0x00, 0x10, 0x00]); // min / max block size 4096
        out.extend_from_slice(&[0; 6]); // min / max frame size
        // 44100 Hz (20 bits), 2 channels - 1 (3 bits), 16 bits - 1 (5 bits), 44100 samples (36 bits)
        let packed: u64 = (44_100u64 << 44) | (1u64 << 41) | (15u64 << 36) | 44_100u64;
        out.extend_from_slice(&packed.to_be_bytes());
        out.extend_from_slice(&[0; 16]); // md5
        // Encoders end the metadata with a padding block (type 1, last).
        out.extend_from_slice(&[0x81, 0, 0x04, 0]);
        out.extend(std::iter::repeat_n(0u8, 1024));
        // Stand-in for the audio frames (lofty only edits the metadata in front of them).
        out.extend(std::iter::repeat_n(0xAAu8, 256));
        out
    }

    /// A tiny valid PNG (1x1).
    pub fn png() -> Vec<u8> {
        const PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52, 0, 0, 0,
            1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 0x0D, 0x49, 0x44, 0x41, 0x54,
            0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0, 0x05, 0, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D,
            0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44,
        ];
        PNG.to_vec()
    }

    /// Writes `bytes` to a new file named `name` in a fresh temp directory.
    pub fn temp_file(name: &str, bytes: &[u8]) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "listen-tags-{}-{}",
            std::process::id(),
            name.replace('.', "-")
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    pub fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    fn edit(pairs: &[(TagField, &str)]) -> TagEdit {
        TagEdit { fields: pairs.iter().map(|(f, v)| (*f, v.to_string())).collect(), cover: CoverEdit::Keep }
    }

    fn round_trip(name: &str, bytes: Vec<u8>) {
        let (dir, path) = temp_file(name, &bytes);
        let before = read(&path).unwrap();
        assert!(before.fields.is_empty(), "{before:?}");

        let e = TagEdit {
            fields: [
                (TagField::Title, "Hello"),
                (TagField::Artist, "Ann; Bob"),
                (TagField::Album, "Greatest"),
                (TagField::AlbumArtist, "Ann"),
                (TagField::Track, "3/12"),
                (TagField::Disc, "2"),
                (TagField::Year, "1999"),
                (TagField::Genre, "Rock; Pop"),
                (TagField::Comment, "a note"),
                (TagField::Lyrics, "la la\nla"),
            ]
            .into_iter()
            .map(|(f, v)| (f, v.to_string()))
            .collect(),
            cover: CoverEdit::Set(png()),
        };
        let preview = changes(&before, &e);
        assert_eq!(preview.len(), 11, "{preview:?}");
        write(&path, &e).unwrap();

        let after = read(&path).unwrap();
        assert_eq!(after.get(TagField::Title), "Hello");
        assert_eq!(after.get(TagField::Artist), "Ann; Bob");
        assert_eq!(after.get(TagField::Album), "Greatest");
        assert_eq!(after.get(TagField::AlbumArtist), "Ann");
        assert_eq!(after.get(TagField::Track), "3");
        assert_eq!(after.get(TagField::Disc), "2");
        assert_eq!(after.get(TagField::Year), "1999");
        assert_eq!(after.get(TagField::Genre), "Rock; Pop");
        assert_eq!(after.get(TagField::Comment), "a note");
        assert_eq!(after.get(TagField::Lyrics), "la la\nla");
        assert_eq!(after.cover.as_ref().map(|c| c.bytes), Some(png().len()));
        // Writing the same again changes nothing.
        assert!(changes(&after, &e).iter().all(|c| c.field == "Cover"), "{:?}", changes(&after, &e));

        // Partial edit: only the title; clear the genre; drop the cover.
        let e2 =
            TagEdit { cover: CoverEdit::Remove, ..edit(&[(TagField::Title, "Bye"), (TagField::Genre, "")]) };
        write(&path, &e2).unwrap();
        let after = read(&path).unwrap();
        assert_eq!(after.get(TagField::Title), "Bye");
        assert_eq!(after.get(TagField::Genre), "");
        assert_eq!(after.get(TagField::Album), "Greatest");
        assert!(after.cover.is_none());
        cleanup(&dir);
    }

    #[test]
    fn round_trips_on_a_generated_mp3() {
        round_trip("t.mp3", mp3());
    }

    #[test]
    fn round_trips_on_a_generated_flac() {
        round_trip("t.flac", flac());
    }

    #[test]
    fn the_preview_lists_only_real_changes() {
        let mut before = FileTags::default();
        before.fields.insert(TagField::Title, "Same".into());
        before.fields.insert(TagField::Year, "2000".into());
        let c = changes(
            &before,
            &edit(&[(TagField::Title, " Same "), (TagField::Year, "2001"), (TagField::Genre, "a ;b")]),
        );
        assert_eq!(
            c,
            [
                Change { field: "Year".into(), old: "2000".into(), new: "2001".into() },
                Change { field: "Genre".into(), old: String::new(), new: "a; b".into() },
            ]
        );
        assert!(changes(&before, &TagEdit::default()).is_empty());
    }

    #[test]
    fn rejects_bad_numbers_and_covers_before_touching_the_file() {
        let (dir, path) = temp_file("bad.mp3", &mp3());
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(
            write(&path, &edit(&[(TagField::Year, "soon")])).unwrap_err().contains("Year must be a number")
        );
        let bad_cover = TagEdit { cover: CoverEdit::Set(b"not an image".to_vec()), ..Default::default() };
        assert!(write(&path, &bad_cover).unwrap_err().contains("PNG or JPEG"));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), size);
        cleanup(&dir);
    }

    #[test]
    fn formats_iso_times() {
        assert_eq!(iso_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(UNIX_EPOCH + Duration::from_secs(1_760_617_800)), "2025-10-16T12:30:00Z");
    }
}
