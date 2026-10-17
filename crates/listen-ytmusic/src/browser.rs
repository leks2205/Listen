//! "Use my browser's sign-in": reads the youtube.com cookies from a local browser profile on every start.
//! Only cookies a browser would send to music.youtube.com are kept.
//!
//! * Chromium-based (Chrome, Chromium, Ecosia, Brave, Edge, Vivaldi, also Flatpaks): `Cookies` SQLite,
//!   AES-128-CBC; `v10` with Chromium's built-in key (no keyring), `v11` with a Secret Service key
//!   (`secret-tool`). Chromium 130+ (meta version ≥ 24) prefixes the plaintext with SHA-256 of the host.
//! * Firefox: `cookies.sqlite`, unencrypted (on Windows in `%APPDATA%\Mozilla\Firefox\Profiles`).
//! * Windows Chromium browsers: `v10` is AES-256-GCM with a DPAPI-protected key in `Local State`. Chrome
//!   127+ app-bound `v20` can't be decrypted outside the browser; the user is pointed to Firefox or a file.

use crate::cookies::{self, Cookie};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Key that marks a stored secret as "read the cookies from this browser".
const SECRET_KEY: &str = "browser_cookies";
/// Seconds between 1601-01-01 (Chromium's epoch) and 1970-01-01.
const WINDOWS_EPOCH_OFFSET: i64 = 11_644_473_600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Engine {
    Chromium,
    Firefox,
}

/// A browser profile with a cookie database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// e.g. "Ecosia"
    pub browser: String,
    /// e.g. "Default", "Profile 1"
    pub profile: String,
    pub engine: Engine,
    /// The cookie database.
    pub cookies: PathBuf,
    /// Secret Service `application` names to try for `v11` cookies.
    #[serde(default)]
    pub keyring_apps: Vec<String>,
    /// The channel (brand account) whose library to use; `None` = the account itself.
    #[serde(default)]
    pub page_id: Option<String>,
    /// Its name, for display.
    #[serde(default)]
    pub channel: Option<String>,
}

impl Profile {
    /// "Ecosia" or "Chrome (Profile 1)".
    pub fn label(&self) -> String {
        if self.profile == "Default" || self.engine == Engine::Firefox {
            self.browser.clone()
        } else {
            format!("{} ({})", self.browser, self.profile)
        }
    }

    /// What gets stored in the keyring for the server (no cookies, just where to read them).
    pub fn to_secret(&self) -> String {
        serde_json::json!({ SECRET_KEY: self }).to_string()
    }

    /// Reads a stored secret; `None` for other kinds of secrets.
    pub fn from_secret(secret: &str) -> Option<Profile> {
        let v: serde_json::Value = serde_json::from_str(secret.trim()).ok()?;
        serde_json::from_value(v.get(SECRET_KEY)?.clone()).ok()
    }

    /// The `Cookie` header for music.youtube.com from this profile, read now.
    pub fn youtube_header(&self) -> Result<String, String> {
        let list = match self.engine {
            Engine::Chromium => read_chromium(&self.cookies, &self.keyring_apps)?,
            Engine::Firefox => read_firefox(&self.cookies)?,
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let signed_in = list.iter().any(|c| c.name == "SAPISID" && c.expires.is_none_or(|e| e > now));
        if !signed_in {
            return Err(format!(
                "{} isn't signed in to YouTube — open music.youtube.com in {} and sign in, then try again",
                self.label(),
                self.browser
            ));
        }
        cookies::header(list, true)
    }
}

/// (browser, Chromium config dir relative to $HOME, keyring application names)
#[cfg(unix)]
const CHROMIUM_DIRS: &[(&str, &str, &[&str])] = &[
    ("Ecosia", ".var/app/org.ecosia.Browser/config/EcosiaBrowser", &["ecosia", "chromium", "chrome"]),
    ("Ecosia", ".config/EcosiaBrowser", &["ecosia", "chromium", "chrome"]),
    ("Chrome", ".config/google-chrome", &["chrome"]),
    ("Chrome", ".var/app/com.google.Chrome/config/google-chrome", &["chrome"]),
    ("Chromium", ".config/chromium", &["chromium"]),
    ("Chromium", ".var/app/org.chromium.Chromium/config/chromium", &["chromium"]),
    ("Brave", ".config/BraveSoftware/Brave-Browser", &["brave"]),
    ("Brave", ".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser", &["brave"]),
    ("Edge", ".config/microsoft-edge", &["chromium", "chrome"]),
    ("Vivaldi", ".config/vivaldi", &["chrome", "chromium"]),
];

#[cfg(unix)]
const FIREFOX_DIRS: &[&str] = &[".mozilla/firefox", ".var/app/org.mozilla.firefox/.mozilla/firefox"];

/// All browser profiles with a cookie database, most recently used first.
pub fn profiles() -> Vec<Profile> {
    #[cfg(unix)]
    {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return vec![] };
        profiles_in(&home)
    }
    #[cfg(windows)]
    {
        let mut found = Vec::new();
        if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            found.extend(windows_chromium_profiles(&local));
        }
        if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
            found.extend(firefox_profiles(&appdata.join("Mozilla").join("Firefox").join("Profiles")));
        }
        sorted_by_use(found)
    }
}

