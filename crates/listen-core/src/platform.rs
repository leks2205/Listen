//! Per-OS helpers for the crates that start helper programs (mpv, yt-dlp, a JS runtime for yt-dlp).
//! Linux: from `PATH`. Windows: next to `listen.exe`, on `PATH` or in `%LOCALAPPDATA%\Listen\bin`.

use std::path::PathBuf;

/// `CREATE_NO_WINDOW`: a helper started from the GUI must not flash a console window.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Keeps `cmd` from opening a console window (Windows; no-op elsewhere).
/// For a tokio command pass `cmd.as_std_mut()`.
pub fn no_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// The directory of the running executable.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(PathBuf::from)
}

/// The user's home directory (`HOME`, `USERPROFILE` on Windows).
pub fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Where the Windows installer / a manual setup may keep helper programs:
/// `%LOCALAPPDATA%\Listen\bin`.
pub fn local_bin_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(|d| PathBuf::from(d).join("Listen").join("bin"))
}

/// File name of the helper program `name` on this OS (`mpv` -> `mpv.exe` on Windows).
pub fn exe_name(name: &str) -> String {
    if cfg!(windows) && !name.to_ascii_lowercase().ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The helper program `name` to run. Windows: next to `listen.exe`, then `PATH`, then
/// `%LOCALAPPDATA%\Listen\bin`, else the bare name (so the error names it). Elsewhere the bare name.
pub fn tool(name: &str) -> PathBuf {
    if !cfg!(windows) {
        return PathBuf::from(name);
    }
    find_tool(name, exe_dir(), std::env::var_os("PATH"), extra_tool_dirs())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Whether [`tool`] finds `name` (Windows; elsewhere the helpers come from the
/// package manager and are looked up on `PATH` when started).
pub fn tool_installed(name: &str) -> bool {
    !cfg!(windows) || find_tool(name, exe_dir(), std::env::var_os("PATH"), extra_tool_dirs()).is_some()
}

/// Where Windows helpers end up besides `PATH`: Listen's own `bin`, winget's
/// links for portable packages (on `PATH` only for programs started after the
/// install) and the folder of winget's mpv package.
pub fn extra_tool_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = local_bin_dir().into_iter().collect();
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(local).join("Microsoft").join("WinGet").join("Links"));
    }
    if let Some(pf) = std::env::var_os("ProgramFiles").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(pf).join("MPV Player"));
    }
    dirs
}

/// [`tool`]'s search with the locations passed in (testable).
pub fn find_tool(
    name: &str,
    exe_dir: Option<PathBuf>,
    path_var: Option<std::ffi::OsString>,
    extra: Vec<PathBuf>,
) -> Option<PathBuf> {
    let file = exe_name(name);
    let from_path = path_var.into_iter().flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>());
    exe_dir.into_iter().chain(from_path).chain(extra).map(|d| d.join(&file)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_dir_wins_over_path_and_local_bin() {
        let root = std::env::temp_dir().join(format!("listen-platform-{}", std::process::id()));
        let (a, b, c) = (root.join("a"), root.join("b"), root.join("c"));
        for d in [&a, &b, &c] {
            std::fs::create_dir_all(d).unwrap();
        }
        let file = exe_name("tool-x");
        std::fs::write(b.join(&file), "").unwrap();
        std::fs::write(c.join(&file), "").unwrap();
        let path = std::env::join_paths([&b]).unwrap();
        assert_eq!(
            find_tool("tool-x", Some(a.clone()), Some(path.clone()), vec![c.clone()]),
            Some(b.join(&file))
        );
        std::fs::write(a.join(&file), "").unwrap();
        assert_eq!(find_tool("tool-x", Some(a.clone()), Some(path), vec![c.clone()]), Some(a.join(&file)));
        assert_eq!(find_tool("tool-x", None, None, vec![c.clone()]), Some(c.join(&file)));
        assert_eq!(find_tool("missing-tool", Some(a), None, vec![]), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exe_suffix_only_on_windows() {
        assert_eq!(exe_name("mpv"), if cfg!(windows) { "mpv.exe" } else { "mpv" });
        assert_eq!(exe_name("a.exe"), "a.exe");
    }
}
