//! CD ripping (Linux): UDisks2 drive events start `listen_rip::pipeline` jobs (auto-rip, as in MusicBee)
//! shown in the rip panel; a filed album triggers a Navidrome scan and a notification.

use super::{Listen, Message, Setting};
use iced::Task;
use iced::futures::SinkExt;
use listen_rip::detect::DriveEvent;
use listen_rip::history::{Entry, History};
use listen_rip::pipeline::{self, Candidate, Command, JobConfig, Lookup, Report, Stage, Update};
use listen_rip::queue::RipQueue;
use listen_rip::{DriveInfo, Ripper, whipper};
use listen_source::SourceError;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// One disc being ripped (or just finished) — what the rip panel shows.
#[derive(Debug)]
pub struct RipJob {
    pub id: u64,
    pub device: PathBuf,
    pub stage: Stage,
    pub activity: String,
    pub track: u32,
    pub total: u32,
    pub percent: u32,
    pub artist: String,
    pub album: String,
    pub candidates: Vec<Candidate>,
    pub chosen: usize,
    /// The picker auto-confirms at this time.
    pub picker_until: Option<Instant>,
    pub cover: Option<iced::widget::image::Handle>,
    pub warnings: Vec<String>,
    pub report: Option<Box<Report>>,
    /// Ripped and ejected: the drive is free again.
    drive_free: bool,
    commands: Option<mpsc::UnboundedSender<Command>>,
}

impl RipJob {
    pub fn title(&self) -> String {
        match (self.artist.is_empty(), self.album.is_empty()) {
            (false, false) => format!("{} – {}", self.artist, self.album),
            _ => "Audio CD".into(),
        }
    }
    pub fn seconds_left(&self) -> Option<u64> {
        self.picker_until.map(|t| t.saturating_duration_since(Instant::now()).as_secs())
    }
}

#[derive(Debug, Default)]
pub struct RipState {
    /// Optical drives (empty: no drive, the feature stays out of the way).
    pub drives: Vec<DriveInfo>,
    pub jobs: Vec<RipJob>,
    /// Audio CDs in a drive that aren't being ripped; "Rip CD" rips the first.
    pub waiting_discs: Vec<PathBuf>,
    queue: RipQueue,
    next_id: u64,
    pub history: History,
    history_path: Option<PathBuf>,
    /// Destination folder field in settings.
    pub dest_input: String,
    /// Offset field in settings ("" = automatic).
    pub offset_input: String,
    /// Status line in Settings → CD ripping.
    pub status: Option<String>,
    pub verifying_offset: bool,
}

impl RipState {
    pub fn active(&self) -> bool {
        self.jobs.iter().any(|j| !j.stage.finished())
    }

    /// A job without a pipeline (tests and snapshots feed it updates). Returns its id.
    #[doc(hidden)]
    pub fn test_job(&mut self, device: &str) -> u64 {
        self.next_id += 1;
        self.jobs.push(RipJob {
            id: self.next_id,
            device: device.into(),
            stage: Stage::Ripping,
            activity: "Reading table".into(),
            track: 0,
            total: 0,
            percent: 0,
            artist: String::new(),
            album: String::new(),
            candidates: Vec::new(),
            chosen: 0,
            picker_until: None,
            cover: None,
            warnings: Vec::new(),
            report: None,
            drive_free: false,
            commands: None,
        });
        self.next_id
    }
}

#[derive(Debug, Clone)]
pub enum RipMsg {
    Drive(DriveEvent),
    Update(u64, Update),
    Choose(u64, usize),
    Cancel(u64),
    PublishAnyway(u64),
    Discard(u64),
    Retry(u64),
    Dismiss(u64),
    /// "Rip CD": the given drive, or the first with a disc.
    RipNow(Option<PathBuf>),
    BrowseDestination,
    DestinationChosen(Option<PathBuf>),
    ScanDone(Result<String, String>),
    OffsetChecked(Result<String, String>),
    /// Once a second while a picker counts down.
    Tick,
}