/// (browser, "User Data" folder relative to `%LOCALAPPDATA%`)
#[cfg(any(windows, test))]
const WINDOWS_CHROMIUM_DIRS: &[(&str, &str)] = &[
    ("Chrome", "Google/Chrome/User Data"),
    ("Edge", "Microsoft/Edge/User Data"),
    ("Brave", "BraveSoftware/Brave-Browser/User Data"),
    ("Vivaldi", "Vivaldi/User Data"),
    ("Chromium", "Chromium/User Data"),
    ("Ecosia", "EcosiaBrowser/User Data"),
];

/// The Chromium profiles (`Default`, `Profile N`) with a cookie database under `local`.
#[cfg(any(windows, test))]
fn windows_chromium_profiles(local: &Path) -> Vec<Profile> {
    let mut found = Vec::new();
    for (browser, dir) in WINDOWS_CHROMIUM_DIRS {
        let Ok(entries) = std::fs::read_dir(local.join(dir)) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name != "Default" && !name.starts_with("Profile ") {
                continue;
            }
            let cookies = [entry.path().join("Network").join("Cookies"), entry.path().join("Cookies")]
                .into_iter()
                .find(|p| p.is_file());
            if let Some(cookies) = cookies {
                found.push(Profile {
                    browser: browser.to_string(),
                    profile: name,
                    engine: Engine::Chromium,
                    cookies,
                    keyring_apps: vec![],
                    page_id: None,
                    channel: None,
                });
            }
        }
    }
    found
}

/// The Firefox profiles (folders with a `cookies.sqlite`) under `dir`.
fn firefox_profiles(dir: &Path) -> Vec<Profile> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    entries
        .flatten()
        .filter_map(|entry| {
            let cookies = entry.path().join("cookies.sqlite");
            cookies.is_file().then(|| Profile {
                browser: "Firefox".into(),
                profile: entry.file_name().to_string_lossy().to_string(),
                engine: Engine::Firefox,
                cookies,
                keyring_apps: vec![],
                page_id: None,
                channel: None,
            })
        })
        .collect()
}

/// Most recently used cookie database first.
fn sorted_by_use(profiles: Vec<Profile>) -> Vec<Profile> {
    let mut found: Vec<(std::time::SystemTime, Profile)> = profiles
        .into_iter()
        .map(|p| {
            let modified =
                std::fs::metadata(&p.cookies).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (modified, p)
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, p)| p).collect()
}

