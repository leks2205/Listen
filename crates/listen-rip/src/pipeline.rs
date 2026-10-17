//! One disc, start to finish; the app sends [`Command`]s and receives [`Update`]s.
//!
//! 1. Rip (whipper) while MusicBrainz is asked; the best match is taken after `picker_timeout`
//!    unless the user picks another first.
//! 2. Eject.
//! 3. In the work folder: FLAC -8 (MD5 checked), tags, cover, ReplayGain, file names, rip log.
//! 4. Unreadable tracks: ask whether to publish or discard.
//! 5. Publish to the destination, waiting for it if it isn't mounted.

use crate::cover;
use crate::drive::DriveInfo;
use crate::flac;
use crate::layout;
use crate::musicbrainz::{self, Release};
use crate::publish;
use crate::ripper::{RipError, RipEvent, RipOutcome, RipRequest, Ripper};
use crate::tags::{AlbumMeta, MetaSource, TrackMeta};
use crate::toc::Toc;
use crate::whipper::{self, ArSummary};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone)]
pub struct JobConfig {
    pub drive: DriveInfo,
    pub toc: Toc,
    pub offset: i32,
    pub destination: PathBuf,
    /// Local scratch folder for this job (removed when done).
    pub work_dir: PathBuf,
    /// Listen's logs dir (whipper's output is kept there).
    pub log_dir: PathBuf,
    pub replay_gain: bool,
    pub eject: bool,
    pub picker_timeout: Duration,
    /// Name suffix for unknown albums, e.g. "2026-10-01 14.32".
    pub stamp: String,
    /// Where the metadata comes from.
    pub lookup: Lookup,
    /// Retry interval while the destination isn't available.
    pub destination_retry: Duration,
}

#[derive(Debug, Clone)]
pub enum Lookup {
    /// MusicBrainz, Cover Art Archive, iTunes.
    Online,
    /// Nothing (CD-Text or "Unknown").
    Offline,
    /// Already known (tests): these releases, this cover.
    Given { releases: Vec<Release>, cover: Option<Vec<u8>> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    Ripping,
    Tagging,
    /// Tracks couldn't be read: publish anyway or discard?
    NeedsDecision(String),
    Publishing,
    /// The destination isn't ready (reason); retried.
    WaitingForDestination(String),
    Done,
    Failed(String),
    Cancelled,
}

impl Stage {
    pub fn finished(&self) -> bool {
        matches!(self, Stage::Done | Stage::Failed(_) | Stage::Cancelled)
    }
}

/// A release in the picker.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub title: String,
    pub artist: String,
    pub details: String,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub artist: String,
    pub album: String,
    pub folder: PathBuf,
    pub accuraterip: ArSummary,
    pub source: MetaSource,
    pub disc_id: String,
    pub submit_url: String,
    pub warnings: Vec<String>,
}

impl Report {
    /// "AccurateRip: 12/12 tracks verified" (+ what didn't match).
    pub fn accuraterip_line(&self) -> String {
        let a = self.accuraterip;
        let mut s = format!("AccurateRip: {}/{} tracks verified", a.verified, a.tracks);
        if a.mismatched > 0 {
            s.push_str(&format!(", {} mismatch", a.mismatched));
        }
        if a.not_in_db > 0 {
            s.push_str(&format!(", {} not in the AccurateRip database", a.not_in_db));
        }
        s
    }
}

#[derive(Debug, Clone)]
pub enum Update {
    Stage(Stage),
    /// Whipper's current step ("Reading track 3 of 12").
    Activity(String),
    Progress {
        track: u32,
        total: u32,
        percent: u32,
    },
    /// Several releases match: the picker (best one first, `chosen` selected).
    Candidates {
        candidates: Vec<Candidate>,
        chosen: usize,
        timeout: Duration,
    },
    /// What the disc is (known after the lookup / choice).
    Identified {
        artist: String,
        album: String,
        source: MetaSource,
    },
    Cover(Vec<u8>),
    Warning(String),
    /// The drive is free (ripped and ejected): the next disc can start.
    DriveFree,
    Finished(Box<Report>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Use candidate `n` (index in the picker list).
    Choose(usize),
    Cancel,
    /// The disc left the drive.
    DiscRemoved,
    PublishAnyway,
    Discard,
    RetryPublish,
}

struct Job {
    cfg: JobConfig,
    tx: mpsc::UnboundedSender<Update>,
    rx: mpsc::UnboundedReceiver<Command>,
    /// Releases in picker order (best first).
    releases: Vec<Release>,
    chosen: usize,
    user_chose: bool,
    deadline: Option<Instant>,
    warnings: Vec<String>,
}

impl Job {
    fn send(&self, u: Update) {
        let _ = self.tx.send(u);
    }
    fn stage(&self, s: Stage) {
        tracing::info!("rip {}: {s:?}", self.cfg.toc.musicbrainz_id());
        self.send(Update::Stage(s));
    }
    fn warn(&mut self, w: String) {
        tracing::warn!("rip: {w}");
        self.send(Update::Warning(w.clone()));
        self.warnings.push(w);
    }

