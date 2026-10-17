//! Log files for diagnosing problems (e.g. a track that doesn't play):
//! `listen.log` from the app and `mpv.log` from the player, in the data
//! directory's `logs/` (`~/.local/share/listen/logs` on Linux). The previous
//! run is kept as `*.log.1`. Cookies and stream URLs are never logged.

use std::path::PathBuf;
use std::sync::Mutex;
use tracing_subscriber::{EnvFilter, Layer, fmt, prelude::*};

/// Detail written to `listen.log` (the terminal keeps `RUST_LOG` / info).
const FILE_FILTER: &str = "warn,listen=debug,listen_core=debug,listen_source=debug,listen_ytmusic=debug,\
                           listen_player=debug,listen_dj=debug,listen_subsonic=info,listen_jellyfin=info,listen_local=info,listen_lyrics=info";

/// Same crates at `info`: what "Info" writes to `listen.log`.
const FILE_FILTER_INFO: &str = "warn,listen=info,listen_core=info,listen_source=info,listen_ytmusic=info,\
                                listen_player=info,listen_dj=info,listen_subsonic=info,listen_jellyfin=info,listen_local=info,listen_lyrics=info";

type FileHandle = tracing_subscriber::reload::Handle<EnvFilter, tracing_subscriber::Registry>;

static FILE_LEVEL: std::sync::OnceLock<FileHandle> = std::sync::OnceLock::new();

/// The filter directives `listen.log` uses at `level`.
pub fn file_filter(level: listen_core::settings::LogLevel) -> &'static str {
    match level {
        listen_core::settings::LogLevel::Info => FILE_FILTER_INFO,
        listen_core::settings::LogLevel::Debug => FILE_FILTER,
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
    let (filter, handle) = tracing_subscriber::reload::Layer::new(EnvFilter::new(FILE_FILTER));
    let _ = FILE_LEVEL.set(handle);
    let file_layer =
        file.map(|f| fmt::layer().with_ansi(false).with_writer(Mutex::new(f)).with_filter(filter));
    tracing_subscriber::registry().with(file_layer).with(terminal).init();
    if let Some(dir) = dir() {
        tracing::info!("Listen {} — logs in {}", env!("CARGO_PKG_VERSION"), dir.display());
    }
}
