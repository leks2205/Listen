//! Binary cache of scanned files, one file per (server, root).
//!
//! The cache is an optimisation only: anything unreadable, corrupt or from another
//! format version is ignored and the folder is scanned again.

use crate::scan::Scanned;
use listen_core::model::ServerId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAGIC: &[u8; 8] = b"LSTNSCAN";
const VERSION: u32 = 1;
/// Bump when a field changes meaning; the version alone is not enough.
const SCHEMA: &str = "rel,mtime,len,title,artists,album,album_artist,track,disc,year,genres,codec,bit_rate,sample_rate,bit_depth,bpm,gain,duration,embedded_art";

/// An exact modification time: seconds since the epoch plus nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub secs: i64,
    pub nanos: u32,
}

pub(crate) fn stamp(time: SystemTime) -> Option<Stamp> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(d) => Some(Stamp { secs: i64::try_from(d.as_secs()).ok()?, nanos: d.subsec_nanos() }),
        Err(e) => {
            let d = e.duration();
            let secs = i64::try_from(d.as_secs()).ok()?;
            // Keep a canonical negative stamp: -1.25 s is secs -2, nanos 750_000_000.
            if d.subsec_nanos() == 0 {
                Some(Stamp { secs: -secs, nanos: 0 })
            } else {
                Some(Stamp { secs: -secs - 1, nanos: 1_000_000_000 - d.subsec_nanos() })
            }
        }
    }
}

pub(crate) fn system_time(stamp: Stamp) -> Option<SystemTime> {
    if stamp.nanos >= 1_000_000_000 {
        return None;
    }
    if stamp.secs >= 0 {
        return UNIX_EPOCH.checked_add(Duration::new(u64::try_from(stamp.secs).ok()?, stamp.nanos));
    }
    // Canonical negative stamp: -1.25 s is secs -2 + nanos 750_000_000.
    let secs = stamp.secs.unsigned_abs();
    UNIX_EPOCH.checked_sub(Duration::new(secs - 1, 1_000_000_000 - stamp.nanos))
}

/// One file: its stat key and what was read (or `None` for an unreadable file).
pub(crate) struct CacheEntry {
    pub mtime: Stamp,
    pub len: u64,
    pub scanned: Option<Scanned>,
}

/// The cached entries of one root, keyed by the file's path below the root.
pub(crate) struct ScanCache {
    entries: HashMap<String, CacheEntry>,
}