    fn set_releases(&mut self, releases: Vec<Release>) {
        let order = musicbrainz::rank(&releases, &self.cfg.toc);
        self.releases = order.into_iter().map(|i| releases[i].clone()).collect();
        self.chosen = 0;
        match self.releases.len() {
            0 => {}
            1 => self.identify_release(),
            _ => {
                self.deadline = Some(Instant::now() + self.cfg.picker_timeout);
                self.send(Update::Candidates {
                    candidates: self
                        .releases
                        .iter()
                        .map(|r| Candidate {
                            title: r.title.clone(),
                            artist: r.artist(),
                            details: r.details(),
                            id: r.id.clone(),
                        })
                        .collect(),
                    chosen: 0,
                    timeout: self.cfg.picker_timeout,
                });
                self.identify_release();
            }
        }
    }

    fn identify_release(&self) {
        if let Some(r) = self.releases.get(self.chosen) {
            self.send(Update::Identified {
                artist: r.artist(),
                album: r.title.clone(),
                source: MetaSource::MusicBrainz,
            });
        }
    }

    /// Applies a command that can arrive at any time; returns a reason to stop.
    fn command(&mut self, c: Command) -> Option<Stage> {
        match c {
            Command::Choose(n) if n < self.releases.len() => {
                self.chosen = n;
                self.user_chose = true;
                self.identify_release();
                None
            }
            Command::Cancel | Command::Discard => Some(Stage::Cancelled),
            Command::DiscRemoved => Some(Stage::Failed("the disc was removed".into())),
            _ => None,
        }
    }
}

/// Runs one job to the end. Local files are cleaned up; nothing on the destination is removed.
pub async fn run(
    cfg: JobConfig,
    ripper: Ripper,
    http: reqwest::Client,
    tx: mpsc::UnboundedSender<Update>,
    rx: mpsc::UnboundedReceiver<Command>,
) {
    let work = cfg.work_dir.clone();
    let mut job = Job {
        cfg,
        tx,
        rx,
        releases: Vec::new(),
        chosen: 0,
        user_chose: false,
        deadline: None,
        warnings: Vec::new(),
    };
    let end = run_job(&mut job, &ripper, &http).await;
    let keep_local = matches!(&end, Stage::Failed(r) if r.starts_with("publishing"));
    if !keep_local {
        // The local scratch folder only (never the destination).
        let _ = std::fs::remove_dir_all(&work);
    }
    job.stage(end);
}

async fn run_job(job: &mut Job, ripper: &Ripper, http: &reqwest::Client) -> Stage {
    let cfg = job.cfg.clone();
    let disc_id = cfg.toc.musicbrainz_id();
    let rip_dir = cfg.work_dir.join("whipper");
    if let Err(e) = std::fs::create_dir_all(&rip_dir) {
        return Stage::Failed(format!("{}: {e}", rip_dir.display()));
    }
    job.stage(Stage::Ripping);

    // Lookup in parallel with the rip.
    let lookup = {
        let (http, toc, mode) = (http.clone(), cfg.toc.clone(), cfg.lookup.clone());
        tokio::spawn(async move {
            match mode {
                Lookup::Online => musicbrainz::lookup(&http, &toc).await.map_err(|e| e.to_string()),
                Lookup::Offline => Ok(Vec::new()),
                Lookup::Given { releases, .. } => Ok(releases),
            }
        })
    };
    let mut lookup = Some(lookup);

    let stamp = time_stamp_for_log();
    let req = RipRequest {
        device: cfg.drive.device.clone(),
        offset: cfg.offset,
        out_dir: rip_dir.clone(),
        output_log: cfg.log_dir.join(format!("rip-{stamp}-{}.log", disc_id.trim_end_matches('-'))),
        audio_tracks: cfg.toc.audio_count() as u32,
    };
    let (ev_tx, mut events) = mpsc::unbounded_channel();
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let rip = ripper.rip(&req, &ev_tx, cancel_rx);
    tokio::pin!(rip);
    let mut stop: Option<Stage> = None;
    let mut track = (0, cfg.toc.audio_count() as u32);
    let outcome: Result<RipOutcome, RipError> = loop {
        tokio::select! {
            r = &mut rip => break r,
            Some(ev) = events.recv() => match ev {
                RipEvent::Stage(s) => job.send(Update::Activity(s)),
                RipEvent::Track { number, total } => {
                    track = (number, total);
                    job.send(Update::Progress { track: number, total, percent: 0 });
                }
                RipEvent::Percent(p) => job.send(Update::Progress { track: track.0, total: track.1, percent: p }),
                RipEvent::Warning(w) => tracing::info!("whipper: {w}"),
            },
            Ok(found) = async { lookup.as_mut().expect("guarded").await }, if lookup.is_some() => {
                lookup = None;
                match found {
                    Ok(rs) => job.set_releases(rs),
                    Err(e) => job.warn(format!("MusicBrainz lookup failed: {e}")),
                }
            }
            Some(c) = job.rx.recv() => {
                if let Some(s) = job.command(c) {
                    stop = Some(s);
                    let _ = cancel_tx.send(true);
                }
            }
        }
    };
    let outcome = match outcome {
        Ok(o) => o,
        Err(RipError::Cancelled) => return stop.unwrap_or(Stage::Cancelled),
        Err(e) => return stop.unwrap_or(Stage::Failed(e.to_string())),
    };
    if outcome.files.is_empty() {
        return Stage::Failed("whipper produced no files".into());
    }
    if cfg.eject
        && let Err(e) = crate::drive::eject(&cfg.drive.device).await
    {
        job.warn(format!("couldn't eject: {e}"));
    }
    job.send(Update::DriveFree);

    // The lookup (if still running) and the picker's deadline.
    if let Some(l) = lookup.take() {
        match tokio::time::timeout(Duration::from_secs(45), l).await {
            Ok(Ok(Ok(rs))) => job.set_releases(rs),
            Ok(Ok(Err(e))) => job.warn(format!("MusicBrainz lookup failed: {e}")),
            _ => job.warn("MusicBrainz didn't answer in time".into()),
        }
    }
    while let Some(deadline) = job.deadline.filter(|d| !job.user_chose && Instant::now() < *d) {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline.into()) => break,
            Some(c) = job.rx.recv() => if let Some(s) = job.command(c) { return s },
        }
    }
    job.deadline = None;

    job.stage(Stage::Tagging);
    let meta = match job.releases.get(job.chosen) {
        Some(r) => {
            let medium = r.medium_for(&disc_id, cfg.toc.audio_count()).unwrap_or(0);
            AlbumMeta::from_release(r, medium, &cfg.toc)
        }
        None => {
            let text = outcome
                .toc_file
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|t| whipper::parse_toc_cdtext(&t))
                .unwrap_or_default();
            if text.is_empty() {
                AlbumMeta::unknown(&cfg.toc, &cfg.stamp)
            } else {
                AlbumMeta::from_cdtext(&text, &cfg.toc, &cfg.stamp)
            }
        }
    };
    job.send(Update::Identified {
        artist: meta.album_artist.clone(),
        album: meta.album.clone(),
        source: meta.source,
    });
    if meta.needs_fix {
        job.warn(format!(
            "MusicBrainz doesn't know this disc — tagged from {}",
            match meta.source {
                MetaSource::CdText => "CD-Text",
                _ => "nothing (Unknown Album)",
            }
        ));
    }

    let album_dir = cfg.work_dir.join("album");
    match tag_album(job, &meta, &outcome, &album_dir, http).await {
        Ok(()) => {}
        Err(e) => return Stage::Failed(e),
    }
    if let Ok(c) = job.rx.try_recv()
        && let Some(s) = job.command(c)
    {
        return s;
    }

    let ar = outcome.log.summary();
    let missing: Vec<u32> =
        (1..=cfg.toc.audio_count() as u32).filter(|n| !outcome.files.iter().any(|(f, _)| f == n)).collect();
    if ar.skipped > 0 || !missing.is_empty() {
        let what =
            format!("{} track(s) couldn't be read: {:?}", missing.len().max(ar.skipped as usize), missing);
        job.stage(Stage::NeedsDecision(what));
        loop {
            match job.rx.recv().await {
                Some(Command::PublishAnyway) => break,
                Some(Command::Discard) | Some(Command::Cancel) | None => return Stage::Cancelled,
                Some(_) => {}
            }
        }
    }

    // Publish, waiting for the destination if needed.
    let need = publish::folder_size(&album_dir);
    let rel = layout::album_dir(&meta);
    loop {
        let dest = cfg.destination.clone();
        let checked = tokio::task::spawn_blocking(move || publish::check(&dest, need)).await;
        match checked {
            Ok(Ok(())) => break,
            Ok(Err(e)) => {
                job.stage(Stage::WaitingForDestination(e.to_string()));
                tokio::select! {
                    _ = tokio::time::sleep(cfg.destination_retry) => {}
                    c = job.rx.recv() => match c {
                        Some(Command::Discard) | Some(Command::Cancel) | None => return Stage::Cancelled,
                        _ => {}
                    },
                }
            }
            Err(e) => return Stage::Failed(e.to_string()),
        }
    }
    job.stage(Stage::Publishing);
    let (dest, staging, rel2, tag) = (
        cfg.destination.clone(),
        album_dir.clone(),
        rel.clone(),
        format!("{}-{stamp}", disc_id.trim_end_matches('-')),
    );
    let folder =
        match tokio::task::spawn_blocking(move || publish::publish(&staging, &dest, &rel2, &tag)).await {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => return Stage::Failed(format!("publishing: {e}")),
            Err(e) => return Stage::Failed(format!("publishing: {e}")),
        };
    if folder != cfg.destination.join(&rel) {
        let w = format!("{} already existed; this rip is in {}", rel.display(), folder.display());
        job.warn(w);
    }
    if ar.mismatched > 0 {
        job.warn(format!("AccurateRip: {} track(s) didn't match (files kept)", ar.mismatched));
    }
    let report = Report {
        artist: meta.album_artist.clone(),
        album: meta.album.clone(),
        folder,
        accuraterip: ar,
        source: meta.source,
        disc_id,
        submit_url: cfg.toc.submit_url(),
        warnings: job.warnings.clone(),
    };
    job.send(Update::Finished(Box::new(report)));
    Stage::Done
}

