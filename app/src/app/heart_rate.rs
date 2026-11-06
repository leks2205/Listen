//! Heart-rate tempo mode: songs whose tempo fits the listener's heart rate (half and double time count).
//! The heart button (player bar, full-screen player, hotkey `toggleHeartRate`, MCP `set_heart_rate`)
//! switches it at any time; the song that plays never stops.
//!
//! On: readings from the source ([`crate::heart_rate`]) are smoothed over 30 s
//! ([`listen_mix::tempo::Tracker`]); when the target moved more than 5 BPM the upcoming *automatic* queue
//! items are re-ranked ([`listen_mix::tempo::rerank_queue`]: misfits go, the item after the current one
//! stays), and every Auto-DJ / AI DJ fill picks from songs that fit ([`listen_mix::tempo::tempo_fill`]:
//! fitting fetched songs, topped up from the analysis store, misfits only as a last resort). Songs the user
//! queued are never touched. In Off or Shuffle the mode switches the play mode to Auto-DJ.
//! Off: the following fills are ordinary again; tempo picks already queued stay. No tempo stretching.

use super::flow::FlowLookup;
use super::play_mode::PlayMode;
use super::{Listen, Message};
use crate::heart_rate::{HrEvent, SourceKind, SourceSpec};
use iced::{Subscription, Task};
use listen_core::analysis::FlowFeatures;
use listen_core::history::Picker;
use listen_core::model::{ServerId, Song, SourceKind as SongSource};
use listen_core::queue::Why;
use listen_core::scene::{Neighbours, SceneFilter, SceneIndex};
use listen_core::settings::HeartRateSensor;
use listen_core::versions::song_key;
use listen_mix::pick::TempoTarget;
use listen_mix::tempo::{TempoFill, Tracker, flow_bpm, tempo_fill};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

/// The AI DJ plans a set from this many candidates in heart-rate mode (topped up from the store).
pub const TEMPO_POOL: usize = 12;
/// History entries (newest first) offered to the store top-up.
const HISTORY_LOOK: usize = 5000;
/// Songs before and after the current one that may tell the session's scene.
const SESSION_LOOK: usize = 8;

/// What the heart button says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HrStatus {
    #[default]
    Off,
    /// On, looking for a heart-rate source.
    Searching,
    /// On, but no source was found (no sensor in range, Bluetooth off or missing).
    NotFound,
    /// On, readings coming in.
    On,
}

impl HrStatus {
    /// The status line (English; translated where shown).
    pub fn text(self) -> &'static str {
        match self {
            Self::Off => "Heart-rate tempo off",
            Self::Searching => "Looking for a heart-rate source…",
            Self::NotFound => "No heart-rate source found",
            Self::On => "Heart-rate tempo on",
        }
    }
}

#[derive(Debug, Default)]
pub struct HrState {
    pub on: bool,
    pub status: HrStatus,
    pub tracker: Tracker,
    /// The last raw reading.
    pub last_bpm: Option<u16>,
    /// The connected source's name.
    pub source_name: Option<String>,
    /// Bumped on every switch-on and on "Forget sensor": restarts the source.
    pub generation: u64,
    /// Re-ranks since the mode went on (QA, MCP).
    pub reranks: usize,
    /// What the last tempo fill took (`TempoFill::stats`).
    pub last_fill: Option<serde_json::Value>,
    started: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HeartRateMsg {
    /// The heart button.
    Toggle,
    Set(bool),
    /// From the running source.
    Source(HrEvent),
    /// Settings → Playback: "Forget sensor".
    Forget,
}

/// A song's tempo for the fit: the confident beat grid of its analysis, else its BPM tag.
pub(super) fn song_bpm(lookup: &FlowLookup, song: &Song) -> Option<f32> {
    lookup(song).and_then(|f| flow_bpm(&f)).or(song.bpm.filter(|b| *b > 0).map(|b| b as f32))
}

/// What the store top-up chooses from, gathered on the UI thread and read where the fill runs.
pub(super) struct TempoStore {
    analysis: Option<super::analysis::AnalysisHandle>,
    /// History songs of connected servers, newest first, one per song.
    known: Vec<Song>,
    ytm: Option<ServerId>,
    /// The scene of what is playing: the describe-it index, the artists heard next to each other, the
    /// current song then the ones played before it (newest first) and the queued ones.
    scenes: Arc<SceneIndex>,
    neighbours: Neighbours,
    around: Vec<Song>,
    /// Song keys and record ids of songs played (not skipped) in the history.
    heard: HashSet<String>,
}

impl TempoStore {
    /// Analysed history songs, then every other analysed YouTube Music song of the store.
    pub(super) fn candidates(&self) -> Vec<(Song, FlowFeatures)> {
        let Some(analysis) = &self.analysis else { return Vec::new() };
        let (songs, records) = listen_analysis::bridge_candidates(
            self.known.iter().cloned(),
            |s| analysis.lookup(s),
            analysis.all_records(),
            self.ytm.as_ref(),
            &HashSet::new(),
            usize::MAX,
        );
        songs
            .into_iter()
            .zip(records)
            .filter_map(|(s, r)| r.features().map(|f| (s, FlowFeatures::of(f))))
            .collect()
    }