#[cfg(unix)]
fn profiles_in(home: &Path) -> Vec<Profile> {
    let mut found: Vec<Profile> = Vec::new();
    let mut add = |profile: Profile| found.push(profile);
    for (browser, dir, apps) in CHROMIUM_DIRS {
        let Ok(entries) = std::fs::read_dir(home.join(dir)) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name != "Default" && !name.starts_with("Profile ") {
                continue;
            }
            // Newer Chromium keeps it in Network/Cookies.
            let cookies = [entry.path().join("Network/Cookies"), entry.path().join("Cookies")]
                .into_iter()
                .find(|p| p.is_file());
            if let Some(cookies) = cookies {
                add(Profile {
                    browser: browser.to_string(),
                    profile: name,
                    engine: Engine::Chromium,
                    cookies,
                    keyring_apps: apps.iter().map(|a| a.to_string()).collect(),
                    page_id: None,
                    channel: None,
                });
            }
        }
    }
    for dir in FIREFOX_DIRS {
        for profile in firefox_profiles(&home.join(dir)) {
            add(profile);
        }
    }
    sorted_by_use(found)
}

/// The most recently used profile that is signed in to YouTube, with its header.
pub fn detect() -> Result<(Profile, String), String> {
    signed_in().map(|mut found| found.remove(0))
}

/// Every profile that is signed in to YouTube, with its header, most recently
/// used first (never empty); otherwise why none could be used.
pub fn signed_in() -> Result<Vec<(Profile, String)>, String> {
    let mut found = Vec::new();
    let mut problems = Vec::new();
    for (profile, header) in all()? {
        match header {
            Ok(header) => found.push((profile, header)),
            Err(e) => problems.push(e),
        }
    }
    if found.is_empty() { Err(why_unusable(&problems)) } else { Ok(found) }
}

/// A browser profile with its YouTube header, or why it can't be used.
pub type Found = (Profile, Result<String, String>);

/// Every browser profile found, most recently used first, each with its
/// YouTube header or why it can't be used; an explanation when there is none.
pub fn all() -> Result<Vec<Found>, String> {
    let all = profiles();
    if all.is_empty() {
        return Err(if cfg!(windows) {
            "no supported browser found (Chrome, Edge, Brave, Vivaldi, Chromium, Ecosia, Firefox); \
             sign in with Firefox, or use a cookies file"
                .into()
        } else {
            "no supported browser found (Chrome, Chromium, Ecosia, Brave, Edge, Vivaldi, Firefox)".into()
        });
    }
    Ok(all
        .into_iter()
        .map(|profile| {
            let header = profile.youtube_header();
            (profile, header)
        })
        .collect())
}

/// Why none of the browsers can be used. Prefers a "not signed in"
/// explanation over decryption details.
pub fn why_unusable(problems: &[String]) -> String {
    problems.iter().find(|e| e.contains("isn't signed in")).cloned().unwrap_or_else(|| problems.join("; "))
}

/// Opens a copy of the database (the browser keeps it locked while running).
fn open_copy(db: &Path) -> Result<(rusqlite::Connection, TempDir), String> {
    let dir = TempDir::new()?;
    let copy = dir.0.join("cookies.db");
    copy_shared(db, &copy).map_err(|e| {
        // ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION: the running browser holds it exclusively.
        if cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33)) {
            "the browser is open and keeps its cookies locked; close it completely, then try again"
                .to_string()
        } else {
            format!("can't read {}: {e}", db.display())
        }
    })?;
    for suffix in ["-wal", "-journal"] {
        let side = PathBuf::from(format!("{}{suffix}", db.display()));
        if side.is_file() {
            let _ = copy_shared(&side, &dir.0.join(format!("cookies.db{suffix}")));
        }
    }
    let conn = rusqlite::Connection::open_with_flags(&copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("can't open the cookie database: {e}"))?;
    Ok((conn, dir))
}

/// Copies a file the browser may have open. Windows browsers lock their
/// cookie database; reading with full sharing still works.
fn copy_shared(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        let mut src = std::fs::OpenOptions::new().read(true).share_mode(7).open(from)?;
        let mut dst = std::fs::File::create(to)?;
        std::io::copy(&mut src, &mut dst)?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// A private temp directory, removed on drop (it briefly holds a cookie copy).
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        let base = std::env::temp_dir();
        for attempt in 0..100u32 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let dir = base.join(format!("listen-cookies-{}-{nanos}-{attempt}", std::process::id()));
            #[cfg(unix)]
            let created = {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new().mode(0o700).create(&dir)
            };
            #[cfg(not(unix))]
            let created = std::fs::create_dir(&dir);
            if created.is_ok() {
                return Ok(Self(dir));
            }
        }
        Err("can't create a temporary directory".into())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn youtube_host(host: &str) -> bool {
    let h = host.trim_start_matches('.');
    h == "youtube.com" || h == "music.youtube.com"
}

