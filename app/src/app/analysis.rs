//! Background audio analysis into the shared store (queue first, then the DJ's candidate pool, then the
//! Navidrome library), and a handle to look results up.
//!
//! YouTube Music is opt-in and analysed on demand only (its fetches are budgeted). The store may sit in a
//! folder shared between machines: each appends to its own shard and reads everyone's.

use super::{Listen, Message};
use iced::Subscription;
use iced::futures::{SinkExt, Stream};
use listen_analysis::record_key;
use listen_analysis::store::{self, ShardStore};
use listen_analysis::{BeatModel, BeatSource, Cancel, Opts};
use listen_core::analysis::{
    ANALYZER_VERSION, FileId, FlowFeatures, MIN_COMPATIBLE_VERSION, Outcome, TrackAnalysis,
};
use listen_core::model::{Song, SourceKind};
use listen_source::{AnalysisSource, MusicSource, SongQuery, Urgency};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// Shared, cheap to clone; read from the UI thread and background tasks.
#[derive(Clone, Default)]
pub struct AnalysisHandle {
    store: Arc<RwLock<Option<ShardStore>>>,
    /// Records not in a store (tests; `--check-analysis`).
    extra: Arc<RwLock<HashMap<String, Vec<Arc<TrackAnalysis>>>>>,
    /// Songs whose candidate pool was already asked for (one radio request
    /// per song, however often the track-change hook runs).
    pooled: Arc<Mutex<HashSet<String>>>,
}

impl std::fmt::Debug for AnalysisHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AnalysisHandle")
    }
}

impl AnalysisHandle {
    /// Adds records without a store.
    pub fn insert(&self, records: impl IntoIterator<Item = Arc<TrackAnalysis>>) {
        let Ok(mut extra) = self.extra.write() else { return };
        for r in records {
            extra.entry(r.song_id.clone()).or_default().push(r);
        }
    }

    /// The best record for `song`, keyed by [`record_key`] (videoIds are namespaced).
    /// A YouTube Music `FileId` only knows the listing duration (often 0), so it's matched leniently.
    pub fn lookup(&self, song: &Song) -> Option<Arc<TrackAnalysis>> {
        if !matches!(song.source, SourceKind::Subsonic | SourceKind::YouTubeMusic) {
            return None;
        }
        let key = record_key(song);
        let file = FileId::of(song);
        // The store shares its records: this is a reference count, not a copy.
        let from_store = self.store.read().ok()?.as_ref().and_then(|s| s.lookup(&key, &file).cloned());
        let from_extra = self.extra.read().ok().and_then(|extra| {
            extra
                .get(&key)?
                .iter()
                .filter(|r| r.version >= MIN_COMPATIBLE_VERSION && r.file.matches(&file))
                .reduce(|best, r| if r.better_than(best) { r } else { best })
                .cloned()
        });
        match (from_store, from_extra) {
            (Some(a), Some(b)) => Some(if b.better_than(&a) { b } else { a }),
            (a, b) => a.or(b),
        }
    }

    pub fn flow(&self, song: &Song) -> Option<FlowFeatures> {
        self.lookup(song).and_then(|r| r.features().map(FlowFeatures::of))
    }

    /// Already analysed for this analyser version (or not analysable).
    fn done(&self, song: &Song) -> bool {
        self.lookup(song).is_some_and(|r| match &r.outcome {
            Outcome::Ok(_) => r.version >= ANALYZER_VERSION,
            Outcome::Failed { transient, .. } => !transient && r.version >= ANALYZER_VERSION,
        })
    }

    /// True the first time it's called for `song` (the pool is requested once).
    fn first_pool(&self, song: &Song) -> bool {
        let Ok(mut pooled) = self.pooled.lock() else { return false };
        if pooled.len() > 2000 {
            pooled.clear();
        }
        pooled.insert(record_key(song))
    }
}

/// How urgently a song is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Playing or next: a transition depends on it.
    Now,
    /// Further down the queue.
    Queue,
    /// The DJ's candidate pool (similar songs to the current one), so the
    /// next pick can be ordered by flow.
    Pool,
    /// The rest of the library (Navidrome).
    Library,
}

impl Priority {
    /// What the source is told: queue work may spend YouTube's request
    /// budget right away, pool and library work may be refused.
    pub fn urgency(self) -> Urgency {
        match self {
            Priority::Now | Priority::Queue => Urgency::Now,
            Priority::Pool | Priority::Library => Urgency::Background,
        }
    }
}

