//! Audio stream URLs via `yt-dlp` (it keeps up with YouTube's signature / PO-token changes). URLs are
//! cached until shortly before their `expire=` time (usually ~6 h).

use listen_core::analysis::FileId;
use listen_core::settings::StreamQuality;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct Resolved {
    pub url: String,
    pub expires_at: Option<SystemTime>,
    /// Which exact file the URL serves (itag, size, duration).
    pub facts: UrlFacts,
}

/// The audio format, pinned so playback and analysis decode the same bytes:
/// Opus 251, else AAC 140, else the best plain-https audio that is not a
/// "stable volume" (`-drc`) variant, else anything (HLS: plays, not fetchable).
pub const FORMAT: &str = "251/140/bestaudio[format_id!*=drc][protocol=https]/bestaudio";

/// Playback-only formats for a capped quality (analysis keeps [`FORMAT`]):
/// Normal = AAC 128 kbit/s (140), Low = Opus ~70 kbit/s (250) or AAC 48 kbit/s (139).
pub fn quality_format(quality: StreamQuality) -> Option<&'static str> {
    match quality {
        StreamQuality::Original | StreamQuality::High => None,
        StreamQuality::Normal => Some("140/bestaudio[abr<=130][protocol=https]/bestaudio[abr<=130]"),
        StreamQuality::Low => Some("250/139/worstaudio[protocol=https]/worstaudio"),
    }
}
/// The video picture only (the audio comes from Listen's own player).
pub const VIDEO_FORMAT: &str = "bestvideo[height<=720][protocol=https]/bestvideo[height<=720]/bestvideo";

/// What a googlevideo URL says about the file it serves (free: no request).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UrlFacts {
    /// YouTube's format id, e.g. "251" (Opus) or "140" (AAC).
    pub itag: Option<String>,
    /// File size in bytes; missing for HLS.
    pub clen: Option<u64>,
    /// Duration in milliseconds.
    pub dur_ms: Option<u32>,
    /// e.g. "audio/webm".
    pub mime: Option<String>,
}

impl UrlFacts {
    /// Lower-case codec suffix for [`FileId`]: "opus" or "m4a".
    pub fn suffix(&self) -> Option<&'static str> {
        match self.mime.as_deref() {
            Some(m) if m.starts_with("audio/webm") => Some("opus"),
            Some(m) if m.starts_with("audio/mp4") => Some("m4a"),
            _ => match self.itag.as_deref()? {
                "249" | "250" | "251" | "774" => Some("opus"),
                "139" | "140" | "141" => Some("m4a"),
                _ => None,
            },
        }
    }

    pub fn file_id(&self) -> FileId {
        FileId {
            duration_ms: self.dur_ms.unwrap_or(0),
            suffix: self.suffix().map(str::to_string),
            format: self.itag.clone(),
            bytes: self.clen,
        }
    }
}

/// Reads `itag`, `clen`, `dur` and `mime` from a googlevideo URL.
pub fn url_facts(url: &str) -> UrlFacts {
    let Some((_, query)) = url.split_once('?') else {
        return UrlFacts::default();
    };
    let get = |key: &str| query.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='));
    UrlFacts {
        itag: get("itag").filter(|v| !v.is_empty()).map(str::to_string),
        clen: get("clen").and_then(|v| v.parse().ok()),
        dur_ms: get("dur").and_then(|v| v.parse::<f64>().ok()).map(|s| (s * 1000.0).round() as u32),
        mime: get("mime").map(|v| v.replace("%2F", "/").replace("%2f", "/")),
    }
}

pub struct YtDlp {
    binary: PathBuf,
    js_runtime: Option<String>,
    cache: Mutex<HashMap<String, Resolved>>,
}

/// Refresh URLs this long before they expire.
const MARGIN: Duration = Duration::from_secs(15 * 60);

impl Default for YtDlp {
    fn default() -> Self {
        Self::new()
    }
}

impl YtDlp {
    pub fn new() -> Self {
        let binary = std::env::var_os("LISTEN_YTDLP")
            .map(PathBuf::from)
            .unwrap_or_else(|| listen_core::platform::tool("yt-dlp"));
        Self { binary, js_runtime: find_js_runtime(), cache: Mutex::default() }
    }

    /// `yt-dlp --version`, or an explanation of what is missing.
    pub async fn check(&self) -> Result<String, String> {
        let mut cmd = Command::new(&self.binary);
        listen_core::platform::no_window(cmd.as_std_mut());
        let out = cmd.arg("--version").output().await.map_err(|e| {
            if cfg!(windows) {
                format!("yt-dlp not found ({e}). Settings → Advanced → Helper programs installs it.")
            } else {
                format!("yt-dlp not found ({e}). Install it: sudo dnf install yt-dlp")
            }
        })?;
        let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
        match &self.js_runtime {
            Some(_) => Ok(version),
            None => Err(format!(
                "yt-dlp {version} needs a JavaScript runtime for YouTube; install Node.js or Deno \
                 ({})",
                if cfg!(windows) {
                    "Settings → Advanced → Helper programs, or: winget install DenoLand.Deno"
                } else {
                    "sudo dnf install nodejs"
                }
            )),
        }
    }