fn read_firefox(db: &Path) -> Result<Vec<Cookie>, String> {
    let (conn, _dir) = open_copy(db)?;
    let mut stmt = conn
        .prepare("SELECT host, name, value, expiry FROM moz_cookies WHERE host LIKE '%youtube.com'")
        .map_err(|e| format!("unexpected Firefox cookie database: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Cookie {
                domain: r.get(0)?,
                name: r.get(1)?,
                value: r.get(2)?,
                expires: r.get::<_, Option<i64>>(3)?.filter(|e| *e > 0).map(|e| e as f64),
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().filter(|c| youtube_host(&c.domain)).collect())
}

fn read_chromium(db: &Path, keyring_apps: &[String]) -> Result<Vec<Cookie>, String> {
    let (conn, _dir) = open_copy(db)?;
    let version: i64 = conn
        .query_row("SELECT value FROM meta WHERE key = 'version'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut stmt = conn
        .prepare(
            "SELECT host_key, name, value, encrypted_value, expires_utc FROM cookies \
             WHERE host_key LIKE '%youtube.com'",
        )
        .map_err(|e| format!("unexpected Chromium cookie database: {e}"))?;
    type Row = (String, String, String, Vec<u8>, i64);
    let rows: Vec<Row> = stmt
        .query_map([], |r| -> rusqlite::Result<Row> {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                r.get(3)?,
                r.get(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|(host, ..)| youtube_host(host))
        .collect();

    let v10 = chromium_key(b"peanuts");
    let mut win_key: Option<Result<[u8; 32], String>> = None; // Windows: looked up once, on demand
    let mut app_bound = 0;
    let mut v11: Option<Option<[u8; 16]>> = None; // looked up once, on demand
    let mut out = Vec::new();
    let mut undecryptable = 0;
    for (host, name, plain, encrypted, expires_utc) in rows {
        let value = if !plain.is_empty() || encrypted.is_empty() {
            Some(plain)
        } else if cfg!(windows) && (encrypted.starts_with(b"v10") || encrypted.starts_with(b"v20")) {
            if encrypted.starts_with(b"v20") {
                app_bound += 1;
                None
            } else {
                match win_key.get_or_insert_with(|| windows_cookie_key(db)) {
                    Ok(key) => decrypt_gcm(key, &encrypted[3..], &host, version),
                    Err(_) => None,
                }
            }
        } else if let Some(data) = encrypted.strip_prefix(b"v10") {
            decrypt(&v10, data, &host, version)
        } else if let Some(data) = encrypted.strip_prefix(b"v11") {
            let key = *v11.get_or_insert_with(|| keyring_key(keyring_apps));
            key.and_then(|k| decrypt(&k, data, &host, version))
        } else {
            None
        };
        match value {
            Some(value) => out.push(Cookie {
                domain: host,
                name,
                value,
                expires: (expires_utc > 0).then(|| (expires_utc / 1_000_000 - WINDOWS_EPOCH_OFFSET) as f64),
            }),
            None => undecryptable += 1,
        }
    }
    if out.is_empty() && app_bound > 0 {
        return Err(APP_BOUND_MESSAGE.into());
    }
    if out.is_empty()
        && undecryptable > 0
        && let Some(Err(e)) = win_key
    {
        return Err(e);
    }
    if out.is_empty() && undecryptable > 0 {
        return Err(format!(
            "the browser's cookies are encrypted with a key Listen can't read ({undecryptable} cookies) — \
             use a cookies file instead"
        ));
    }
    Ok(out)
}

const APP_BOUND_MESSAGE: &str = "this browser protects its cookies with app-bound encryption (v20), \
which only the browser itself can read; sign in with Firefox instead, or use a cookies file";

/// Windows: AES-256-GCM value `nonce(12) ‖ ciphertext ‖ tag(16)` (after the `v10` prefix).
fn decrypt_gcm(key: &[u8; 32], data: &[u8], host: &str, db_version: i64) -> Option<String> {
    use aes_gcm::aead::{Aead, KeyInit};
    if data.len() < 12 + 16 {
        return None;
    }
    let (nonce, ciphertext) = data.split_at(12);
    let cipher = aes_gcm::Aes256Gcm::new(key.into());
    let plain = cipher.decrypt(nonce.into(), ciphertext).ok()?;
    let plain = strip_host_hash(&plain, host, db_version);
    String::from_utf8(plain.to_vec()).ok()
}

/// Chromium 130+ prepends SHA-256(host_key) to the value.
fn strip_host_hash<'a>(plain: &'a [u8], host: &str, db_version: i64) -> &'a [u8] {
    if db_version >= 24 && plain.len() >= 32 {
        use sha2::Digest;
        if plain[..32] == sha2::Sha256::digest(host.as_bytes())[..] {
            return &plain[32..];
        }
    }
    plain
}

/// The DPAPI-protected `os_crypt.encrypted_key` of a `Local State` file
/// (`"DPAPI"` ‖ blob), or an explanation when only the app-bound key exists.
fn local_state_key_blob(local_state: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    let json: serde_json::Value =
        serde_json::from_str(local_state).map_err(|e| format!("unreadable browser key file: {e}"))?;
    let crypt = &json["os_crypt"];
    let Some(key) = crypt["encrypted_key"].as_str() else {
        return Err(if crypt["app_bound_encrypted_key"].is_string() {
            APP_BOUND_MESSAGE.into()
        } else {
            "the browser has no cookie key; use a cookies file instead".into()
        });
    };
    let raw = base64::engine::general_purpose::STANDARD
        .decode(key)
        .map_err(|e| format!("unreadable browser key: {e}"))?;
    raw.strip_prefix(b"DPAPI").map(<[u8]>::to_vec).ok_or_else(|| "unexpected browser key format".to_string())
}

/// Windows: the browser's cookie key from `Local State`, unprotected with DPAPI.
fn windows_cookie_key(db: &Path) -> Result<[u8; 32], String> {
    let state = db
        .ancestors()
        .map(|d| d.join("Local State"))
        .find(|p| p.is_file())
        .ok_or("can't find the browser's Local State file")?;
    let text = std::fs::read_to_string(&state).map_err(|e| format!("can't read {}: {e}", state.display()))?;
    let blob = local_state_key_blob(&text)?;
    let key = dpapi_unprotect(&blob)?;
    <[u8; 32]>::try_from(key.as_slice()).map_err(|_| "unexpected browser key length".to_string())
}

#[cfg(windows)]
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData};
    let input = CRYPT_INTEGER_BLOB { cbData: blob.len() as u32, pbData: blob.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: `input` points at `blob` for the call; `output` is filled by the
    // system with a LocalAlloc buffer that is copied and freed below.
    unsafe {
        let ok = CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut output,
        );
        if ok == 0 {
            return Err(
                "Windows could not unlock the browser's cookie key (DPAPI); use a cookies file".into()
            );
        }
        let key = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as _);
        Ok(key)
    }
}

