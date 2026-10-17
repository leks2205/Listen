//! Analysis records shared between machines through a synced folder (Nextcloud):
//! `<dir>/v1/<machine-id>/NNNNNN.jsonl`, one [`TrackAnalysis`] JSON per line. Each machine appends only to
//! its own directory and rotated chunks are immutable, so no file has two writers. Readers skip torn tails,
//! re-read shrunk files and read "conflicted copy" files; merging is deterministic
//! ([`TrackAnalysis::better_than`]).

use listen_core::analysis::{FileId, MIN_COMPATIBLE_VERSION, TrackAnalysis};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Layout version (directory name).
const LAYOUT: &str = "v1";
/// Buffered records are written at least this often…
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
/// …or once this many are pending (fewer, larger writes: less sync churn).
pub const FLUSH_RECORDS: usize = 50;
/// A chunk is closed (and becomes immutable) past either limit.
pub const ROTATE_BYTES: u64 = 4 * 1024 * 1024;
pub const ROTATE_RECORDS: usize = 2000;

#[derive(Debug, Default, Clone, Copy)]
struct FileState {
    /// Bytes consumed (up to the last complete line).
    offset: u64,
}

#[derive(Debug)]
struct Writer {
    number: u32,
    bytes: u64,
    records: usize,
    pending: Vec<String>,
    last_flush: Instant,
}

/// The merged view of all machines' records plus this machine's writer.
#[derive(Debug)]
pub struct ShardStore {
    root: PathBuf,
    machine: String,
    writer: Writer,
    files: HashMap<PathBuf, FileState>,
    /// song id → best record per distinct file.
    index: HashMap<String, Vec<Arc<TrackAnalysis>>>,
    /// Records per machine as read (for the settings page).
    per_machine: HashMap<String, usize>,
}

fn chunk_number(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".jsonl")?;
    stem.parse().ok()
}

impl ShardStore {
    /// Opens (creating) `<dir>/v1/<machine>` and reads every machine's records.
    pub fn open(dir: &Path, machine: &str) -> io::Result<Self> {
        let root = dir.join(LAYOUT);
        let own = root.join(machine);
        fs::create_dir_all(&own)?;
        // Continue the newest own chunk if it is still open.
        let last = fs::read_dir(&own)?
            .filter_map(|e| e.ok())
            .filter_map(|e| chunk_number(&e.path()).map(|n| (n, e.path())))
            .max_by_key(|(n, _)| *n);
        let mut writer =
            Writer { number: 1, bytes: 0, records: 0, pending: Vec::new(), last_flush: Instant::now() };
        if let Some((n, path)) = last {
            let text = fs::read(&path)?;
            let records = text.iter().filter(|&&b| b == b'\n').count();
            writer.number = n;
            writer.bytes = text.len() as u64;
            writer.records = records;
            if writer.bytes >= ROTATE_BYTES || records >= ROTATE_RECORDS {
                writer = Writer { number: n + 1, bytes: 0, records: 0, ..writer };
            }
        }
        let mut store = ShardStore {
            root,
            machine: machine.to_string(),
            writer,
            files: HashMap::new(),
            index: HashMap::new(),
            per_machine: HashMap::new(),
        };
        store.refresh()?;
        Ok(store)
    }

    pub fn machine(&self) -> &str {
        &self.machine
    }

    fn own_dir(&self) -> PathBuf {
        self.root.join(&self.machine)
    }

    fn chunk_path(&self, number: u32) -> PathBuf {
        self.own_dir().join(format!("{number:06}.jsonl"))
    }

    /// Adds a record (visible to [`Self::lookup`] at once; written by the
    /// next due flush).
    pub fn append(&mut self, record: TrackAnalysis) -> io::Result<()> {
        let line = serde_json::to_string(&record).map_err(io::Error::other)?;
        self.insert(record);
        self.writer.pending.push(line);
        self.flush_if_due()
    }

    /// Writes pending records when 50 are waiting or 30 s have passed.
    /// Call it from a timer too, so a quiet period still gets written.
    pub fn flush_if_due(&mut self) -> io::Result<()> {
        if self.writer.pending.len() >= FLUSH_RECORDS
            || (!self.writer.pending.is_empty() && self.writer.last_flush.elapsed() >= FLUSH_INTERVAL)
        {
            self.flush()
        } else {
            Ok(())
        }
    }

