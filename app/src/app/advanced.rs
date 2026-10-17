//! The Advanced tab: updater settings, settings export / import, log level,
//! diagnostics export and the caches. (The updater itself is `updater.rs`.)

use super::updater::{self, UpdateState};
use super::{Listen, Message};
use iced::Task;
use listen_core::settings::{ImportReport, LogLevel, UpdateChannel};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    Covers,
    YtmAudio,
    Waveforms,
}

impl CacheKind {
    pub const ALL: [Self; 3] = [Self::Covers, Self::YtmAudio, Self::Waveforms];

    pub fn label(self) -> &'static str {
        match self {
            Self::Covers => "Cover art",
            Self::YtmAudio => "YouTube Music audio",
            Self::Waveforms => "Waveforms",
        }
    }

    /// The folder under the cache directory.
    fn folder(self) -> &'static str {
        match self {
            Self::Covers => "covers",
            Self::YtmAudio => "ytm-audio",
            Self::Waveforms => "waveforms",
        }
    }
}

#[derive(Debug, Clone)]
pub enum AdvancedMsg {
    CheckOnStart(bool),
    Channel(UpdateChannel),
    LogLevel(LogLevel),
    OpenReleases,
    ExportSettings,
    ImportPath(String),
    ImportAsk,
    ImportApply,
    ExportDiagnostics,
    DiagnosticsDone(Result<PathBuf, String>),
    RefreshCaches,
    CacheSizes(Vec<(CacheKind, u64)>),
    /// Soft clear: the in-memory caches.
    ClearMemory,
    ClearAsk(Option<CacheKind>),
    /// Hard clear of one cache, or of all of them.
    Clear(Option<CacheKind>),
    Cleared(Result<(usize, u64), String>),
}

#[derive(Debug, Default)]
pub struct AdvancedState {
    pub update: UpdateState,
    pub import_path: String,
    pub pending_import: Option<ImportReport>,
    pub diagnostics_busy: bool,
    /// Size on disk of each cache; empty until measured.
    pub cache_sizes: Vec<(CacheKind, u64)>,
}

impl AdvancedState {
    pub fn cache_size(&self, kind: CacheKind) -> Option<u64> {
        self.cache_sizes.iter().find(|(k, _)| *k == kind).map(|(_, b)| *b)
    }
}

// ---------------------------------------------------------------- caches

/// Total size of the files under `dir` (0 when it does not exist).
pub fn dir_size(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Deletes the files under `dir` (folders are emptied, the top folder stays). Unfinished
/// downloads (`*.part`) are left alone. Returns (files removed, bytes freed).
pub fn clear_dir(dir: &Path) -> std::io::Result<(usize, u64)> {
    let mut files = 0;
    let mut bytes = 0;
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(e) => return Err(e),
    };
    for e in rd.flatten() {
        let path = e.path();
        let meta = e.metadata()?;
        if meta.is_dir() {
            let (f, b) = clear_dir(&path)?;
            files += f;
            bytes += b;
            let _ = std::fs::remove_dir(&path);
        } else if !path.extension().is_some_and(|x| x == "part") {
            std::fs::remove_file(&path)?;
            files += 1;
            bytes += meta.len();
        }
    }
    Ok((files, bytes))
}

pub fn format_bytes(b: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", UNITS[i]) }
}

// ---------------------------------------------------------------- redaction

const HEADER_KEYS: [&str; 5] =
    ["cookie", "set-cookie", "authorization", "proxy-authorization", "x-goog-authuser"];
/// Names whose value is a secret wherever they appear before `=` or `:`.
const SECRET_KEYS: [&str; 24] = [
    "password",
    "passwd",
    "pwd",
    "token",
    "secret",
    "apikey",
    "api_key",
    "api-key",
    "access_token",
    "refresh_token",
    "sapisid",
    "sapisidhash",
    "psid",
    "signature",
    "authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "credential",
    "credentials",
    "salt",
    "visitor_data",
    "po_token",
    "client_secret",
];
/// Short names that are secrets only in a URL query (`?t=...&s=...`).
const QUERY_KEYS: [&str; 10] = ["t", "s", "u", "p", "key", "sid", "auth", "sig", "user", "username"];
const TOKEN_PREFIXES: [&str; 9] =
    ["ghp_", "gho_", "ghs_", "ghu_", "github_pat_", "tskey-", "ya29.", "sk-", "AIza"];