#[cfg(not(windows))]
fn dpapi_unprotect(_blob: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI is only available on Windows".into())
}

/// PBKDF2-HMAC-SHA1(password, "saltysalt", 1 iteration, 16 bytes): with one
/// iteration that is the first 16 bytes of HMAC(password, salt ‖ 1).
fn chromium_key(password: &[u8]) -> [u8; 16] {
    use hmac::{Hmac, Mac};
    let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(password).expect("any key length");
    mac.update(b"saltysalt\x00\x00\x00\x01");
    let full = mac.finalize().into_bytes();
    let mut key = [0u8; 16];
    key.copy_from_slice(&full[..16]);
    key
}

/// The `v11` key: the browser's password in the Secret Service.
#[cfg(not(unix))]
fn keyring_key(_apps: &[String]) -> Option<[u8; 16]> {
    None
}

#[cfg(unix)]
fn keyring_key(apps: &[String]) -> Option<[u8; 16]> {
    for app in apps {
        for schema in ["chrome_libsecret_os_crypt_password_v2", "chrome_libsecret_os_crypt_password_v1"] {
            let out = std::process::Command::new("secret-tool")
                .args(["lookup", "xdg:schema", schema, "application", app])
                .output()
                .ok()?;
            let password = String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string();
            if out.status.success() && !password.is_empty() {
                return Some(chromium_key(password.as_bytes()));
            }
        }
    }
    None
}

