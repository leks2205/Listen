//! Server passwords and tokens in the OS keyring (Secret Service, Windows Credential Manager), with a
//! 0600 `credentials.toml` fallback when there is no keyring or it refuses a secret (Credential
//! Manager caps entries at ~1280 chars). The store is "Auto" or "File", as Feishin's safeStorage
//! selector; switching moves the secrets.

use keyring::Entry;
use listen_core::config::SecretStore;
use listen_core::model::ServerId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
const KEYRING_NAME: &str = "Windows Credential Manager";
#[cfg(not(windows))]
const KEYRING_NAME: &str = "Secret Service";
/// What the settings row says about the fallback file's protection.
#[cfg(unix)]
const PRIVATE: &str = "; permissions 0600";
#[cfg(not(unix))]
const PRIVATE: &str = "; in your user profile";

const SERVICE: &str = listen_core::config::APP_ID;

/// Runs a keyring call on a plain thread.
///
/// The Secret Service client (`keyring` → `secret-service` → `zbus`) starts a runtime of its own,
/// and creating a runtime inside this app's tokio runtime panics ("Cannot start a runtime from
/// within a runtime"). That panic kills the worker thread, and with it whatever task was waiting
/// there — at start-up that was the session restore, so no server was ever connected and the Home
/// page stayed empty until the user switched servers by hand. A plain `std::thread` has no runtime
/// context, which makes the call safe wherever it is made from.
fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::spawn(f).join().ok()
}

pub struct Secrets {
    fallback: PathBuf,
    /// `SecretStore::File`: the keyring is never touched.
    file_only: AtomicBool,
}

impl Secrets {
    pub fn new(config_dir: &Path) -> Self {
        Self::with_store(config_dir, SecretStore::Auto)
    }

    pub fn with_store(config_dir: &Path, store: SecretStore) -> Self {
        Self {
            fallback: config_dir.join("credentials.toml"),
            file_only: AtomicBool::new(store == SecretStore::File),
        }
    }

    pub fn store(&self) -> SecretStore {
        if self.file_only.load(Ordering::Relaxed) { SecretStore::File } else { SecretStore::Auto }
    }

    /// Switches the store and moves the secrets of `servers` to it.
    pub fn switch_store(&self, store: SecretStore, servers: &[ServerId]) {
        let secrets: Vec<(&ServerId, String)> =
            servers.iter().filter_map(|id| self.get(id).map(|s| (id, s))).collect();
        if store == SecretStore::File {
            for (id, _) in &secrets {
                self.delete_keyring(id);
            }
        }
        self.file_only.store(store == SecretStore::File, Ordering::Relaxed);
        for (id, secret) in &secrets {
            self.set(id, secret);
        }
    }

    /// Whether the system keyring answers (blocking: ask off the UI thread).
    pub fn keyring_available() -> bool {
        off_runtime(|| match Entry::new(SERVICE, "listen-probe").and_then(|e| e.get_password()) {
            Ok(_) | Err(keyring::Error::NoEntry) => true,
            Err(_) => false,
        })
        .unwrap_or(false)
    }

    /// Where the secrets are kept right now, for the settings row.
    pub fn describe(&self, keyring_ok: Option<bool>) -> String {
        match (self.store(), keyring_ok) {
            (SecretStore::File, _) => format!("the file {}{}", self.fallback.display(), PRIVATE),
            (SecretStore::Auto, Some(true)) => format!("the system keyring ({KEYRING_NAME})"),
            (SecretStore::Auto, Some(false)) => {
                format!("the file {} (no keyring found{})", self.fallback.display(), PRIVATE)
            }
            (SecretStore::Auto, None) => {
                "the system keyring, or a private file if there is none (checking…)".into()
            }
        }
    }

    fn delete_keyring(&self, server: &ServerId) {
        let id = server.0.clone();
        off_runtime(move || {
            if let Ok(entry) = Entry::new(SERVICE, &id) {
                let _ = entry.delete_credential();
            }
        });
    }

    pub fn get(&self, server: &ServerId) -> Option<String> {
        if self.file_only.load(Ordering::Relaxed) {
            return self.read_fallback().remove(&server.0);
        }
        let id = server.0.clone();
        let read = off_runtime(move || Entry::new(SERVICE, &id).and_then(|e| e.get_password()));
        match read {
            Some(Ok(secret)) => Some(secret),
            Some(Err(keyring::Error::NoEntry)) => self.read_fallback().remove(&server.0),
            Some(Err(e)) => {
                tracing::warn!("keyring unavailable ({e}); using fallback file");
                self.read_fallback().remove(&server.0)
            }
            None => {
                tracing::warn!("the keyring call panicked; using fallback file");
                self.read_fallback().remove(&server.0)
            }
        }
    }

    pub fn set(&self, server: &ServerId, secret: &str) {
        if self.file_only.load(Ordering::Relaxed) {
            let mut all = self.read_fallback();
            all.insert(server.0.clone(), secret.to_string());
            self.write_fallback(&all);
            return;
        }
        let id = server.0.clone();
        let value = secret.to_string();
        let keep = value.clone();
        let written = off_runtime(move || Entry::new(SERVICE, &id).and_then(|e| e.set_password(&value)));
        match written {
            Some(Ok(())) => {
                // Don't leave an older copy in the fallback file.
                let mut all = self.read_fallback();
                if all.remove(&server.0).is_some() {
                    self.write_fallback(&all);
                }
            }
            other => {
                match other {
                    Some(Err(e)) => tracing::warn!(
                        "keyring unavailable ({e}); storing credentials in {}",
                        self.fallback.display()
                    ),
                    _ => tracing::warn!(
                        "the keyring call panicked; storing credentials in {}",
                        self.fallback.display()
                    ),
                }
                let mut all = self.read_fallback();
                all.insert(server.0.clone(), keep.clone());
                self.write_fallback(&all);
            }
        }
    }

    pub fn delete(&self, server: &ServerId) {
        if !self.file_only.load(Ordering::Relaxed) {
            self.delete_keyring(server);
        }
        let mut all = self.read_fallback();
        if all.remove(&server.0).is_some() {
            self.write_fallback(&all);
        }
    }

    fn read_fallback(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.fallback).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    fn write_fallback(&self, all: &BTreeMap<String, String>) {
        let write = || -> std::io::Result<()> {
            if let Some(dir) = self.fallback.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let text = toml::to_string(all).map_err(std::io::Error::other)?;
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            std::io::Write::write_all(&mut opts.open(&self.fallback)?, text.as_bytes())
        };
        if let Err(e) = write() {
            tracing::error!("could not write {}: {e}", self.fallback.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("listen-secrets-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn file_store_never_needs_the_keyring_and_is_private() {
        let d = dir("file");
        let secrets = Secrets::with_store(&d, SecretStore::File);
        let id = ServerId("nd-1".into());
        secrets.set(&id, "pw");
        assert_eq!(secrets.get(&id).as_deref(), Some("pw"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(d.join("credentials.toml")).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "only the user may read it");
        }
        secrets.delete(&id);
        assert_eq!(secrets.get(&id), None);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn describes_the_store_in_use() {
        let d = dir("describe");
        let auto = Secrets::new(&d);
        assert!(auto.describe(Some(true)).contains("keyring"));
        assert!(auto.describe(Some(false)).contains("credentials.toml"));
        assert!(auto.describe(None).contains("checking"));
        let file = Secrets::with_store(&d, SecretStore::File);
        assert!(file.describe(Some(true)).contains("credentials.toml"));
        assert_eq!(file.store(), SecretStore::File);
        std::fs::remove_dir_all(d).ok();
    }
}