    /// Runs a fill with the session's scene gate (`listen_mix::tempo::tempo_fill`'s `scene`).
    pub(super) fn with_scene<R>(&self, fill: impl FnOnce(Option<&SceneFilter>) -> R) -> R {
        let session = listen_analysis::tempo_session(&self.scenes, &self.neighbours, self.around.iter());
        let filter = listen_analysis::tempo_scene_filter(&self.scenes, &session, &self.heard);
        fill(Some(&filter))
    }
}

/// Logs what a tempo fill took (a warning when it had to pad with misfits) and what the scene gate did.
pub(super) fn log_fill(who: &str, fill: &TempoFill, target: &TempoTarget, want: usize) {
    if let Some(note) = fill.scene_note(target) {
        tracing::info!("{who}: {note}");
    }
    match fill.padded_note(target) {
        Some(note) => tracing::warn!("{who}: {note}"),
        None => tracing::info!(
            "{who}: heart-rate tempo {:.0}: {} songs fit ({} from the store)",
            target.bpm,
            fill.fitting,
            fill.from_store
        ),
    }
    tracing::debug!("{who}: tempo fill {}", fill.stats(target, want));
}

impl Listen {
    pub fn hr_on(&self) -> bool {
        self.hr.on
    }

    pub fn hr_status(&self) -> HrStatus {
        if self.hr.on { self.hr.status } else { HrStatus::Off }
    }

    /// The tempo the fills aim at: on, with a reading.
    pub fn hr_target(&self) -> Option<TempoTarget> {
        if self.hr.on { self.hr.tracker.target() } else { None }
    }

    /// The target BPM shown on the heart button ("142").
    pub fn hr_button_bpm(&self) -> Option<String> {
        self.hr_target().map(|t| format!("{:.0}", t.bpm))
    }

    /// The heart button's tooltip: the status, the target and the source.
    pub fn hr_tip(&self) -> String {
        let status = crate::i18n::tr(self.hr_status().text()).to_string();
        let mut out = match self.hr_target() {
            Some(t) => crate::i18n::trf("{} · target {} BPM", &[&status, &format!("{:.0}", t.bpm)]),
            None => status,
        };
        if let (true, Some(name)) = (self.hr.on, &self.hr.source_name) {
            out = format!("{out} · {name}");
        }
        out
    }

    /// The mode for MCP `get_now_playing` / `set_heart_rate`.
    pub fn hr_json(&self) -> serde_json::Value {
        serde_json::json!({
            "on": self.hr.on,
            "status": match self.hr_status() {
                HrStatus::Off => "off",
                HrStatus::Searching => "searching",
                HrStatus::NotFound => "not_found",
                HrStatus::On => "on",
            },
            "bpm": self.hr.last_bpm,
            "smoothed": self.hr.tracker.smoothed().filter(|_| self.hr.on),
            "target": self.hr_target().map(|t| t.bpm),
            "source": self.hr.source_name,
            "sensor": self.config.settings.playback.heart_rate_sensor.as_ref().map(|s| &s.name),
            "reranks": self.hr.reranks,
            "last_fill": self.hr.last_fill,
        })
    }

