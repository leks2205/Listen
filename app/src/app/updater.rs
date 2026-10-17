//! In-app updater for GitHub releases of `leks2205/Listen`: download, check against `SHA256SUMS`,
//! then `pkexec dnf install`, replace the `~/.local/bin` binary, or run the Windows installer.
//! The helpers take their URLs as parameters so tests never reach GitHub.

use super::{Listen, Message};
use iced::Task;
use listen_core::settings::UpdateChannel;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const REPO: &str = "leks2205/Listen";
pub const API: &str = "https://api.github.com";
/// The modal key of the update dialog (`ui::overlay`).
pub const MODAL: &str = "update-available";
const OLD_TOKEN_KEY: &str = "updater-github-token";
/// "Remind later" waits this long.
const REMIND_SECS: i64 = 24 * 3600;

// ---------------------------------------------------------------- versions

/// A release version such as `0.28.0` or `0.29.0-beta.1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    /// Parses `v0.28.0`, `0.28`, `1.2.3-beta.1+build5`.
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().trim_start_matches(['v', 'V']);
        let t = t.split('+').next()?;
        let (core, pre) = match t.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            Some(_) => return None,
            None => (t, None),
        };
        let mut nums = core.split('.').map(|p| p.parse::<u64>().ok());
        let major = nums.next()??;
        let minor = nums.next().unwrap_or(Some(0))?;
        let patch = nums.next().unwrap_or(Some(0))?;
        if nums.next().is_some() {
            return None;
        }
        Some(Self { major, minor, patch, pre })
    }

    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }

    /// The running version.
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Self { major: 0, minor: 0, patch: 0, pre: None })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        match &self.pre {
            Some(p) => write!(f, "-{p}"),
            None => Ok(()),
        }
    }
}

fn cmp_pre(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.split('.'), b.split('.'));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let o = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(o.major, o.minor, o.patch)).then_with(|| {
            match (&self.pre, &o.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => cmp_pre(a, b),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

// ---------------------------------------------------------------- releases

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    /// API URL (`Accept: application/octet-stream` downloads it, also from a private repo).
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub version: Version,
    pub prerelease: bool,
    pub body: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

/// Parses the `GET /repos/{repo}/releases` answer; drafts and tags that are no version are dropped.
pub fn parse_releases(json: &str) -> Result<Vec<Release>, String> {
    let raw: Vec<RawRelease> = serde_json::from_str(json).map_err(|e| format!("unexpected answer: {e}"))?;
    Ok(raw
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let version = Version::parse(&r.tag_name)?;
            Some(Release {
                prerelease: r.prerelease || version.is_prerelease(),
                tag: r.tag_name,
                version,
                body: r.body.unwrap_or_default(),
                page: r.html_url,
                assets: r
                    .assets
                    .into_iter()
                    .map(|a| Asset { name: a.name, url: a.url, size: a.size })
                    .collect(),
            })
        })
        .collect())
}

/// The newest release on `channel` that is newer than `current`.
pub fn pick_update(releases: &[Release], current: &Version, channel: UpdateChannel) -> Option<Release> {
    releases
        .iter()
        .filter(|r| channel == UpdateChannel::Beta || !r.prerelease)
        .filter(|r| r.version > *current)
        .max_by(|a, b| a.version.cmp(&b.version))
        .cloned()
}

/// What to download for an install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Package {
    Rpm,
    Tarball,
    /// `Listen-Setup-<version>.exe`, the per-user Inno Setup installer.
    WindowsInstaller,
}

pub const SUMS_NAME: &str = "SHA256SUMS";

