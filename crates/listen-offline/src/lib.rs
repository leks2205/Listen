//! Songs downloaded for offline playback, in a private directory (0700 on Unix) with a JSON index.
//! Files count only once committed, never as `*.part`. YouTube Music downloads are capped and
//! deleted when the account is removed or unverified for [`YTM_MAX_OFFLINE`].

use listen_core::model::{ServerId, Song, SourceKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
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
        let mut index: Index = std::fs::read(dir.join("index.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        index.entries.retain(|e| files.join(&e.file).is_file());
        let store = Self { dir, index: Mutex::new(index) };
        store.save();
        Ok(store)
    }

    fn files(&self) -> PathBuf {
        self.dir.join("files")
    }

    fn save(&self) {
        let idx = self.index.lock().expect("index");
        let tmp = self.dir.join("index.json.tmp");
        let result = serde_json::to_vec(&*idx)
            .map_err(std::io::Error::other)
            .and_then(|b| std::fs::write(&tmp, b))
            .and_then(|()| std::fs::rename(&tmp, self.dir.join("index.json")));
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

    pub fn remove_all(&self) {
        self.remove_where(|_| true);
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
    use listen_core::model::GainInfo;

    fn song(server: &str, id: &str, kind: SourceKind) -> Song {
        Song {
            mbz: Default::default(),
            extra: Default::default(),
            id: id.into(),
            server_id: ServerId(server.into()),
            source: kind,
            title: "T".into(),
            artists: vec![],
            album_artists: vec![],
            album: None,
            album_id: None,
            image: None,
            duration: Duration::from_secs(10),
            track_number: None,
            disc_number: None,
            year: None,
            genres: vec![],
            codec: None,
            bit_rate: None,
            sample_rate: None,
            bit_depth: None,
            bpm: None,
            favorite: false,
            rating: None,
            play_count: None,
            gain: GainInfo::default(),
            explicit: false,
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: None,
            comment: None,
        }
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
