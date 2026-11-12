//! Songs downloaded for offline playback, in a private directory (0700 on Unix) with a JSON index.
//! Files count only once committed, never as `*.part`. YouTube Music downloads are capped and
//! deleted when the account is removed or unverified for [`YTM_MAX_OFFLINE`].
//! An index that cannot be read is never overwritten and files it no longer names are never deleted
//! on the spot: see [`Store::open`].

use listen_core::model::{ServerId, Song, SourceKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Most YouTube Music songs kept offline.
pub const YTM_MAX_TRACKS: usize = 50;
/// Most bytes of YouTube Music audio kept offline.
pub const YTM_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Offline YouTube Music files are deleted if the account was not verified online for this long.
pub const YTM_MAX_OFFLINE: Duration = Duration::from_secs(30 * 24 * 3600);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub song: Song,
    /// File name inside the store's `files` directory.
    pub file: String,
    pub bytes: u64,
    /// Seconds since the Unix epoch.
    pub added: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Index {
    entries: Vec<Entry>,
    /// Last time each YouTube Music server was reached online (epoch seconds).
    verified: HashMap<String, u64>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Refused {
    #[error("YouTube Music offline is limited to {YTM_MAX_TRACKS} songs")]
    TooManySongs,
    #[error("YouTube Music offline is limited to 2 GB")]
    TooManyBytes,
}

pub struct Store {
    dir: PathBuf,
    index: Mutex<Index>,
}

const INDEX: &str = "index.json";
/// Prefix of the folders (`unindexed-<epoch secs>`) that hold files no index entry names.
const UNINDEXED: &str = "unindexed-";

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn same(e: &Entry, server: &ServerId, id: &str) -> bool {
    &e.song.server_id == server && e.song.id == id
}

/// A file name that is safe on every filesystem and unique per song.
pub fn file_name(song: &Song, ext: &str) -> String {
    let clean = |s: &str| -> String {
        s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
    };
    let ext = clean(ext);
    format!(
        "{}-{}.{}",
        clean(&song.server_id.0),
        clean(&song.id),
        if ext.is_empty() { "audio".into() } else { ext }
    )
}

impl Store {
    /// Opens (or creates) the store in `dir`; leftover `.part` files are removed.
    ///
    /// An `index.json` that does not parse is renamed to `index.json.bad-<epoch secs>`, never overwritten,
    /// and every entry in it that still parses is kept (fields this version needs but the file lacks get
    /// their [`Song::new`] defaults). One that cannot be read or set aside fails `open`. Files that no entry
    /// names are moved to `unindexed-<epoch secs>/`: [`Self::remove_all`] deletes them, and `open` does so
    /// after [`YTM_MAX_OFFLINE`] (their source is unknown, so the YouTube Music limit applies).
    pub fn open(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        let files = dir.join("files");
        std::fs::create_dir_all(&files)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for d in [&dir, &files] {
                std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        for e in std::fs::read_dir(&files)?.flatten() {
            if e.path().extension().is_some_and(|x| x == "part") {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let mut index = load_index(&dir)?;
        index.entries.retain(|e| files.join(&e.file).is_file());
        set_aside_unindexed(&dir, &index);
        let cutoff = now().saturating_sub(YTM_MAX_OFFLINE.as_secs());
        for (aside, _) in unindexed_dirs(&dir).into_iter().filter(|(_, secs)| *secs < cutoff) {
            match std::fs::remove_dir_all(&aside) {
                Ok(()) => tracing::info!("deleted expired unindexed offline files in {}", aside.display()),
                Err(e) => tracing::warn!("could not delete {}: {e}", aside.display()),
            }
        }
        let store = Self { dir, index: Mutex::new(index) };
        store.save();
        Ok(store)
    }

    fn files(&self) -> PathBuf {
        self.dir.join("files")
    }

    fn save(&self) {
        let idx = self.index.lock().expect("index");
        let result = serde_json::to_vec(&*idx)
            .map_err(std::io::Error::other)
            .and_then(|b| listen_core::files::write_atomic(&self.dir.join(INDEX), &b));
        if let Err(e) = result {
            tracing::warn!("could not save the offline index: {e}");
        }
    }

    /// Where a download is written before [`Self::commit`].
    pub fn part_path(&self, name: &str) -> PathBuf {
        self.files().join(format!("{name}.part"))
    }

    /// The downloaded file of a song, if it is there.
    pub fn lookup(&self, server: &ServerId, id: &str) -> Option<PathBuf> {
        let idx = self.index.lock().expect("index");
        let e = idx.entries.iter().find(|e| same(e, server, id))?;
        let path = self.files().join(&e.file);
        path.is_file().then_some(path)
    }

    pub fn contains(&self, server: &ServerId, id: &str) -> bool {
        self.index.lock().expect("index").entries.iter().any(|e| same(e, server, id))
    }

    pub fn entries(&self) -> Vec<Entry> {
        self.index.lock().expect("index").entries.clone()
    }

    pub fn total_bytes(&self) -> u64 {
        self.index.lock().expect("index").entries.iter().map(|e| e.bytes).sum()
    }

    /// Whether another song of `kind` fits (only YouTube Music is capped).
    pub fn check_add(&self, kind: SourceKind, extra_bytes: u64) -> Result<(), Refused> {
        if kind != SourceKind::YouTubeMusic {
            return Ok(());
        }
        let idx = self.index.lock().expect("index");
        let ytm = idx.entries.iter().filter(|e| e.song.source == SourceKind::YouTubeMusic);
        let (count, bytes) = ytm.fold((0usize, 0u64), |(c, b), e| (c + 1, b + e.bytes));
        if count >= YTM_MAX_TRACKS {
            Err(Refused::TooManySongs)
        } else if bytes + extra_bytes > YTM_MAX_BYTES {
            Err(Refused::TooManyBytes)
        } else {
            Ok(())
        }
    }

    /// Moves the finished `.part` file of `name` into the store and indexes it.
    pub fn commit(&self, song: &Song, name: &str) -> std::io::Result<()> {
        let part = self.part_path(name);
        let bytes = std::fs::metadata(&part)?.len();
        if bytes == 0 {
            let _ = std::fs::remove_file(&part);
            return Err(std::io::Error::other("empty download"));
        }
        std::fs::rename(&part, self.files().join(name))?;
        {
            let mut idx = self.index.lock().expect("index");
            idx.entries.retain(|e| !same(e, &song.server_id, &song.id));
            idx.entries.push(Entry { song: song.clone(), file: name.to_string(), bytes, added: now() });
        }
        self.save();
        Ok(())
    }

    /// Deletes one song's download.
    pub fn remove(&self, server: &ServerId, id: &str) {
        self.remove_where(|e| same(e, server, id));
    }

    /// Deletes everything downloaded from a server (it was removed or signed out).
    pub fn remove_server(&self, server: &ServerId) {
        self.remove_where(|e| &e.song.server_id == server);
        self.index.lock().expect("index").verified.remove(&server.0);
        self.save();
    }

    /// Deletes every download, including files set aside as unindexed by [`Self::open`].
    pub fn remove_all(&self) {
        self.remove_where(|_| true);
        for (aside, _) in unindexed_dirs(&self.dir) {
            if let Err(e) = std::fs::remove_dir_all(&aside) {
                tracing::warn!("could not delete {}: {e}", aside.display());
            }
        }
    }

    fn remove_where(&self, doomed: impl Fn(&Entry) -> bool) {
        {
            let mut idx = self.index.lock().expect("index");
            let files = self.files();
            idx.entries.retain(|e| {
                if doomed(e) {
                    let _ = std::fs::remove_file(files.join(&e.file));
                    false
                } else {
                    true
                }
            });
        }
        self.save();
    }

    /// Notes that a YouTube Music server was just reached with a valid sign-in.
    pub fn mark_verified(&self, server: &ServerId) {
        self.index.lock().expect("index").verified.insert(server.0.clone(), now());
        self.save();
    }

    /// Deletes YouTube Music files whose server went unverified for [`YTM_MAX_OFFLINE`] (counted from
    /// the first download if never verified); returns how many were deleted.
    pub fn expire_unverified(&self) -> usize {
        let cutoff = now().saturating_sub(YTM_MAX_OFFLINE.as_secs());
        let doomed: Vec<ServerId> = {
            let idx = self.index.lock().expect("index");
            let mut servers: Vec<&Entry> =
                idx.entries.iter().filter(|e| e.song.source == SourceKind::YouTubeMusic).collect();
            servers.sort_by_key(|e| e.added);
            let mut out: Vec<ServerId> = Vec::new();
            for e in servers {
                let last = idx.verified.get(&e.song.server_id.0).copied().unwrap_or(e.added);
                if last < cutoff && !out.contains(&e.song.server_id) {
                    out.push(e.song.server_id.clone());
                }
            }
            out
        };
        let before = self.entries().len();
        for s in &doomed {
            self.remove_server(s);
        }
        before - self.entries().len()
    }
}

/// The index in `dir`: empty when there is none; an unparsable one is set aside and salvaged (see
/// [`Store::open`]).
fn load_index(dir: &Path) -> std::io::Result<Index> {
    let path = dir.join(INDEX);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Index::default()),
        Err(e) => return Err(e),
    };
    match serde_json::from_slice::<Index>(&bytes) {
        Ok(index) => Ok(index),
        Err(e) => {
            let index = salvage(&bytes);
            let aside = dir.join(format!("{INDEX}.bad-{}", now()));
            std::fs::rename(&path, &aside)?;
            tracing::warn!(
                "offline index unreadable ({e}); kept {} entries, the original is at {}",
                index.entries.len(),
                aside.display()
            );
            Ok(index)
        }
    }
}

/// The entries of an index that does not parse as a whole, each parsed on its own.
fn salvage(bytes: &[u8]) -> Index {
    let Ok(Value::Object(raw)) = serde_json::from_slice::<Value>(bytes) else {
        return Index::default();
    };
    let entries =
        raw.get("entries").and_then(Value::as_array).map(|a| a.iter().filter_map(salvage_entry).collect());
    let verified = raw.get("verified").and_then(|v| serde_json::from_value(v.clone()).ok());
    Index { entries: entries.unwrap_or_default(), verified: verified.unwrap_or_default() }
}

/// One entry as written, or with the song fields it lacks filled from [`Song::new`] (an index written
/// before a field existed); `None` without a readable id, server, source and title.
fn salvage_entry(raw: &Value) -> Option<Entry> {
    if let Ok(entry) = Entry::deserialize(raw) {
        return Some(entry);
    }
    let mut raw = raw.clone();
    let song = raw.get_mut("song")?.as_object_mut()?;
    let base = Song::new(
        serde_json::from_value::<String>(song.get("id")?.clone()).ok()?,
        serde_json::from_value(song.get("server_id")?.clone()).ok()?,
        serde_json::from_value(song.get("source")?.clone()).ok()?,
        serde_json::from_value::<String>(song.get("title")?.clone()).ok()?,
    );
    let Value::Object(mut full) = serde_json::to_value(base).ok()? else {
        return None;
    };
    full.extend(std::mem::take(song));
    *song = full;
    serde_json::from_value(raw).ok()
}

/// Moves the files in `files/` that no entry names into a new `unindexed-<now>/` folder. They may be
/// downloads whose entry could not be read, so they are not deleted on the spot.
fn set_aside_unindexed(dir: &Path, index: &Index) {
    let named: HashSet<&str> = index.entries.iter().map(|e| e.file.as_str()).collect();
    let Ok(listing) = std::fs::read_dir(dir.join("files")) else {
        return;
    };
    let strays: Vec<PathBuf> = listing
        .flatten()
        .filter(|e| e.path().is_file() && !e.file_name().to_str().is_some_and(|n| named.contains(n)))
        .map(|e| e.path())
        .collect();
    if strays.is_empty() {
        return;
    }
    let aside = dir.join(format!("{UNINDEXED}{}", now()));
    if let Err(e) = std::fs::create_dir_all(&aside) {
        tracing::warn!("could not create {}: {e}", aside.display());
        return;
    }
    // Counted outside the log macro: its arguments are not evaluated when nothing listens.
    let moved = strays
        .iter()
        .filter(|p| p.file_name().is_some_and(|n| std::fs::rename(p, aside.join(n)).is_ok()))
        .count();
    tracing::warn!("moved {moved} offline files that no index entry names to {}", aside.display());
}

/// The `unindexed-<epoch secs>` folders in `dir`, with their time.
fn unindexed_dirs(dir: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    listing
        .flatten()
        .filter_map(|e| {
            let secs = e.file_name().to_str()?.strip_prefix(UNINDEXED)?.parse().ok()?;
            e.path().is_dir().then(|| (e.path(), secs))
        })
        .collect()
}

static GLOBAL: OnceLock<Store> = OnceLock::new();

/// The app-wide store, set once at startup.
pub fn init(store: Store) -> &'static Store {
    GLOBAL.get_or_init(|| store)
}

pub fn global() -> Option<&'static Store> {
    GLOBAL.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(server: &str, id: &str, kind: SourceKind) -> Song {
        Song { duration: Duration::from_secs(10), ..Song::new(id, ServerId(server.into()), kind, "T") }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("listen-offline-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn put(store: &Store, s: &Song, bytes: usize) {
        let name = file_name(s, "mp3");
        std::fs::write(store.part_path(&name), vec![1u8; bytes]).unwrap();
        store.commit(s, &name).unwrap();
    }

    #[test]
    fn commit_lookup_remove_and_reopen() {
        let dir = tmp("basic");
        let store = Store::open(&dir).unwrap();
        let s = song("nd", "a/../b", SourceKind::Subsonic);
        assert!(store.lookup(&s.server_id, &s.id).is_none());
        put(&store, &s, 5);
        let path = store.lookup(&s.server_id, &s.id).unwrap();
        assert!(path.starts_with(dir.join("files")), "unsafe id must not escape the store: {path:?}");
        assert_eq!(store.total_bytes(), 5);
        drop(store);
        let store = Store::open(&dir).unwrap();
        assert!(store.contains(&s.server_id, &s.id));
        store.remove(&s.server_id, &s.id);
        assert!(!path.exists() && store.entries().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn partial_and_missing_files_do_not_count() {
        let dir = tmp("partial");
        let store = Store::open(&dir).unwrap();
        let s = song("nd", "1", SourceKind::Subsonic);
        let name = file_name(&s, "mp3");
        std::fs::write(store.part_path(&name), b"half").unwrap();
        assert!(!store.contains(&s.server_id, &s.id));
        put(&store, &s, 3);
        let path = store.lookup(&s.server_id, &s.id).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(store.lookup(&s.server_id, &s.id).is_none());
        drop(store);
        let store = Store::open(&dir).unwrap();
        assert!(store.entries().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_download_is_rejected() {
        let dir = tmp("empty");
        let store = Store::open(&dir).unwrap();
        let s = song("nd", "1", SourceKind::Subsonic);
        let name = file_name(&s, "mp3");
        std::fs::write(store.part_path(&name), b"").unwrap();
        assert!(store.commit(&s, &name).is_err());
        assert!(store.entries().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ytm_is_capped_but_navidrome_is_not() {
        let dir = tmp("cap");
        let store = Store::open(&dir).unwrap();
        for i in 0..YTM_MAX_TRACKS {
            put(&store, &song("yt", &i.to_string(), SourceKind::YouTubeMusic), 1);
        }
        assert_eq!(store.check_add(SourceKind::YouTubeMusic, 1), Err(Refused::TooManySongs));
        assert_eq!(store.check_add(SourceKind::Subsonic, 1), Ok(()));
        store.remove(&ServerId("yt".into()), "0");
        assert_eq!(store.check_add(SourceKind::YouTubeMusic, 1), Ok(()));
        assert_eq!(store.check_add(SourceKind::YouTubeMusic, YTM_MAX_BYTES), Err(Refused::TooManyBytes));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ytm_expires_when_unverified_and_is_kept_when_verified() {
        let dir = tmp("expire");
        let store = Store::open(&dir).unwrap();
        let (a, b, nd) = (
            song("yt-a", "1", SourceKind::YouTubeMusic),
            song("yt-b", "1", SourceKind::YouTubeMusic),
            song("nd", "1", SourceKind::Subsonic),
        );
        for s in [&a, &b, &nd] {
            put(&store, s, 2);
        }
        assert_eq!(store.expire_unverified(), 0);
        let old = now() - YTM_MAX_OFFLINE.as_secs() - 10;
        {
            let mut idx = store.index.lock().unwrap();
            idx.verified.insert("yt-a".into(), old);
            idx.verified.insert("yt-b".into(), now());
        }
        assert_eq!(store.expire_unverified(), 1);
        assert!(!store.contains(&a.server_id, "1"));
        assert!(store.contains(&b.server_id, "1") && store.contains(&nd.server_id, "1"));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn names_in(dir: &Path, prefix: &str) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| n.starts_with(prefix))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn unreadable_index_is_set_aside_and_its_files_are_not_orphaned() {
        let dir = tmp("unreadable");
        std::fs::create_dir_all(dir.join("files")).unwrap();
        std::fs::write(dir.join(INDEX), br#"{"entries":[{"bogus":1}]}"#).unwrap();
        std::fs::write(dir.join("files/nd-1.mp3"), b"song").unwrap();
        let store = Store::open(&dir).unwrap();
        assert!(store.entries().is_empty());
        assert_eq!(names_in(&dir, "index.json.bad-").len(), 1, "the unreadable index is kept");
        assert!(!dir.join("files/nd-1.mp3").exists(), "an unindexed file leaves files/");
        let aside = names_in(&dir, UNINDEXED);
        assert_eq!(aside.len(), 1);
        assert!(dir.join(&aside[0]).join("nd-1.mp3").is_file(), "it is set aside, not deleted");
        store.remove_all();
        assert!(names_in(&dir, UNINDEXED).is_empty(), "Remove all deletes the unindexed files too");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn index_without_verified_still_loads() {
        let dir = tmp("noverified");
        let s = song("nd", "1", SourceKind::Subsonic);
        let store = Store::open(&dir).unwrap();
        put(&store, &s, 3);
        drop(store);
        let mut index: Value = serde_json::from_slice(&std::fs::read(dir.join(INDEX)).unwrap()).unwrap();
        index.as_object_mut().unwrap().remove("verified");
        std::fs::write(dir.join(INDEX), serde_json::to_vec(&index).unwrap()).unwrap();
        let store = Store::open(&dir).unwrap();
        assert!(store.contains(&s.server_id, "1"));
        assert!(names_in(&dir, "index.json.bad-").is_empty() && names_in(&dir, UNINDEXED).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn entries_of_an_unparsable_index_that_still_parse_are_kept() {
        let dir = tmp("salvage");
        let (old, ok) = (song("nd", "old", SourceKind::Subsonic), song("nd", "ok", SourceKind::Subsonic));
        let store = Store::open(&dir).unwrap();
        put(&store, &old, 3);
        put(&store, &ok, 3);
        drop(store);
        let mut index: Value = serde_json::from_slice(&std::fs::read(dir.join(INDEX)).unwrap()).unwrap();
        let entries = index["entries"].as_array_mut().unwrap();
        // An entry written before the `explicit` field existed, and one this version cannot read at all.
        entries[0]["song"].as_object_mut().unwrap().remove("explicit");
        entries.push(
            serde_json::json!({ "song": { "source": "NoSuchSource" }, "file": "x", "bytes": 1, "added": 0 }),
        );
        std::fs::write(dir.join(INDEX), serde_json::to_vec(&index).unwrap()).unwrap();
        let store = Store::open(&dir).unwrap();
        assert!(store.contains(&old.server_id, "old") && store.contains(&ok.server_id, "ok"));
        assert_eq!(store.entries().len(), 2);
        assert_eq!(names_in(&dir, "index.json.bad-").len(), 1);
        assert!(names_in(&dir, UNINDEXED).is_empty(), "every file is still named");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unindexed_files_expire_after_the_offline_limit() {
        let dir = tmp("unindexed-expiry");
        let fresh = format!("{UNINDEXED}{}", now());
        for d in [format!("{UNINDEXED}1"), fresh.clone()] {
            std::fs::create_dir_all(dir.join(&d)).unwrap();
            std::fs::write(dir.join(&d).join("nd-1.mp3"), b"song").unwrap();
        }
        let _store = Store::open(&dir).unwrap();
        assert_eq!(names_in(&dir, UNINDEXED), vec![fresh]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn remove_server_deletes_only_that_server() {
        let dir = tmp("server");
        let store = Store::open(&dir).unwrap();
        let (a, b) = (song("yt", "1", SourceKind::YouTubeMusic), song("nd", "1", SourceKind::Subsonic));
        put(&store, &a, 2);
        put(&store, &b, 2);
        store.remove_server(&a.server_id);
        assert!(!store.contains(&a.server_id, "1") && store.contains(&b.server_id, "1"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