    /// The running source (only while the mode is on): the fake one in demo mode or with
    /// `LISTEN_HR_FAKE=bpm:secs,…`, else Bluetooth.
    pub(super) fn hr_subscription(&self) -> Option<Subscription<Message>> {
        if !self.hr.on || self.ephemeral && !self.demo {
            return None;
        }
        let script = std::env::var("LISTEN_HR_FAKE").ok().map(|s| crate::heart_rate::parse_script(&s));
        let kind = match script {
            Some(s) if !s.is_empty() => SourceKind::Fake(s),
            _ if self.demo => {
                SourceKind::Fake(crate::heart_rate::parse_script(crate::heart_rate::DEMO_SCRIPT))
            }
            _ => SourceKind::Bluetooth {
                preferred: self.config.settings.playback.heart_rate_sensor.as_ref().map(|s| s.id.clone()),
            },
        };
        Some(crate::heart_rate::subscription(SourceSpec { generation: self.hr.generation, kind }))
    }

    pub(super) fn on_heart_rate(&mut self, msg: HeartRateMsg) -> Task<Message> {
        match msg {
            HeartRateMsg::Toggle => self.on_heart_rate(HeartRateMsg::Set(!self.hr.on)),
            HeartRateMsg::Set(on) if on == self.hr.on => Task::none(),
            HeartRateMsg::Set(on) => self.hr_set(on),
            HeartRateMsg::Source(ev) => self.hr_event(ev),
            HeartRateMsg::Forget => {
                self.config.settings.playback.heart_rate_sensor = None;
                self.save_config();
                // A running Bluetooth source starts over without the remembered sensor.
                self.hr.generation += 1;
                Task::none()
            }
        }
    }

    fn hr_set(&mut self, on: bool) -> Task<Message> {
        self.hr.on = on;
        if !on {
            tracing::info!("heart rate: tempo mode off");
            self.hr.status = HrStatus::Off;
            self.hr.source_name = None;
            self.toast_info("Heart-rate tempo off: the DJs pick as usual again.".into());
            return Task::none();
        }
        tracing::info!("heart rate: tempo mode on");
        self.hr.generation += 1;
        self.hr.status = HrStatus::Searching;
        self.hr.tracker = Tracker::new();
        self.hr.started = Some(Instant::now());
        self.hr.last_bpm = None;
        self.hr.reranks = 0;
        self.toast_success("Heart-rate tempo on: songs that fit your heart rate.".into());
        // The tempo needs a DJ to pick the songs: Off and Shuffle switch to Auto-DJ, AI DJ stays.
        match self.play_mode() {
            PlayMode::Off | PlayMode::Shuffle => self.set_play_mode(PlayMode::AutoDj),
            PlayMode::AutoDj | PlayMode::AiDj => Task::none(),
        }
    }

    fn hr_event(&mut self, ev: HrEvent) -> Task<Message> {
        if !self.hr.on {
            return Task::none();
        }
        match ev {
            HrEvent::Searching => {
                if self.hr.status != HrStatus::On {
                    self.hr.status = HrStatus::Searching;
                }
                Task::none()
            }
            HrEvent::NotFound(_) | HrEvent::Lost => {
                self.hr.status = HrStatus::NotFound;
                Task::none()
            }
            HrEvent::Connected { id, name } => {
                self.hr.status = HrStatus::On;
                self.hr.source_name = Some(name.clone());
                if id != "fake" {
                    let sensor = HeartRateSensor { id, name };
                    if self.config.settings.playback.heart_rate_sensor.as_ref() != Some(&sensor) {
                        self.config.settings.playback.heart_rate_sensor = Some(sensor);
                        self.save_config();
                    }
                }
                Task::none()
            }
            HrEvent::Sample(bpm) => {
                self.hr.status = HrStatus::On;
                self.hr.last_bpm = Some(bpm);
                let t = self.hr.started.get_or_insert_with(Instant::now).elapsed().as_secs_f64();
                self.hr_sample_at(f32::from(bpm), t)
            }
        }
    }

    /// A reading of `bpm` at `t_s` seconds since the mode went on: re-ranks the queue when the target
    /// moved enough. Public for tests (scripted curves without waiting).
    pub fn hr_sample_at(&mut self, bpm: f32, t_s: f64) -> Task<Message> {
        if !self.hr.on {
            return Task::none();
        }
        self.hr.tracker.push(bpm, t_s);
        match self.hr.tracker.due() {
            Some(target) => self.hr_rerank(target),
            None => Task::none(),
        }
    }

