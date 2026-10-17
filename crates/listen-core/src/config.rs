//! Persistent configuration: settings plus the configured servers.
//! Secrets (passwords, tokens, cookies) are never stored here — see the keyring.

use crate::model::{ServerId, SourceKind};
use crate::settings::Settings;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Reverse-DNS application id (also the Wayland app id and desktop file name).
pub const APP_ID: &str = "io.github.leks2205.Listen";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub id: ServerId,
    pub name: String,
    pub kind: SourceKind,
    /// Base URL; empty for YouTube Music.
    pub url: String,
    pub username: String,
}

/// Per-server options outside the sign-in (Feishin's "legacy authentication", "ignore SSL",
/// "prefer instant mix"), kept in [`Config::server_options`] by server id.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerOptions {
    /// Subsonic: send the password (`p=enc:<hex>`) instead of token + salt.
    pub legacy_auth: bool,
    /// Accept invalid / self-signed certificates of this server only.
    pub ignore_ssl: bool,
    /// Jellyfin: build radio from InstantMix without trying `/Similar` first.
    pub prefer_instant_mix: bool,
}

/// Where passwords and tokens are kept (Feishin's safeStorage selector).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretStore {
    /// The system keyring (Secret Service), the 0600 file when none is running.
    #[default]
    Auto,
    /// Only the 0600 `credentials.toml` in the config directory.
    File,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub active_server: Option<ServerId>,
    pub servers: Vec<ServerConfig>,
    pub settings: Settings,
    pub server_options: std::collections::BTreeMap<String, ServerOptions>,
    pub secret_store: SecretStore,
    /// Where a server's music folder is mounted on this computer, keyed by
    /// server id ("Show in file manager" for Navidrome songs).
    pub music_dirs: std::collections::BTreeMap<String, MusicDir>,
}

/// Maps the path a server reports for a song to a folder on this computer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicDir {
    /// The server's music folder as it appears in song paths (`/music`);
    /// empty when the server sends paths relative to it.
    pub server_prefix: String,
    /// The same folder on this computer (`/mnt/nas/Music`).
    pub local: String,
}

impl MusicDir {
    pub fn is_empty(&self) -> bool {
        self.local.trim().is_empty()
    }

    /// The local file for `server_path`; `None` when no folder is set, the path lies outside
    /// `server_prefix` or leaves the folder (`..`). Relative paths (Subsonic) map as they are;
    /// separators may be `\`.
    pub fn map(&self, server_path: &str) -> Option<PathBuf> {
        let local = self.local.trim();
        if local.is_empty() {
            return None;
        }
        let path = server_path.replace('\\', "/");
        let prefix = self.server_prefix.trim().replace('\\', "/");
        let prefix = prefix.trim_end_matches('/');
        let rest = if prefix.is_empty() {
            path.trim_start_matches('/')
        } else if let Some(rest) = path.strip_prefix(prefix).filter(|r| r.starts_with('/')) {
            rest.trim_start_matches('/')
        } else if !path.starts_with('/') {
            path.as_str()
        } else {
            return None;
        };
        if rest.is_empty() || rest.split('/').any(|c| c == "..") {
            return None;
        }
        Some(Path::new(local).join(rest))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not determine a home directory")]
    NoHome,
    #[error("I/O error on {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("invalid config file {path}: {source}")]
    Parse { path: PathBuf, source: toml::de::Error },
    #[error("could not serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Standard per-user directories (XDG on Linux, `%APPDATA%` / `%LOCALAPPDATA%` on Windows).
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self, ConfigError> {
        let dirs = ProjectDirs::from("", "", "Listen").ok_or(ConfigError::NoHome)?;
        Ok(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            cache_dir: dirs.cache_dir().to_path_buf(),
            data_dir: dirs.data_dir().to_path_buf(),
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn queue_file(&self) -> PathBuf {
        self.data_dir.join("queue.json")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}

impl Config {
    /// Loads the config, or returns defaults if the file does not exist yet.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut cfg: Self = toml::from_str(&text)
                    .map_err(|source| ConfigError::Parse { path: path.into(), source })?;
                cfg.settings.sidebar.add_missing_items();
                for (action, key) in Settings::default().hotkeys {
                    cfg.settings.hotkeys.entry(action).or_insert(key);
                }
                // The old default (Feishin's Discord application) becomes Listen's own.
                let discord = &mut cfg.settings.window.discord_client_id;
                if discord == crate::settings::FEISHIN_DISCORD_CLIENT_ID {
                    *discord = crate::settings::DEFAULT_DISCORD_CLIENT_ID.into();
                }
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io { path: path.into(), source }),
        }
    }

    /// Writes atomically (temp file + rename) so a crash never truncates it.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let io = |source| ConfigError::Io { path: path.into(), source };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)
    }