/// At most this many pool songs per playing song (YouTube's request budget).
pub const POOL_JOBS: usize = 20;
/// A background request the source turned down (budget) is tried again after this.
const BUDGET_RETRY: Duration = Duration::from_secs(120);
/// A failed resolve (network, edge refusal) is retried this often, after 30 s, 60 s.
const RESOLVE_RETRIES: u8 = 2;

/// A song to analyse. Its audio is resolved when the job starts, not when queued (YouTube URLs expire).
#[derive(Clone)]
pub struct Job {
    pub song: Song,
    pub source: Arc<dyn MusicSource>,
    pub priority: Priority,
    /// Failed resolves so far.
    retries: u8,
}

impl Job {
    pub fn new(song: Song, source: Arc<dyn MusicSource>, priority: Priority) -> Self {
        Job { song, source, priority, retries: 0 }
    }
}

pub enum Command {
    Want(Vec<Job>),
    /// Queue the whole library of this server at low priority.
    Crawl(Arc<dyn MusicSource>),
    /// A DJ transition is running: library work waits (CPU for the mix).
    Hold(bool),
}

/// What the settings page shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnalysisStatus {
    pub dir: PathBuf,
    pub machine: String,
    /// Songs with a record, and records per machine.
    pub songs: usize,
    pub machines: Vec<(String, usize)>,
    pub queued: usize,
    /// Library songs found while crawling, and whether the crawl finished.
    pub library: Option<(usize, bool)>,
    pub current: Option<String>,
    /// Analysed by this machine since start.
    pub done_here: usize,
    pub beats: &'static str,
    pub waiting: Option<&'static str>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AnalysisMsg {
    Ready(AnalysisHandle, tokio::sync::mpsc::UnboundedSender<CommandBox>),
    Status(AnalysisStatus),
    /// A song's analysis was stored.
    Analysed(String),
    /// Candidates the DJ may pick next (the current song's radio): analysed
    /// at [`Priority::Pool`].
    Pool(Vec<Song>),
}

/// `Command` for messages (needs Debug/Clone through iced).
#[derive(Clone)]
pub struct CommandBox(pub Arc<std::sync::Mutex<Option<Command>>>);

impl std::fmt::Debug for CommandBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Command")
    }
}

/// Everything that restarts the worker when it changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnalysisConfig {
    pub shared_dir: PathBuf,
    pub models_dir: PathBuf,
    pub library: bool,
    pub on_battery: bool,
}

impl Listen {
    /// The worker's configuration, if analysis runs here.
    pub(super) fn analysis_config(&self) -> Option<AnalysisConfig> {
        let s = &self.config.settings.analysis;
        if !s.enabled || self.demo || self.ephemeral && std::env::var("LISTEN_ANALYSIS").is_err() {
            return None;
        }
        let paths = self.paths.as_ref()?;
        let shared_dir = if s.shared_dir.trim().is_empty() {
            paths.data_dir.join("analysis")
        } else {
            super::shellexpand_home(s.shared_dir.trim())
        };
        Some(AnalysisConfig {
            shared_dir,
            models_dir: paths.cache_dir.join("models"),
            library: s.analyze_library,
            on_battery: s.on_battery,
        })
    }

    /// Whether songs of `source` get analysed; YouTube Music only when switched on with a results folder.
    fn may_analyse(&self, source: &dyn MusicSource) -> bool {
        let s = &self.config.settings.analysis;
        source.analysable()
            && (source.kind() != SourceKind::YouTubeMusic
                || s.youtube_music && !s.shared_dir.trim().is_empty())
    }

    fn analysis_send(&self, cmd: Command) {
        if let Some(tx) = &self.analysis_tx {
            let _ = tx.send(CommandBox(Arc::new(std::sync::Mutex::new(Some(cmd)))));
        }
    }

    /// Asks for the playing, next and queued songs (current first).
    pub(super) fn analysis_want(&self) {
        if self.analysis_tx.is_none() {
            return;
        }
        let position = self.queue.current_position().unwrap_or(0);
        let jobs: Vec<Job> = self
            .queue
            .iter()
            .skip(position)
            .take(12)
            .enumerate()
            .flat_map(|(i, item)| {
                let priority = if i < 2 { Priority::Now } else { Priority::Queue };
                // Mixed queues: each song through its own server. A YouTube Music song that plays
                // from an own server is analysed there too (its transitions use that file).
                let own = self.playable(&item.song);
                [(own.id != item.song.id).then_some(own), Some(item.song.clone())]
                    .into_iter()
                    .flatten()
                    .filter_map(move |song| {
                        let source =
                            self.sources.get(&song.server_id).filter(|s| self.may_analyse(s.as_ref()))?;
                        Some(Job::new(song, source.clone(), priority))
                    })
            })
            .collect();
        if !jobs.is_empty() {
            self.analysis_send(Command::Want(jobs));
        }
    }