/// The package asset and the `SHA256SUMS` asset. `dist_tag` (e.g. `fc44`) prefers that Fedora's RPM.
pub fn select_assets(
    release: &Release,
    package: Package,
    dist_tag: Option<&str>,
) -> Result<(Asset, Asset), String> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(format!("releases are built for x86_64 only, this is {}", std::env::consts::ARCH));
    }
    let candidates: Vec<&Asset> = release
        .assets
        .iter()
        .filter(|a| match package {
            Package::Rpm => a.name.starts_with("listen-") && a.name.ends_with(".x86_64.rpm"),
            Package::Tarball => a.name.starts_with("listen-") && a.name.ends_with("-x86_64.tar.gz"),
            Package::WindowsInstaller => a.name.starts_with("Listen-Setup-") && a.name.ends_with(".exe"),
        })
        .collect();
    let chosen = dist_tag
        .and_then(|d| candidates.iter().find(|a| a.name.contains(&format!(".{d}."))))
        .or(candidates.first())
        .ok_or_else(|| {
            format!(
                "release {} has no {} for this system",
                release.tag,
                match package {
                    Package::Rpm => "RPM",
                    Package::Tarball => "tarball",
                    Package::WindowsInstaller => "Windows installer",
                }
            )
        })?;
    let sums = release.assets.iter().find(|a| a.name == SUMS_NAME).ok_or_else(|| {
        format!("release {} has no {SUMS_NAME}; refusing to install unverified files", release.tag)
    })?;
    Ok(((*chosen).clone(), sums.clone()))
}

// ---------------------------------------------------------------- checksums

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `sha256sum` output: `<hash>  <name>` (or `<hash> *<name>`).
pub fn parse_sums(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let (hash, name) = l.trim().split_once(char::is_whitespace)?;
            let name = name.trim().trim_start_matches('*');
            (hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) && !name.is_empty())
                .then(|| (name.to_string(), hash.to_lowercase()))
        })
        .collect()
}

/// Checks `file` against the entry for `name` in a `SHA256SUMS` text.
pub fn verify_sha256(file: &Path, name: &str, sums: &str) -> Result<(), String> {
    let expected =
        parse_sums(sums).remove(name).ok_or_else(|| format!("{SUMS_NAME} has no entry for {name}"))?;
    let actual = sha256_file(file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "checksum mismatch for {name} (expected {expected}, got {actual}); the download was discarded"
        ))
    }
}

// ---------------------------------------------------------------- install modes

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMode {
    /// Installed system-wide from the RPM: `pkexec dnf install`.
    Rpm,
    /// A binary the user owns (`~/.local/bin/listen`): replaced in place.
    Local(PathBuf),
    /// Installed by the Windows installer (`unins000.exe` beside it); the new one runs silently over it.
    WindowsInstaller,
    /// Nothing Listen can update by itself, and why.
    Unsupported(String),
}

/// How the program at `exe` is installed.
pub fn classify_exe(exe: &Path, home: Option<&Path>) -> InstallMode {
    if cfg!(windows) {
        if exe.components().any(|c| c.as_os_str() == "target") {
            return InstallMode::Unsupported(
                "This is a development build; update it with git and cargo.".into(),
            );
        }
        if exe.parent().is_some_and(|dir| dir.join("unins000.exe").is_file()) {
            return InstallMode::WindowsInstaller;
        }
        return InstallMode::Unsupported(
            "This copy of Listen was not set up by the installer; download the new installer from the release page."
                .into(),
        );
    }
    let exe =
        exe.to_str().map(|s| PathBuf::from(s.trim_end_matches(" (deleted)"))).unwrap_or(exe.to_path_buf());
    if exe.components().any(|c| c.as_os_str() == "target") {
        return InstallMode::Unsupported("This is a development build; update it with git and cargo.".into());
    }
    if exe.starts_with("/usr/") {
        return InstallMode::Rpm;
    }
    if let Some(home) = home
        && exe.starts_with(home)
    {
        return InstallMode::Local(exe);
    }
    InstallMode::Unsupported(format!(
        "Listen runs from {}; update it with the tool you installed it with.",
        exe.display()
    ))
}

pub fn detect_install() -> InstallMode {
    match std::env::current_exe() {
        Ok(exe) => classify_exe(&exe, listen_core::platform::home_dir().as_deref()),
        Err(e) => InstallMode::Unsupported(format!("cannot find the running program: {e}")),
    }
}

/// `fc44` on Fedora 44.
pub fn fedora_dist_tag() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    dist_tag_from_os_release(&text)
}

pub fn dist_tag_from_os_release(text: &str) -> Option<String> {
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")).map(|v| v.trim().trim_matches('"').to_string()))
    };
    (get("ID")? == "fedora").then(|| format!("fc{}", get("VERSION_ID").unwrap_or_default()))
}