impl ScanCache {
    pub(crate) fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        Self::decode(&data)
    }

    fn decode(data: &[u8]) -> Option<Self> {
        let mut r = Reader { data, at: 0 };
        if r.bytes(8)? != MAGIC {
            return None;
        }
        if r.u32()? != VERSION || r.u64()? != crate::fnv1a(SCHEMA) {
            return None;
        }
        let body_len = usize::try_from(r.u64()?).ok()?;
        let checksum = r.u64()?;
        let body = r.rest();
        if body.len() != body_len || crate::fnv1a_bytes(body) != checksum {
            return None;
        }
        let mut body = Reader { data: body, at: 0 };
        let count = usize::try_from(body.u64()?).ok()?;
        // Each entry needs at least its type tag and a small key; this also stops
        // a corrupt count from making a huge `HashMap`.
        if count > body.data.len() {
            return None;
        }
        let mut entries = HashMap::with_capacity(count);
        for _ in 0..count {
            let rel = body.string()?;
            let mtime = Stamp { secs: body.i64()?, nanos: body.u32()? };
            let len = body.u64()?;
            let scanned = match body.u8()? {
                0 => None,
                1 => Some(body.scanned(&rel, mtime)?),
                _ => return None,
            };
            entries.insert(rel, CacheEntry { mtime, len, scanned });
        }
        if body.at != body.data.len() {
            return None;
        }
        Some(Self { entries })
    }

    pub(crate) fn remove(&mut self, rel: &str) -> Option<CacheEntry> {
        self.entries.remove(rel)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Serialises the current scan. `written` holds `(index into scanned, len)` for
/// the files that were read successfully (their mtime comes from `Scanned::modified`);
/// `unreadable` holds the stat keys of files whose tags could not be read.
pub(crate) fn encode(
    scanned: &[Scanned],
    written: &[(usize, u64)],
    unreadable: &[(String, Stamp, u64)],
) -> Vec<u8> {
    let mtime = |i: usize| scanned[i].modified.and_then(stamp);
    let count = written.iter().filter(|(i, _)| mtime(*i).is_some()).count() + unreadable.len();
    let mut body = Vec::with_capacity(64 * count);
    put_u64(&mut body, count as u64);
    for &(i, len) in written {
        let s = &scanned[i];
        let Some(mtime) = mtime(i) else { continue };
        put_string(&mut body, &s.rel);
        put_i64(&mut body, mtime.secs);
        put_u32(&mut body, mtime.nanos);
        put_u64(&mut body, len);
        put_u8(&mut body, 1);
        put_scanned(&mut body, s);
    }
    for (rel, mtime, len) in unreadable {
        put_string(&mut body, rel);
        put_i64(&mut body, mtime.secs);
        put_u32(&mut body, mtime.nanos);
        put_u64(&mut body, *len);
        put_u8(&mut body, 0);
    }

    let mut out = Vec::with_capacity(body.len() + 40);
    out.extend_from_slice(MAGIC);
    put_u32(&mut out, VERSION);
    put_u64(&mut out, crate::fnv1a(SCHEMA));
    put_u64(&mut out, body.len() as u64);
    put_u64(&mut out, crate::fnv1a_bytes(&body));
    out.extend_from_slice(&body);
    out
}

/// Writes next to the target and renames, so a reader sees the old file or the new one.
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let Some(dir) = path.parent() else { return Err(std::io::Error::other("cache path has no folder")) };
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("cache.bin");
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, data) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // std::fs::rename replaces the target on Unix and on Windows (MoveFileEx with
    // MOVEFILE_REPLACE_EXISTING), so there is no window without a cache file.
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Where this root's cache lives: the platform cache folder, one file per server and
/// canonical root path. `None` if no cache folder can be determined.
pub(crate) fn cache_file(server: &ServerId, root: &Path) -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "Listen")?;
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let name = format!(
        "library-{:016x}-{:016x}.bin",
        crate::fnv1a(server.0.as_str()),
        crate::fnv1a(&root.to_string_lossy())
    );
    Some(dirs.cache_dir().join("listen-local").join(name))
}

// ---- binary encoding -------------------------------------------------------

