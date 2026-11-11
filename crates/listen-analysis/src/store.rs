//! Analysis records shared between machines through a synced folder (Nextcloud):
//! `<dir>/v1/<machine-id>/NNNNNN.jsonl`, one [`TrackAnalysis`] JSON per line. Each machine appends only to
//! its own directory and rotated chunks are immutable, so no file has two writers. Readers skip torn tails,
//! re-read shrunk files and read "conflicted copy" files; the writer never appends to a chunk that ends in
//! a torn line (it starts the next one). Merging is deterministic ([`TrackAnalysis::better_than`]).

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
    /// Opens (creating) `<dir>/v1/<machine>` and reads every machine's records. `dir` is the folder that
    /// holds `v1/` (e.g. `…/Listen/analysis`), never `v1/` itself: that is refused, since it would create
    /// `v1/v1/<machine>` and read an empty store.
    pub fn open(dir: &Path, machine: &str) -> io::Result<Self> {
        if dir.file_name().is_some_and(|n| n == LAYOUT) && !dir.join(LAYOUT).is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{} is the store's {LAYOUT}/ folder; pass the folder that contains it ({})",
                    dir.display(),
                    dir.parent().map_or_else(|| "its parent".into(), |p| p.display().to_string())
                ),
            ));
        }
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
            // A torn last line (a crash or a full disk mid-write) would swallow the next record: start a new chunk.
            let torn = text.last().is_some_and(|&b| b != b'\n');
            if torn || writer.bytes >= ROTATE_BYTES || records >= ROTATE_RECORDS {
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
    ///
    /// On an I/O error nothing is lost: the unwritten records stay pending and the next flush retries
    /// them (in a new chunk when the failed write may have left a torn line).
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.last_flush = Instant::now();
        while !self.writer.pending.is_empty() {
            let path = self.chunk_path(self.writer.number);
            let mut block = String::new();
            let mut taken = 0;
            for line in &self.writer.pending {
                if taken > 0
                    && (self.writer.bytes + (block.len() + line.len() + 1) as u64 > ROTATE_BYTES
                        || self.writer.records + taken >= ROTATE_RECORDS)
                {
                    break;
                }
                block.push_str(line);
                block.push('\n');
                taken += 1;
            }
            let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
            if let Err(e) = file.write_all(block.as_bytes()).and_then(|()| file.flush()) {
                // Part of the block may be on disk without its newline: appending the retry there would
                // glue two records into one damaged line, so the retry starts the next chunk.
                self.writer.number += 1;
                self.writer.bytes = 0;
                self.writer.records = 0;
                return Err(e);
            }
            // Only written records leave the queue (a failed write keeps them for the next flush).
            self.writer.pending.drain(..taken);
            self.writer.records += taken;
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

    /// Every song's best compatible analysis (one record per song id, failed ones included).
    pub fn best_records(&self) -> impl Iterator<Item = &Arc<TrackAnalysis>> {
        self.index.values().filter_map(|list| {
            list.iter()
                .filter(|r| r.version >= MIN_COMPATIBLE_VERSION)
                .reduce(|best, r| if r.better_than(best) { r } else { best })
        })
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

/// This machine's shard name, also its device id for handoff and the history: the sanitised host name plus a
/// short hash of the system's machine id (two machines may share a host name), e.g. `fedora-3fa2c1`. It must
/// differ between machines, or two writers share one shard. Sources, first found wins:
/// - Linux: host `/etc/hostname`, `/proc/sys/kernel/hostname`, `$HOSTNAME`; id `/etc/machine-id`,
///   `/var/lib/dbus/machine-id`.
/// - Windows: host `%COMPUTERNAME%`; id the registry's `MachineGuid`.
/// - macOS: host `gethostname`; id the `IOPlatformUUID`.
/// - Elsewhere: host `/etc/hostname`, `$HOSTNAME`; id `/etc/machine-id`.
///
/// Without a machine id the host name stands in (logged once). Where `/etc/hostname` and `/etc/machine-id`
/// exist the result is what it always was, so existing shard folders keep their writer. Read once per process.
pub fn machine_id() -> String {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        let (host, ids) = machine_sources();
        machine_id_from_sources(host, ids)
    })
    .clone()
}

/// This system's host name and machine-id candidates, in the order of [`machine_id`]'s doc.
#[cfg(target_os = "linux")]
fn machine_sources() -> (Option<String>, Vec<Option<String>>) {
    let host = read_trimmed("/etc/hostname")
        .or_else(|| read_trimmed("/proc/sys/kernel/hostname"))
        .or_else(|| std::env::var("HOSTNAME").ok());
    (host, vec![read_trimmed("/etc/machine-id"), read_trimmed("/var/lib/dbus/machine-id")])
}

#[cfg(windows)]
fn machine_sources() -> (Option<String>, Vec<Option<String>>) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    // Full path: a `reg.exe` earlier on PATH must not decide the id.
    let guid = std::process::Command::new(PathBuf::from(root).join(r"System32\reg.exe"))
        .args(["query", r"HKLM\SOFTWARE\Microsoft\Cryptography", "/v", "MachineGuid"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .and_then(|o| reg_value(&String::from_utf8_lossy(&o.stdout), "MachineGuid"));
    (std::env::var("COMPUTERNAME").ok(), vec![guid])
}

#[cfg(target_os = "macos")]
fn machine_sources() -> (Option<String>, Vec<Option<String>>) {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its whole length, which is passed along.
    let host = (unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0).then(|| {
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    });
    let uuid = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()
        .and_then(|o| ioreg_platform_uuid(&String::from_utf8_lossy(&o.stdout)));
    (host, vec![uuid])
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn machine_sources() -> (Option<String>, Vec<Option<String>>) {
    (
        read_trimmed("/etc/hostname").or_else(|| std::env::var("HOSTNAME").ok()),
        vec![read_trimmed("/etc/machine-id")],
    )
}

#[cfg(not(any(windows, target_os = "macos")))]
fn read_trimmed(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// A value from `reg query … /v <name>` output (`    MachineGuid    REG_SZ    <value>`).
#[cfg(any(windows, test))]
fn reg_value(out: &str, name: &str) -> Option<String> {
    out.lines()
        .map(str::trim)
        .find(|l| l.split_whitespace().next() == Some(name))
        .and_then(|l| l.split_whitespace().nth(2))
        .map(str::to_string)
}

/// The `"IOPlatformUUID" = "<uuid>"` value from `ioreg -rd1 -c IOPlatformExpertDevice` output.
#[cfg(any(target_os = "macos", test))]
fn ioreg_platform_uuid(out: &str) -> Option<String> {
    out.lines()
        .find(|l| l.contains("\"IOPlatformUUID\""))
        .and_then(|l| l.split('"').nth(3))
        .map(str::to_string)
}

/// The shard name from a host name and machine-id candidates (the first non-empty id wins; none: the host
/// name stands in, so machines then differ only by host name).
fn machine_id_from_sources(host: Option<String>, ids: impl IntoIterator<Item = Option<String>>) -> String {
    let host = host.map(|h| h.trim().to_string()).filter(|h| !h.is_empty()).unwrap_or_else(|| "host".into());
    let id = ids.into_iter().flatten().map(|i| i.trim().to_string()).find(|i| !i.is_empty());
    let id = id.unwrap_or_else(|| {
        tracing::warn!(
            "no machine id found: the analysis shard and device id use the host name \"{host}\" alone"
        );
        host.clone()
    });
    machine_id_from(&host, &id)
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

    #[test]
    fn open_refuses_the_layout_folder_itself() {
        let dir = TempDir::new("layout");
        let v1 = dir.0.join("v1");
        fs::create_dir_all(v1.join("desk-aaaaaa")).unwrap();
        let Err(err) = ShardStore::open(&v1, "m-1") else { panic!("v1/ itself is refused") };
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("pass the folder that contains it"), "{err}");
        assert!(!v1.join("v1").exists(), "nothing is created under the wrong folder");
        assert!(ShardStore::open(&dir.0, "m-1").is_ok());
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
                    voiced: None,
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
    fn a_failed_flush_keeps_its_records_for_the_next_one() {
        let dir = TempDir::new("failed-flush");
        let chunk = dir.0.join("v1/m-1/000001.jsonl");
        {
            let mut a = ShardStore::open(&dir.0, "m-1").unwrap();
            for i in 0..3 {
                a.append(rec(&format!("s{i}"), "m-1", 1, true)).unwrap();
            }
            // The chunk can't be opened for writing (a directory in its place: unmounted folder, lock, …).
            fs::create_dir_all(&chunk).unwrap();
            assert!(a.flush().is_err());
            assert_eq!(a.writer.pending.len(), 3, "nothing is dropped");
            assert_eq!(a.writer.records, 0, "the rotation count only counts written records");
            fs::remove_dir(&chunk).unwrap();
            a.flush().unwrap();
            assert!(a.writer.pending.is_empty());
            assert_eq!(a.writer.records, 3);
        }
        let b = ShardStore::open(&dir.0, "m-2").unwrap();
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn a_torn_own_chunk_is_closed_not_appended_to() {
        let dir = TempDir::new("torn-own");
        let own = dir.0.join("v1/m-1");
        fs::create_dir_all(&own).unwrap();
        let line = |r: &TrackAnalysis| serde_json::to_string(r).unwrap() + "\n";
        let (full, second) = (line(&rec("s1", "m-1", 5, true)), line(&rec("s2", "m-1", 6, true)));
        fs::write(own.join("000001.jsonl"), format!("{full}{}", &second[..20])).unwrap();
        {
            let mut a = ShardStore::open(&dir.0, "m-1").unwrap();
            assert_eq!(a.writer.number, 2, "the torn chunk is closed");
            a.append(rec("s3", "m-1", 7, true)).unwrap();
            a.flush().unwrap();
        }
        assert!(own.join("000002.jsonl").exists());
        let b = ShardStore::open(&dir.0, "m-2").unwrap();
        assert!(b.lookup("s1", &file()).is_some());
        assert!(b.lookup("s3", &file()).is_some(), "the new record isn't glued onto the torn line");
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
    fn machine_ids_differ_without_etc_machine_id() {
        let id = |host: &str, ids: &[Option<&str>]| {
            machine_id_from_sources(Some(host.into()), ids.iter().map(|i| i.map(str::to_string)))
        };
        // Windows: COMPUTERNAME + MachineGuid; macOS: host name + IOPlatformUUID.
        assert_ne!(id("DESKTOP-A1", &[Some("guid-a")]), id("DESKTOP-B2", &[Some("guid-b")]));
        assert_ne!(id("pc", &[Some("guid-a")]), id("pc", &[Some("guid-b")]), "same host name, other machine");
        // No machine id at all: the host name alone still tells machines apart.
        assert_ne!(id("a", &[None]), id("b", &[None]));
        // Linux ids don't move: `/etc/machine-id` first, the dbus copy when it is missing.
        assert_eq!(id("fedora\n", &[Some("abc\n"), Some("zzz")]), machine_id_from("fedora", "abc"));
        assert_eq!(id("fedora", &[None, Some("abc")]), machine_id_from("fedora", "abc"));
        assert_eq!(machine_id_from_sources(None, [None]), machine_id_from("host", "host"));

        let reg = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Cryptography\r\n    MachineGuid    REG_SZ    \
                   1b2c3d4e-0000-4000-8000-000000000001\r\n\r\n";
        assert_eq!(reg_value(reg, "MachineGuid").as_deref(), Some("1b2c3d4e-0000-4000-8000-000000000001"));
        assert_eq!(reg_value("ERROR: not found", "MachineGuid"), None);
        let ioreg = "+-o Mac  <class IOPlatformExpertDevice>\n    {\n      \"IOPlatformUUID\" = \"564D1C2E-1111-2222\"\n    }\n";
        assert_eq!(ioreg_platform_uuid(ioreg).as_deref(), Some("564D1C2E-1111-2222"));
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
