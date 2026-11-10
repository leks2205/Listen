//! Log files for diagnosing problems (e.g. a track that doesn't play):
//! `listen.log` from the app and `mpv.log` from the player, in the data
//! directory's `logs/` (`~/.local/share/listen/logs` on Linux). The previous
//! run is kept as `*.log.1`. Cookies and stream URLs are never logged.

use std::path::PathBuf;
use std::sync::Mutex;
use tracing_subscriber::{EnvFilter, Layer, fmt, prelude::*};

/// Every workspace crate that logs, with what `listen.log` records of it at the "Debug" log level (at
/// "Info" all of them record info). A crate missing here only reaches the file with warnings: a new
/// crate must be added (`every_workspace_crate_reaches_the_log_file` checks it). The chatty per-request
/// or per-song crates stay at info even at "Debug".
const CRATES: &[(&str, &str)] = &[
    ("listen", "debug"),
    ("listen_analysis", "info"),
    ("listen_core", "debug"),
    ("listen_dj", "debug"),
    ("listen_dlna", "debug"),
    ("listen_jellyfin", "info"),
    ("listen_lastfm", "debug"),
    ("listen_local", "info"),
    ("listen_lyrics", "info"),
    ("listen_mix", "debug"),
    ("listen_offline", "debug"),
    ("listen_player", "debug"),
    ("listen_rip", "debug"),
    ("listen_source", "debug"),
    ("listen_subsonic", "info"),
    ("listen_visualizer", "info"),
    ("listen_ytmusic", "debug"),
];

fn directives(debug: bool) -> String {
    let crates = CRATES.iter().map(|(name, level)| format!("{name}={}", if debug { level } else { &"info" }));
    std::iter::once("warn".to_string()).chain(crates).collect::<Vec<_>>().join(",")
}

/// Detail written to `listen.log` (the terminal keeps `RUST_LOG` / info).
static FILE_FILTER: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| directives(true));

/// What "Info" writes to `listen.log`.
static FILE_FILTER_INFO: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| directives(false));

type FileHandle = tracing_subscriber::reload::Handle<EnvFilter, tracing_subscriber::Registry>;

static FILE_LEVEL: std::sync::OnceLock<FileHandle> = std::sync::OnceLock::new();

/// The filter directives `listen.log` uses at `level`.
pub fn file_filter(level: listen_core::settings::LogLevel) -> &'static str {
    match level {
        listen_core::settings::LogLevel::Info => FILE_FILTER_INFO.as_str(),
        listen_core::settings::LogLevel::Debug => FILE_FILTER.as_str(),
    }
}

/// Changes how much `listen.log` records, effective immediately.
pub fn set_level(level: listen_core::settings::LogLevel) {
    if let Some(handle) = FILE_LEVEL.get() {
        let _ = handle.reload(EnvFilter::new(file_filter(level)));
    }
}

pub fn dir() -> Option<PathBuf> {
    listen_core::config::Paths::discover().ok().map(|p| p.log_dir())
}

pub fn mpv_log() -> Option<PathBuf> {
    dir().map(|d| d.join("mpv.log"))
}

/// Keeps the previous run's log as `<name>.1`.
fn rotate(path: &std::path::Path) {
    if path.exists() {
        let mut old = path.as_os_str().to_owned();
        old.push(".1");
        let _ = std::fs::rename(path, old);
    }
}

/// Sets up logging to the terminal and to `listen.log`.
pub fn init() {
    let terminal = fmt::layer().with_filter(
        EnvFilter::try_from_default_env().unwrap_or_else(|_| "listen=info,listen_core=info,warn".into()),
    );
    let file = dir().and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("listen.log");
        rotate(&path);
        if let Some(mpv) = mpv_log() {
            rotate(&mpv);
        }
        std::fs::File::create(&path).ok()
    });
    let (filter, handle) = tracing_subscriber::reload::Layer::new(EnvFilter::new(FILE_FILTER.as_str()));
    let _ = FILE_LEVEL.set(handle);
    let file_layer =
        file.map(|f| fmt::layer().with_ansi(false).with_writer(Mutex::new(f)).with_filter(filter));
    tracing_subscriber::registry().with(file_layer).with(terminal).init();
    if let Some(dir) = dir() {
        tracing::info!("Listen {} — logs in {}", env!("CARGO_PKG_VERSION"), dir.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_workspace_crate_reaches_the_log_file() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates");
        for entry in std::fs::read_dir(crates).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().replace('-', "_");
            if name.starts_with("listen_") {
                assert!(CRATES.iter().any(|(c, _)| *c == name), "{name} is missing from logging::CRATES");
            }
        }
        assert!(file_filter(listen_core::settings::LogLevel::Debug).contains("listen_rip=debug"));
        assert!(!file_filter(listen_core::settings::LogLevel::Info).contains("=debug"));
    }
}