fn is_delim(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '&' | ',' | ';' | ')' | '}' | ']' | '>' | '<' | '\\')
}

/// Hides what must not leave this computer in a line of log text: credentials in URLs,
/// secret query parameters and `key=value` pairs, cookies, authorization headers and
/// well-known token shapes.
pub fn redact_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let lower: Vec<char> = line.to_ascii_lowercase().chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < n {
        let c = chars[i];
        // user:password@ in a URL
        if c == ':' && chars.get(i + 1) == Some(&'/') && chars.get(i + 2) == Some(&'/') {
            out.push_str("://");
            i += 3;
            let end =
                (i..n).find(|&j| matches!(chars[j], '/' | '?' | '#') || is_delim(chars[j])).unwrap_or(n);
            if let Some(at) = (i..end).rev().find(|&j| chars[j] == '@') {
                out.push_str("***@");
                i = at + 1;
            }
            continue;
        }
        // key=value / key: value
        if (c == '=' || c == ':') && i > 0 {
            let mut k_end = i;
            if matches!(chars[k_end - 1], '"' | '\'') {
                k_end -= 1;
            }
            let k_start = (0..k_end)
                .rev()
                .take_while(|&j| chars[j].is_ascii_alphanumeric() || matches!(chars[j], '_' | '-' | '.'))
                .last()
                .unwrap_or(k_end);
            let key: String = lower[k_start..k_end].iter().collect();
            let in_query = k_start > 0 && matches!(chars[k_start - 1], '?' | '&');
            let secret = SECRET_KEYS.contains(&key.as_str())
                || (in_query && QUERY_KEYS.contains(&key.as_str()))
                || SECRET_KEYS
                    .iter()
                    .any(|s| key.ends_with(&format!("_{s}")) || key.ends_with(&format!("-{s}")));
            if secret && k_end > k_start {
                let mut v = i + 1;
                while v < n && (chars[v] == ' ' || matches!(chars[v], '"' | '\'')) {
                    v += 1;
                }
                let v_end = if HEADER_KEYS.contains(&key.as_str()) {
                    n
                } else {
                    (v..n).find(|&j| is_delim(chars[j])).unwrap_or(n)
                };
                if v_end > v {
                    out.extend(&chars[i..v]);
                    out.push_str("***");
                    i = v_end;
                    continue;
                }
            }
        }
        // `Bearer <token>`
        if lower[i..].starts_with(&['b', 'e', 'a', 'r', 'e', 'r', ' '])
            && (i == 0 || !chars[i - 1].is_alphanumeric())
        {
            out.extend(&chars[i..i + 7]);
            i += 7;
            let end = (i..n).find(|&j| is_delim(chars[j])).unwrap_or(n);
            if end > i {
                out.push_str("***");
                i = end;
            }
            continue;
        }
        // well-known token shapes at a word start
        if i == 0 || !chars[i - 1].is_alphanumeric() {
            let rest: String = chars[i..n.min(i + 12)].iter().collect();
            if let Some(p) = TOKEN_PREFIXES.iter().find(|p| rest.starts_with(**p)) {
                let end = (i..n).find(|&j| is_delim(chars[j])).unwrap_or(n);
                if end - i > p.len() + 6 {
                    out.push_str("***");
                    i = end;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// [`redact_line`] for every line.
pub fn redact_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (n, line) in text.lines().enumerate() {
        if n > 0 {
            out.push('\n');
        }
        out.push_str(&redact_line(line));
    }
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn redact_toml(v: &mut toml::Value) {
    match v {
        toml::Value::Table(t) => {
            for (k, val) in t.iter_mut() {
                let key = k.to_lowercase();
                let secret = listen_core::settings::is_secret_key(&key)
                    || matches!(key.as_str(), "username" | "user" | "email" | "key" | "salt");
                if secret && !val.is_table() && !val.is_array() {
                    *val = toml::Value::String("***".into());
                } else {
                    redact_toml(val);
                }
            }
        }
        toml::Value::Array(a) => a.iter_mut().for_each(redact_toml),
        toml::Value::String(s) => *s = redact_line(s),
        _ => {}
    }
}

/// The config file with credentials removed.
pub fn redact_config(text: &str) -> String {
    match toml::from_str::<toml::Value>(text) {
        Ok(mut v) => {
            redact_toml(&mut v);
            toml::to_string_pretty(&v).unwrap_or_else(|_| redact_text(text))
        }
        Err(_) => redact_text(text),
    }
}

// ---------------------------------------------------------------- zip

/// A minimal zip writer (deflate, one entry after the other).
#[derive(Default)]
pub struct ZipWriter {
    buf: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

fn dos_time() -> (u16, u16) {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    let time = ((now.hour() as u16) << 11) | ((now.minute() as u16) << 5) | (now.second() as u16 / 2);
    let year = (now.year().clamp(1980, 2107) - 1980) as u16;
    (time, (year << 9) | ((now.month() as u16) << 5) | now.day() as u16)
}

impl ZipWriter {
    pub fn add(&mut self, name: &str, data: &[u8]) -> std::io::Result<()> {
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(data)?;
        let packed = enc.finish()?;
        let crc = crc32fast::hash(data);
        let (time, date) = dos_time();
        let offset = self.buf.len() as u32;
        let mut common = Vec::new();
        common.extend(20u16.to_le_bytes()); // version needed
        common.extend(0x0800u16.to_le_bytes()); // UTF-8 names
        common.extend(8u16.to_le_bytes()); // deflate
        common.extend(time.to_le_bytes());
        common.extend(date.to_le_bytes());
        common.extend(crc.to_le_bytes());
        common.extend((packed.len() as u32).to_le_bytes());
        common.extend((data.len() as u32).to_le_bytes());
        common.extend((name.len() as u16).to_le_bytes());
        common.extend(0u16.to_le_bytes()); // extra
        self.buf.extend(0x0403_4b50u32.to_le_bytes());
        self.buf.extend(&common);
        self.buf.extend(name.as_bytes());
        self.buf.extend(&packed);
        self.central.extend(0x0201_4b50u32.to_le_bytes());
        self.central.extend(20u16.to_le_bytes()); // made by
        self.central.extend(&common);
        self.central.extend(0u16.to_le_bytes()); // comment
        self.central.extend(0u16.to_le_bytes()); // disk
        self.central.extend(0u16.to_le_bytes()); // internal attrs
        self.central.extend(0u32.to_le_bytes()); // external attrs
        self.central.extend(offset.to_le_bytes());
        self.central.extend(name.as_bytes());
        self.count += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Vec<u8> {
        let start = self.buf.len() as u32;
        let size = self.central.len() as u32;
        self.buf.extend(&self.central);
        self.buf.extend(0x0605_4b50u32.to_le_bytes());
        self.buf.extend([0u8; 4]); // disk numbers
        self.buf.extend(self.count.to_le_bytes());
        self.buf.extend(self.count.to_le_bytes());
        self.buf.extend(size.to_le_bytes());
        self.buf.extend(start.to_le_bytes());
        self.buf.extend(0u16.to_le_bytes());
        self.buf
    }
}

// ---------------------------------------------------------------- diagnostics

/// Logs are read up to this many bytes from their end.
const LOG_TAIL: u64 = 4 * 1024 * 1024;

fn read_tail(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    if len > LOG_TAIL {
        f.seek(SeekFrom::Start(len - LOG_TAIL))?;
    }
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

/// What a diagnostics export contains, gathered by the caller.
pub struct DiagnosticsInput {
    pub log_dir: Option<PathBuf>,
    pub config_file: Option<PathBuf>,
    pub versions: String,
}

/// The diagnostics zip: `logs/*` and `versions.txt` and `config.toml`, all redacted.
pub fn build_diagnostics(input: &DiagnosticsInput) -> std::io::Result<Vec<u8>> {
    let mut zip = ZipWriter::default();
    zip.add("versions.txt", redact_text(&input.versions).as_bytes())?;
    if let Some(cfg) = &input.config_file
        && let Ok(text) = std::fs::read_to_string(cfg)
    {
        zip.add("config.toml", redact_config(&text).as_bytes())?;
    }
    if let Some(dir) = &input.log_dir
        && let Ok(rd) = std::fs::read_dir(dir)
    {
        let mut names: Vec<_> = rd.flatten().filter(|e| e.path().is_file()).collect();
        names.sort_by_key(|e| e.file_name());
        for e in names {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.contains(".log") {
                continue;
            }
            if let Ok(bytes) = read_tail(&e.path()) {
                zip.add(&format!("logs/{name}"), redact_text(&String::from_utf8_lossy(&bytes)).as_bytes())?;
            }
        }
    }
    Ok(zip.finish())
}

fn first_line(cmd: &str, args: &[&str]) -> String {
    let mut command = std::process::Command::new(listen_core::platform::tool(cmd));
    listen_core::platform::no_window(&mut command);
    command
        .args(args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(str::to_string))
        .unwrap_or_else(|| "not found".into())
}

fn versions_text(channel: UpdateChannel, level: LogLevel) -> String {
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string()))
        })
        .unwrap_or_else(|| std::env::consts::OS.into());
    format!(
        "Listen {}\nOS: {os} ({})\nInstall: {:?}\nUpdate channel: {channel}\nLog level: {level}\nmpv: {}\nyt-dlp: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        updater::detect_install(),
        first_line("mpv", &["--version"]),
        first_line("yt-dlp", &["--version"]),
    )
}

/// The Downloads folder (home when there is none).
fn downloads_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(Path::to_path_buf))
        .or_else(|| directories::UserDirs::new().map(|d| d.home_dir().to_path_buf()))
        .unwrap_or_else(std::env::temp_dir)
}

/// Writes `bytes` as `dir/name`, or `name (2)` etc. if it exists.
fn write_unique(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    std::fs::write(&path, bytes)?;
    Ok(path)
}

fn stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string()
}