    /// The stream URL of `video_id` in [`FORMAT`]. A yt-dlp run counts
    /// against the request [`BUDGET`](crate::budget::BUDGET) but is never held back
    /// (playback must not wait).
    pub async fn resolve(&self, video_id: &str) -> Result<Resolved, String> {
        self.resolve_inner(video_id, true).await
    }

    /// [`Self::resolve`] for a caller that already took a budget slot.
    pub(crate) async fn resolve_reserved(&self, video_id: &str) -> Result<Resolved, String> {
        self.resolve_inner(video_id, false).await
    }

    /// The stream URL for playback at `quality` (a capped quality gets its own
    /// format and cache entry; the others share [`Self::resolve`]).
    pub async fn resolve_quality(&self, video_id: &str, quality: StreamQuality) -> Result<Resolved, String> {
        match quality_format(quality) {
            None => self.resolve(video_id).await,
            Some(format) => {
                self.resolve_format(video_id, true, format, &format!("{video_id}#{}", quality.to_u8())).await
            }
        }
    }

    async fn resolve_inner(&self, video_id: &str, count: bool) -> Result<Resolved, String> {
        self.resolve_format(video_id, count, FORMAT, video_id).await
    }

    async fn resolve_format(
        &self,
        video_id: &str,
        count: bool,
        format: &str,
        key: &str,
    ) -> Result<Resolved, String> {
        if let Some(hit) = self.cached(key) {
            return Ok(hit);
        }
        if count {
            crate::budget::BUDGET.note();
        }
        let mut cmd = Command::new(&self.binary);
        listen_core::platform::no_window(cmd.as_std_mut());
        cmd.args(["-q", "--no-warnings", "--no-playlist", "-f", format, "-g"]);
        if let Some(rt) = &self.js_runtime {
            cmd.args(["--js-runtimes", rt]);
        }
        cmd.arg(format!("https://music.youtube.com/watch?v={video_id}"));
        cmd.kill_on_drop(true);
        let out = tokio::time::timeout(Duration::from_secs(45), cmd.output())
            .await
            .map_err(|_| "yt-dlp timed out".to_string())?
            .map_err(|e| format!("could not run yt-dlp: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = err.lines().rev().take(6).collect();
            tracing::warn!(
                "yt-dlp failed for {video_id} (exit {:?}): {}",
                out.status.code(),
                tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
            );
            let line = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or(err.trim());
            return Err(format!("yt-dlp failed: {}", line.trim()));
        }
        let url = String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.starts_with("http"))
            .ok_or("yt-dlp returned no URL")?
            .to_string();
        let resolved = Resolved { expires_at: expiry(&url), facts: url_facts(&url), url };
        self.cache.lock().expect("cache").insert(key.to_string(), resolved.clone());
        Ok(resolved)
    }

    /// A video-only stream URL (at most 720p, plain https) of `video_id`.
    pub async fn resolve_video(&self, video_id: &str) -> Result<String, String> {
        crate::budget::BUDGET.note();
        let mut cmd = Command::new(&self.binary);
        listen_core::platform::no_window(cmd.as_std_mut());
        cmd.args(["-q", "--no-warnings", "--no-playlist", "-f", VIDEO_FORMAT, "-g"]);
        if let Some(rt) = &self.js_runtime {
            cmd.args(["--js-runtimes", rt]);
        }
        cmd.arg(format!("https://music.youtube.com/watch?v={video_id}"));
        cmd.kill_on_drop(true);
        let out = tokio::time::timeout(Duration::from_secs(45), cmd.output())
            .await
            .map_err(|_| "yt-dlp timed out".to_string())?
            .map_err(|e| format!("could not run yt-dlp: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let line = err.lines().rev().find(|l| l.contains("ERROR")).unwrap_or(err.trim());
            return Err(format!("yt-dlp failed: {}", line.trim()));
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.starts_with("http"))
            .map(str::to_string)
            .ok_or_else(|| "yt-dlp returned no video URL".to_string())
    }

    /// Forgets a stream URL that failed to play.
    pub fn forget(&self, video_id: &str) {
        let mut cache = self.cache.lock().expect("cache");
        cache.remove(video_id);
        let capped = format!("{video_id}#");
        cache.retain(|k, _| !k.starts_with(&capped));
    }

    fn cached(&self, video_id: &str) -> Option<Resolved> {
        let cache = self.cache.lock().expect("cache");
        let hit = cache.get(video_id)?;
        let fresh = hit.expires_at.is_some_and(|t| t > SystemTime::now() + MARGIN);
        fresh.then(|| hit.clone())
    }
}

/// The same stream on the backup server: `mn` names two edge nodes (`sn-primary,sn-backup`) and the
/// host is `rrN---<primary>`; YouTube's own player falls back to the second.
pub fn fallback_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    let (prefix, primary) = host.split_once("---")?;
    let primary = primary.strip_suffix(".googlevideo.com")?;
    let query = path.split_once('?')?.1;
    let mn = query.split('&').find_map(|kv| kv.strip_prefix("mn="))?;
    let nodes: Vec<&str> = mn.split("%2C").flat_map(|n| n.split(',')).collect();
    let backup = nodes.iter().find(|n| **n != primary)?;
    Some(format!("https://{prefix}---{backup}.googlevideo.com/{path}"))
}