/// Level 8, tags, cover, ReplayGain and names, into `album_dir`.
async fn tag_album(
    job: &mut Job,
    meta: &AlbumMeta,
    outcome: &RipOutcome,
    album_dir: &Path,
    http: &reqwest::Client,
) -> Result<(), String> {
    std::fs::create_dir_all(album_dir).map_err(|e| e.to_string())?;
    // Cover first (so every file gets it embedded).
    let mut cover_file: Option<PathBuf> = None;
    let found = match &job.cfg.lookup {
        Lookup::Offline => None,
        Lookup::Given { cover, .. } => {
            cover.clone().map(|bytes| cover::Cover { bytes, source: "given".into() })
        }
        Lookup::Online => match job.releases.get(job.chosen) {
            Some(r) => cover::fetch(http, r).await,
            None if meta.source == MetaSource::CdText => {
                cover::fetch_itunes(http, &meta.album_artist, &meta.album).await
            }
            None => None,
        },
    };
    if !matches!(job.cfg.lookup, Lookup::Offline) {
        match found {
            Some(c) => {
                tracing::info!("cover from {}", c.source);
                let path = album_dir.join(c.file_name());
                std::fs::write(&path, &c.bytes).map_err(|e| e.to_string())?;
                job.send(Update::Cover(c.bytes.clone()));
                cover_file = Some(path);
            }
            None => job.warn("no cover found".into()),
        }
    }
    let mut files = Vec::new();
    let total = outcome.files.len() as u32;
    for (i, (number, src)) in outcome.files.iter().enumerate() {
        job.send(Update::Activity(format!("Tagging track {} of {total}", i + 1)));
        job.send(Update::Progress { track: i as u32 + 1, total, percent: 0 });
        flac::recompress(src).await?;
        let track = meta.tracks.iter().find(|t| t.number == *number).cloned().unwrap_or_else(|| TrackMeta {
            number: *number,
            title: if *number == 0 { "Hidden Track".into() } else { format!("Track {number:02}") },
            artist: meta.album_artist.clone(),
            ..Default::default()
        });
        flac::write_tags(src, &meta.vorbis_comments(&track)).await?;
        if let Some(c) = &cover_file {
            flac::embed_picture(src, c).await?;
        }
        let dst = album_dir.join(layout::track_file(meta, &track));
        std::fs::rename(src, &dst).map_err(|e| e.to_string())?;
        files.push(dst);
    }
    if job.cfg.replay_gain {
        job.send(Update::Activity("ReplayGain".into()));
        if let Err(e) = flac::add_replay_gain(&files).await {
            job.warn(format!("ReplayGain: {e}"));
        }
    }
    if let Some(log) = &outcome.log_file {
        let name = format!("{}.log", layout::sanitize(&meta.album));
        std::fs::copy(log, album_dir.join(name)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// "20261001-143200" (UTC) for file names.
fn time_stamp_for_log() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// "2026-10-01 14.32" in local time (for "Unknown Album (…)").
pub fn local_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    #[cfg(unix)]
    {
        let t = secs as libc::time_t;
        // SAFETY: localtime_r writes into our zeroed struct only.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return format!(
                "{:04}-{:02}-{:02} {:02}.{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min
            );
        }
    }
    let (y, mo, d, h, mi, _) = civil(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}.{mi:02}")
}

/// Unix seconds → (year, month, day, hour, minute, second), UTC.
pub fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32)
}

#[cfg(test)]
mod tests {
    #[test]
    fn civil_dates() {
        assert_eq!(super::civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(super::civil(1_790_000_000), (2026, 9, 21, 14, 13, 20));
    }
}