    /// Re-ranks the upcoming automatic items for `target` (only while a DJ fills the queue) and refills.
    fn hr_rerank(&mut self, target: TempoTarget) -> Task<Message> {
        let mode = self.play_mode();
        if !matches!(mode, PlayMode::AutoDj | PlayMode::AiDj) {
            return Task::none();
        }
        self.hr.reranks += 1;
        let lookup = self.flow_lookup();
        // The next item stays: the player has it loaded or a mix armed.
        let r = listen_mix::tempo::rerank_queue(&mut self.queue, &target, true, |s| song_bpm(&lookup, s));
        tracing::info!(
            "heart rate: target {:.0} BPM, queue re-ranked ({} kept, {} taken out)",
            target.bpm,
            r.kept,
            r.removed.len()
        );
        if r.removed.is_empty() {
            return Task::none();
        }
        if mode == PlayMode::AiDj {
            let upcoming = self.queue.upcoming();
            return if upcoming <= 1 { self.dj_plan_past(upcoming) } else { Task::none() };
        }
        self.maybe_autodj()
    }

    /// What the store top-up chooses from now (history of connected servers, analysed YouTube Music songs).
    pub(super) fn hr_store(&self) -> TempoStore {
        let mut seen = HashSet::new();
        let known = self
            .history
            .entries
            .iter()
            .rev()
            .take(HISTORY_LOOK)
            .map(|e| e.to_song())
            .filter(|s| self.demo || self.sources.contains_key(&s.server_id))
            .filter(|s| seen.insert(song_key(s)))
            .collect();
        let ytm =
            self.sources.iter().find(|(_, s)| s.kind() == SongSource::YouTubeMusic).map(|(id, _)| id.clone());
        let scenes = self.analysis.as_ref().map(|a| a.scenes()).unwrap_or_default();
        let neighbours = Neighbours::from_history(&self.history.entries);
        let heard = self
            .history
            .entries
            .iter()
            .filter(|e| e.played())
            .flat_map(|e| {
                let s = e.to_song();
                [song_key(&s), listen_analysis::record_key(&s)]
            })
            .collect();
        TempoStore {
            analysis: self.analysis.clone(),
            known,
            ytm,
            scenes,
            neighbours,
            around: self.hr_around(),
            heard,
        }
    }

    /// The current song, then the songs before it in the queue (newest first), then the queued ones: what
    /// tells the session's scene.
    fn hr_around(&self) -> Vec<Song> {
        let Some(pos) = self.queue.current_position() else { return Vec::new() };
        let at = |i: usize| self.queue.item_at(i).filter(|it| it.bridge.is_none()).map(|it| it.song.clone());
        let mut out: Vec<Song> = at(pos).into_iter().collect();
        out.extend((pos.saturating_sub(SESSION_LOOK)..pos).rev().filter_map(at));
        out.extend((pos + 1..self.queue.len().min(pos + 1 + SESSION_LOOK)).filter_map(at));
        out
    }

    /// An Auto-DJ fill in heart-rate mode: the songs of `picked` that fit `target`, topped up from the
    /// store (never a song in `skip`: queued, played this session, blocked, worn out); misfits only when
    /// nothing else is left.
    pub(super) fn hr_fill(
        &mut self,
        picked: Vec<Song>,
        skip: &HashSet<String>,
        target: &TempoTarget,
        n: usize,
    ) -> Vec<Song> {
        let lookup = self.flow_lookup();
        let here = self.queue.current().and_then(|i| lookup(&i.song));
        let store = self.hr_store();
        let fill = store.with_scene(|scene| {
            tempo_fill(
                &picked,
                |s| song_bpm(&lookup, s),
                || store.candidates(),
                skip,
                target,
                n,
                here.as_ref(),
                scene,
            )
        });
        log_fill("Auto-DJ", &fill, target, n);
        self.hr.last_fill = Some(fill.stats(target, n));
        fill.songs
    }

    /// The reason for an Auto-DJ pick in heart-rate mode: "Matches your tempo" when it fits.
    pub(super) fn hr_why(&self, song: &Song, target: &TempoTarget) -> Option<Why> {
        let bpm = song_bpm(&self.flow_lookup(), song)?;
        listen_mix::tempo::fits(target, bpm).then(|| listen_mix::tempo::why(Picker::AutoDj, bpm))
    }
}