/// Reads `expire=<unix seconds>` from a googlevideo URL.
pub fn expiry(url: &str) -> Option<SystemTime> {
    let query = url.split_once('?')?.1;
    let secs = query.split('&').find_map(|kv| kv.strip_prefix("expire="))?.parse::<u64>().ok()?;
    Some(UNIX_EPOCH + Duration::from_secs(secs))
}

/// yt-dlp needs Deno or Node for YouTube's player JS; desktop launches may lack the shell's PATH (nvm),
/// so the usual places are searched. Returns the `--js-runtimes` value (`deno:<path>`).
pub fn find_js_runtime() -> Option<String> {
    let home = listen_core::platform::home_dir();
    let mut path_dirs: Vec<PathBuf> = listen_core::platform::exe_dir().into_iter().collect();
    path_dirs.extend(
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default(),
    );
    path_dirs.extend(listen_core::platform::extra_tool_dirs());
    for base in ["deno", "node"] {
        let name = format!("{base}{}", std::env::consts::EXE_SUFFIX);
        let name = name.as_str();
        let mut candidates: Vec<PathBuf> = path_dirs.iter().map(|d| d.join(name)).collect();
        if let Some(home) = &home {
            candidates.push(home.join(".local/bin").join(name));
            candidates.push(home.join(".deno/bin").join(name));
            if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
                let mut versions: Vec<PathBuf> =
                    entries.flatten().map(|e| e.path().join("bin").join(name)).collect();
                versions.sort();
                candidates.extend(versions.into_iter().rev());
            }
        }
        #[cfg(unix)]
        candidates.push(PathBuf::from("/usr/bin").join(name));
        #[cfg(windows)]
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            candidates.push(PathBuf::from(pf).join("nodejs").join(name));
        }
        if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
            return Some(format!("{base}:{}", found.display()));
        }
    }
    None
}

#[cfg(test)]
mod tests {

    #[test]
    fn only_capped_qualities_change_the_playback_format() {
        use super::{StreamQuality, quality_format};
        assert_eq!(quality_format(StreamQuality::Original), None);
        assert_eq!(quality_format(StreamQuality::High), None);
        assert!(quality_format(StreamQuality::Normal).unwrap().starts_with("140/"));
        assert!(quality_format(StreamQuality::Low).unwrap().starts_with("250/"));
    }

    use super::*;

    #[test]
    fn backup_server_url() {
        let u = "https://rr2---sn-2oi5h5-5j.googlevideo.com/videoplayback?expire=1&mn=sn-2oi5h5-5j%2Csn-i5heen7z&c=X";
        assert_eq!(
            fallback_url(u).as_deref(),
            Some(
                "https://rr2---sn-i5heen7z.googlevideo.com/videoplayback?expire=1&mn=sn-2oi5h5-5j%2Csn-i5heen7z&c=X"
            )
        );
        assert_eq!(fallback_url("https://example.com/a.mp3"), None);
    }

    #[test]
    fn parses_url_facts() {
        let u = "https://rr3---sn-x.googlevideo.com/videoplayback?expire=1&itag=251&mime=audio%2Fwebm\
                 &gir=yes&clen=3445462&dur=213.581&lmt=1&c=VISIONOS";
        let f = url_facts(u);
        assert_eq!(f.itag.as_deref(), Some("251"));
        assert_eq!(f.clen, Some(3_445_462));
        assert_eq!(f.dur_ms, Some(213_581));
        assert_eq!(f.mime.as_deref(), Some("audio/webm"));
        let id = f.file_id();
        assert_eq!(id.duration_ms, 213_581);
        assert_eq!(id.suffix.as_deref(), Some("opus"));
        assert_eq!(id.format.as_deref(), Some("251"));
        assert_eq!(id.bytes, Some(3_445_462));
        // HLS manifests carry no clen; `xitag=` must not be read as `itag=`.
        let hls = url_facts("https://manifest.googlevideo.com/api?xitag=9&itag=234&dur=1.5");
        assert_eq!((hls.itag.as_deref(), hls.clen, hls.dur_ms), (Some("234"), None, Some(1500)));
        assert_eq!(url_facts("https://example.com/a.mp3"), UrlFacts::default());
        let aac = UrlFacts { itag: Some("140".into()), ..Default::default() };
        assert_eq!(aac.suffix(), Some("m4a"));
    }

    #[test]
    fn parses_expiry() {
        let t = expiry("https://rr5.googlevideo.com/videoplayback?expire=1790591014&ei=x").unwrap();
        assert_eq!(t.duration_since(UNIX_EPOCH).unwrap().as_secs(), 1_790_591_014);
        assert!(expiry("https://example.com/a").is_none());
    }
}