/// Replaces `target` with the file `new` atomically: copy next to it, then rename over it.
/// A running program keeps working (its old file stays open until it exits).
pub fn replace_binary(new: &Path, target: &Path) -> std::io::Result<()> {
    let dir = target.parent().ok_or_else(|| std::io::Error::other("target has no folder"))?;
    let staged = dir.join(format!(".listen-update-{}", std::process::id()));
    let result = (|| {
        std::fs::copy(new, &staged)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&staged, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    result
}

/// Whether running `binary --version` prints `version` (a broken download is not installed).
pub fn binary_reports(binary: &Path, version: &Version) -> Result<(), String> {
    let out = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|e| format!("the new program does not start: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if out.status.success() && text.contains(&version.to_string()) {
        Ok(())
    } else {
        Err(format!("the new program reports \"{}\", not {version}", text.trim()))
    }
}

// ---------------------------------------------------------------- network

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("Listen/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

fn request(c: &reqwest::Client, url: &str, accept: &str) -> reqwest::RequestBuilder {
    c.get(url).header("Accept", accept).header("X-GitHub-Api-Version", "2022-11-28")
}

/// Lists the releases of `repo` (`api` is the API root, a parameter for tests).
pub async fn fetch_releases(api: &str, repo: &str) -> Result<Vec<Release>, String> {
    let c = client()?;
    let url = format!("{api}/repos/{repo}/releases?per_page=30");
    let resp = request(&c, &url, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("could not reach GitHub: {}", e.without_url()))?;
    match resp.status().as_u16() {
        200 => parse_releases(&resp.text().await.map_err(|e| e.to_string())?),
        403 | 429 => Err("GitHub is rate-limiting update checks right now; try again in an hour.".into()),
        404 => Err(format!("GitHub cannot find the releases of {repo}.")),
        s => Err(format!("GitHub answered {s}")),
    }
}

/// Downloads `asset` into `dest`.
pub async fn download_asset(asset: &Asset, dest: &Path) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let c = client()?;
    let mut resp = request(&c, &asset.url, "application/octet-stream")
        .send()
        .await
        .map_err(|e| format!("download of {} failed: {}", asset.name, e.without_url()))?;
    if !resp.status().is_success() {
        return Err(format!("download of {} failed: HTTP {}", asset.name, resp.status().as_u16()));
    }
    let mut out = tokio::fs::File::create(dest).await.map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut done = 0u64;
    while let Some(chunk) =
        resp.chunk().await.map_err(|e| format!("download of {} failed: {}", asset.name, e.without_url()))?
    {
        done += chunk.len() as u64;
        out.write_all(&chunk).await.map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    out.flush().await.map_err(|e| e.to_string())?;
    if asset.size > 0 && done != asset.size {
        return Err(format!("download of {} is incomplete ({done} of {} bytes)", asset.name, asset.size));
    }
    Ok(())
}

async fn run(cmd: &mut tokio::process::Command, what: &str) -> Result<(), String> {
    let out = cmd.output().await.map_err(|e| format!("cannot run {what}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    Err(match out.status.code() {
        Some(126) | Some(127) if what == "pkexec" => "the installation was cancelled".to_string(),
        code => format!(
            "{what} failed ({}): {last}",
            code.map(|c| c.to_string()).unwrap_or_else(|| "signal".into())
        ),
    })
}

/// Result of a finished install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    /// New RPM installed; the running program is still the old one.
    Rpm,
    /// Binary replaced.
    Local,
    /// The new installer was started; Listen has to exit so it can replace the files.
    Windows,
}

/// Downloads, verifies and installs `release`. `work` is a scratch folder (removed afterwards).
pub async fn install(release: Release, mode: InstallMode, work: PathBuf) -> Result<Installed, String> {
    let package = match &mode {
        InstallMode::Rpm => Package::Rpm,
        InstallMode::Local(_) => Package::Tarball,
        InstallMode::WindowsInstaller => Package::WindowsInstaller,
        InstallMode::Unsupported(why) => return Err(why.clone()),
    };
    let (asset, sums) = select_assets(&release, package, fedora_dist_tag().as_deref())?;
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let result = install_in(&release, &mode, &work, &asset, &sums).await;
    // The installer keeps running from `work` after Listen has exited; the next update clears it.
    if !matches!(result, Ok(Installed::Windows)) {
        let _ = std::fs::remove_dir_all(&work);
    }
    result
}

async fn install_in(
    release: &Release,
    mode: &InstallMode,
    work: &Path,
    asset: &Asset,
    sums: &Asset,
) -> Result<Installed, String> {
    let sums_path = work.join(SUMS_NAME);
    download_asset(sums, &sums_path).await?;
    let file = work.join(&asset.name);
    download_asset(asset, &file).await?;
    let sums_text = std::fs::read_to_string(&sums_path).map_err(|e| e.to_string())?;
    verify_sha256(&file, &asset.name, &sums_text)?;
    match mode {
        InstallMode::Rpm => {
            run(tokio::process::Command::new("pkexec").args(["dnf", "install", "-y"]).arg(&file), "pkexec")
                .await?;
            Ok(Installed::Rpm)
        }
        InstallMode::Local(target) => {
            let unpacked = work.join("unpacked");
            std::fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
            run(tokio::process::Command::new("tar").arg("-xzf").arg(&file).arg("-C").arg(&unpacked), "tar")
                .await?;
            let new = find_binary(&unpacked).ok_or("the release tarball contains no listen program")?;
            binary_reports(&new, &release.version)?;
            replace_binary(&new, target)
                .map_err(|e| format!("could not replace {}: {e}", target.display()))?;
            Ok(Installed::Local)
        }
        InstallMode::WindowsInstaller => {
            run_windows_installer(&file)?;
            Ok(Installed::Windows)
        }
        InstallMode::Unsupported(why) => Err(why.clone()),
    }
}

/// The silent per-user installer arguments (no prompts, no reboot, restarts Listen when done).
pub const WINDOWS_INSTALLER_ARGS: [&str; 5] =
    ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/RELAUNCH"];

/// Starts the downloaded installer detached from Listen, so it survives Listen's exit.
fn run_windows_installer(installer: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new(installer);
    cmd.args(WINDOWS_INSTALLER_ARGS);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        cmd.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("cannot start the installer: {e}"))
}

/// The `listen` file one folder down in an unpacked release tarball.
pub fn find_binary(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join("listen");
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find_map(|p| {
        let c = p.join("listen");
        (p.is_dir() && c.is_file()).then_some(c)
    })
}

// ---------------------------------------------------------------- app state

/// Where the install of an offered update stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Installing,
    /// Installed; Listen has to restart to run the new version.
    Done(Installed),
    Failed(String),
}

