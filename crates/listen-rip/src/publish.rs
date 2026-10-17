//! Puts a finished album into the music folder Navidrome scans.
//!
//! - Nothing on the destination is deleted or overwritten; the only removal is `remove_dir` of the
//!   (empty) staging root.
//! - Files are copied and synced into hidden `<dest>/.listen-incoming/<job>/`, then renamed into
//!   `Album Artist/Album` in one step (an existing album gets `Album (2)`).

use crate::layout;
use std::path::{Path, PathBuf};

pub const INCOMING: &str = ".listen-incoming";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DestError {
    #[error("no destination folder is set")]
    NotSet,
    #[error("{0} doesn't exist")]
    Missing(PathBuf),
    #[error("{0} isn't mounted (the NAS share isn't connected)")]
    NotMounted(PathBuf),
    #[error("{0} isn't writable")]
    NotWritable(PathBuf),
    #[error("not enough space on {path} ({free_mb} MB free, {need_mb} MB needed)")]
    Full { path: PathBuf, free_mb: u64, need_mb: u64 },
}

/// The mount point and filesystem type `path` lives on, from
/// `/proc/self/mountinfo` text (longest matching mount point).
pub fn mount_for(mountinfo: &str, path: &Path) -> Option<(PathBuf, String)> {
    let mut best: Option<(PathBuf, String)> = None;
    for line in mountinfo.lines() {
        // id parent maj:min root mountpoint options… - fstype source superopts
        let fields: Vec<&str> = line.split(' ').collect();
        let Some(mp) = fields.get(4) else { continue };
        let Some(dash) = fields.iter().position(|f| *f == "-") else { continue };
        let Some(fstype) = fields.get(dash + 1) else { continue };
        let mp = PathBuf::from(mp.replace("\\040", " ").replace("\\011", "\t").replace("\\134", "\\"));
        if path.starts_with(&mp)
            && best.as_ref().is_none_or(|(b, _)| mp.as_os_str().len() >= b.as_os_str().len())
        {
            best = Some((mp, fstype.to_string()));
        }
    }
    best
}

/// A folder under /mnt, /media or /run/media that sits on the root
/// filesystem is an unmounted share's empty mount point, not the NAS.
pub fn looks_unmounted(path: &Path, mount: Option<&(PathBuf, String)>) -> bool {
    let under_mounts = ["/mnt", "/media", "/run/media"].iter().any(|m| path.starts_with(m));
    under_mounts && mount.is_none_or(|(mp, fs)| mp == Path::new("/") || fs == "autofs")
}