    /// Fetches the playing YouTube Music song's radio (and the AI DJ seed's) once and queues its top
    /// [`POOL_JOBS`] for analysis, so the next DJ pick has flow data. Navidrome is covered by the crawl.
    pub(super) fn analysis_pool(&self) -> iced::Task<Message> {
        let (Some(handle), true) = (self.analysis.as_ref(), self.analysis_tx.is_some()) else {
            return iced::Task::none();
        };
        if self.demo || !(self.config.settings.auto_dj.enabled || self.ai_dj_on()) {
            return iced::Task::none();
        }
        let current = self.queue.current().map(|i| i.song.clone());
        let seed = if self.ai_dj_on() { self.dj.seed.clone() } else { None };
        let tasks = [current, seed].into_iter().flatten().filter_map(|song| {
            if song.source != SourceKind::YouTubeMusic {
                return None;
            }
            let source = self.sources.get(&song.server_id).filter(|s| self.may_analyse(s.as_ref()))?.clone();
            if !handle.first_pool(&song) {
                return None;
            }
            Some(iced::Task::perform(
                async move {
                    source.similar_songs(&song, POOL_JOBS + 5).await.unwrap_or_else(|e| {
                        tracing::debug!("analysis pool for “{}”: {e}", song.title);
                        vec![]
                    })
                },
                |songs| Message::Analysis(AnalysisMsg::Pool(songs)),
            ))
        });
        iced::Task::batch(tasks.collect::<Vec<_>>())
    }

    /// Queues pool candidates (not analysed yet, capped) at [`Priority::Pool`].
    fn analysis_pool_jobs(&self, songs: Vec<Song>) -> Vec<Job> {
        let Some(handle) = self.analysis.as_ref() else { return vec![] };
        songs
            .into_iter()
            .filter(|s| !handle.done(s))
            .filter_map(|song| {
                let source =
                    self.sources.get(&song.server_id).filter(|s| self.may_analyse(s.as_ref()))?.clone();
                Some(Job::new(song, source, Priority::Pool))
            })
            .take(POOL_JOBS)
            .collect()
    }

    /// Starts the library crawl on the active server (Navidrome only).
    pub(super) fn analysis_crawl(&self) {
        if let Some(source) = &self.source
            && source.kind() == SourceKind::Subsonic
            && self.config.settings.analysis.analyze_library
        {
            self.analysis_send(Command::Crawl(source.clone()));
        }
    }

    pub(super) fn analysis_hold(&self, hold: bool) {
        self.analysis_send(Command::Hold(hold));
    }

    pub(super) fn on_analysis(&mut self, msg: AnalysisMsg) -> iced::Task<Message> {
        match msg {
            AnalysisMsg::Ready(handle, tx) => {
                self.analysis = Some(handle);
                self.analysis_tx = Some(tx);
                self.analysis_want();
                self.analysis_crawl();
                return self.analysis_pool();
            }
            AnalysisMsg::Status(status) => self.analysis_status = Some(status),
            AnalysisMsg::Analysed(id) => {
                return iced::Task::batch([self.mix_consider(), self.waveform_analysed(&id)]);
            }
            AnalysisMsg::Pool(songs) => {
                let jobs = self.analysis_pool_jobs(songs);
                if !jobs.is_empty() {
                    tracing::info!("analysis: {} pool candidates queued", jobs.len());
                    self.analysis_send(Command::Want(jobs));
                }
            }
        }
        iced::Task::none()
    }
}

pub fn subscription(config: AnalysisConfig) -> Subscription<Message> {
    Subscription::run_with(config, worker)
}

