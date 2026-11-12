//! Saving files so that a crash, a full disk or a killed process never leaves a half-written one.
//!
//! Every save of a whole file goes through [`write_atomic`] (or [`write_atomic_private`] for secrets)
//! instead of a hand-written "write a temp file, rename it" (`source_rules.rs` in the app checks).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Replaces the file at `path` with `bytes` atomically: the bytes go to a temporary file next to it
/// (the full file name plus `.tmp-<process id>-<counter>`), are flushed to disk, and the temporary
/// file is renamed over `path`.
///
/// Contract:
/// - readers see the old content or the new one, never a mix; a crash leaves the old file (and at
///   worst a stray temporary file);
/// - the rename is atomic because the temporary file lies in the same folder, so on the same
///   filesystem; the folder must exist (create it first where the caller may write there first);
/// - every call gets its own temporary file, so two threads or processes saving the same `path`
///   never write into each other's file: the last rename wins with a whole file;
/// - when a step fails, the temporary file is removed and `path` is left as it was.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write(path, bytes, Private::No)
}

/// [`write_atomic`] for files that hold secrets: on Unix the new file is readable and writable by
/// its owner only (0600) from its first byte; elsewhere it is the same as [`write_atomic`].
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write(path, bytes, Private::Yes)
}

#[derive(Clone, Copy, PartialEq)]
enum Private {
    No,
    Yes,
}

fn write(path: &Path, bytes: &[u8], private: Private) -> std::io::Result<()> {
    let tmp = temp_path(path)?;
    let result = create(&tmp, private)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            // On disk before the rename, so a crash cannot leave the new name with unflushed data.
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// The temporary file for `path`: appended to the whole file name (never `with_extension`, which
/// would give `x.json` and `x.toml` the same temporary file).
fn temp_path(path: &Path) -> std::io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let Some(name) = path.file_name() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{}: not a file path", path.display()),
        ));
    };
    let mut tmp = name.to_os_string();
    tmp.push(format!(".tmp-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    Ok(path.with_file_name(tmp))
}

/// Creates the temporary file fresh (never opens an existing file or a link someone placed there).
/// A leftover of a killed process that had the same id is removed and the create tried once more.
fn create(tmp: &Path, private: Private) -> std::io::Result<File> {
    let open = || {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        if private == Private::Yes {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private;
        opts.open(tmp)
    };
    match open() {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(tmp)?;
            open()
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp folder (never the user's data folders).
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("listen-core-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn an_atomic_write_replaces_the_content_and_leaves_no_temp_file() {
        let dir = temp_dir("replace");
        let path = dir.join("queue.json");
        write_atomic(&path, b"old, longer content").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(names(&dir), ["queue.json"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn siblings_with_other_extensions_never_share_a_temp_file() {
        let a = temp_path(Path::new("dir/state.json")).unwrap();
        let b = temp_path(Path::new("dir/state.toml")).unwrap();
        let a_name = a.file_name().unwrap().to_string_lossy().into_owned();
        assert!(a_name.starts_with("state.json.tmp-"), "{a_name}");
        assert!(b.file_name().unwrap().to_string_lossy().starts_with("state.toml.tmp-"), "{}", b.display());
        assert_eq!(a.parent(), Some(Path::new("dir")), "the temp file lies next to the target");
        assert_ne!(temp_path(Path::new("dir/state.json")).unwrap(), a, "every call gets its own temp file");

        let dir = temp_dir("siblings");
        write_atomic(&dir.join("state.json"), b"json").unwrap();
        write_atomic(&dir.join("state.toml"), b"toml").unwrap();
        assert_eq!(std::fs::read(dir.join("state.json")).unwrap(), b"json");
        assert_eq!(std::fs::read(dir.join("state.toml")).unwrap(), b"toml");
        assert_eq!(names(&dir), ["state.json", "state.toml"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_rename_leaves_the_target_and_no_temp_file() {
        let dir = temp_dir("failed");
        // A folder at the target makes the rename fail after the temp file was written.
        let blocked = dir.join("cache.bin");
        std::fs::create_dir_all(blocked.join("inside")).unwrap();
        assert!(write_atomic(&blocked, b"data").is_err());
        assert!(blocked.join("inside").is_dir(), "the target is untouched");
        assert_eq!(names(&dir), ["cache.bin"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_is_an_error_not_a_stray_file() {
        let dir = temp_dir("missing");
        assert!(write_atomic(&dir.join("absent").join("x.json"), b"x").is_err());
        assert!(write_atomic(Path::new("/"), b"x").is_err(), "a path without a file name is refused");
        assert!(names(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_writers_of_one_file_each_leave_a_whole_file() {
        let dir = temp_dir("concurrent");
        let path = dir.join("history.jsonl");
        let contents: Vec<Vec<u8>> = (0..8u8).map(|i| vec![b'a' + i; 64 * 1024]).collect();
        std::thread::scope(|s| {
            for c in &contents {
                let path = &path;
                s.spawn(move || write_atomic(path, c).unwrap());
            }
        });
        assert!(contents.contains(&std::fs::read(&path).unwrap()), "the file is one writer's whole content");
        assert_eq!(names(&dir), ["history.jsonl"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_private_write_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("private");
        let path = dir.join("credentials.toml");
        write_atomic_private(&path, b"token = \"x\"").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