fn msg(m: RipMsg) -> Message {
    Message::Rip(m)
}

/// Drive events for the app (Linux: UDisks2 / polling).
pub fn subscription() -> iced::Subscription<Message> {
    iced::Subscription::run(|| {
        iced::stream::channel(8, async |mut out: iced::futures::channel::mpsc::Sender<Message>| {
            let (tx, mut rx) = mpsc::channel(16);
            let watch = listen_rip::detect::watch(tx);
            tokio::pin!(watch);
            loop {
                tokio::select! {
                    _ = &mut watch => return,
                    Some(ev) = rx.recv() => {
                        if out.send(msg(RipMsg::Drive(ev))).await.is_err() {
                            return;
                        }
                    }
                }
            }
        })
    })
}

impl Listen {
    pub(super) fn rip_boot(&mut self) {
        let r = &mut self.rip;
        r.history_path = self.paths.as_ref().map(|p| p.data_dir.join("rips.json"));
        if let Some(p) = &r.history_path {
            r.history = History::load(p);
        }
        let s = &self.config.settings.ripping;
        r.dest_input = s.destination.clone();
        r.offset_input = s.read_offset.map(|o| format!("{o:+}")).unwrap_or_default();
    }

    /// Show rip features (a drive exists and this is Linux).
    pub fn rip_available(&self) -> bool {
        cfg!(target_os = "linux") && !self.demo && !self.rip.drives.is_empty()
    }

    pub(super) fn rip_ticking(&self) -> bool {
        self.rip.jobs.iter().any(|j| j.picker_until.is_some())
    }

    fn drive(&self, device: &Path) -> DriveInfo {
        self.rip
            .drives
            .iter()
            .find(|d| d.device == device)
            .cloned()
            .unwrap_or_else(|| listen_rip::drive::info(device))
    }

    pub(super) fn on_rip(&mut self, m: RipMsg) -> Task<Message> {
        match m {
            RipMsg::Drive(ev) => return self.on_drive(ev),
            RipMsg::Update(id, u) => return self.on_rip_update(id, u),
            RipMsg::Choose(id, n) => {
                if let Some(j) = self.rip.jobs.iter_mut().find(|j| j.id == id) {
                    j.chosen = n;
                    j.picker_until = None;
                    if let Some(c) = &j.commands {
                        let _ = c.send(Command::Choose(n));
                    }
                }
            }
            RipMsg::Cancel(id) => self.rip_command(id, Command::Cancel),
            RipMsg::PublishAnyway(id) => self.rip_command(id, Command::PublishAnyway),
            RipMsg::Discard(id) => self.rip_command(id, Command::Discard),
            RipMsg::Retry(id) => self.rip_command(id, Command::RetryPublish),
            RipMsg::Dismiss(id) => self.rip.jobs.retain(|j| j.id != id || !j.stage.finished()),
            RipMsg::RipNow(device) => {
                let device = device.or_else(|| self.rip.waiting_discs.first().cloned()).or_else(|| {
                    self.rip
                        .drives
                        .iter()
                        .map(|d| d.device.clone())
                        .find(|d| listen_rip::drive::disc_state(d) == listen_rip::drive::DiscState::Audio)
                });
                match device {
                    Some(d) => return self.offer_disc(d),
                    None => self.toast_info("No audio CD in the drive.".into()),
                }
            }
            RipMsg::BrowseDestination => {
                return Task::perform(pick_folder(self.config.settings.ripping.destination.clone()), |p| {
                    msg(RipMsg::DestinationChosen(p))
                });
            }
            RipMsg::DestinationChosen(Some(p)) => {
                self.rip.dest_input = p.display().to_string();
                return self.change_setting(Setting::RipSaveDestination);
            }
            RipMsg::DestinationChosen(None) => {}
            RipMsg::ScanDone(r) => {
                let line = r.unwrap_or_else(|e| e);
                tracing::info!("Navidrome scan: {line}");
                self.rip.status = Some(line);
            }
            RipMsg::OffsetChecked(r) => {
                self.rip.verifying_offset = false;
                self.rip.status = Some(match r {
                    Ok(o) => o,
                    Err(e) => format!("Offset check failed: {e}"),
                });
            }
            RipMsg::Tick => {
                for j in &mut self.rip.jobs {
                    if j.picker_until.is_some_and(|t| Instant::now() >= t) {
                        j.picker_until = None;
                    }
                }
            }
        }
        Task::none()
    }