fn worker(config: &AnalysisConfig) -> impl Stream<Item = Message> + use<> {
    let config = config.clone();
    iced::stream::channel(16, async move |mut out: iced::futures::channel::mpsc::Sender<Message>| {
        let machine = store::machine_id();
        let dir = config.shared_dir.clone();
        let opened = {
            let (dir, machine) = (dir.clone(), machine.clone());
            tokio::task::spawn_blocking(move || ShardStore::open(&dir, &machine)).await
        };
        let mut status = AnalysisStatus { dir: dir.clone(), machine: machine.clone(), ..Default::default() };
        let store = match opened {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                tracing::warn!("analysis store at {}: {e}", dir.display());
                status.error = Some(format!("Can't use {}: {e}", dir.display()));
                let _ = out.send(Message::Analysis(AnalysisMsg::Status(status))).await;
                std::future::pending::<()>().await;
                return;
            }
            Err(_) => return,
        };
        tracing::info!("analysis store {} ({} songs analysed)", dir.display(), store.len());
        let handle = AnalysisHandle { store: Arc::new(RwLock::new(Some(store))), ..Default::default() };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CommandBox>();
        let _ = out.send(Message::Analysis(AnalysisMsg::Ready(handle.clone(), tx))).await;

        // The beat model (≈11 MB, downloaded once); the built-in tracker without it.
        let beats = match BeatModel::ensure(&config.models_dir).await {
            Ok(model) => BeatSource::BeatThis(model),
            Err(e) => {
                tracing::warn!("beat model unavailable, using the built-in tracker: {e}");
                BeatSource::Own
            }
        };
        status.beats = if matches!(beats, BeatSource::Own) { "built-in tracker" } else { "Beat This!" };

        let mut worker = listen_analysis::Worker::new();
        let mut queue = JobQueue::default();
        let mut hold = false;
        let mut tried: HashSet<String> = HashSet::new();
        let mut last_refresh = Instant::now();
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        let server_hint = String::new();
        // Last status sent; the app only hears about changes.
        let mut sent: Option<AnalysisStatus> = None;
        loop {
            // Take every pending command first (they may change priorities).
            while let Ok(cmd) = rx.try_recv() {
                let Some(cmd) = cmd.0.lock().ok().and_then(|mut c| c.take()) else { continue };
                apply(cmd, &mut queue, &handle, &mut hold, &mut status, &mut out).await;
            }
            let allow_library = config.library && !hold && (config.on_battery || !on_battery());
            // Pool work is on demand (no library setting, also on battery),
            // but leaves the CPU to a running DJ transition.
            let lanes = Lanes { pool: !hold, library: allow_library };
            status.waiting = if queue.library.is_empty() || allow_library {
                None
            } else if hold {
                Some("paused during a DJ transition")
            } else if !config.library {
                Some("library analysis is off")
            } else {
                Some("paused on battery")
            };
            let job = queue.next(Instant::now(), lanes, |s| handle.done(s) || tried.contains(&record_key(s)));
            status.queued = queue.len();
            status.songs = with_store(&handle, |s| s.len()).unwrap_or(0);
            let Some(job) = job else {
                status.current = None;
                send_status(&mut out, &status, &mut sent).await;
                tokio::select! {
                    cmd = rx.recv() => match cmd {
                        Some(cmd) => {
                            if let Some(cmd) = cmd.0.lock().ok().and_then(|mut c| c.take()) {
                                apply(cmd, &mut queue, &handle, &mut hold, &mut status, &mut out).await;
                            }
                        }
                        None => break,
                    },
                    _ = tick.tick() => {
                        worker.stop_if_idle().await;
                        housekeeping(&handle, &mut last_refresh, &mut status);
                    }
                }
                continue;
            };
            status.current = Some(format!("{} – {}", job.song.artist_line(), job.song.title));
            send_status(&mut out, &status, &mut sent).await;
            let Some((job, input)) = resolve(job, &mut queue, &mut tried).await else { continue };
            let opts = Opts { beats: beats.clone(), bpm_hint: job.song.bpm.map(|b| b as f32) };
            let started = Instant::now();
            // mpv decodes a URL and a local file alike; the helper process
            // gives the memory back when the queue runs dry.
            let result = worker.analyze(&input.input.as_mpv_arg(), &opts, &Cancel::default()).await;
            match &result {
                Ok(f) => tracing::info!(
                    "analysed “{}” in {:.1}s: {}",
                    job.song.title,
                    started.elapsed().as_secs_f32(),
                    FlowFeatures::of(f).describe()
                ),
                Err(e) => tracing::warn!("analysis of “{}” failed: {e}", job.song.title),
            }
            if let Some(record) =
                listen_analysis::record(&job.song, input.file, &server_hint, &machine, result)
            {
                let stored = handle.store.write().ok().and_then(|mut s| s.as_mut().map(|s| s.append(record)));
                if let Some(Err(e)) = stored {
                    status.error = Some(format!("Can't write the analysis: {e}"));
                }
                status.done_here += 1;
                let _ = out.send(Message::Analysis(AnalysisMsg::Analysed(job.song.id.clone()))).await;
            }
            housekeeping(&handle, &mut last_refresh, &mut status);
        }
        if let Ok(mut store) = handle.store.write()
            && let Some(store) = store.as_mut()
        {
            let _ = store.flush();
        }
        std::future::pending::<()>().await;
    })
}