#[derive(Debug, Default)]
pub struct UpdateState {
    pub checking: bool,
    pub available: Option<Release>,
    /// "Up to date" / the error of the last check.
    pub status: Option<String>,
    pub phase: Phase,
}

#[derive(Debug, Clone)]
pub enum UpdateMsg {
    CheckNow,
    /// `manual`: user-triggered; reports "up to date" and ignores a skipped version.
    Checked {
        manual: bool,
        result: Result<Vec<Release>, String>,
    },
    Open,
    Install,
    Installed(Result<Installed, String>),
    RemindLater,
    SkipVersion,
    /// Close the dialog without choosing.
    Dismiss,
    Restart,
}

impl Listen {
    fn updates_enabled(&self) -> bool {
        !self.demo && !self.ephemeral
    }

    fn check_task(&self, manual: bool) -> Task<Message> {
        Task::future(async move { fetch_releases(API, REPO).await })
            .map(move |result| Message::Update(UpdateMsg::Checked { manual, result }))
    }

    pub(super) fn update_boot(&mut self) -> Task<Message> {
        if !self.updates_enabled() {
            return Task::none();
        }
        // Delete the GitHub token that older versions kept in the keyring.
        let dir = self.paths.as_ref().map(|p| p.config_dir.clone());
        let forget = Task::future(async move {
            if let Some(d) = dir {
                crate::secrets::Secrets::new(&d).delete(&listen_core::model::ServerId(OLD_TOKEN_KEY.into()));
            }
        })
        .discard();
        if !self.config.settings.advanced.update_check_on_start {
            return forget;
        }
        self.adv.update.checking = true;
        Task::batch([forget, self.check_task(false)])
    }