    fn rip_command(&mut self, id: u64, c: Command) {
        if let Some(j) = self.rip.jobs.iter().find(|j| j.id == id)
            && let Some(tx) = &j.commands
        {
            let _ = tx.send(c);
        }
    }

    fn on_drive(&mut self, ev: DriveEvent) -> Task<Message> {
        let auto = self.config.settings.ripping.auto_rip;
        match ev {
            DriveEvent::Drives(d) => {
                tracing::info!("optical drives: {:?}", d.iter().map(|d| d.label()).collect::<Vec<_>>());
                self.rip.drives = d;
            }
            DriveEvent::Inserted { device } => {
                if auto {
                    return self.offer_disc(device);
                }
                self.rip.waiting_discs.push(device);
                self.toast_info("Audio CD inserted — “Rip CD” in the sidebar rips it.".into());
            }
            DriveEvent::Present { device } => {
                // Already in the drive at start: rip unless it was ripped before.
                let known = listen_rip::drive::read_toc(&device)
                    .map(|t| self.rip.history.contains(&t.musicbrainz_id()))
                    .unwrap_or(false);
                if auto && !known {
                    return self.offer_disc(device);
                }
                self.rip.waiting_discs.push(device);
                if known {
                    self.toast_info("The CD in the drive was ripped before — “Rip CD” rips it again.".into());
                }
            }
            DriveEvent::Removed { device } => {
                self.rip.waiting_discs.retain(|d| d != &device);
                self.rip.queue.removed(&device);
                for j in &self.rip.jobs {
                    if j.device == device
                        && !j.drive_free
                        && !j.stage.finished()
                        && let Some(c) = &j.commands
                    {
                        let _ = c.send(Command::DiscRemoved);
                    }
                }
            }
        }
        Task::none()
    }

    /// A disc is ready: rip it now, or queue it behind the current rip.
    fn offer_disc(&mut self, device: PathBuf) -> Task<Message> {
        self.rip.waiting_discs.retain(|d| d != &device);
        if self.rip.queue.offer(&device) {
            self.start_rip(device)
        } else {
            self.toast_info("Another CD is being ripped; this one is next.".into());
            Task::none()
        }
    }

