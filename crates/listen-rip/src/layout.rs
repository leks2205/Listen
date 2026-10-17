//! Rip file layout: `Album Artist/Album/NN Title.flac` as in MusicBee (multi-disc: `D-NN Title.flac`),
//! with names safe for SMB / Windows.

use crate::tags::{AlbumMeta, TrackMeta};
use std::path::{Path, PathBuf};

/// Longest name part in bytes (ext4/ZFS allow 255; leave room for "(2)").
const MAX_BYTES: usize = 180;

/// A file or folder name that works on Linux, SMB and Windows, and never
/// hides itself (no leading dot) or escapes its folder.
pub fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            '*' | '?' | '<' | '>' | '|' => '_',
            '"' => '\'',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    // No trailing dots/spaces (Windows), no leading dots (hidden) or spaces.
    out = out.trim_end_matches(['.', ' ']).trim_start_matches(['.', ' ']).to_string();
    if out.len() > MAX_BYTES {
        let mut cut = MAX_BYTES;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out = out.trim_end_matches(['.', ' ']).to_string();
    }
    if out.is_empty() { "_".into() } else { out }
}

/// `Album Artist/Album`, relative to the destination.
pub fn album_dir(meta: &AlbumMeta) -> PathBuf {
    PathBuf::from(sanitize(&meta.album_artist)).join(sanitize(&meta.album))
}

/// `07 Title.flac`, or `2-07 Title.flac` on multi-disc releases.
pub fn track_file(meta: &AlbumMeta, track: &TrackMeta) -> String {
    let n = if meta.disc_total > 1 {
        format!("{}-{:02}", meta.disc_number, track.number)
    } else {
        format!("{:02}", track.number)
    };
    sanitize(&format!("{n} {}.flac", track.title))
}

/// `rel` under `dest`, or `rel (2)`, `rel (3)` … if that folder exists:
/// existing albums are never merged into or overwritten.
pub fn free_dir(dest: &Path, rel: &Path) -> PathBuf {
    let first = dest.join(rel);
    if !first.exists() {
        return first;
    }
    let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = dest.join(rel.parent().unwrap_or(Path::new("")));
    (2..).map(|i| parent.join(format!("{name} ({i})"))).find(|p| !p.exists()).expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toc::Toc;

    fn meta(disc: u32, of: u32) -> AlbumMeta {
        let toc = Toc::from_offsets(1, 30_000, &[150, 10_000, 20_000]);
        let mut m = AlbumMeta::unknown(&toc, "x");
        m.album_artist = "IVE".into();
        m.album = "REVIVE+".into();
        m.disc_number = disc;
        m.disc_total = of;
        m.tracks[1].title = "BANG BANG".into();
        m
    }

    #[test]
    fn musicbee_layout() {
        let m = meta(1, 1);
        assert_eq!(album_dir(&m), PathBuf::from("IVE/REVIVE+"));
        assert_eq!(track_file(&m, &m.tracks[1]), "02 BANG BANG.flac");
        let m2 = meta(2, 2);
        assert_eq!(track_file(&m2, &m2.tracks[1]), "2-02 BANG BANG.flac");
    }

    #[test]
    fn sanitizes_for_smb() {
        assert_eq!(sanitize("AC/DC"), "AC-DC");
        assert_eq!(sanitize("What? Why: *now* <3 | \"yes\""), "What_ Why- _now_ _3 _ 'yes'");
        assert_eq!(sanitize("..hidden"), "hidden");
        assert_eq!(sanitize("Vol. 2..."), "Vol. 2");
        assert_eq!(sanitize("../../etc"), "-..-etc");
        assert_eq!(sanitize(""), "_");
        assert_eq!(sanitize("숨바꼭질 (Hush)"), "숨바꼭질 (Hush)");
        let long = "가".repeat(100); // 300 bytes
        let s = sanitize(&long);
        assert!(s.len() <= 180 && s.chars().all(|c| c == '가'));
    }

    #[test]
    fn never_reuses_an_existing_album_folder() {
        let dest = std::env::temp_dir().join(format!("listen-layout-{}", std::process::id()));
        let rel = PathBuf::from("IVE/REVIVE+");
        assert_eq!(free_dir(&dest, &rel), dest.join("IVE/REVIVE+"));
        std::fs::create_dir_all(dest.join("IVE/REVIVE+")).unwrap();
        assert_eq!(free_dir(&dest, &rel), dest.join("IVE/REVIVE+ (2)"));
        std::fs::create_dir_all(dest.join("IVE/REVIVE+ (2)")).unwrap();
        assert_eq!(free_dir(&dest, &rel), dest.join("IVE/REVIVE+ (3)"));
        std::fs::remove_dir_all(&dest).unwrap();
    }
}