    pub(super) fn on_update(&mut self, msg: UpdateMsg) -> Task<Message> {
        match msg {
            UpdateMsg::CheckNow => {
                if !self.updates_enabled() {
                    self.toast_info("Updates are not checked in demo mode.".into());
                    return Task::none();
                }
                if self.adv.update.checking || self.adv.update.phase == Phase::Installing {
                    return Task::none();
                }
                self.adv.update.checking = true;
                self.adv.update.status = None;
                return self.check_task(true);
            }
            UpdateMsg::Checked { manual, result } => {
                self.adv.update.checking = false;
                let a = &self.config.settings.advanced;
                match result {
                    Err(e) => {
                        tracing::warn!("update check failed: {e}");
                        self.adv.update.status = Some(e.clone());
                        if manual {
                            self.toast_error(e);
                        }
                    }
                    Ok(releases) => match pick_update(&releases, &Version::current(), a.update_channel) {
                        None => {
                            self.adv.update.available = None;
                            self.adv.update.status =
                                Some(format!("Listen {} is up to date.", Version::current()));
                            if manual {
                                self.toast_success("Listen is up to date.".into());
                            }
                        }
                        Some(rel) => {
                            let hidden = a.skipped_version == rel.tag
                                || listen_core::history::now_secs() < a.update_remind_after;
                            self.adv.update.status = Some(format!("Version {} is available.", rel.version));
                            if self.adv.update.available.as_ref().map(|r| &r.tag) != Some(&rel.tag) {
                                self.adv.update.phase = Phase::Idle;
                            }
                            self.adv.update.available = Some(rel);
                            if manual || !hidden {
                                self.open_modal(MODAL);
                            }
                        }
                    },
                }
            }
            UpdateMsg::Open => {
                if self.adv.update.available.is_some() {
                    self.open_modal(MODAL);
                }
            }
            UpdateMsg::Install => {
                let Some(release) = self.adv.update.available.clone() else { return Task::none() };
                if self.adv.update.phase == Phase::Installing || !self.updates_enabled() {
                    return Task::none();
                }
                let mode = detect_install();
                let Some(cache) = self.paths.as_ref().map(|p| p.cache_dir.join("updates")) else {
                    return Task::none();
                };
                self.adv.update.phase = Phase::Installing;
                return Task::future(install(release, mode, cache))
                    .map(|r| Message::Update(UpdateMsg::Installed(r)));
            }
            UpdateMsg::Installed(result) => match result {
                Ok(Installed::Windows) => {
                    // The installer replaces the files and starts the new Listen.
                    self.save_queue_now();
                    self.save_window_state_now();
                    return iced::exit();
                }
                Ok(done) => {
                    self.adv.update.phase = Phase::Done(done);
                    self.toast_success("Update installed. Restart Listen to use it.".into());
                }
                Err(e) => {
                    tracing::warn!("update failed: {e}");
                    self.adv.update.phase = Phase::Failed(e);
                }
            },
            UpdateMsg::RemindLater => {
                self.config.settings.advanced.update_remind_after =
                    listen_core::history::now_secs() + REMIND_SECS;
                self.close_modal();
                self.save_config();
            }
            UpdateMsg::SkipVersion => {
                if let Some(r) = &self.adv.update.available {
                    self.config.settings.advanced.skipped_version = r.tag.clone();
                }
                self.close_modal();
                self.save_config();
            }
            UpdateMsg::Dismiss => self.close_modal(),
            UpdateMsg::Restart => {
                if matches!(self.adv.update.phase, Phase::Done(_)) {
                    return self.update(Message::Shell(super::settings_shell::ShellMsg::Relaunch));
                }
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("listen-updater-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(v("v0.28.0"), Version { major: 0, minor: 28, patch: 0, pre: None });
        assert_eq!(v("1.2"), v("1.2.0"));
        assert!(Version::parse("abc").is_none() && Version::parse("1.2.3.4").is_none());
        assert!(v("0.28.0") > v("0.27.9"));
        assert!(v("0.10.0") > v("0.9.0"), "numeric, not textual");
        assert!(v("1.0.0") > v("1.0.0-beta.1"));
        assert!(v("1.0.0-beta.2") > v("1.0.0-beta.1"));
        assert!(v("1.0.0-beta.10") > v("1.0.0-beta.9"));
        assert!(v("1.0.0-rc.1") > v("1.0.0-beta.9"));
        assert!(v("1.0.0-beta") < v("1.0.0-beta.1"));
        assert_eq!(v("1.0.0+build5"), v("1.0.0"));
        assert_eq!(v("0.29.0-beta.1").to_string(), "0.29.0-beta.1");
    }

    const RELEASES: &str = r###"[
      {"tag_name":"v0.30.0-beta.1","draft":false,"prerelease":true,"body":"beta notes","html_url":"https://github.com/leks2205/Listen/releases/tag/v0.30.0-beta.1","assets":[]},
      {"tag_name":"v0.29.0","draft":false,"prerelease":false,"body":"## 0.29\n- new","html_url":"u","assets":[
        {"name":"listen-0.29.0-1.fc44.x86_64.rpm","url":"https://api.github.com/a/1","size":100},
        {"name":"listen-0.29.0-1.fc43.x86_64.rpm","url":"https://api.github.com/a/5","size":100},
        {"name":"listen-0.29.0-x86_64.tar.gz","url":"https://api.github.com/a/2","size":50},
        {"name":"SHA256SUMS","url":"https://api.github.com/a/3","size":10}]},
      {"tag_name":"v0.31.0","draft":true,"prerelease":false,"body":null,"assets":[]},
      {"tag_name":"nightly","draft":false,"prerelease":false,"assets":[]},
      {"tag_name":"v0.27.0","draft":false,"prerelease":false,"body":"old","assets":[]}
    ]"###;

    #[test]
    fn releases_parse_without_drafts_and_odd_tags() {
        let r = parse_releases(RELEASES).unwrap();
        let tags: Vec<_> = r.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(tags, ["v0.30.0-beta.1", "v0.29.0", "v0.27.0"]);
        assert!(r[0].prerelease && !r[1].prerelease);
        assert_eq!(r[1].assets.len(), 4);
        assert!(parse_releases("{}").is_err());
    }

    #[test]
    fn channel_selection() {
        let r = parse_releases(RELEASES).unwrap();
        let cur = v("0.27.0");
        assert_eq!(pick_update(&r, &cur, UpdateChannel::Stable).unwrap().tag, "v0.29.0");
        assert_eq!(pick_update(&r, &cur, UpdateChannel::Beta).unwrap().tag, "v0.30.0-beta.1");
        assert!(pick_update(&r, &v("0.29.0"), UpdateChannel::Stable).is_none());
        assert_eq!(pick_update(&r, &v("0.29.0"), UpdateChannel::Beta).unwrap().tag, "v0.30.0-beta.1");
        assert!(pick_update(&r, &v("0.30.0-beta.1"), UpdateChannel::Beta).is_none());
        // a beta user is offered the final release of the same version
        let r2 = parse_releases(r#"[{"tag_name":"v0.30.0","prerelease":false,"assets":[]}]"#).unwrap();
        assert!(pick_update(&r2, &v("0.30.0-beta.1"), UpdateChannel::Stable).is_some());
        assert!(pick_update(&[], &cur, UpdateChannel::Beta).is_none());
    }

    #[test]
    fn asset_selection() {
        if std::env::consts::ARCH != "x86_64" {
            return;
        }
        let rel = parse_releases(RELEASES).unwrap().remove(1);
        let (rpm, sums) = select_assets(&rel, Package::Rpm, Some("fc43")).unwrap();
        assert_eq!(rpm.name, "listen-0.29.0-1.fc43.x86_64.rpm");
        assert_eq!(sums.name, SUMS_NAME);
        assert_eq!(
            select_assets(&rel, Package::Rpm, Some("fc99")).unwrap().0.name,
            "listen-0.29.0-1.fc44.x86_64.rpm"
        );
        assert_eq!(
            select_assets(&rel, Package::Rpm, None).unwrap().0.name,
            "listen-0.29.0-1.fc44.x86_64.rpm"
        );
        assert_eq!(
            select_assets(&rel, Package::Tarball, None).unwrap().0.name,
            "listen-0.29.0-x86_64.tar.gz"
        );
        let mut no_sums = rel.clone();
        no_sums.assets.retain(|a| a.name != SUMS_NAME);
        assert!(select_assets(&no_sums, Package::Rpm, None).unwrap_err().contains("SHA256SUMS"));
        let mut no_rpm = rel;
        no_rpm.assets.retain(|a| !a.name.ends_with(".rpm"));
        assert!(select_assets(&no_rpm, Package::Rpm, None).unwrap_err().contains("no RPM"));
    }

    #[test]
    fn windows_installer_asset_selection() {
        let json = r#"[{"tag_name":"v0.30.0","draft":false,"prerelease":false,"body":"","assets":[
            {"name":"listen-0.30.0-1.fc44.x86_64.rpm","url":"https://api.github.com/a/1","size":1},
            {"name":"Listen-Setup-0.30.0.exe","url":"https://api.github.com/a/7","size":9},
            {"name":"SHA256SUMS","url":"https://api.github.com/a/8","size":1}]}]"#;
        let rel = parse_releases(json).unwrap().remove(0);
        let (setup, sums) = select_assets(&rel, Package::WindowsInstaller, None).unwrap();
        assert_eq!((setup.name.as_str(), sums.name.as_str()), ("Listen-Setup-0.30.0.exe", SUMS_NAME));
        assert_eq!(
            select_assets(&rel, Package::Rpm, None).unwrap().0.name,
            "listen-0.30.0-1.fc44.x86_64.rpm"
        );
        let mut none = rel;
        none.assets.retain(|a| !a.name.ends_with(".exe"));
        assert!(
            select_assets(&none, Package::WindowsInstaller, None).unwrap_err().contains("Windows installer")
        );
        assert_eq!(WINDOWS_INSTALLER_ARGS[0], "/VERYSILENT");
    }