    fn start_rip(&mut self, device: PathBuf) -> Task<Message> {
        self.rip.next_id += 1;
        let id = self.rip.next_id;
        let mut job = RipJob {
            id,
            device: device.clone(),
            stage: Stage::Ripping,
            activity: "Reading the disc".into(),
            track: 0,
            total: 0,
            percent: 0,
            artist: String::new(),
            album: String::new(),
            candidates: Vec::new(),
            chosen: 0,
            picker_until: None,
            cover: None,
            warnings: Vec::new(),
            report: None,
            drive_free: false,
            commands: None,
        };
        let fail = |this: &mut Self, mut job: RipJob, why: String| {
            tracing::warn!("rip of {}: {why}", job.device.display());
            job.stage = Stage::Failed(why);
            job.drive_free = true;
            this.rip.jobs.push(job);
            this.rip_drive_free(&device)
        };
        let toc = match listen_rip::drive::read_toc(&device) {
            Ok(t) => t,
            Err(e) => return fail(self, job, format!("Couldn't read the disc: {e}")),
        };
        let drive = self.drive(&device);
        let s = self.config.settings.ripping.clone();
        let Some((offset, source)) = whipper::resolve_offset(s.read_offset, &drive) else {
            return fail(
                self,
                job,
                format!(
                    "The read offset of “{}” isn't known — set it in Settings → CD ripping (or “Verify offset”).",
                    drive.label()
                ),
            );
        };
        tracing::info!(
            "ripping disc {} in {} (offset {offset:+} from {})",
            toc.musicbrainz_id(),
            drive.label(),
            source.label()
        );
        job.total = toc.audio_count() as u32;
        let Some(paths) = self.paths.clone() else {
            return fail(self, job, "no data directory".into());
        };
        let cfg = JobConfig {
            drive,
            offset,
            destination: PathBuf::from(s.destination.trim()),
            work_dir: paths.cache_dir.join("rips").join(format!(
                "{}-{}",
                toc.musicbrainz_id().trim_end_matches('-'),
                super::unix_millis()
            )),
            log_dir: paths.log_dir(),
            replay_gain: s.replay_gain,
            eject: s.eject_when_done,
            picker_timeout: Duration::from_secs(u64::from(s.picker_timeout_secs.max(5))),
            stamp: pipeline::local_stamp(),
            lookup: Lookup::Online,
            destination_retry: Duration::from_secs(30),
            toc,
        };
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        job.commands = Some(cmd_tx);
        self.rip.jobs.push(job);
        let http = listen_rip::musicbrainz::http_client();
        Task::stream(iced::stream::channel(
            64,
            move |mut out: iced::futures::channel::mpsc::Sender<Message>| async move {
                let (tx, mut rx) = mpsc::unbounded_channel();
                let run = pipeline::run(cfg, Ripper::Whipper, http, tx, cmd_rx);
                tokio::pin!(run);
                let mut done = false;
                loop {
                    tokio::select! {
                        _ = &mut run, if !done => done = true,
                        u = rx.recv() => match u {
                            Some(u) => { let _ = out.send(msg(RipMsg::Update(id, u))).await; }
                            None => break,
                        },
                    }
                }
            },
        ))
    }

    /// The drive of a finished/failed rip is free: start the next disc.
    fn rip_drive_free(&mut self, device: &Path) -> Task<Message> {
        match self.rip.queue.drive_free(device) {
            Some(next) => self.start_rip(next),
            None => Task::none(),
        }
    }

    fn on_rip_update(&mut self, id: u64, u: Update) -> Task<Message> {
        let Some(j) = self.rip.jobs.iter_mut().find(|j| j.id == id) else { return Task::none() };
        match u {
            Update::Activity(a) => j.activity = a,
            Update::Progress { track, total, percent } => {
                j.track = track;
                j.total = total;
                j.percent = percent;
            }
            Update::Candidates { candidates, chosen, timeout } => {
                j.candidates = candidates;
                j.chosen = chosen;
                j.picker_until = Some(Instant::now() + timeout);
            }
            Update::Identified { artist, album, .. } => {
                j.artist = artist;
                j.album = album;
            }
            Update::Cover(bytes) => j.cover = Some(iced::widget::image::Handle::from_bytes(bytes)),
            Update::Warning(w) => j.warnings.push(w),
            Update::DriveFree => {
                j.drive_free = true;
                let device = j.device.clone();
                return self.rip_drive_free(&device);
            }
            Update::Stage(stage) => {
                let finished = stage.finished();
                if matches!(stage, Stage::Tagging | Stage::Publishing) || finished {
                    j.picker_until = None;
                }
                j.stage = stage.clone();
                if !finished {
                    return Task::none();
                }
                j.commands = None;
                let device = j.device.clone();
                let freed = j.drive_free;
                let title = j.title();
                let mut tasks = Vec::new();
                if !freed {
                    j.drive_free = true;
                    tasks.push(self.rip_drive_free(&device));
                }
                if let Stage::Failed(why) = &stage {
                    tasks.push(notify_task(format!("Rip failed: {title}"), why.clone(), true, false));
                }
                return Task::batch(tasks);
            }
            Update::Finished(report) => {
                j.report = Some(report.clone());
                return self.rip_finished(&report);
            }
        }
        Task::none()
    }