// ---------------------------------------------------------------- handler

impl Listen {
    pub(super) fn advanced_boot(&mut self) -> Task<Message> {
        crate::logging::set_level(self.config.settings.advanced.log_level);
        if self.demo || self.ephemeral {
            return Task::none();
        }
        self.helpers_boot();
        Task::batch([self.update_boot(), self.measure_caches()])
    }

    fn cache_dir(&self, kind: CacheKind) -> Option<PathBuf> {
        self.paths.as_ref().map(|p| p.cache_dir.join(kind.folder()))
    }

    fn measure_caches(&self) -> Task<Message> {
        let dirs: Vec<(CacheKind, PathBuf)> =
            CacheKind::ALL.iter().filter_map(|k| self.cache_dir(*k).map(|d| (*k, d))).collect();
        Task::future(async move {
            tokio::task::spawn_blocking(move || dirs.into_iter().map(|(k, d)| (k, dir_size(&d))).collect())
                .await
                .unwrap_or_default()
        })
        .map(|sizes| Message::Advanced(AdvancedMsg::CacheSizes(sizes)))
    }

    pub(super) fn on_advanced(&mut self, msg: AdvancedMsg) -> Task<Message> {
        match msg {
            AdvancedMsg::CheckOnStart(v) => {
                self.config.settings.advanced.update_check_on_start = v;
                self.save_config();
            }
            AdvancedMsg::Channel(c) => {
                self.config.settings.advanced.update_channel = c;
                self.save_config();
            }
            AdvancedMsg::LogLevel(l) => {
                self.config.settings.advanced.log_level = l;
                crate::logging::set_level(l);
                self.save_config();
            }
            AdvancedMsg::OpenReleases => super::album_page::open_url(super::sidebar_state::RELEASES_URL),
            AdvancedMsg::ExportSettings => {
                let text =
                    listen_core::settings::export_json(&self.config.settings, env!("CARGO_PKG_VERSION"));
                match write_unique(
                    &self.export_dir(),
                    &format!("listen-settings-{}", stamp()),
                    "json",
                    text.as_bytes(),
                ) {
                    Ok(p) => self.toast_success(format!("Settings exported to {}.", p.display())),
                    Err(e) => self.toast_error(format!("Could not export the settings: {e}")),
                }
            }
            AdvancedMsg::ImportPath(p) => self.adv.import_path = p,
            AdvancedMsg::ImportAsk => return self.import_ask(),
            AdvancedMsg::ImportApply => {
                let Some(report) = self.adv.pending_import.take() else { return Task::none() };
                let tab = self.config.settings.settings_tab;
                self.config.settings =
                    listen_core::settings::Settings { settings_tab: tab, ..report.settings };
                self.after_settings_replaced();
                self.toast_success(format!("Imported {} settings.", report.applied));
                return self.apply_audio();
            }
            AdvancedMsg::ExportDiagnostics => {
                if self.adv.diagnostics_busy {
                    return Task::none();
                }
                self.adv.diagnostics_busy = true;
                let input = DiagnosticsInput {
                    log_dir: crate::logging::dir(),
                    config_file: self.paths.as_ref().map(|p| p.config_file()),
                    versions: versions_text(
                        self.config.settings.advanced.update_channel,
                        self.config.settings.advanced.log_level,
                    ),
                };
                let dir = self.export_dir();
                return Task::future(async move {
                    tokio::task::spawn_blocking(move || {
                        let zip = build_diagnostics(&input).map_err(|e| e.to_string())?;
                        write_unique(&dir, &format!("listen-diagnostics-{}", stamp()), "zip", &zip)
                            .map_err(|e| e.to_string())
                    })
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()))
                })
                .map(|r| Message::Advanced(AdvancedMsg::DiagnosticsDone(r)));
            }
            AdvancedMsg::DiagnosticsDone(r) => {
                self.adv.diagnostics_busy = false;
                match r {
                    Ok(p) => self.toast_success(format!("Diagnostics saved to {}.", p.display())),
                    Err(e) => self.toast_error(format!("Could not export the diagnostics: {e}")),
                }
            }
            AdvancedMsg::RefreshCaches => return self.measure_caches(),
            AdvancedMsg::CacheSizes(s) => self.adv.cache_sizes = s,
            AdvancedMsg::ClearMemory => {
                self.images.clear_memory();
                self.toast_success("Memory cache cleared; covers load again as you browse.".into());
                let route = self.route.clone();
                return self.on_enter(route);
            }
            AdvancedMsg::ClearAsk(kind) => {
                let what = kind.map_or("all caches".to_string(), |k| k.label().to_lowercase());
                let extra = if matches!(kind, None | Some(CacheKind::YtmAudio)) {
                    " Songs you played from YouTube Music are downloaded again the next time."
                } else {
                    ""
                };
                self.confirm(
                    super::overlay::Confirm::new(
                        "Clear cache",
                        format!(
                            "Delete the cached files of {what}? They are fetched again when needed.{extra}"
                        ),
                        Message::Advanced(AdvancedMsg::Clear(kind)),
                    )
                    .labels("Clear", "Cancel")
                    .danger(),
                );
            }
            AdvancedMsg::Clear(kind) => {
                self.images.clear_memory();
                let dirs: Vec<PathBuf> = match kind {
                    Some(k) => self.cache_dir(k).into_iter().collect(),
                    None => CacheKind::ALL.iter().filter_map(|k| self.cache_dir(*k)).collect(),
                };
                return Task::future(async move {
                    tokio::task::spawn_blocking(move || {
                        let mut total = (0, 0);
                        for d in dirs {
                            let (f, b) = clear_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
                            total = (total.0 + f, total.1 + b);
                        }
                        Ok(total)
                    })
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()))
                })
                .map(|r| Message::Advanced(AdvancedMsg::Cleared(r)));
            }
            AdvancedMsg::Cleared(r) => {
                match r {
                    Ok((files, bytes)) => {
                        self.toast_success(format!("Cache cleared: {files} files, {}.", format_bytes(bytes)))
                    }
                    Err(e) => self.toast_error(format!("Could not clear the cache: {e}")),
                }
                let route = self.route.clone();
                return Task::batch([self.measure_caches(), self.on_enter(route)]);
            }
        }
        Task::none()
    }

    /// Where exports go: Downloads (the temp folder in demo mode and tests).
    fn export_dir(&self) -> PathBuf {
        if self.demo || self.ephemeral { std::env::temp_dir() } else { downloads_dir() }
    }

    /// Everything that follows from the settings being replaced wholesale (import).
    pub(super) fn after_settings_replaced(&mut self) {
        self.accent_input.clear();
        self.settings_search.clear();
        self.discord_id_input = None;
        self.hotkey_recording = None;
        let quality = self.config.settings.playback.stream_quality;
        for source in self.sources.values().chain(self.source.iter()) {
            source.set_stream_quality(quality);
        }
        self.desktop_theme = self
            .config
            .settings
            .appearance
            .follow_desktop_theme
            .then(|| {
                listen_core::theme::desktop::config_dir().and_then(|d| listen_core::theme::desktop::read(&d))
            })
            .flatten();
        crate::logging::set_level(self.config.settings.advanced.log_level);
        self.apply_theme();
        self.save_config();
    }

    fn import_ask(&mut self) -> Task<Message> {
        let path = super::shellexpand_home(self.adv.import_path.trim());
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.toast_error(format!("Could not read {}: {e}", path.display()));
                return Task::none();
            }
        };
        match listen_core::settings::import_json(&self.config.settings, &text) {
            Err(e) => self.toast_error(format!("Cannot import: {e}")),
            Ok(report) => {
                let mut msg = format!(
                    "{} settings will replace your current ones. Servers and sign-ins are not touched.",
                    report.applied
                );
                if !report.issues.is_empty() {
                    msg.push_str(&format!(
                        "\n\n{} could not be imported and keep their value:",
                        report.issues.len()
                    ));
                    for i in report.issues.iter().take(6) {
                        msg.push_str(&format!("\n- {i}"));
                    }
                    if report.issues.len() > 6 {
                        msg.push_str("\n- ...");
                    }
                }
                self.adv.pending_import = Some(report);
                self.confirm(
                    super::overlay::Confirm::new(
                        "Import settings",
                        msg,
                        Message::Advanced(AdvancedMsg::ImportApply),
                    )
                    .labels("Import", "Cancel")
                    .danger(),
                );
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("listen-advanced-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn redacts_url_credentials_and_query_secrets() {
        let r = redact_line(
            "GET https://alex:hunter2@music.example.com/rest/ping?u=alex&t=abc123&s=salt&c=listen&v=1.16",
        );
        assert!(!r.contains("hunter2") && !r.contains("abc123") && !r.contains("alex"), "{r}");
        assert!(
            r.contains("music.example.com/rest/ping") && r.contains("c=listen") && r.contains("v=1.16"),
            "{r}"
        );
        let r = redact_line("fetch https://host/x?api_key=KEY123&id=7 done");
        assert!(!r.contains("KEY123") && r.contains("id=7") && r.ends_with("done"), "{r}");
    }

    #[test]
    fn redacts_cookies_headers_and_tokens() {
        let r = redact_line("headers: Cookie: SAPISID=abc/def; __Secure-3PSID=xyz"); // ship-scan:allow
        assert!(!r.contains("abc/def") && !r.contains("xyz"), "{r}");
        let r = redact_line("Authorization: Bearer eyJhbGciOi.payload.sig");
        assert!(!r.contains("eyJ"), "{r}");
        let r = redact_line("sending with token ghp_abcdefghijklmnopqrstuvwxyz0123456789 ok"); // ship-scan:allow
        assert!(!r.contains("abcdefghij") && r.contains("ok"), "{r}");
        let r = redact_line(r#"{"password": "p4ss word", "name": "x"}"#);
        assert!(!r.contains("p4ss") && r.contains(r#""name": "x""#), "{r}");
        let r = redact_line("Set-Cookie: a=b; Path=/; HttpOnly");
        assert!(!r.contains("a=b"), "{r}");
        let r = redact_line("password=hunter2&user=alex");
        assert!(!r.contains("hunter2"), "{r}");
        assert_eq!(
            redact_line("ordinary: nothing to hide here, 12:30:01"),
            "ordinary: nothing to hide here, 12:30:01"
        );
        assert_eq!(redact_line("no token found"), "no token found");
        assert_eq!(redact_line("naïve é ünïcode https://a:b@h/"), "naïve é ünïcode https://***@h/");
    }

    #[test]
    fn redact_text_keeps_line_structure() {
        assert_eq!(redact_text("a\nb=1\n"), "a\nb=1\n");
        assert_eq!(redact_text("a\npassword=x\nc"), "a\npassword=***\nc");
    }

    #[test]
    fn config_redaction() {
        let cfg = r#"
active_server = "abc"
[[servers]]
id = "abc"
url = "https://alex:pw@nd.example.com:4533"
username = "alex"
password = "plaintext-pw"
[settings.history]
enabled = true
"#; // ship-scan:allow
        let r = redact_config(cfg);
        assert!(!r.contains("plaintext-pw") && !r.contains("alex") && !r.contains(":pw@"), "{r}");
        assert!(r.contains("nd.example.com") && r.contains("enabled = true"), "{r}");
        assert!(!redact_config("not toml = = =\npassword=zzz").contains("zzz"));
    }

    fn read_zip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        use std::io::Read;
        let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]) as usize;
        let u32_at =
            |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as usize;
        let eocd = bytes.len() - 22;
        assert_eq!(u32_at(eocd), 0x0605_4b50);
        let count = u16_at(eocd + 10);
        let mut p = u32_at(eocd + 16);
        let mut out = Vec::new();
        for _ in 0..count {
            assert_eq!(u32_at(p), 0x0201_4b50);
            let (crc, csize, usize_, nlen) = (u32_at(p + 16), u32_at(p + 20), u32_at(p + 24), u16_at(p + 28));
            let local = u32_at(p + 42);
            let name = String::from_utf8(bytes[p + 46..p + 46 + nlen].to_vec()).unwrap();
            assert_eq!(u32_at(local), 0x0403_4b50);
            let data = local + 30 + u16_at(local + 26) + u16_at(local + 28);
            let mut plain = Vec::new();
            flate2::read::DeflateDecoder::new(&bytes[data..data + csize]).read_to_end(&mut plain).unwrap();
            assert_eq!(plain.len(), usize_);
            assert_eq!(crc32fast::hash(&plain) as usize, crc);
            out.push((name, plain));
            p += 46 + nlen + u16_at(p + 30) + u16_at(p + 32);
        }
        out
    }

    #[test]
    fn zip_round_trip() {
        let mut z = ZipWriter::default();
        z.add("a.txt", b"hello hello hello").unwrap();
        z.add("dir/b.txt", &vec![7u8; 100_000]).unwrap();
        z.add("empty", b"").unwrap();
        let files = read_zip(&z.finish());
        assert_eq!(files.len(), 3);
        assert_eq!(files[0], ("a.txt".to_string(), b"hello hello hello".to_vec()));
        assert_eq!(files[1].1.len(), 100_000);
        assert!(files[2].1.is_empty());
    }

    #[test]
    fn diagnostics_zip_is_redacted() {
        let d = scratch("diag");
        let logs = d.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("listen.log"), "INFO ping https://u:secretpw@h/rest?t=tok123&x=1\n")
            .unwrap();
        std::fs::write(logs.join("listen.log.1"), "old\n").unwrap();
        std::fs::write(logs.join("notes.txt"), "ignored").unwrap();
        std::fs::write(
            d.join("config.toml"),
            "[[servers]]\npassword = \"pw-in-config\"\nurl = \"http://h\"\n",
        )
        .unwrap();
        let zip = build_diagnostics(&DiagnosticsInput {
            log_dir: Some(logs),
            config_file: Some(d.join("config.toml")),
            versions: "Listen 1.0\nCookie: SID=zzz\n".into(),
        })
        .unwrap();
        let files = read_zip(&zip);
        let names: Vec<_> = files.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(names, ["versions.txt", "config.toml", "logs/listen.log", "logs/listen.log.1"]);
        let all: String = files.iter().map(|f| String::from_utf8_lossy(&f.1).to_string()).collect();
        for secret in ["secretpw", "tok123", "pw-in-config", "SID=zzz"] {
            assert!(!all.contains(secret), "{secret} leaked:\n{all}");
        }
        assert!(all.contains("x=1") && all.contains("Listen 1.0"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn cache_size_and_clear() {
        let d = scratch("cache");
        let c = d.join("covers");
        std::fs::create_dir_all(c.join("sub")).unwrap();
        std::fs::write(c.join("a"), vec![0u8; 1000]).unwrap();
        std::fs::write(c.join("sub/b"), vec![0u8; 500]).unwrap();
        std::fs::write(c.join("c.part"), vec![0u8; 10]).unwrap();
        assert_eq!(dir_size(&c), 1510);
        assert_eq!(dir_size(&d.join("missing")), 0);
        assert_eq!(clear_dir(&c).unwrap(), (2, 1500));
        assert_eq!(dir_size(&c), 10, "unfinished downloads stay");
        assert!(c.is_dir());
        assert_eq!(clear_dir(&d.join("missing")).unwrap(), (0, 0));
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn unique_export_names() {
        let d = scratch("unique");
        let a = write_unique(&d, "x", "json", b"1").unwrap();
        let b = write_unique(&d, "x", "json", b"2").unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read(&a).unwrap(), b"1");
        let _ = std::fs::remove_dir_all(d);
    }
}