/// Sends `status` unless it is what was sent last.
async fn send_status(
    out: &mut iced::futures::channel::mpsc::Sender<Message>,
    status: &AnalysisStatus,
    sent: &mut Option<AnalysisStatus>,
) {
    if sent.as_ref() == Some(status) {
        return;
    }
    *sent = Some(status.clone());
    let _ = out.send(Message::Analysis(AnalysisMsg::Status(status.clone()))).await;
}

fn with_store<T>(handle: &AnalysisHandle, f: impl FnOnce(&ShardStore) -> T) -> Option<T> {
    handle.store.read().ok()?.as_ref().map(f)
}

/// Writes due records and picks up other machines' new ones (every 5 min).
fn housekeeping(handle: &AnalysisHandle, last_refresh: &mut Instant, status: &mut AnalysisStatus) {
    let Ok(mut guard) = handle.store.write() else { return };
    let Some(store) = guard.as_mut() else { return };
    if let Err(e) = store.flush_if_due() {
        status.error = Some(format!("Can't write the analysis: {e}"));
    }
    if last_refresh.elapsed() >= Duration::from_secs(300) {
        *last_refresh = Instant::now();
        if let Err(e) = store.refresh() {
            tracing::warn!("analysis store refresh: {e}");
        }
    }
    status.machines = store.machines();
    status.songs = store.len();
}

async fn apply(
    cmd: Command,
    queue: &mut JobQueue,
    handle: &AnalysisHandle,
    hold: &mut bool,
    status: &mut AnalysisStatus,
    out: &mut iced::futures::channel::mpsc::Sender<Message>,
) {
    match cmd {
        Command::Want(jobs) => {
            for job in jobs {
                queue.push(job);
            }
        }
        Command::Hold(h) => *hold = h,
        Command::Crawl(source) => {
            // Page through the library (a few requests per thousand songs).
            let mut start = 0;
            loop {
                let q = SongQuery { start, limit: 500, ..Default::default() };
                let page = match source.songs(&q).await {
                    Ok(p) => p.items,
                    Err(e) => {
                        tracing::warn!("analysis: library listing failed: {e}");
                        break;
                    }
                };
                let n = page.len();
                for song in page {
                    if handle.done(&song) {
                        continue;
                    }
                    queue.push(Job::new(song, source.clone(), Priority::Library));
                }
                start += n;
                status.library = Some((start, false));
                let _ = out.send(Message::Analysis(AnalysisMsg::Status(status.clone()))).await;
                if n < 500 {
                    break;
                }
            }
            status.library = Some((start, true));
            tracing::info!("analysis: {start} library songs listed, {} to analyse", queue.library.len());
        }
    }
}

/// Resolves a job's audio through its source. `None`: deferred (budget refusal, retries left) or given up
/// for this session (`tried`).
async fn resolve(
    mut job: Job,
    queue: &mut JobQueue,
    tried: &mut HashSet<String>,
) -> Option<(Job, AnalysisSource)> {
    let key = record_key(&job.song);
    if !job.source.analysable() {
        tried.insert(key);
        return None;
    }
    let urgency = job.priority.urgency();
    match job.source.analysis_source(&job.song, urgency).await {
        Ok(Some(input)) => {
            tried.insert(key);
            Some((job, input))
        }
        // Budget used up: not a failure, ask again later.
        Ok(None) if urgency == Urgency::Background => {
            tracing::debug!("analysis: “{}” deferred (request budget)", job.song.title);
            queue.defer(job, Instant::now() + BUDGET_RETRY);
            None
        }
        Ok(None) => {
            tried.insert(key);
            None
        }
        Err(e) if job.retries < RESOLVE_RETRIES => {
            tracing::info!("analysis: can't get “{}” yet: {e}", job.song.title);
            job.retries += 1;
            let wait = Duration::from_secs(30) * u32::from(job.retries);
            queue.defer(job, Instant::now() + wait);
            None
        }
        Err(e) => {
            tracing::warn!("analysis: giving up on “{}”: {e}", job.song.title);
            tried.insert(key);
            None
        }
    }
}

/// Which background lanes may run now (the urgent one always may).
#[derive(Debug, Clone, Copy)]
struct Lanes {
    pool: bool,
    library: bool,
}

/// Jobs by priority; a song is queued once (at its most urgent priority),
/// keyed by [`record_key`] so a Navidrome id and a videoId never merge.
#[derive(Default)]
struct JobQueue {
    urgent: VecDeque<Job>,
    pool: VecDeque<Job>,
    library: VecDeque<Job>,
    /// Put back until the instant (budget refusals, failed resolves).
    deferred: Vec<(Instant, Job)>,
    ids: HashMap<String, Priority>,
}