    fn rip_finished(&mut self, report: &Report) -> Task<Message> {
        let r = &mut self.rip;
        r.history.rips.push(Entry {
            disc_id: report.disc_id.clone(),
            when: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            artist: report.artist.clone(),
            album: report.album.clone(),
            folder: report.folder.clone(),
            needs_fix: report.source != listen_rip::tags::MetaSource::MusicBrainz,
            accuraterip: report.accuraterip_line(),
            submit_url: if report.source == listen_rip::tags::MetaSource::MusicBrainz {
                String::new()
            } else {
                report.submit_url.clone()
            },
        });
        if let (Some(p), false) = (&r.history_path, self.ephemeral)
            && let Err(e) = r.history.save(p)
        {
            tracing::warn!("saving the rip history: {e}");
        }
        let s = &self.config.settings.ripping;
        let a = report.accuraterip;
        let warning = a.mismatched > 0 || a.skipped > 0;
        let mut body = report.accuraterip_line();
        if a.mismatched > 0 {
            body.push_str(" — AccurateRip mismatch, files kept");
        }
        if report.source != listen_rip::tags::MetaSource::MusicBrainz {
            body.push_str(" — not on MusicBrainz, marked for fixing");
        }
        let mut tasks =
            vec![notify_task(format!("Album {} ripped", report.album), body, warning, s.notify_sound)];
        if s.scan_navidrome {
            tasks.push(self.navidrome_scan());
        }
        Task::batch(tasks)
    }

    /// `startScan` on the active Navidrome server (or the first connected one).
    fn navidrome_scan(&self) -> Task<Message> {
        use listen_core::model::SourceKind;
        let is_nd = |id: &listen_core::model::ServerId| {
            self.config.servers.iter().any(|s| &s.id == id && s.kind == SourceKind::Subsonic)
        };
        let target = self
            .config
            .active_server
            .as_ref()
            .filter(|id| is_nd(id))
            .and_then(|id| self.sources.get(id).cloned())
            .or_else(|| self.sources.iter().find(|(id, _)| is_nd(id)).map(|(_, s)| s.clone()));
        let Some(source) = target else {
            return Task::done(msg(RipMsg::ScanDone(Err(
                "No Navidrome server connected; its file watcher will pick the album up.".into(),
            ))));
        };
        Task::perform(
            async move {
                match source.start_scan().await {
                    Ok(_) => Ok("Navidrome is scanning for the new album.".to_string()),
                    Err(SourceError::Auth(_)) => Err(
                        "Navidrome only lets admins start a scan; its file watcher will pick the album up."
                            .to_string(),
                    ),
                    Err(e) => Err(format!("Navidrome scan: {e}")),
                }
            },
            |r| msg(RipMsg::ScanDone(r)),
        )
    }