    /// Writes all pending records (one `write` per chunk, so a reader sees
    /// whole lines or a torn last line, never interleaved garbage).
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.last_flush = Instant::now();
        let mut pending = std::mem::take(&mut self.writer.pending).into_iter().peekable();
        while pending.peek().is_some() {
            let path = self.chunk_path(self.writer.number);
            let mut block = String::new();
            while let Some(line) = pending.peek() {
                if !block.is_empty()
                    && (self.writer.bytes + (block.len() + line.len() + 1) as u64 > ROTATE_BYTES
                        || self.writer.records >= ROTATE_RECORDS)
                {
                    break;
                }
                block.push_str(line);
                block.push('\n');
                self.writer.records += 1;
                pending.next();
            }
            let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
            file.write_all(block.as_bytes())?;
            file.flush()?;
            self.writer.bytes += block.len() as u64;
            // Written by this process: the reader needn't parse them.
            let len = file.metadata()?.len();
            let state = self.files.entry(path).or_default();
            if state.offset + block.len() as u64 == len {
                state.offset = len;
            }
            *self.per_machine.entry(self.machine.clone()).or_default() += block.lines().count();
            if self.writer.bytes >= ROTATE_BYTES || self.writer.records >= ROTATE_RECORDS {
                self.writer.number += 1;
                self.writer.bytes = 0;
                self.writer.records = 0;
            }
        }
        Ok(())
    }

    /// Reads new data from every machine's chunks. Returns the number of
    /// records read.
    pub fn refresh(&mut self) -> io::Result<usize> {
        let mut read = 0;
        let Ok(machines) = fs::read_dir(&self.root) else {
            return Ok(0);
        };
        for m in machines.filter_map(|e| e.ok()) {
            if !m.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Ok(files) = fs::read_dir(m.path()) else { continue };
            let mut paths: Vec<PathBuf> = files
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
                .collect();
            paths.sort();
            for path in paths {
                match self.read_new(&path) {
                    Ok(n) => read += n,
                    Err(e) => tracing::warn!("analysis shard {}: {e}", path.display()),
                }
            }
        }
        Ok(read)
    }

    fn read_new(&mut self, path: &Path) -> io::Result<usize> {
        let mut file = fs::File::open(path)?;
        let len = file.metadata()?.len();
        let state = self.files.entry(path.to_path_buf()).or_default();
        if len < state.offset {
            // Replaced by a shorter version (sync rollback): read it again.
            state.offset = 0;
        }
        if len == state.offset {
            return Ok(0);
        }
        file.seek(SeekFrom::Start(state.offset))?;
        let mut buf = Vec::with_capacity((len - state.offset) as usize);
        file.take(len - state.offset).read_to_end(&mut buf)?;
        // Only complete lines; a torn tail is retried next time.
        let Some(end) = buf.iter().rposition(|&b| b == b'\n') else {
            return Ok(0);
        };
        state.offset += end as u64 + 1;
        let mut n = 0;
        for line in buf[..end].split(|&b| b == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<TrackAnalysis>(line) {
                Ok(rec) => {
                    *self.per_machine.entry(rec.machine.clone()).or_default() += 1;
                    self.insert(rec);
                    n += 1;
                }
                // Unknown future formats and damaged lines are skipped.
                Err(e) => tracing::debug!("analysis shard {}: bad line: {e}", path.display()),
            }
        }
        Ok(n)
    }

    /// Keeps the best record per (song, file).
    fn insert(&mut self, rec: TrackAnalysis) {
        if rec.version < MIN_COMPATIBLE_VERSION {
            return;
        }
        let list = self.index.entry(rec.song_id.clone()).or_default();
        match list.iter_mut().find(|r| r.file.matches(&rec.file)) {
            Some(existing) => {
                if rec.better_than(existing) {
                    *existing = Arc::new(rec);
                }
            }
            None => list.push(Arc::new(rec)),
        }
    }

    /// The best compatible record for this song and file.
    pub fn lookup(&self, song_id: &str, file: &FileId) -> Option<&Arc<TrackAnalysis>> {
        self.index
            .get(song_id)?
            .iter()
            .filter(|r| r.file.matches(file) && r.version >= MIN_COMPATIBLE_VERSION)
            .reduce(|best, r| if r.better_than(best) { r } else { best })
    }

    /// Songs with at least one record.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Records read or written per machine, sorted by name.
    pub fn machines(&self) -> Vec<(String, usize)> {
        let mut v: Vec<_> = self.per_machine.iter().map(|(k, v)| (k.clone(), *v)).collect();
        v.sort();
        v
    }
}

impl Drop for ShardStore {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            tracing::warn!("analysis store: final flush failed: {e}");
        }
    }
}

/// This machine's shard name: sanitised hostname plus a short hash of
/// `/etc/machine-id` (two machines may share a hostname), e.g. `fedora-3fa2c1`.
pub fn machine_id() -> String {
    let host = fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "host".into());
    let id = fs::read_to_string("/etc/machine-id").unwrap_or_else(|_| host.clone());
    machine_id_from(&host, id.trim())
}

