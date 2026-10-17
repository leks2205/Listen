//! Audio analysis for DJ transitions: a song (Navidrome URL or local file) to
//! [`listen_core::analysis::Features`] (trim points, beat grid, key, loudness, curves, phrases,
//! sections, timbre, mood), stored in a [`store::ShardStore`]. Decoding runs the installed mpv
//! so beat times match playback; the CPU work is synchronous, for a low-priority thread.

pub mod beats;
pub mod decode;
pub mod key;
pub mod models;
pub mod spectral;
pub mod store;
pub mod structure;
#[cfg(test)]
mod test_util;
pub mod timbre;
pub mod waveform;
pub mod worker;

pub use decode::{Pcm, decode, mpv_version};
pub use models::BeatModel;
pub use worker::{Worker, run_worker};

use listen_core::analysis::{ANALYZER_VERSION, Features, FileId, Outcome, TrackAnalysis};
use listen_core::model::{Song, SourceKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

#[derive(Debug, Clone, thiserror::Error)]
pub enum AnalysisError {
    #[error("decoding failed: {reason}")]
    Decode { reason: String, transient: bool },
    #[error("longer than {} minutes", decode::MAX_SECONDS / 60)]
    TooLong,
    #[error("cancelled")]
    Cancelled,
    #[error("model: {0}")]
    Model(String),
}

impl AnalysisError {
    /// Worth retrying later (network trouble, timeouts).
    pub fn is_transient(&self) -> bool {
        matches!(self, AnalysisError::Decode { transient: true, .. } | AnalysisError::Model(_))
    }
}

/// Cooperative cancellation shared between the scheduler and a running
/// analysis (kills mpv, stops between stages). Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<(AtomicBool, Notify)>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.0.store(true, Ordering::SeqCst);
        self.0.1.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.0.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        loop {
            let notified = self.0.1.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// Where beats come from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum BeatSource {
    /// Beat This! (neural beats and downbeats); the own tracker if the
    /// model fails.
    BeatThis(BeatModel),
    /// Own dynamic-programming tracker (no model files).
    #[default]
    Own,
}

#[derive(Debug, Clone, Default)]
pub struct Opts {
    pub beats: BeatSource,
    /// The server's BPM tag: picks the tempo octave when it says half or
    /// double. Optional and only a prior.
    pub bpm_hint: Option<f32>,
}

/// How long each stage took (for `--validate-analysis` and the bench).
#[derive(Debug, Clone, Default)]
pub struct Timings {
    pub stages: Vec<(&'static str, Duration)>,
}

impl Timings {
    fn time<T>(&mut self, name: &'static str, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let out = f();
        self.stages.push((name, t.elapsed()));
        out
    }

    pub fn total(&self) -> Duration {
        self.stages.iter().map(|(_, d)| *d).sum()
    }
}

/// The full analysis of decoded audio. Pure CPU work, a few hundred
/// milliseconds plus the beat model; call it off the async runtime.
pub fn analyze(pcm: &Pcm, opts: &Opts) -> Features {
    analyze_timed(pcm, opts).0
}

/// [`analyze`] with per-stage timings.
pub fn analyze_timed(pcm: &Pcm, opts: &Opts) -> (Features, Timings) {
    let mut tm = Timings::default();
    let (start, end) = tm.time("trim", || structure::trim(pcm));
    let frames = tm.time("stft", || spectral::analyze(pcm));
    let hint = opts.bpm_hint.filter(|b| (40.0..=250.0).contains(b));

    let mono = pcm.mono();
    let estimate = tm.time("beats", || match &opts.beats {
        BeatSource::BeatThis(model) => model
            .track(&mono, pcm.sample_rate)
            .inspect_err(|e| tracing::warn!("Beat This! failed, using the own tracker: {e}"))
            .ok(),
        BeatSource::Own => None,
    });
    let estimate = estimate.or_else(|| tm.time("beats-own", || beats::track_own(&frames, start, end, hint)));
    let grid = tm.time("grid", || {
        let mut grid = estimate.as_ref().and_then(|e| beats::build_grid(e, &frames, start, end));
        if let Some(g) = grid.as_mut() {
            beats::refine_to_onsets(g, &mono, pcm.sample_rate);
        }
        if let (Some(g), Some(h)) = (grid.as_mut(), hint) {
            beats::apply_hint(g, h);
        }
        grid
    });

    let bounds = structure::bar_bounds(grid.as_ref(), start, end);
    let stats = tm.time("bars", || structure::bar_stats(&frames, &bounds));
    let (phrase_bars, phrase_offset) = structure::phrase_lattice(&stats.novelty);
    let sections = structure::sections(&stats, phrase_bars, phrase_offset);
    let (intro, outro) = structure::instrumental_edges(&stats.vocal);
    let key = tm.time("key", || {
        // Phrase starts only mean something on a real beat grid.
        let starts =
            if grid.is_some() { key::phrase_starts(&bounds, phrase_bars, phrase_offset) } else { vec![] };
        key::detect(&key::pitch_profile(&frames, start, end, &starts))
    });
    let (lufs, lra) = tm.time("loudness", || timbre::loudness(pcm, start, end));
    let timbre = tm.time("timbre", || {
        let s = ((start * pcm.sample_rate as f32) as usize).min(mono.len());
        let e = ((end * pcm.sample_rate as f32) as usize).clamp(s, mono.len());
        timbre::bliss_vector(&mono[s..e])
    });
    let mood = timbre::mood(lufs, lra, grid.as_ref(), &timbre);

    let features = Features {
        audio_start: start,
        audio_end: end,
        grid,
        key,
        lufs,
        lra,
        bars: structure::curves(&stats),
        phrase_bars,
        phrase_offset,
        sections,
        intro_instrumental_bars: intro,
        outro_instrumental_bars: outro,
        timbre,
        mood,
    };
    (features, tm)
}

/// Lowers the calling thread's CPU priority to nice 19 (Linux; best
/// effort). Threads it spawns (the model's worker pool) inherit it.
pub fn lower_thread_priority() {
    #[cfg(target_os = "linux")]
    // SAFETY: plain syscalls on the current thread id.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 19);
    }
}

/// Threads for the analysis' parallel work from the `threads` setting: 0 = automatic, a
/// quarter of the cores.
pub fn thread_count(setting: u32) -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    match setting {
        0 => (cores / 4).max(1),
        n => (n as usize).min(cores),
    }
}