fn decrypt(key: &[u8; 16], data: &[u8], host: &str, db_version: i64) -> Option<String> {
    use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
    let iv = [b' '; 16];
    let mut buf = data.to_vec();
    let plain = cbc::Decryptor::<aes::Aes128>::new(key.into(), &iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .ok()?;
    // Chromium 130+ prepends SHA-256(host_key) to the value.
    let plain = strip_host_hash(plain, host, db_version);
    String::from_utf8(plain.to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn encrypt(key: &[u8; 16], plain: &[u8]) -> Vec<u8> {
        use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
        let iv = [b' '; 16];
        cbc::Encryptor::<aes::Aes128>::new(key.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plain)
    }

    fn gcm_encrypt(key: &[u8; 32], plain: &[u8]) -> Vec<u8> {
        use aes_gcm::aead::{Aead, KeyInit};
        let nonce = [7u8; 12];
        let mut out = nonce.to_vec();
        out.extend(aes_gcm::Aes256Gcm::new(key.into()).encrypt((&nonce).into(), plain).unwrap());
        out
    }

    #[test]
    fn windows_v10_values_decrypt_with_and_without_host_hash() {
        use sha2::Digest;
        let key = [9u8; 32];
        let plain = gcm_encrypt(&key, b"secret");
        assert_eq!(decrypt_gcm(&key, &plain, ".youtube.com", 20).as_deref(), Some("secret"));
        let mut hashed = sha2::Sha256::digest(b".youtube.com").to_vec();
        hashed.extend(b"secret");
        let enc = gcm_encrypt(&key, &hashed);
        assert_eq!(decrypt_gcm(&key, &enc, ".youtube.com", 24).as_deref(), Some("secret"));
        assert_eq!(decrypt_gcm(&[1u8; 32], &enc, ".youtube.com", 24), None);
        assert_eq!(decrypt_gcm(&key, b"short", ".youtube.com", 24), None);
    }

    #[test]
    fn local_state_key_and_app_bound_detection() {
        use base64::Engine as _;
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let ok = format!(r#"{{"os_crypt":{{"encrypted_key":"{}"}}}}"#, b64(b"DPAPIblob"));
        assert_eq!(local_state_key_blob(&ok).unwrap(), b"blob");
        let bound = r#"{"os_crypt":{"app_bound_encrypted_key":"QVBQQg=="}}"#;
        assert!(local_state_key_blob(bound).unwrap_err().contains("app-bound"));
        assert!(local_state_key_blob("{}").is_err());
        let wrong = format!(r#"{{"os_crypt":{{"encrypted_key":"{}"}}}}"#, b64(b"XXXXblob"));
        assert!(local_state_key_blob(&wrong).is_err());
    }

    #[test]
    fn finds_windows_chromium_profiles() {
        let local = temp_home("winchromium");
        let net = local.join("Microsoft/Edge/User Data/Profile 2/Network");
        std::fs::create_dir_all(&net).unwrap();
        std::fs::write(net.join("Cookies"), b"x").unwrap();
        std::fs::create_dir_all(local.join("Google/Chrome/User Data/Default")).unwrap(); // no database
        let found = windows_chromium_profiles(&local);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].browser.as_str(), found[0].profile.as_str()), ("Edge", "Profile 2"));
    }

    /// Chromium's key for "peanuts" (the value yt-dlp and Chromium use).
    #[test]
    fn v10_key() {
        let k = chromium_key(b"peanuts");
        assert_eq!(
            k.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "fd621fe5a2b402539dfa147ca9272778"
        );
    }

    /// A Chromium profile like Ecosia's Flatpak one: v10 values, meta version 24.
    #[cfg(unix)]
    fn fake_chromium_profile(home: &Path, signed_in: bool) -> PathBuf {
        let dir = home.join(".var/app/org.ecosia.Browser/config/EcosiaBrowser/Default");
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("Cookies");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta(key TEXT, value TEXT); INSERT INTO meta VALUES('version','24');
             CREATE TABLE cookies(host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB, expires_utc INTEGER);",
        )
        .unwrap();
        let key = chromium_key(b"peanuts");
        let far = (4_102_444_800i64 + WINDOWS_EPOCH_OFFSET) * 1_000_000; // 2100
        let add = |host: &str, name: &str, value: &str, expires: i64| {
            use sha2::Digest;
            let mut plain = sha2::Sha256::digest(host.as_bytes()).to_vec();
            plain.extend_from_slice(value.as_bytes());
            let mut enc = b"v10".to_vec();
            enc.extend(encrypt(&key, &plain));
            conn.execute(
                "INSERT INTO cookies VALUES(?1, ?2, '', ?3, ?4)",
                rusqlite::params![host, name, enc, expires],
            )
            .unwrap();
        };
        if signed_in {
            add(".youtube.com", "SAPISID", "abc/DEF", far);
        } else {
            add(".youtube.com", "SAPISID", "old", 13_000_000_000_000_000); // 2012: expired
        }
        add(".youtube.com", "SOCS", "CAI", far);
        add("music.youtube.com", "YSC", "session", 0);
        add(".google.com", "NID", "not-youtube", far);
        db
    }

    /// Firefox keeps `cookies.sqlite` plain in `<Profiles>/<id>.default-release/`
    /// (on Windows under `%APPDATA%\\Mozilla\\Firefox\\Profiles`).
    #[test]
    fn reads_a_firefox_profile_folder() {
        let root = temp_home("ff");
        let profile = root.join("abc123.default-release");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::create_dir_all(root.join("Crash Reports")).unwrap(); // no cookies.sqlite: not a profile
        let conn = rusqlite::Connection::open(profile.join("cookies.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_cookies(host TEXT, name TEXT, value TEXT, expiry INTEGER);
             INSERT INTO moz_cookies VALUES('.youtube.com','SAPISID','ff-sid',4102444800);
             INSERT INTO moz_cookies VALUES('.youtube.com','SOCS','CAI',4102444800);
             INSERT INTO moz_cookies VALUES('.google.com','NID','nope',4102444800);",
        )
        .unwrap();
        drop(conn);
        let found = sorted_by_use(firefox_profiles(&root));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].label(), "Firefox");
        assert_eq!(found[0].youtube_header().unwrap(), "SAPISID=ff-sid; SOCS=CAI");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn temp_home(name: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("listen-browser-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        home
    }

    #[cfg(unix)]
    #[test]
    fn reads_ecosia_flatpak_profile() {
        let home = temp_home("ok");
        fake_chromium_profile(&home, true);
        let found = profiles_in(&home);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].label(), "Ecosia");
        let header = found[0].youtube_header().unwrap();
        assert_eq!(header, "SAPISID=abc/DEF; SOCS=CAI; YSC=session");
        // The stored secret only says where to read, never the cookies.
        let secret = found[0].to_secret();
        assert!(!secret.contains("abc/DEF"));
        assert_eq!(Profile::from_secret(&secret), Some(found[0].clone()));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn explains_signed_out_browser() {
        let home = temp_home("out");
        fake_chromium_profile(&home, false);
        let err = profiles_in(&home)[0].youtube_header().unwrap_err();
        assert!(err.contains("isn't signed in"), "{err}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn other_secrets_are_not_profiles() {
        assert_eq!(Profile::from_secret("SAPISID=x; SID=y"), None);
        assert_eq!(Profile::from_secret(r#"[{"domain":".youtube.com"}]"#), None);
    }
}