impl JobQueue {
    fn push(&mut self, job: Job) {
        let key = record_key(&job.song);
        match self.ids.get(&key) {
            Some(p) if *p <= job.priority => return,
            Some(_) => {
                // Promote: drop the older, less urgent entry.
                let other = |j: &Job| record_key(&j.song) != key;
                self.library.retain(other);
                self.pool.retain(other);
                self.urgent.retain(other);
                self.deferred.retain(|(_, j)| other(j));
            }
            None => {}
        }
        self.ids.insert(key, job.priority);
        self.enqueue(job);
    }

    fn enqueue(&mut self, job: Job) {
        match job.priority {
            Priority::Now => self.urgent.push_front(job),
            Priority::Queue => self.urgent.push_back(job),
            Priority::Pool => self.pool.push_back(job),
            Priority::Library => self.library.push_back(job),
        }
    }

    /// Holds `job` back until `until`; it keeps its place in `ids`, so a
    /// more urgent request for the song promotes it right away.
    fn defer(&mut self, job: Job, until: Instant) {
        self.ids.insert(record_key(&job.song), job.priority);
        self.deferred.push((until, job));
    }

    fn next(&mut self, now: Instant, lanes: Lanes, skip: impl Fn(&Song) -> bool) -> Option<Job> {
        let (due, later): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.deferred).into_iter().partition(|(t, _)| *t <= now);
        self.deferred = later;
        for (_, job) in due {
            self.enqueue(job);
        }
        loop {
            let job = match self.urgent.pop_front() {
                Some(j) => j,
                None if lanes.pool && !self.pool.is_empty() => self.pool.pop_front()?,
                None if lanes.library => self.library.pop_front()?,
                None => return None,
            };
            self.ids.remove(&record_key(&job.song));
            if !skip(&job.song) {
                return Some(job);
            }
        }
    }

    fn len(&self) -> usize {
        self.urgent.len() + self.pool.len() + self.library.len() + self.deferred.len()
    }
}