/// Caps the beat model's thread pool (rten's process-wide pool, sized once from
/// `RTEN_NUM_THREADS`) at `threads`, unless that variable is already set.
///
/// # Safety
/// Sets an environment variable: call it before any other thread exists
/// (first thing in `main`).
pub unsafe fn limit_threads(threads: usize) {
    if std::env::var_os("RTEN_NUM_THREADS").is_none() {
        // SAFETY: single-threaded at this point (the caller's contract).
        unsafe { std::env::set_var("RTEN_NUM_THREADS", threads.max(1).to_string()) };
    }
}

/// Runs [`analyze`] on a fresh nice-19 thread (a lowered thread can't raise its priority
/// back, so pooled threads aren't used). `None` when cancelled.
pub async fn analyze_in_background(pcm: Pcm, opts: Opts, cancel: &Cancel) -> Option<Features> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("listen-analysis".into())
        .spawn(move || {
            lower_thread_priority();
            let _ = tx.send(analyze(&pcm, &opts));
        })
        .ok()?;
    tokio::select! {
        r = rx => r.ok(),
        () = cancel.cancelled() => None,
    }
}

/// Decodes `url` (or a local path) with mpv and analyses it in the background.
pub async fn analyze_url(url: &str, opts: &Opts, cancel: &Cancel) -> Result<Features, AnalysisError> {
    let pcm = decode::decode(url, cancel).await?;
    if cancel.is_cancelled() {
        return Err(AnalysisError::Cancelled);
    }
    analyze_in_background(pcm, opts.clone(), cancel).await.ok_or(AnalysisError::Cancelled)
}