    /// The options of a server (all off for servers without an entry).
    pub fn options_of(&self, id: &ServerId) -> ServerOptions {
        self.server_options.get(&id.0).copied().unwrap_or_default()
    }

    pub fn active(&self) -> Option<&ServerConfig> {
        let id = self.active_server.as_ref()?;
        self.servers.iter().find(|s| &s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::MusicDir;

    #[test]
    fn music_dir_maps_server_paths_to_the_local_mount() {
        let m = MusicDir { server_prefix: "/music/".into(), local: "/mnt/nas/Music".into() };
        assert_eq!(m.map("/music/Artist/Album/01.flac"), Some("/mnt/nas/Music/Artist/Album/01.flac".into()));
        // Subsonic sends paths relative to the music folder.
        assert_eq!(m.map("Artist/Album/01.flac"), Some("/mnt/nas/Music/Artist/Album/01.flac".into()));
        // Outside the prefix, escaping the folder, or the prefix itself: nothing.
        assert_eq!(m.map("/other/Artist/01.flac"), None);
        assert_eq!(m.map("/music/../etc/passwd"), None);
        assert_eq!(m.map("/musicbox/x.flac"), None);
        assert_eq!(m.map("/music"), None);
        // No prefix: absolute paths are taken relative to the local folder.
        let bare = MusicDir { local: "/mnt/m".into(), ..Default::default() };
        assert_eq!(bare.map("/a/b.flac"), Some("/mnt/m/a/b.flac".into()));
        assert_eq!(bare.map("a\\b.flac"), Some("/mnt/m/a/b.flac".into()));
        // Nothing set: no mapping.
        assert!(MusicDir::default().is_empty());
        assert_eq!(MusicDir::default().map("a.flac"), None);
    }
    use super::*;

    #[test]
    fn load_adds_missing_default_hotkeys_without_overwriting() {
        let dir = std::env::temp_dir().join(format!("listen-cfg-hk-{}", std::process::id()));
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "[settings.hotkeys]\nplayPause = \"p\"\n").unwrap();
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.settings.hotkeys["playPause"], "p");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_moves_the_old_discord_default_to_listens_application() {
        let dir = std::env::temp_dir().join(format!("listen-cfg-discord-{}", std::process::id()));
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "[settings.window]\ndiscord_client_id = \"1165957668758900787\"\n").unwrap();
        assert_eq!(Config::load(&path).unwrap().settings.window.discord_client_id, "1560790551337640127");
        // A custom application is kept.
        std::fs::write(&path, "[settings.window]\ndiscord_client_id = \"42\"\n").unwrap();
        assert_eq!(Config::load(&path).unwrap().settings.window.discord_client_id, "42");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("listen-cfg-{}", std::process::id()));
        let path = dir.join("config.toml");
        assert_eq!(Config::load(&path).unwrap(), Config::default());

        let mut cfg = Config::default();
        cfg.servers.push(ServerConfig {
            id: ServerId("nd1".into()),
            name: "Home".into(),
            kind: SourceKind::Subsonic,
            url: "https://music.example".into(),
            username: "me".into(),
        });
        cfg.active_server = Some(ServerId("nd1".into()));
        cfg.save(&path).unwrap();

        let back = Config::load(&path).unwrap();
        assert_eq!(back.active().unwrap().name, "Home");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
