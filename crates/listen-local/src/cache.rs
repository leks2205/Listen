//! Binary cache of scanned files, one file per (server, root).
//!
//! The cache is an optimisation only: anything unreadable, corrupt or from another
//! format version is ignored and the folder is scanned again.

use crate::scan::Scanned;
use listen_core::model::ServerId;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAGIC: &[u8; 8] = b"LSTNSCAN";
const VERSION: u32 = 1;
/// Bump when a field changes meaning; the version alone is not enough.
const SCHEMA: &str = "rel,mtime,len,title,artists,album,album_artist,track,disc,year,genres,codec,bit_rate,sample_rate,bit_depth,bpm,gain,duration,embedded_art";
/// Upper bound for the capacity reserved from a cache-file count. Corrupt or
/// hostile counts must not turn into a large allocation before the entries fail.
const MAX_PREALLOC: usize = 65_536;
/// Longest plausible track duration (about 115 days); also keeps the sum in
/// `Index::build` far away from `Duration`'s overflow panic.
const MAX_DURATION_SECS: u64 = 10_000_000;

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
        // An entry is at least: key length + mtime (8+4) + length (8) + type tag (1),
        // i.e. 25 bytes with an empty key. Checking this before allocating stops a
        // corrupt count from reserving a huge `HashMap`.
        if count > body.data.len() / 25 {
            return None;
        }
        let mut entries = HashMap::with_capacity(count.min(MAX_PREALLOC));
        for _ in 0..count {
            let rel = body.string()?;
            let mtime = Stamp { secs: body.i64()?, nanos: body.u32()? };
            if mtime.nanos >= 1_000_000_000 {
                return None;
            }
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

/// Removes temp files a killed writer left behind. Only files matching our own
/// `.library-*.tmp` pattern and older than an hour are touched, so another
/// process's fresh temp file is never deleted.
fn sweep_stale_tmp(dir: &Path) {
    const STALE: Duration = Duration::from_secs(60 * 60);
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let Some(cutoff) = SystemTime::now().checked_sub(STALE) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !(name.starts_with(".library-") && name.ends_with(".tmp")) {
            continue;
        }
        let stale = entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < cutoff);
        if stale && let Err(e) = std::fs::remove_file(entry.path()) {
            tracing::debug!("could not remove stale cache temp {}: {e}", entry.path().display());
        }
    }
}