    #[test]
    fn dist_tag() {
        assert_eq!(dist_tag_from_os_release("ID=fedora\nVERSION_ID=44\n").as_deref(), Some("fc44"));
        assert_eq!(dist_tag_from_os_release("ID=\"fedora\"\nVERSION_ID=\"43\"").as_deref(), Some("fc43"));
        assert_eq!(dist_tag_from_os_release("ID=ubuntu\nVERSION_ID=24.04"), None);
    }

    #[test]
    fn sha256_verification() {
        let d = scratch("sha");
        let f = d.join("listen-1.rpm");
        std::fs::write(&f, b"hello").unwrap();
        // sha256("hello")
        let good = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert_eq!(sha256_file(&f).unwrap(), good);
        let sums = format!("{good}  listen-1.rpm\n{} *other\nnot a line\n", "0".repeat(64));
        assert!(verify_sha256(&f, "listen-1.rpm", &sums).is_ok());
        assert!(
            verify_sha256(&f, "listen-1.rpm", &sums.to_uppercase().replace("LISTEN-1.RPM", "listen-1.rpm"))
                .is_ok()
        );
        assert!(verify_sha256(&f, "other", &sums).unwrap_err().contains("mismatch"));
        assert!(verify_sha256(&f, "missing", &sums).unwrap_err().contains("no entry"));
        std::fs::write(&f, b"tampered").unwrap();
        assert!(verify_sha256(&f, "listen-1.rpm", &sums).unwrap_err().contains("mismatch"));
        assert_eq!(parse_sums(&sums).len(), 2);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn install_modes() {
        let home = Path::new("/home/alex");
        if cfg!(windows) {
            assert!(
                matches!(classify_exe(Path::new("C:/x/listen.exe"), None), InstallMode::Unsupported(m) if m.contains("installer"))
            );
            return;
        }
        assert_eq!(classify_exe(Path::new("/usr/bin/listen"), Some(home)), InstallMode::Rpm);
        assert_eq!(classify_exe(Path::new("/usr/bin/listen (deleted)"), Some(home)), InstallMode::Rpm);
        let local = PathBuf::from("/home/alex/.local/bin/listen");
        assert_eq!(classify_exe(&local, Some(home)), InstallMode::Local(local.clone()));
        assert!(matches!(
            classify_exe(Path::new("/home/alex/Listen/target/release/listen"), Some(home)),
            InstallMode::Unsupported(_)
        ));
        assert!(matches!(classify_exe(Path::new("/opt/listen"), Some(home)), InstallMode::Unsupported(_)));
    }

    #[test]
    fn binary_replace_is_atomic_and_leaves_no_staging_file() {
        let d = scratch("replace");
        let target = d.join("listen");
        let new = d.join("listen.new");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        replace_binary(&new, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read(&new).unwrap(), b"new", "the source stays");
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
        }
        // a missing source leaves the target alone
        assert!(replace_binary(&d.join("nope"), &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(d);
    }

    #[cfg(unix)]
    #[test]
    fn new_binary_must_report_its_version() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("report");
        let bin = d.join("listen");
        std::fs::write(&bin, "#!/bin/sh\necho 'listen 0.29.0'\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(binary_reports(&bin, &v("0.29.0")).is_ok());
        assert!(binary_reports(&bin, &v("0.30.0")).is_err());
        assert!(binary_reports(&d.join("missing"), &v("0.29.0")).is_err());
        let sub = d.join("listen-0.29.0-x86_64");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::rename(&bin, sub.join("listen")).unwrap();
        assert_eq!(find_binary(&d), Some(sub.join("listen")));
        let _ = std::fs::remove_dir_all(d);
    }

    /// Serves `body` to every request on a local port; returns the port and the requests seen.
    async fn mock(
        body: Vec<u8>,
        status: &'static str,
    ) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                log.lock().unwrap().push(String::from_utf8_lossy(&buf[..n]).to_string());
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
                let _ = s.shutdown().await;
            }
        });
        (port, seen)
    }

    #[tokio::test]
    async fn fetch_asks_the_given_server_without_credentials() {
        let (port, seen) = mock(RELEASES.as_bytes().to_vec(), "200 OK").await;
        let api = format!("http://127.0.0.1:{port}");
        let r = fetch_releases(&api, "o/r").await.unwrap();
        assert_eq!(r.len(), 3);
        let req = seen.lock().unwrap()[0].to_lowercase();
        assert!(req.starts_with("get /repos/o/r/releases"), "{req}");
        assert!(!req.contains("authorization"), "{req}");
        let (port, _) = mock(b"{}".to_vec(), "404 Not Found").await;
        let e = fetch_releases(&format!("http://127.0.0.1:{port}"), "o/r").await.unwrap_err();
        assert!(e.contains("o/r"), "{e}");
        let (port, _) = mock(b"{}".to_vec(), "403 Forbidden").await;
        let e = fetch_releases(&format!("http://127.0.0.1:{port}"), "o/r").await.unwrap_err();
        assert!(e.contains("rate-limiting"), "{e}");
    }

    #[tokio::test]
    async fn download_checks_the_size() {
        let (port, _) = mock(b"hello".to_vec(), "200 OK").await;
        let d = scratch("download");
        let mut a = Asset { name: "f".into(), url: format!("http://127.0.0.1:{port}/a"), size: 5 };
        download_asset(&a, &d.join("f")).await.unwrap();
        assert_eq!(std::fs::read(d.join("f")).unwrap(), b"hello");
        a.size = 9;
        assert!(download_asset(&a, &d.join("g")).await.unwrap_err().contains("incomplete"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn install_of_unsupported_mode_fails_without_touching_anything() {
        let rel = parse_releases(RELEASES).unwrap().remove(1);
        let d = scratch("unsupported");
        let e = install(rel, InstallMode::Unsupported("nope".into()), d.join("w")).await.unwrap_err();
        assert_eq!(e, "nope");
        assert!(!d.join("w").exists());
        let _ = std::fs::remove_dir_all(d);
    }
}