fn machine_id_from(host: &str, id: &str) -> String {
    let mut name: String = host
        .split('.')
        .next()
        .unwrap_or("")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .take(24)
        .collect();
    if name.trim_matches('-').is_empty() {
        name = "host".into();
    }
    let hash = Sha256::digest(id.as_bytes());
    format!("{}-{:02x}{:02x}{:02x}", name.trim_matches('-'), hash[0], hash[1], hash[2])
}

/// The shared analysis folder: the first Nextcloud sync folder from
/// `~/.config/Nextcloud/nextcloud.cfg` + `Listen/analysis`, else
/// `~/Nextcloud/Listen/analysis` when that folder exists, else `fallback`.
pub fn detect_shared_dir(home: &Path, fallback: &Path) -> PathBuf {
    let mut cfgs = vec![home.join(".config/Nextcloud/nextcloud.cfg")];
    // Windows: %APPDATA%\Nextcloud\nextcloud.cfg
    cfgs.extend(std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Nextcloud/nextcloud.cfg")));
    if let Some(text) = cfgs.iter().find_map(|c| fs::read_to_string(c).ok())
        && let Some(local) = text.lines().find_map(|l| {
            let (key, value) = l.split_once('=')?;
            key.trim().ends_with("localPath").then(|| value.trim().to_string())
        })
        && !local.is_empty()
    {
        return PathBuf::from(local).join("Listen/analysis");
    }
    let nc = home.join("Nextcloud");
    if nc.is_dir() {
        return nc.join("Listen/analysis");
    }
    fallback.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::analysis::{BarCurves, Features, Mood, Outcome};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!("listen-store-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn rec(song: &str, machine: &str, at: u64, ok: bool) -> TrackAnalysis {
        TrackAnalysis {
            version: 1,
            analyzed_at: at,
            machine: machine.into(),
            decoder: "mpv 0.41.0".into(),
            song_id: song.into(),
            server_hint: "musik".into(),
            file: FileId { duration_ms: 200_000, suffix: Some("flac".into()), format: None, bytes: None },
            title_hint: "A – B".into(),
            outcome: if ok {
                Outcome::Ok(Box::new(Features {
                    audio_start: 0.0,
                    audio_end: 200.0,
                    grid: None,
                    key: None,
                    lufs: -9.0,
                    lra: 4.0,
                    bars: BarCurves::default(),
                    phrase_bars: 8,
                    phrase_offset: 0,
                    sections: vec![],
                    intro_instrumental_bars: 0,
                    outro_instrumental_bars: 0,
                    timbre: vec![],
                    mood: Mood::default(),
                }))
            } else {
                Outcome::Failed { reason: "x".into(), transient: false }
            },
        }
    }

    fn file() -> FileId {
        FileId { duration_ms: 200_400, suffix: Some("flac".into()), format: None, bytes: None }
    }

    #[test]
    fn two_machines_read_each_other() {
        let dir = TempDir::new("two");
        let mut a = ShardStore::open(&dir.0, "desk-aaaaaa").unwrap();
        let mut b = ShardStore::open(&dir.0, "lap-bbbbbb").unwrap();
        a.append(rec("s1", "desk-aaaaaa", 10, true)).unwrap();
        b.append(rec("s2", "lap-bbbbbb", 11, true)).unwrap();
        assert!(a.lookup("s1", &file()).is_some(), "own records at once");
        assert!(a.lookup("s2", &file()).is_none(), "not flushed yet");
        a.flush().unwrap();
        b.flush().unwrap();
        a.refresh().unwrap();
        b.refresh().unwrap();
        assert!(a.lookup("s2", &file()).is_some());
        assert!(b.lookup("s1", &file()).is_some());
        // Own writes aren't read back twice.
        assert_eq!(a.machines(), [("desk-aaaaaa".into(), 1), ("lap-bbbbbb".into(), 1)]);
        // Other files and suffixes don't match.
        assert!(
            a.lookup("s1", &FileId { duration_ms: 100_000, suffix: None, format: None, bytes: None })
                .is_none()
        );
        drop(a);
        // A restart continues the open chunk and sees everything.
        let a = ShardStore::open(&dir.0, "desk-aaaaaa").unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(fs::read_dir(dir.0.join("v1/desk-aaaaaa")).unwrap().count(), 1);
    }

    #[test]
    fn flushes_on_drop_and_in_batches() {
        let dir = TempDir::new("drop");
        {
            let mut a = ShardStore::open(&dir.0, "m-1").unwrap();
            for i in 0..FLUSH_RECORDS - 1 {
                a.append(rec(&format!("s{i}"), "m-1", 1, true)).unwrap();
            }
            assert!(!dir.0.join("v1/m-1/000001.jsonl").exists(), "buffered");
            a.append(rec("last", "m-1", 1, true)).unwrap();
            assert!(dir.0.join("v1/m-1/000001.jsonl").exists(), "50 pending → written");
            a.append(rec("after", "m-1", 1, true)).unwrap();
        }
        let b = ShardStore::open(&dir.0, "m-2").unwrap();
        assert_eq!(b.len(), FLUSH_RECORDS + 1);
    }

    #[test]
    fn torn_line_shrink_and_conflicted_copy() {
        let dir = TempDir::new("torn");
        let other = dir.0.join("v1/other-1");
        fs::create_dir_all(&other).unwrap();
        let line = |r: &TrackAnalysis| serde_json::to_string(r).unwrap() + "\n";
        let full = line(&rec("s1", "other-1", 5, true));
        let second = line(&rec("s2", "other-1", 6, true));
        // A sync in progress: the second line is cut off.
        fs::write(other.join("000001.jsonl"), format!("{full}{}", &second[..20])).unwrap();
        let mut me = ShardStore::open(&dir.0, "me-1").unwrap();
        assert!(me.lookup("s1", &file()).is_some());
        assert!(me.lookup("s2", &file()).is_none());
        fs::write(other.join("000001.jsonl"), format!("{full}{second}")).unwrap();
        assert_eq!(me.refresh().unwrap(), 1);
        assert!(me.lookup("s2", &file()).is_some());
        // Rolled back to a shorter file with a different record: read again.
        fs::write(other.join("000001.jsonl"), line(&rec("s3", "other-1", 7, true))).unwrap();
        assert_eq!(me.refresh().unwrap(), 1);
        assert!(me.lookup("s3", &file()).is_some());
        // Conflicted copies are read too; garbage lines are skipped.
        fs::write(
            other.join("000001 (conflicted copy 2026-10-02 120000).jsonl"),
            format!("not json\n{}", line(&rec("s4", "other-1", 8, true))),
        )
        .unwrap();
        me.refresh().unwrap();
        assert!(me.lookup("s4", &file()).is_some());
    }

    #[test]
    fn rotation_closes_chunks() {
        let dir = TempDir::new("rotate");
        let mut a = ShardStore::open(&dir.0, "m-1").unwrap();
        for i in 0..ROTATE_RECORDS + 10 {
            a.append(rec(&format!("s{i}"), "m-1", 1, false)).unwrap();
        }
        a.flush().unwrap();
        let count = |p: &str| fs::read_to_string(dir.0.join("v1/m-1").join(p)).unwrap().lines().count();
        assert_eq!(count("000001.jsonl"), ROTATE_RECORDS);
        assert_eq!(count("000002.jsonl"), 10);
        drop(a);
        let a = ShardStore::open(&dir.0, "m-1").unwrap();
        assert_eq!(a.writer.number, 2, "continues the open chunk");
        assert_eq!(a.len(), ROTATE_RECORDS + 10);
    }

    #[test]
    fn merge_prefers_ok_then_newer_then_machine() {
        let dir = TempDir::new("merge");
        let mut a = ShardStore::open(&dir.0, "a-1").unwrap();
        a.append(rec("s", "b-1", 50, false)).unwrap();
        a.append(rec("s", "c-1", 10, true)).unwrap();
        assert_eq!(a.lookup("s", &file()).unwrap().machine, "c-1", "ok beats failed");
        a.append(rec("s", "d-1", 20, true)).unwrap();
        assert_eq!(a.lookup("s", &file()).unwrap().machine, "d-1", "newer");
        a.append(rec("s", "a-1", 20, true)).unwrap();
        assert_eq!(a.lookup("s", &file()).unwrap().machine, "a-1", "tie → machine order");
        let mut old = rec("s", "z-1", 99, true);
        old.version = 0;
        a.append(old).unwrap();
        assert_eq!(a.lookup("s", &file()).unwrap().machine, "a-1", "incompatible ignored");
    }

    #[test]
    fn machine_ids_and_shared_dir() {
        let id = machine_id_from("Fedora.local", "abc");
        assert!(id.starts_with("fedora-") && id.len() == "fedora-".len() + 6, "{id}");
        assert_ne!(machine_id_from("x", "a"), machine_id_from("x", "b"));
        assert!(machine_id().len() > 7);

        let home = TempDir::new("home");
        let fb = home.0.join("data/analysis");
        assert_eq!(detect_shared_dir(&home.0, &fb), fb);
        fs::create_dir_all(home.0.join("Nextcloud")).unwrap();
        assert_eq!(detect_shared_dir(&home.0, &fb), home.0.join("Nextcloud/Listen/analysis"));
        fs::create_dir_all(home.0.join(".config/Nextcloud")).unwrap();
        fs::write(
            home.0.join(".config/Nextcloud/nextcloud.cfg"),
            "[Accounts]\n0\\Folders\\1\\ignoreHiddenFiles=false\n0\\Folders\\1\\localPath=/data/NC/\n",
        )
        .unwrap();
        assert_eq!(detect_shared_dir(&home.0, &fb), PathBuf::from("/data/NC/Listen/analysis"));
    }
}