/// Running on battery (Linux: a mains supply exists and none is online).
fn on_battery() -> bool {
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else { return false };
    let mut mains = false;
    for e in entries.flatten() {
        let path = e.path();
        if std::fs::read_to_string(path.join("type")).is_ok_and(|t| t.trim() == "Mains") {
            mains = true;
            if std::fs::read_to_string(path.join("online")).is_ok_and(|o| o.trim() == "1") {
                return false;
            }
        }
    }
    mains
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::{ImageRef, Page, Playlist, SearchResults};
    use listen_source::{AlbumQuery, AudioInput, Capabilities, ListQuery, StreamSource};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A YouTube-Music-like source: `budget` background fetches, then
    /// `Ok(None)`; the first `failures` resolves fail.
    struct Fake {
        kind: SourceKind,
        budget: AtomicUsize,
        failures: AtomicUsize,
        calls: AtomicUsize,
    }

    impl Fake {
        fn new(kind: SourceKind, budget: usize, failures: usize) -> Arc<Self> {
            Arc::new(Fake { kind, budget: budget.into(), failures: failures.into(), calls: 0.into() })
        }
    }

    #[async_trait::async_trait]
    impl MusicSource for Fake {
        fn kind(&self) -> SourceKind {
            self.kind
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::empty()
        }
        fn analysable(&self) -> bool {
            true
        }
        async fn analysis_source(
            &self,
            song: &Song,
            urgency: Urgency,
        ) -> listen_source::Result<Option<AnalysisSource>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.failures.load(Ordering::SeqCst) > 0 {
                self.failures.fetch_sub(1, Ordering::SeqCst);
                return Err(listen_source::SourceError::Network("edge refused".into()));
            }
            if urgency == Urgency::Background {
                if self.budget.load(Ordering::SeqCst) == 0 {
                    return Ok(None);
                }
                self.budget.fetch_sub(1, Ordering::SeqCst);
            }
            Ok(Some(AnalysisSource {
                input: AudioInput::File(format!("/cache/{}.webm", song.id).into()),
                file: FileId {
                    duration_ms: 213_581,
                    suffix: None,
                    format: Some("251".into()),
                    bytes: Some(9),
                },
            }))
        }
        async fn ping(&self) -> listen_source::Result<()> {
            Ok(())
        }
        async fn albums(&self, _: &AlbumQuery) -> listen_source::Result<Page<listen_core::model::Album>> {
            unimplemented!()
        }
        async fn album(&self, _: &str) -> listen_source::Result<listen_core::model::Album> {
            unimplemented!()
        }
        async fn artists(&self, _: &ListQuery) -> listen_source::Result<Page<listen_core::model::Artist>> {
            unimplemented!()
        }
        async fn artist(&self, _: &str) -> listen_source::Result<listen_core::model::Artist> {
            unimplemented!()
        }
        async fn artist_albums(&self, _: &str) -> listen_source::Result<Vec<listen_core::model::Album>> {
            unimplemented!()
        }
        async fn top_songs(
            &self,
            _: &listen_core::model::Artist,
            _: usize,
        ) -> listen_source::Result<Vec<Song>> {
            unimplemented!()
        }
        async fn songs(&self, _: &SongQuery) -> listen_source::Result<Page<Song>> {
            unimplemented!()
        }
        async fn random_songs(&self, _: usize, _: Option<&str>) -> listen_source::Result<Vec<Song>> {
            unimplemented!()
        }
        async fn search(&self, _: &str, _: usize) -> listen_source::Result<SearchResults> {
            unimplemented!()
        }
        async fn playlists(&self) -> listen_source::Result<Vec<Playlist>> {
            unimplemented!()
        }
        async fn playlist_songs(&self, _: &str) -> listen_source::Result<Vec<Song>> {
            unimplemented!()
        }
        async fn set_favorite(&self, _: &[String], _: bool) -> listen_source::Result<()> {
            unimplemented!()
        }
        async fn stream(&self, _: &Song) -> listen_source::Result<StreamSource> {
            unimplemented!()
        }
        fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
            None
        }
    }

    fn song(id: &str, kind: SourceKind) -> Song {
        let mut song = crate::demo::album_songs(0)[0].clone();
        song.id = id.into();
        song.source = kind;
        if kind == SourceKind::YouTubeMusic {
            // Artist top songs and some lists carry no duration.
            song.duration = Duration::ZERO;
            song.codec = None;
        }
        song
    }

    fn job(id: &str, priority: Priority) -> Job {
        Job::new(song(id, SourceKind::Subsonic), Fake::new(SourceKind::Subsonic, 0, 0), priority)
    }

    const ALL: Lanes = Lanes { pool: true, library: true };

    #[test]
    fn queue_orders_by_urgency_and_promotes() {
        let mut q = JobQueue::default();
        let now = Instant::now();
        q.push(job("lib1", Priority::Library));
        q.push(job("lib2", Priority::Library));
        q.push(job("pool1", Priority::Pool));
        q.push(job("q1", Priority::Queue));
        q.push(job("now1", Priority::Now));
        q.push(job("lib2", Priority::Now)); // promoted
        q.push(job("q1", Priority::Library)); // not demoted
        let order: Vec<String> =
            std::iter::from_fn(|| q.next(now, ALL, |_| false)).map(|j| j.song.id).collect();
        assert_eq!(order, ["lib2", "now1", "q1", "pool1", "lib1"]);
        // Background work waits when not allowed; pool runs without library.
        q.push(job("lib3", Priority::Library));
        q.push(job("pool2", Priority::Pool));
        let none = Lanes { pool: false, library: false };
        assert!(q.next(now, none, |_| false).is_none());
        let pool_only = Lanes { pool: true, library: false };
        assert_eq!(q.next(now, pool_only, |_| false).map(|j| j.song.id).as_deref(), Some("pool2"));
        assert!(q.next(now, pool_only, |_| false).is_none());
        assert_eq!(q.next(now, ALL, |s| s.id == "lib3").map(|j| j.song.id), None);
    }

    #[test]
    fn queue_keeps_a_videoid_apart_from_the_same_navidrome_id() {
        let mut q = JobQueue::default();
        let src = Fake::new(SourceKind::YouTubeMusic, 0, 0);
        q.push(job("abc", Priority::Library));
        q.push(Job::new(song("abc", SourceKind::YouTubeMusic), src, Priority::Now));
        assert_eq!(q.len(), 2, "not a promotion of the Navidrome job");
    }

    #[tokio::test]
    async fn budget_refusals_are_retried_later_and_now_always_resolves() {
        let src = Fake::new(SourceKind::YouTubeMusic, 1, 0);
        let mut q = JobQueue::default();
        let mut tried = HashSet::new();
        for id in ["p1", "p2"] {
            q.push(Job::new(song(id, SourceKind::YouTubeMusic), src.clone(), Priority::Pool));
        }
        // p1 spends the budget; p2 is refused and put back, not given up.
        let job = q.next(Instant::now(), ALL, |_| false).unwrap();
        let (job, input) = resolve(job, &mut q, &mut tried).await.expect("budget left");
        assert_eq!(input.input, AudioInput::File("/cache/p1.webm".into()));
        assert!(tried.contains(&record_key(&job.song)));
        let job = q.next(Instant::now(), ALL, |_| false).unwrap();
        assert!(resolve(job, &mut q, &mut tried).await.is_none());
        assert!(!tried.contains("ytm:p2"));
        assert_eq!(q.len(), 1);
        assert!(q.next(Instant::now(), ALL, |_| false).is_none(), "not before the retry time");
        let later = Instant::now() + BUDGET_RETRY + Duration::from_secs(1);
        let again = q.next(later, ALL, |_| false).expect("due again");
        assert_eq!(again.song.id, "p2");
        // Wanted now (it got queued): urgent requests ignore the budget.
        q.push(Job::new(song("p2", SourceKind::YouTubeMusic), src.clone(), Priority::Now));
        let job = q.next(later, ALL, |_| false).unwrap();
        assert_eq!(job.priority, Priority::Now);
        assert!(resolve(job, &mut q, &mut tried).await.is_some());
    }

    #[tokio::test]
    async fn a_deferred_pool_job_is_promoted_when_queued() {
        let src = Fake::new(SourceKind::YouTubeMusic, 0, 0);
        let mut q = JobQueue::default();
        let mut tried = HashSet::new();
        q.push(Job::new(song("p", SourceKind::YouTubeMusic), src.clone(), Priority::Pool));
        let job = q.next(Instant::now(), ALL, |_| false).unwrap();
        assert!(resolve(job, &mut q, &mut tried).await.is_none());
        q.push(Job::new(song("p", SourceKind::YouTubeMusic), src, Priority::Queue));
        let job = q.next(Instant::now(), ALL, |_| false).expect("promoted out of the deferred list");
        assert_eq!((job.priority, q.len()), (Priority::Queue, 0));
    }

    #[tokio::test]
    async fn failed_resolves_are_retried_then_given_up() {
        let src = Fake::new(SourceKind::YouTubeMusic, 0, 10);
        let mut q = JobQueue::default();
        let mut tried = HashSet::new();
        q.push(Job::new(song("x", SourceKind::YouTubeMusic), src.clone(), Priority::Now));
        let far = Instant::now() + Duration::from_secs(3600);
        while let Some(job) = q.next(far, ALL, |s| tried.contains(&record_key(s))) {
            assert!(resolve(job, &mut q, &mut tried).await.is_none());
        }
        assert_eq!(src.calls.load(Ordering::SeqCst), 1 + usize::from(RESOLVE_RETRIES));
        assert!(tried.contains("ytm:x"));
    }

    /// A record for `song` as the worker writes it.
    fn record(song: &Song, file: FileId) -> Arc<TrackAnalysis> {
        let features = listen_core::analysis::Features {
            audio_start: 0.0,
            audio_end: 200.0,
            grid: None,
            key: None,
            lufs: -9.0,
            lra: 5.0,
            bars: Default::default(),
            phrase_bars: 8,
            phrase_offset: 0,
            sections: vec![],
            intro_instrumental_bars: 0,
            outro_instrumental_bars: 0,
            timbre: vec![],
            mood: listen_core::analysis::Mood { energy: 0.5, danceability: 0.5, brightness: 0.5 },
        };
        Arc::new(listen_analysis::record(song, file, "", "test", Ok(features)).unwrap())
    }

    #[test]
    fn youtube_music_lookups_by_namespaced_videoid() {
        let handle = AnalysisHandle::default();
        let yt = song("lYBUbBu4W08", SourceKind::YouTubeMusic);
        let file = FileId { duration_ms: 213_581, suffix: None, format: Some("251".into()), bytes: Some(9) };
        handle.insert([record(&yt, file.clone())]);
        // Listed without a duration: the videoId decides.
        let found = handle.lookup(&yt).expect("found by videoId");
        assert_eq!((found.song_id.as_str(), &found.file), ("ytm:lYBUbBu4W08", &file));
        // With the listing's rounded duration too.
        let listed = Song { duration: Duration::from_secs(214), ..yt.clone() };
        assert!(handle.lookup(&listed).is_some());
        // The same id on Navidrome is another song.
        assert!(handle.lookup(&song("lYBUbBu4W08", SourceKind::Subsonic)).is_none());
        // Navidrome records keep their bare id.
        let nd = song("nd1", SourceKind::Subsonic);
        handle.insert([record(&nd, FileId::of(&nd))]);
        assert!(handle.lookup(&nd).is_some_and(|r| r.song_id == "nd1"));
        assert!(handle.lookup(&song("nd1", SourceKind::YouTubeMusic)).is_none());
        // Sources that aren't analysed aren't looked up.
        assert!(handle.lookup(&song("nd1", SourceKind::Jellyfin)).is_none());
    }
}