    pub(super) fn rip_setting(&mut self, change: Setting) -> Task<Message> {
        let s = &mut self.config.settings.ripping;
        match change {
            Setting::RipAuto(v) => s.auto_rip = v,
            Setting::RipEject(v) => s.eject_when_done = v,
            Setting::RipSound(v) => s.notify_sound = v,
            Setting::RipReplayGain(v) => s.replay_gain = v,
            Setting::RipScan(v) => s.scan_navidrome = v,
            Setting::RipPickerTimeout(v) => s.picker_timeout_secs = v.clamp(5, 300),
            Setting::RipDestination(text) => {
                self.rip.dest_input = text;
                return Task::none();
            }
            Setting::RipSaveDestination => {
                let dest = self.rip.dest_input.trim().to_string();
                let path = super::shellexpand_home(&dest);
                s.destination = path.display().to_string();
                self.rip.dest_input = s.destination.clone();
                self.rip.status = Some(match listen_rip::publish::check(&path, 0) {
                    Ok(()) => format!("Rips go to {}.", path.display()),
                    Err(e) => format!("Saved, but {e}. Rips wait until it is available."),
                });
                // A job waiting for the destination tries again now.
                for j in &self.rip.jobs {
                    if matches!(j.stage, Stage::WaitingForDestination(_))
                        && let Some(c) = &j.commands
                    {
                        let _ = c.send(Command::RetryPublish);
                    }
                }
            }
            Setting::RipOffset(text) => {
                self.rip.offset_input = text;
                return Task::none();
            }
            Setting::RipSaveOffset => {
                let t = self.rip.offset_input.trim().trim_start_matches('+').to_string();
                if t.is_empty() {
                    s.read_offset = None;
                    self.rip.status = Some("Read offset: automatic.".into());
                } else {
                    match t.parse::<i32>() {
                        Ok(o) if o.abs() <= 3000 => {
                            s.read_offset = Some(o);
                            self.rip.status = Some(format!("Read offset set to {o:+}."));
                        }
                        _ => {
                            self.rip.status = Some("The offset is a number of samples, e.g. +48.".into());
                            return Task::none();
                        }
                    }
                }
            }
            Setting::RipVerifyOffset => {
                let Some(device) = self.rip.drives.first().map(|d| d.device.clone()) else {
                    self.rip.status = Some("No CD drive found.".into());
                    return Task::none();
                };
                if self.rip.active() {
                    self.rip.status = Some("Wait until the current rip is done.".into());
                    return Task::none();
                }
                let guess = whipper::resolve_offset(None, &self.drive(&device)).map(|(o, _)| o);
                self.rip.verifying_offset = true;
                self.rip.status = Some(
                    "Checking the offset with whipper (a few minutes; needs a well-known CD in the drive)…"
                        .into(),
                );
                return Task::perform(verify_offset(device, guess), |r| msg(RipMsg::OffsetChecked(r)));
            }
            _ => return Task::none(),
        }
        self.save_config();
        Task::none()
    }
}

fn notify_task(summary: String, body: String, warning: bool, sound: bool) -> Task<Message> {
    Task::perform(
        async move {
            if let Err(e) = listen_rip::notify::notify(&summary, &body, warning).await {
                tracing::info!("notification: {e}");
            }
            if sound {
                listen_rip::notify::play_sound().await;
            }
        },
        |()| Message::Noop,
    )
}

/// `whipper offset find` (stores the result in whipper.conf).
async fn verify_offset(device: PathBuf, guess: Option<i32>) -> Result<String, String> {
    let out = tokio::process::Command::new("whipper")
        .args(whipper::offset_find_args(&device, guess))
        .output()
        .await
        .map_err(|e| format!("whipper: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    let found = text.lines().rev().find(|l| l.to_lowercase().contains("offset of device is")).map(str::trim);
    match (out.status.success(), found) {
        (true, Some(line)) => Ok(format!("whipper: {line} (saved in whipper.conf)")),
        _ => Err(text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no result").trim().to_string()),
    }
}

/// Folder dialog (kdialog / zenity).
async fn pick_folder(start: String) -> Option<PathBuf> {
    use tokio::process::Command;
    let start = if start.is_empty() { std::env::var("HOME").unwrap_or_default() } else { start };
    let mut kd = Command::new("kdialog");
    kd.args(["--title", "Where should ripped albums go?", "--getexistingdirectory", &start]);
    let mut zen = Command::new("zenity");
    zen.args(["--file-selection", "--directory", "--title=Where should ripped albums go?"]);
    for mut cmd in [kd, zen] {
        if let Ok(out) = cmd.output().await {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return (out.status.success() && !path.is_empty()).then(|| path.into());
        }
    }
    None
}