fn put_u8(out: &mut Vec<u8>, v: u8) {
    out.push(v);
}
fn put_bool(out: &mut Vec<u8>, v: bool) {
    out.push(u8::from(v));
}
fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_i64(out: &mut Vec<u8>, v: i64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_f32(out: &mut Vec<u8>, v: f32) {
    put_u32(out, v.to_bits());
}
fn put_string(out: &mut Vec<u8>, v: &str) {
    put_u32(out, v.len() as u32);
    out.extend_from_slice(v.as_bytes());
}
fn put_opt_string(out: &mut Vec<u8>, v: Option<&String>) {
    match v {
        Some(v) => {
            put_u8(out, 1);
            put_string(out, v);
        }
        None => put_u8(out, 0),
    }
}
fn put_strings(out: &mut Vec<u8>, v: &[String]) {
    put_u32(out, v.len() as u32);
    for s in v {
        put_string(out, s);
    }
}
fn put_opt_u32(out: &mut Vec<u8>, v: Option<u32>) {
    match v {
        Some(v) => {
            put_u8(out, 1);
            put_u32(out, v);
        }
        None => put_u8(out, 0),
    }
}
fn put_opt_i32(out: &mut Vec<u8>, v: Option<i32>) {
    match v {
        Some(v) => {
            put_u8(out, 1);
            put_i32(out, v);
        }
        None => put_u8(out, 0),
    }
}
fn put_opt_f32(out: &mut Vec<u8>, v: Option<f32>) {
    match v {
        Some(v) => {
            put_u8(out, 1);
            put_f32(out, v);
        }
        None => put_u8(out, 0),
    }
}

fn put_scanned(out: &mut Vec<u8>, s: &Scanned) {
    put_string(out, &s.title);
    put_strings(out, &s.artists);
    put_opt_string(out, s.album.as_ref());
    put_opt_string(out, s.album_artist.as_ref());
    put_opt_u32(out, s.track);
    put_opt_u32(out, s.disc);
    put_opt_i32(out, s.year);
    put_strings(out, &s.genres);
    put_opt_string(out, s.codec.as_ref());
    put_opt_u32(out, s.bit_rate);
    put_opt_u32(out, s.sample_rate);
    put_opt_u32(out, s.bit_depth);
    put_opt_u32(out, s.bpm);
    put_opt_f32(out, s.gain.track_gain);
    put_opt_f32(out, s.gain.track_peak);
    put_opt_f32(out, s.gain.album_gain);
    put_opt_f32(out, s.gain.album_peak);
    put_u64(out, s.duration.as_secs());
    put_u32(out, s.duration.subsec_nanos());
    put_bool(out, s.embedded_art);
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let out = self.data.get(self.at..end)?;
        self.at = end;
        Some(out)
    }
    fn rest(&mut self) -> &'a [u8] {
        let out = &self.data[self.at..];
        self.at = self.data.len();
        out
    }
    fn u8(&mut self) -> Option<u8> {
        Some(*self.bytes(1)?.first()?)
    }
    fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.bytes(8)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.bytes(8)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_bits(self.u32()?))
    }
    fn string(&mut self) -> Option<String> {
        let n = usize::try_from(self.u32()?).ok()?;
        String::from_utf8(self.bytes(n)?.to_vec()).ok()
    }
    fn opt_string(&mut self) -> Option<Option<String>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.string()?)),
            _ => None,
        }
    }
    fn strings(&mut self) -> Option<Vec<String>> {
        let n = usize::try_from(self.u32()?).ok()?;
        if n > self.data.len() {
            return None;
        }
        (0..n).map(|_| self.string()).collect()
    }
    fn opt_u32(&mut self) -> Option<Option<u32>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.u32()?)),
            _ => None,
        }
    }
    fn opt_i32(&mut self) -> Option<Option<i32>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.i32()?)),
            _ => None,
        }
    }
    fn opt_f32(&mut self) -> Option<Option<f32>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.f32()?)),
            _ => None,
        }
    }
    fn scanned(&mut self, rel: &str, mtime: Stamp) -> Option<Scanned> {
        Some(Scanned {
            rel: rel.to_string(),
            path: PathBuf::new(),
            modified: system_time(mtime),
            title: self.string()?,
            artists: self.strings()?,
            album: self.opt_string()?,
            album_artist: self.opt_string()?,
            track: self.opt_u32()?,
            disc: self.opt_u32()?,
            year: self.opt_i32()?,
            genres: self.strings()?,
            codec: self.opt_string()?,
            bit_rate: self.opt_u32()?,
            sample_rate: self.opt_u32()?,
            bit_depth: self.opt_u32()?,
            bpm: self.opt_u32()?,
            gain: listen_core::model::GainInfo {
                track_gain: self.opt_f32()?,
                track_peak: self.opt_f32()?,
                album_gain: self.opt_f32()?,
                album_peak: self.opt_f32()?,
            },
            duration: Duration::new(self.u64()?, self.u32()?),
            folder_art: None,
            embedded_art: self.bool()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_round_trip_across_the_epoch() {
        let times = [
            UNIX_EPOCH - Duration::new(1, 250_000_000),
            UNIX_EPOCH - Duration::new(1, 0),
            UNIX_EPOCH,
            UNIX_EPOCH + Duration::new(1, 250_000_000),
            UNIX_EPOCH + Duration::new(1_700_000_000, 999_999_999),
        ];
        for time in times {
            let stamp = stamp(time).expect("a stamp");
            assert_eq!(system_time(stamp), Some(time), "{time:?} -> {stamp:?}");
        }
        assert_eq!(
            stamp(UNIX_EPOCH - Duration::new(1, 250_000_000)),
            Some(Stamp { secs: -2, nanos: 750_000_000 })
        );
    }
}