/// Is `dest` ready for an album of `need_bytes`? Touches the path, so a
/// systemd automount gets mounted (may block while the NAS answers).
pub fn check(dest: &Path, need_bytes: u64) -> Result<(), DestError> {
    if dest.as_os_str().is_empty() {
        return Err(DestError::NotSet);
    }
    match std::fs::metadata(dest) {
        Ok(m) if m.is_dir() => {}
        _ => {
            return Err(if looks_unmounted(dest, None) && !dest.exists() {
                DestError::NotMounted(dest.to_path_buf())
            } else {
                DestError::Missing(dest.to_path_buf())
            });
        }
    }
    #[cfg(target_os = "linux")]
    {
        let info = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        let canonical = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
        if looks_unmounted(&canonical, mount_for(&info, &canonical).as_ref()) {
            return Err(DestError::NotMounted(dest.to_path_buf()));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(dest.as_os_str().as_bytes())
            .map_err(|_| DestError::Missing(dest.into()))?;
        // SAFETY: valid C string; access() only reads it.
        if unsafe { libc::access(c.as_ptr(), libc::W_OK | libc::X_OK) } != 0 {
            return Err(DestError::NotWritable(dest.to_path_buf()));
        }
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: valid C string and a zeroed out-struct.
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0 {
            let free = st.f_bavail as u64 * st.f_frsize as u64;
            if free < need_bytes + (64 << 20) {
                return Err(DestError::Full {
                    path: dest.to_path_buf(),
                    free_mb: free >> 20,
                    need_mb: need_bytes >> 20,
                });
            }
        }
    }
    #[cfg(not(unix))]
    let _ = need_bytes;
    Ok(())
}

/// Size of the files directly in `dir`.
pub fn folder_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|d| d.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

/// Copies the files of `staging` (flat) to `dest/rel` as above; returns the album folder used.
pub fn publish(staging: &Path, dest: &Path, rel: &Path, job: &str) -> Result<PathBuf, String> {
    let incoming_root = dest.join(INCOMING);
    std::fs::create_dir_all(&incoming_root).map_err(|e| format!("{}: {e}", incoming_root.display()))?;
    let mut incoming = incoming_root.join(job);
    let mut n = 1;
    while incoming.exists() {
        n += 1;
        incoming = incoming_root.join(format!("{job}-{n}"));
    }
    std::fs::create_dir(&incoming).map_err(|e| format!("{}: {e}", incoming.display()))?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(staging)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    for src in &files {
        let name = src.file_name().ok_or("bad file name")?;
        let dst = incoming.join(name);
        std::fs::copy(src, &dst).map_err(|e| format!("copying {}: {e}", dst.display()))?;
        std::fs::File::open(&dst)
            .and_then(|f| f.sync_all())
            .map_err(|e| format!("syncing {}: {e}", dst.display()))?;
        let (a, b) =
            (src.metadata().map(|m| m.len()).unwrap_or(0), dst.metadata().map(|m| m.len()).unwrap_or(1));
        if a != b {
            return Err(format!("{}: copied {b} of {a} bytes", dst.display()));
        }
    }
    // The album folder appears in one rename. If someone created the same
    // folder in the meantime, take the next free name.
    for _ in 0..5 {
        let target = layout::free_dir(dest, rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        match std::fs::rename(&incoming, &target) {
            Ok(()) => {
                // Only succeeds if our staging root is empty (other jobs may be using it).
                let _ = std::fs::remove_dir(&incoming_root);
                return Ok(target);
            }
            Err(e) if target.exists() => tracing::info!("{} appeared meanwhile ({e})", target.display()),
            Err(e) => {
                return Err(format!(
                    "moving the album into {}: {e} (the files are kept in {})",
                    target.display(),
                    incoming.display()
                ));
            }
        }
    }
    Err(format!("no free album folder name; the files are kept in {}", incoming.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
22 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw
80 22 0:50 / /mnt/nas rw,relatime shared:40 - autofs systemd-1 rw,fd=51
95 80 0:60 / /mnt/nas rw,relatime shared:50 - cifs //192.168.1.10/music rw,vers=3.1.1
96 22 0:61 / /mnt/my\\040disk rw - btrfs /dev/sdb1 rw
";

    #[test]
    fn finds_the_mount() {
        let m = mount_for(MOUNTINFO, Path::new("/mnt/nas/Music")).unwrap();
        assert_eq!(m, (PathBuf::from("/mnt/nas"), "cifs".to_string()));
        assert!(!looks_unmounted(Path::new("/mnt/nas/Music"), Some(&m)));
        // Only the automount trap, or nothing mounted there.
        let unmounted = MOUNTINFO.lines().take(2).collect::<Vec<_>>().join("\n");
        let m = mount_for(&unmounted, Path::new("/mnt/nas/Music"));
        assert!(looks_unmounted(Path::new("/mnt/nas/Music"), m.as_ref()));
        let m = mount_for(MOUNTINFO, Path::new("/mnt/my disk/Music"));
        assert_eq!(m.as_ref().unwrap().1, "btrfs");
        // A folder in the home directory is fine on the root filesystem.
        let m = mount_for(MOUNTINFO, Path::new("/home/alex/Music"));
        assert!(!looks_unmounted(Path::new("/home/alex/Music"), m.as_ref()));
    }

    #[test]
    fn check_reports_missing_and_unset() {
        assert_eq!(check(Path::new(""), 0), Err(DestError::NotSet));
        assert!(matches!(check(Path::new("/nonexistent/listen/x"), 0), Err(DestError::Missing(_))));
        assert!(matches!(check(Path::new("/mnt/listen-test-not-there"), 0), Err(DestError::NotMounted(_))));
        let tmp = std::env::temp_dir();
        assert_eq!(check(&tmp, 1 << 20), Ok(()));
    }
}
