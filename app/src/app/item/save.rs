//! "Download" of the original file (Feishin `download-action.tsx`) into the Downloads folder, one file at a
//! time with a progress toast, and "Show in file manager" for local songs. The in-app offline copy is
//! `app/offline.rs`.

use super::super::menu::MenuTarget;
use super::super::{Listen, Message};
use super::ItemMsg;
use crate::app::overlay::Toast;
use iced::Task;
use listen_core::model::{Song, SourceKind};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What is being saved right now.
#[derive(Debug, Default)]
pub struct SaveState {
    pub active: Option<Active>,
    /// Save here instead of the Downloads folder (tests).
    pub dir: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Active {
    /// The progress toast.
    pub toast: u64,
    pub names: Vec<String>,
    pub index: usize,
    pub saved: Vec<PathBuf>,
    pub failed: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum SaveMsg {
    /// Bytes received of file `index` (the size is 0 when the server did not say).
    Progress {
        index: usize,
        got: u64,
        size: u64,
    },
    File(usize, Result<PathBuf, String>),
    Finished,
    /// Show a saved file in the file manager.
    Reveal(PathBuf),
}

/// One file to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub url: String,
    pub dest: PathBuf,
}

/// A file name that is safe on every file system.
pub fn sanitize(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.is_empty() { "track".into() } else { cleaned }
}

/// The saved file's name: the server's own (the last part of its path), else "Artist - Title.ext".
pub fn file_name(song: &Song) -> String {
    let ext = song
        .suffix
        .clone()
        .or_else(|| song.codec.clone())
        .map(|e| e.to_lowercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "audio".into());
    if let Some(name) =
        song.path.as_deref().and_then(|p| p.rsplit(['/', '\\']).next()).filter(|n| n.contains('.'))
    {
        return sanitize(name);
    }
    sanitize(&format!("{} - {}.{ext}", song.artist_line(), song.title))
}

/// `dir/name`, or `dir/name (2)` and so on when that exists.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    (2..).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).expect("a free name")
}

/// The Downloads folder.
pub fn downloads_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::temp_dir().join("Listen"))
}

/// The jobs for `songs` (with their download URLs) below `base`: several songs
/// go into a folder named after the album.
pub fn plan(songs: &[(Song, String)], base: &Path) -> Vec<Job> {
    let dir = if songs.len() > 1 {
        let album =
            songs[0].0.album.clone().filter(|_| songs.iter().all(|(s, _)| s.album == songs[0].0.album));
        base.join(sanitize(&album.unwrap_or_else(|| "Listen".into())))
    } else {
        base.to_path_buf()
    };
    let mut taken: Vec<PathBuf> = Vec::new();
    songs
        .iter()
        .map(|(song, url)| {
            let mut dest = unique_path(&dir, &file_name(song));
            // Two songs of the batch with the same name.
            let mut n = 2;
            while taken.contains(&dest) {
                let name = file_name(song);
                let (stem, ext) = name.rsplit_once('.').map_or((name.as_str(), ""), |(s, e)| (s, e));
                dest = dir.join(format!("{stem} ({n}).{ext}"));
                n += 1;
            }
            taken.push(dest.clone());
            Job { url: url.clone(), dest }
        })
        .collect()
}

/// Commands that show `path` in the file manager, best first.
pub fn file_manager_commands(path: &Path) -> Vec<Vec<String>> {
    let p = path.display().to_string();
    if cfg!(target_os = "windows") {
        return vec![vec!["explorer".into(), format!("/select,{p}")]];
    }
    if cfg!(target_os = "macos") {
        return vec![vec!["open".into(), "-R".into(), p]];
    }
    let uri = url::Url::from_file_path(path).map(String::from).unwrap_or_else(|_| format!("file://{p}"));
    let folder = path.parent().unwrap_or(path).display().to_string();
    vec![
        // FileManager1.ShowItems selects the file in Dolphin, Nautilus, Nemo, ...
        vec![
            "dbus-send".into(),
            "--session".into(),
            "--print-reply".into(),
            "--dest=org.freedesktop.FileManager1".into(),
            "/org/freedesktop/FileManager1".into(),
            "org.freedesktop.FileManager1.ShowItems".into(),
            format!("array:string:{uri}"),
            "string:".into(),
        ],
        vec!["xdg-open".into(), folder],
    ]
}

/// Shows `path` in the file manager (selecting it where the desktop can).
pub fn show_in_file_manager(path: &Path) {
    for cmd in file_manager_commands(path) {
        let (program, args) = cmd.split_first().expect("a program");
        let mut command = std::process::Command::new(program);
        listen_core::platform::no_window(&mut command);
        let ok = command
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            return;
        }
    }
    tracing::warn!("no file manager opened for {}", path.display());
}

