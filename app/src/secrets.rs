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
    /// The "Auto" store whatever the user chose (tests, and cleanup that must reach both stores). To
    /// read the user's secrets use [`Secrets::for_user`] (CLI tools) or `with_store` with the loaded
    /// config: with "File" chosen the keyring must not even be asked (it may prompt or hang).
    pub(crate) fn new(config_dir: &Path) -> Self {
        Self::with_store(config_dir, SecretStore::Auto)
    }

    /// The store the user chose in `config.toml` of `config_dir` (Auto when it cannot be read): what the
    /// app itself uses, for the `--check-*` tools and examples. Safe to call on any thread.
    pub fn for_user(config_dir: &Path) -> Self {
        let file = listen_core::config::Paths {
            config_dir: config_dir.to_path_buf(),
            cache_dir: PathBuf::new(),
            data_dir: PathBuf::new(),
        }
        .config_file();
        let store = listen_core::config::Config::load(&file).map(|c| c.secret_store).unwrap_or_default();
        Self::with_store(config_dir, store)
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

    /// Switches the store and moves the secrets of `servers` to it: each one is written to the new store
    /// first and only then removed from the keyring, so a failed write never loses a secret.
    pub fn switch_store(&self, store: SecretStore, servers: &[ServerId]) {
        let secrets: Vec<(&ServerId, String)> =
            servers.iter().filter_map(|id| self.get(id).map(|s| (id, s))).collect();
        self.file_only.store(store == SecretStore::File, Ordering::Relaxed);
        for (id, secret) in &secrets {
            if self.set(id, secret) && store == SecretStore::File {
                self.delete_keyring(id);
            }
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
            return self.fallback_secret(server);
        }
        let id = server.0.clone();
        let read = off_runtime(move || Entry::new(SERVICE, &id).and_then(|e| e.get_password()));
        match read {
            Some(Ok(secret)) => Some(secret),
            Some(Err(keyring::Error::NoEntry)) => self.fallback_secret(server),
            Some(Err(e)) => {
                tracing::warn!("keyring unavailable ({e}); using fallback file");
                self.fallback_secret(server)
            }
            None => {
                tracing::warn!("the keyring call panicked; using fallback file");
                self.fallback_secret(server)
            }
        }
    }

    fn fallback_secret(&self, server: &ServerId) -> Option<String> {
        match self.read_fallback() {
            Ok(mut all) => all.remove(&server.0),
            Err(e) => {
                tracing::error!("could not read {}: {e}", self.fallback.display());
                None
            }
        }
    }

    /// Stores `secret`; false when it could not be stored anywhere (the error is logged).
    pub fn set(&self, server: &ServerId, secret: &str) -> bool {
        if self.file_only.load(Ordering::Relaxed) {
            return self.update_fallback(|all| {
                all.insert(server.0.clone(), secret.to_string());
                true
            });
        }
        let id = server.0.clone();
        let value = secret.to_string();
        let keep = value.clone();
        let written = off_runtime(move || Entry::new(SERVICE, &id).and_then(|e| e.set_password(&value)));
        match written {
            Some(Ok(())) => {
                // Don't leave an older copy in the fallback file.
                self.update_fallback(|all| all.remove(&server.0).is_some());
                true
            }
            other => {
                // An older keyring copy would win over the file in `get` (it reads the keyring first), so the
                // new secret only counts once that copy is gone, e.g. a token grown past Credential Manager's cap.
                self.delete_keyring(server);
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
                self.update_fallback(|all| {
                    all.insert(server.0.clone(), keep.clone());
                    true
                })
            }
        }
    }

    pub fn delete(&self, server: &ServerId) {
        if !self.file_only.load(Ordering::Relaxed) {
            self.delete_keyring(server);
        }
        self.update_fallback(|all| all.remove(&server.0).is_some());
    }

    /// The fallback file's secrets: empty when there is no file. A file that exists but cannot be read or
    /// parsed is an error, never "empty": every write rewrites the whole file, so reading it as empty would
    /// erase the secrets of every other server.
    fn read_fallback(&self) -> std::io::Result<BTreeMap<String, String>> {
        match std::fs::read_to_string(&self.fallback) {
            Ok(text) => toml::from_str(&text).map_err(std::io::Error::other),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e),
        }
    }

    /// Reads the file, applies `change` (true = something changed) and writes it back. False when the
    /// file could not be read or written; the file is then left as it was.
    fn update_fallback(&self, change: impl FnOnce(&mut BTreeMap<String, String>) -> bool) -> bool {
        let result = self
            .read_fallback()
            .and_then(|mut all| if change(&mut all) { self.write_fallback(&all) } else { Ok(()) });
        if let Err(e) = &result {
            tracing::error!("could not update {} (left unchanged): {e}", self.fallback.display());
        }
        result.is_ok()
    }

    /// Saves through `listen_core::files::write_atomic_private`: a crash mid-write leaves the old file,
    /// and the new one is created 0600 from its first byte.
    fn write_fallback(&self, all: &BTreeMap<String, String>) -> std::io::Result<()> {
        if let Some(dir) = self.fallback.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(all).map_err(std::io::Error::other)?;
        listen_core::files::write_atomic_private(&self.fallback, text.as_bytes())
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
    fn an_unreadable_credentials_file_is_never_overwritten() {
        let d = dir("corrupt");
        let file = d.join("credentials.toml");
        std::fs::write(&file, "not = [toml").unwrap();
        let secrets = Secrets::with_store(&d, SecretStore::File);
        let id = ServerId("nd-1".into());
        assert!(!secrets.set(&id, "pw"), "a write over a file that does not parse fails");
        secrets.delete(&id);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "not = [toml");
        assert_eq!(secrets.get(&id), None);
        std::fs::remove_dir_all(d).ok();
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_credentials_file_becomes_private_on_the_next_write() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("mode");
        let file = d.join("credentials.toml");
        std::fs::write(&file, "other = \"kept\"\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let secrets = Secrets::with_store(&d, SecretStore::File);
        assert!(secrets.set(&ServerId("nd-1".into()), "pw"));
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o077, 0);
        assert_eq!(secrets.get(&ServerId("other".into())).as_deref(), Some("kept"));
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "no temp file left behind");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn tools_use_the_store_the_user_chose() {
        let d = dir("for-user");
        assert_eq!(Secrets::for_user(&d).store(), SecretStore::Auto, "no config: Auto");
        std::fs::write(d.join("config.toml"), "secret_store = \"file\"\n").unwrap();
        assert_eq!(Secrets::for_user(&d).store(), SecretStore::File);
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