/// Prefix of YouTube Music records in the store.
pub const YTM_PREFIX: &str = "ytm:";

/// Store key of a song's records: Navidrome's id as is, YouTube Music's videoId with `ytm:`
/// (both are 22-char base62 in one store). Other sources keep their bare id.
pub fn record_key(song: &Song) -> String {
    match song.source {
        SourceKind::YouTubeMusic => format!("{YTM_PREFIX}{}", song.id),
        _ => song.id.clone(),
    }
}

/// Wraps a result into a store record. `file` names the analysed audio (from
/// `MusicSource::analysis_source`, e.g. YouTube's itag and size). `None` for cancellations.
pub fn record(
    song: &Song,
    file: FileId,
    server_hint: &str,
    machine: &str,
    result: Result<Features, AnalysisError>,
) -> Option<TrackAnalysis> {
    let outcome = match result {
        Ok(f) => Outcome::Ok(Box::new(f)),
        Err(AnalysisError::Cancelled) => return None,
        Err(e) => Outcome::Failed { reason: e.to_string(), transient: e.is_transient() },
    };
    Some(TrackAnalysis {
        version: ANALYZER_VERSION,
        analyzed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        machine: machine.to_string(),
        decoder: mpv_version().to_string(),
        song_id: record_key(song),
        server_hint: server_hint.to_string(),
        file,
        title_hint: format!("{} – {}", song.artist_line(), song.title),
        outcome,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::click_track;

    #[test]
    fn click_track_end_to_end() {
        let mut pcm = click_track(120.0, 64.0, 2.0, 0);
        // Leading silence before the first click is trimmed.
        let f = analyze(&pcm, &Opts::default());
        assert!((f.audio_start - 2.0).abs() < 0.06, "{}", f.audio_start);
        let g = f.grid.as_ref().expect("grid");
        assert!((g.bpm - 120.0).abs() < 0.5);
        assert!(f.bars.energy.len() >= 28, "{}", f.bars.energy.len());
        assert_eq!(f.bars.energy.len(), f.bars.vocal.len());
        assert!(f.lufs < -5.0 && f.lufs > -40.0, "{}", f.lufs);
        // Silence → no grid, no key, still a record.
        pcm.left.iter_mut().chain(pcm.right.iter_mut()).for_each(|x| *x = 0.0);
        let f = analyze(&pcm, &Opts::default());
        assert!(f.grid.is_none() && f.key.is_none());
    }

    #[test]
    fn records_of_youtube_music_are_namespaced() {
        use listen_core::model::ServerId;
        let mut song = Song {
            mbz: Default::default(),
            extra: Default::default(),
            id: "lYBUbBu4W08".into(),
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            title: "t".into(),
            artists: vec![],
            album_artists: vec![],
            album: None,
            album_id: None,
            image: None,
            duration: Duration::ZERO,
            track_number: None,
            disc_number: None,
            year: None,
            genres: vec![],
            codec: None,
            bit_rate: None,
            sample_rate: None,
            bit_depth: None,
            bpm: None,
            favorite: false,
            rating: None,
            play_count: None,
            gain: Default::default(),
            explicit: false,
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: None,
            comment: None,
        };
        assert_eq!(record_key(&song), "lYBUbBu4W08");
        song.source = SourceKind::YouTubeMusic;
        assert_eq!(record_key(&song), "ytm:lYBUbBu4W08");
        let file = FileId { duration_ms: 213_581, suffix: None, format: Some("251".into()), bytes: Some(3) };
        let r = record(&song, file.clone(), "", "m", Err(AnalysisError::TooLong)).unwrap();
        assert_eq!((r.song_id.as_str(), &r.file), ("ytm:lYBUbBu4W08", &file));
        assert!(record(&song, file, "", "m", Err(AnalysisError::Cancelled)).is_none());
    }

    #[tokio::test]
    async fn cancel_wakes_waiters() {
        let c = Cancel::default();
        let c2 = c.clone();
        let h = tokio::spawn(async move { c2.cancelled().await });
        c.cancel();
        h.await.unwrap();
        assert!(c.is_cancelled());
    }
}