async fn fetch(
    http: &reqwest::Client,
    job: &Job,
    index: usize,
    out: &mut iced::futures::channel::mpsc::Sender<Message>,
) -> Result<PathBuf, String> {
    use iced::futures::SinkExt;
    use tokio::io::AsyncWriteExt;
    let mut resp =
        http.get(&job.url).send().await.map_err(|e| format!("request failed: {}", e.without_url()))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let size = resp.content_length().unwrap_or(0);
    if let Some(dir) = job.dest.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let part = job
        .dest
        .with_extension(format!("{}.part", job.dest.extension().and_then(|e| e.to_str()).unwrap_or("")));
    let mut file = tokio::fs::File::create(&part).await.map_err(|e| format!("{}: {e}", part.display()))?;
    let (mut got, mut last_sent) = (0u64, std::time::Instant::now());
    let result: Result<(), String> = async {
        while let Some(chunk) =
            resp.chunk().await.map_err(|e| format!("download broke off: {}", e.without_url()))?
        {
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
            got += chunk.len() as u64;
            if last_sent.elapsed() >= Duration::from_millis(250) {
                last_sent = std::time::Instant::now();
                let _ = out.send(Message::Item(ItemMsg::Save(SaveMsg::Progress { index, got, size }))).await;
            }
        }
        file.flush().await.map_err(|e| e.to_string())
    }
    .await;
    match result {
        Ok(()) => {
            drop(file);
            tokio::fs::rename(&part, &job.dest).await.map_err(|e| e.to_string())?;
            Ok(job.dest.clone())
        }
        Err(e) => {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            Err(e)
        }
    }
}

impl Listen {
    /// Saves the original files of `songs` (sources with a download endpoint).
    pub(in crate::app) fn save_files(&mut self, songs: Vec<Song>) -> Task<Message> {
        if self.item.save.active.is_some() {
            self.toast_info("A download is already running.".into());
            return Task::none();
        }
        let with_urls: Vec<(Song, String)> = songs
            .into_iter()
            .filter(|s| s.source != SourceKind::YouTubeMusic)
            .filter_map(|s| {
                let url = self.sources.get(&s.server_id)?.download_url(&s)?;
                Some((s, url))
            })
            .collect();
        if with_urls.is_empty() {
            self.toast_info("The original file can't be downloaded from this source.".into());
            return Task::none();
        }
        let base = self.item.save.dir.clone().unwrap_or_else(downloads_dir);
        let jobs = plan(&with_urls, &base);
        let names: Vec<String> = with_urls.iter().map(|(s, _)| s.title.clone()).collect();
        let toast = self.overlay.push(Toast::info(progress_text(&names, 0, 0, 0)).persistent());
        self.item.save.active =
            Some(Active { toast, names, index: 0, saved: Vec::new(), failed: Vec::new() });
        Task::stream(iced::stream::channel(
            32,
            async move |mut out: iced::futures::channel::mpsc::Sender<Message>| {
                use iced::futures::SinkExt;
                let http = reqwest::Client::builder()
                    .user_agent(concat!("Listen/", env!("CARGO_PKG_VERSION")))
                    .connect_timeout(Duration::from_secs(10))
                    .read_timeout(Duration::from_secs(60))
                    .build()
                    .unwrap_or_default();
                for (index, job) in jobs.iter().enumerate() {
                    let result = fetch(&http, job, index, &mut out).await;
                    if out.send(Message::Item(ItemMsg::Save(SaveMsg::File(index, result)))).await.is_err() {
                        return;
                    }
                }
                let _ = out.send(Message::Item(ItemMsg::Save(SaveMsg::Finished))).await;
            },
        ))
    }

    /// The song's file on this computer: the local folder's own path, or a
    /// server's path mapped through its "local music folder" setting (only
    /// when the file is really there, e.g. the NAS is mounted).
    pub(in crate::app) fn song_file(&self, song: &Song) -> Option<PathBuf> {
        self.sources.get(&song.server_id).and_then(|src| src.local_path(song)).or_else(|| {
            let mapped = self.config.music_dirs.get(&song.server_id.0)?.map(song.path.as_deref()?)?;
            mapped.exists().then_some(mapped)
        })
    }

    /// "Show in file manager" for a local song.
    pub(in crate::app) fn show_in_folder(&mut self, target: &MenuTarget) -> Task<Message> {
        let path = target.song_list().and_then(|s| s.first()).and_then(|song| self.song_file(song));
        match path {
            Some(path) => show_in_file_manager(&path),
            None => self.toast_info("This file is not on this computer.".into()),
        }
        Task::none()
    }