/// Writes next to the target and renames, so a reader sees the old file or the new one.
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let Some(dir) = path.parent() else { return Err(std::io::Error::other("cache path has no folder")) };
    std::fs::create_dir_all(dir)?;
    sweep_stale_tmp(dir);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("cache.bin");
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(data)?;
        // Flush the data to disk before the rename, so a crash cannot leave a
        // renamed file with unflushed contents.
        file.sync_all()
    };
    if let Err(e) = write() {
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

/// The file name for a server and root; separate from the platform cache folder so
/// it can be tested without `ProjectDirs` (and without a real home directory).
pub(crate) fn cache_name(server: &ServerId, root: &Path) -> String {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    format!(
        "library-{:016x}-{:016x}.bin",
        crate::fnv1a(server.0.as_str()),
        crate::fnv1a(&root.to_string_lossy())
    )
}

/// Where this root's cache lives: the platform cache folder, one file per server and
/// canonical root path. `None` if no cache folder can be determined.
pub(crate) fn cache_file(server: &ServerId, root: &Path) -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "Listen")?;
    Some(dirs.cache_dir().join("listen-local").join(cache_name(server, root)))
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
        // Each string needs at least its 4-byte length; this bounds the Vec.
        if n > self.data.len() / 4 {
            return None;
        }
        let mut out = Vec::with_capacity(n.min(MAX_PREALLOC));
        for _ in 0..n {
            out.push(self.string()?);
        }
        Some(out)
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
            duration: {
                let secs = self.u64()?;
                let nanos = self.u32()?;
                // `Duration::new` panics on nanos >= 1e9; a huge sec value would
                // also overflow the `Duration` sum over all tracks in `Index::build`.
                if nanos >= 1_000_000_000 || secs > MAX_DURATION_SECS {
                    return None;
                }
                Duration::new(secs, nanos)
            },
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

    fn sample_cache() -> Vec<u8> {
        let scanned = vec![Scanned {
            rel: "a.wav".into(),
            path: PathBuf::new(),
            modified: Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
            title: "Title".into(),
            artists: vec!["Artist".into()],
            album: Some("Album".into()),
            album_artist: Some("Artist".into()),
            track: Some(1),
            disc: Some(1),
            year: Some(2020),
            genres: vec!["Rock".into()],
            codec: Some("flac".into()),
            bit_rate: Some(900_000),
            sample_rate: Some(44_100),
            bit_depth: Some(16),
            bpm: Some(120),
            gain: Default::default(),
            duration: Duration::from_secs(100),
            folder_art: None,
            embedded_art: true,
        }];
        encode(&scanned, &[(0, 1234)], &[("bad.wav".into(), Stamp { secs: 1, nanos: 0 }, 77)])
    }

    /// A cache file around an arbitrary body, with a matching checksum (so the
    /// decoder's own bounds checks are what gets exercised).
    fn wrapped(body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        put_u32(&mut out, VERSION);
        put_u64(&mut out, crate::fnv1a(SCHEMA));
        put_u64(&mut out, body.len() as u64);
        put_u64(&mut out, crate::fnv1a_bytes(body));
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn a_valid_cache_decodes() {
        assert!(ScanCache::decode(&sample_cache()).is_some());
    }

    #[test]
    fn decode_rejects_every_truncation_and_flipped_byte() {
        let data = sample_cache();
        for len in 0..data.len() {
            assert!(ScanCache::decode(&data[..len]).is_none(), "truncated at {len}");
        }
        for i in 0..data.len() {
            let mut flipped = data.clone();
            flipped[i] ^= 0x40;
            assert!(ScanCache::decode(&flipped).is_none(), "flipped byte {i}");
        }
        // Explicit body-checksum mismatch: a body byte without a new checksum.
        let mut body_flip = data.clone();
        body_flip[36] ^= 0x01;
        assert!(ScanCache::decode(&body_flip).is_none(), "body checksum mismatch");
    }

    #[test]
    fn decode_rejects_a_huge_count_before_allocating() {
        // The count needs at least 25 body bytes per entry.
        let mut body = Vec::new();
        put_u64(&mut body, u64::MAX);
        assert!(ScanCache::decode(&wrapped(&body)).is_none());

        // Two entries need at least 50 body bytes, but only 25 are there.
        let mut body = vec![0u8; 8 + 25];
        body[..8].copy_from_slice(&2u64.to_le_bytes());
        assert!(ScanCache::decode(&wrapped(&body)).is_none());
    }

    #[test]
    fn decode_rejects_out_of_range_string_lengths() {
        // A relative path whose length would read past the body.
        let mut body = Vec::new();
        put_u64(&mut body, 1);
        put_u32(&mut body, u32::MAX);
        assert!(ScanCache::decode(&wrapped(&body)).is_none());

        // An artist list with a count that would reserve a huge Vec.
        let mut body = Vec::new();
        put_u64(&mut body, 1);
        put_string(&mut body, "a");
        put_i64(&mut body, 0);
        put_u32(&mut body, 0);
        put_u64(&mut body, 0);
        put_u8(&mut body, 1);
        put_string(&mut body, "title");
        put_u32(&mut body, u32::MAX);
        assert!(ScanCache::decode(&wrapped(&body)).is_none());
    }

    fn duration_body(secs: u64, nanos: u32) -> Vec<u8> {
        let mut body = Vec::new();
        put_u64(&mut body, 1); // count
        put_string(&mut body, "a");
        put_i64(&mut body, 0);
        put_u32(&mut body, 0);
        put_u64(&mut body, 0);
        put_u8(&mut body, 1); // positive entry
        put_string(&mut body, "title");
        put_u32(&mut body, 0); // artists
        put_u8(&mut body, 0); // album
        put_u8(&mut body, 0); // album_artist
        put_u8(&mut body, 0); // track
        put_u8(&mut body, 0); // disc
        put_u8(&mut body, 0); // year
        put_u32(&mut body, 0); // genres
        put_u8(&mut body, 0); // codec
        put_u8(&mut body, 0); // bit_rate
        put_u8(&mut body, 0); // sample_rate
        put_u8(&mut body, 0); // bit_depth
        put_u8(&mut body, 0); // bpm
        for _ in 0..4 {
            put_u8(&mut body, 0); // gain options
        }
        put_u64(&mut body, secs);
        put_u32(&mut body, nanos);
        put_u8(&mut body, 0); // embedded_art
        body
    }

    #[test]
    fn decode_rejects_duration_values_that_would_panic_or_overflow() {
        // `Duration::new(0, 1_000_000_000)` would panic.
        assert!(ScanCache::decode(&wrapped(&duration_body(0, 1_000_000_000))).is_none());
        // A huge sec value would overflow the sum over all tracks in `Index::build`.
        assert!(ScanCache::decode(&wrapped(&duration_body(MAX_DURATION_SECS + 1, 0))).is_none());
        // The largest accepted value still decodes.
        assert!(ScanCache::decode(&wrapped(&duration_body(MAX_DURATION_SECS, 999_999_999))).is_some());
    }
}