    pub(super) fn on_save(&mut self, msg: SaveMsg) -> Task<Message> {
        match msg {
            SaveMsg::Progress { index, got, size } => {
                if let Some(a) = &self.item.save.active {
                    let text = progress_text(&a.names, index, got, size);
                    if let Some(t) = self.overlay.toasts.iter_mut().find(|t| t.id == a.toast) {
                        t.message = text;
                    }
                }
            }
            SaveMsg::File(index, result) => {
                let Some(a) = &mut self.item.save.active else { return Task::none() };
                match result {
                    Ok(path) => a.saved.push(path),
                    Err(e) => a.failed.push((a.names.get(index).cloned().unwrap_or_default(), e)),
                }
                a.index = index + 1;
                let next = a.index.min(a.names.len().saturating_sub(1));
                let text = progress_text(&a.names, next, 0, 0);
                let id = a.toast;
                if let Some(t) = self.overlay.toasts.iter_mut().find(|t| t.id == id) {
                    t.message = text;
                }
            }
            SaveMsg::Finished => {
                let Some(a) = self.item.save.active.take() else { return Task::none() };
                self.overlay.dismiss(a.toast);
                if let Some((name, why)) = a.failed.first() {
                    let n = a.failed.len();
                    self.toast_error(if n == 1 {
                        format!("Could not download “{name}”: {why}.")
                    } else {
                        format!("Could not download {n} files (“{name}”: {why}).")
                    });
                }
                if let Some(last) = a.saved.last() {
                    let text = match a.saved.len() {
                        1 => format!(
                            "Saved “{}”.",
                            last.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
                        ),
                        n => format!("Saved {n} files."),
                    };
                    let reveal = Message::Item(ItemMsg::Save(SaveMsg::Reveal(last.clone())));
                    self.toast(Toast::success(text).action("Show in folder", reveal));
                }
            }
            SaveMsg::Reveal(path) => show_in_file_manager(&path),
        }
        Task::none()
    }
}

/// "Downloading “Title” (2 of 5) 43%".
pub fn progress_text(names: &[String], index: usize, got: u64, size: u64) -> String {
    let name = names.get(index).map_or("", String::as_str);
    let mut text = format!("Downloading “{name}”");
    if names.len() > 1 {
        text += &format!(" ({} of {})", index + 1, names.len());
    }
    if let Some(pct) = (got * 100).checked_div(size) {
        text += &format!(" {}%", pct.min(100));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str, path: Option<&str>) -> Song {
        let mut s = crate::demo::album_songs(0).remove(0);
        s.title = title.into();
        s.path = path.map(String::from);
        s.suffix = Some("flac".into());
        s.album = Some("Alb/um".into());
        s
    }

    #[test]
    fn names_are_safe_and_prefer_the_servers() {
        assert_eq!(file_name(&song("x", Some("Artist/Album/01 - Song.flac"))), "01 - Song.flac");
        assert_eq!(
            file_name(&song("a/b: c", None)),
            format!("{} - a_b_ c.flac", song("", None).artist_line())
        );
        assert_eq!(sanitize("..."), "track");
        assert_eq!(sanitize("a?b|c"), "a_b_c");
    }

    #[test]
    fn existing_files_are_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("listen-save-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.flac"), b"x").unwrap();
        std::fs::write(dir.join("a (2).flac"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a.flac"), dir.join("a (3).flac"));
        assert_eq!(unique_path(&dir, "b.flac"), dir.join("b.flac"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn several_songs_go_into_an_album_folder_with_distinct_names() {
        let base = Path::new("/tmp/dl-plan-test-nonexistent");
        let a = song("One", Some("x/Same.flac"));
        let b = song("Two", Some("y/Same.flac"));
        let jobs = plan(&[(a, "http://h/1".into()), (b, "http://h/2".into())], base);
        assert_eq!(jobs[0].dest, base.join("Alb_um").join("Same.flac"));
        assert_eq!(jobs[1].dest, base.join("Alb_um").join("Same (2).flac"));
        let one = plan(&[(song("One", Some("x/Solo.flac")), "http://h/1".into())], base);
        assert_eq!(one[0].dest, base.join("Solo.flac"));
    }

    #[test]
    fn progress_text_counts_files_and_percent() {
        let names = vec!["A".to_string(), "B".to_string()];
        assert_eq!(progress_text(&names, 1, 50, 200), "Downloading “B” (2 of 2) 25%");
        assert_eq!(progress_text(&names[..1], 0, 0, 0), "Downloading “A”");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_selects_the_file_over_dbus_then_opens_the_folder() {
        let cmds = file_manager_commands(Path::new("/music/a b/x.flac"));
        assert_eq!(cmds[0][0], "dbus-send");
        assert!(cmds[0].contains(&"array:string:file:///music/a%20b/x.flac".to_string()));
        assert_eq!(cmds[1], ["xdg-open", "/music/a b"]);
    }
}
