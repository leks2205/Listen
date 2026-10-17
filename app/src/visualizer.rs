//! Visualizer subscription: captures the decks' mpv streams and emits spectrum frames of their sum
//! (~30 fps) while the visualizer is visible and music plays. Each frame redraws the whole window,
//! so unchanged frames are skipped and in the background (see [`set_background`]) only every third is sent.

use crate::app::Message;
use iced::Subscription;
use iced::futures::{SinkExt, Stream};
use listen_core::settings::{ChannelLayout, FrequencyScale, Visualizer, WeightingFilter};
use listen_visualizer::{Analyzer, Config, Frame, FreqScale, Octave, Weighting};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// PipeWire node names of the music decks (mpv's `--audio-client-name`).
/// The AI DJ voice ("Listen DJ") is deliberately not captured.
const DECKS: [&str; 2] = ["Listen", "Listen B"];

/// A deck silent this long while another still plays counts as stopped (e.g. after a crossfade).
const STALE: Duration = Duration::from_millis(250);

/// Frames a second when the settings don't say (“max FPS” 0).
const DEFAULT_FPS: u32 = 30;

/// The slowest and fastest rate the settings may ask for.
const MAX_FPS: u32 = 60;

/// Bars of the discrete-frequencies mode (halved by “low resolution”) and the line graph.
const DISCRETE_BARS: usize = 128;
const GRAPH_POINTS: usize = 160;

/// Maps audioMotion's options to the analyzer config (drawing is [`crate::ui::visualizer`]).
pub fn analyzer_config(v: &Visualizer) -> Config {
    let fps = if v.max_fps == 0 { DEFAULT_FPS } else { v.max_fps.clamp(1, MAX_FPS) };
    Config {
        fft_size: v.fft_size as usize,
        bands: match v.mode {
            0 if v.lo_res => DISCRETE_BARS / 2,
            0 => DISCRETE_BARS,
            _ => GRAPH_POINTS,
        },
        min_freq: v.min_freq,
        max_freq: v.max_freq,
        min_db: v.min_db,
        max_db: v.max_db,
        smoothing: v.smoothing,
        scale: match v.frequency_scale {
            FrequencyScale::Log => FreqScale::Log,
            FrequencyScale::Linear => FreqScale::Linear,
            FrequencyScale::Bark => FreqScale::Bark,
            FrequencyScale::Mel => FreqScale::Mel,
        },
        weighting: match v.weighting_filter {
            WeightingFilter::None => Weighting::None,
            WeightingFilter::A => Weighting::A,
            WeightingFilter::B => Weighting::B,
            WeightingFilter::C => Weighting::C,
            WeightingFilter::D => Weighting::D,
            WeightingFilter::Z => Weighting::Z,
        },
        octave: Octave::of_mode(v.mode, v.ansi_bands),
        linear_amplitude: v.linear_amplitude,
        linear_boost: v.linear_boost,
        volume: v.volume,
        dual: v.channel_layout != ChannelLayout::Single,
        peak_hold_ms: v.peak_hold_time,
        peak_fade_ms: v.peak_fade_time,
        // audioMotion: fading has no effect while the peak line is shown.
        fade_peaks: v.fade_peaks && !(v.mode == 10 && v.peak_line),
        gravity: v.gravity,
        frame_ms: 1000.0 / fps as f32,
    }
}

/// Options of the running capture; the worker picks up changes on its next tick.
static CONFIG: Mutex<Option<(u64, Config)>> = Mutex::new(None);

/// Sets the analyzer options; called on every subscription build, a no-op when unchanged.
pub fn set_config(cfg: Config) {
    let Ok(mut current) = CONFIG.lock() else { return };
    match current.as_mut() {
        Some((_, c)) if *c == cfg => {}
        Some((generation, c)) => {
            *generation += 1;
            *c = cfg;
        }
        None => *current = Some((1, cfg)),
    }
}

fn current_config() -> (u64, Config) {
    CONFIG.lock().ok().and_then(|c| c.clone()).unwrap_or((0, Config::default()))
}

/// In the background every `BACKGROUND_EVERY`-th frame is sent (~10 fps).
const BACKGROUND_EVERY: u32 = 3;

/// Bars and peaks within this fraction of full height of the last frame sent count as unchanged.
const SETTLED: f32 = 1e-3;

/// Window unfocused with only the queue panel's visualizer showing; read by the worker each tick.
static BACKGROUND: AtomicBool = AtomicBool::new(false);

/// `LISTEN_VIZ_DEBUG=1`: logs frames sent, ticks and background flag once a second, plus capture start/stop.
fn debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LISTEN_VIZ_DEBUG").is_some_and(|v| v != "0"))
}

pub fn set_background(background: bool) {
    BACKGROUND.store(background, Ordering::Relaxed);
}

/// Whether `b` would look the same on screen as `a`.
fn same(a: &Frame, b: &Frame) -> bool {
    let close =
        |x: &[f32], y: &[f32]| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| (p - q).abs() < SETTLED);
    let channel = |a: &Frame, b: &Frame| match (&a.right, &b.right) {
        (Some(x), Some(y)) => close(&x.levels, &y.levels) && close(&x.peaks, &y.peaks),
        (None, None) => true,
        _ => false,
    };
    close(&a.levels, &b.levels)
        && close(&a.peaks, &b.peaks)
        && close(&a.peak_alpha, &b.peak_alpha)
        && channel(a, b)
}

/// Captures deck A, plus deck B when DJ transitions may use it.
pub fn subscription(both_decks: bool) -> Subscription<Message> {
    Subscription::run_with(both_decks, worker)
}

struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if debug() {
            tracing::info!("viz: capture off");
        }
        self.0.abort();
    }
}

fn worker(both_decks: &bool) -> impl Stream<Item = Message> + use<> {
    const DECK_A: [&str; 1] = [DECKS[0]];
    let decks: &'static [&'static str] = if *both_decks { &DECKS } else { &DECK_A };
    iced::stream::channel(8, async move |mut out: iced::futures::channel::mpsc::Sender<Message>| {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(usize, Vec<f32>)>(64);
        let capture = tokio::spawn(async move { listen_visualizer::capture::pipewire_many(decks, tx).await });
        // Drop the recorders as soon as the subscription ends (pause included).
        let _stop = AbortOnDrop(capture.abort_handle());
        if debug() {
            tracing::info!("viz: capture on, decks={}", decks.len());
        }
        let (mut ticks, mut sent_count) = (0u32, 0u32);
        let mut report = tokio::time::interval(Duration::from_secs(1));
        let (mut generation, config) = current_config();
        let mut tick_every = config.frame_ms;
        let mut analyzer = Analyzer::new(config);
        let mut last: [Option<Instant>; DECKS.len()] = [None; DECKS.len()];
        let mut sent: Option<Frame> = None;
        let mut n: u32 = 0;
        let mut tick = tokio::time::interval(Duration::from_secs_f32(tick_every / 1000.0));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                chunk = rx.recv() => match chunk {
                    Some((deck, samples)) => {
                        analyzer.push_source(deck, &samples);
                        if let Some(t) = last.get_mut(deck) {
                            *t = Some(Instant::now());
                        }
                    }
                    None => {
                        let reason = match capture.await {
                            Ok(Err(e)) => e,
                            _ => "audio capture stopped".to_string(),
                        };
                        let _ = out.send(Message::VisualizerFailed(reason)).await;
                        break;
                    }
                },
                _ = report.tick(), if debug() => {
                    tracing::info!(
                        "viz: ticks={ticks} frames_sent={sent_count} background={} capture=on",
                        BACKGROUND.load(Ordering::Relaxed)
                    );
                    ticks = 0;
                    sent_count = 0;
                }
                _ = tick.tick() => {
                    let (g, config) = current_config();
                    if g != generation {
                        generation = g;
                        if (config.frame_ms - tick_every).abs() > 0.5 {
                            tick_every = config.frame_ms;
                            tick = tokio::time::interval(Duration::from_secs_f32(tick_every / 1000.0));
                            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        }
                        analyzer.set_config(config);
                        sent = None; // the shape changed: send the next frame even if it looks settled
                    }
                    let fresh = |t: &Option<Instant>| t.is_some_and(|t| t.elapsed() < STALE);
                    if !last.iter().any(fresh) {
                        continue; // nothing plays: nothing new to show
                    }
                    for (deck, t) in last.iter_mut().enumerate() {
                        if t.is_some() && !fresh(t) {
                            analyzer.clear_source(deck);
                            *t = None;
                        }
                    }
                    // Computed every tick, sent or not, so animation speed is independent of the send rate.
                    let frame = analyzer.frame();
                    n = n.wrapping_add(1);
                    ticks += 1;
                    if BACKGROUND.load(Ordering::Relaxed) && !n.is_multiple_of(BACKGROUND_EVERY) {
                        continue;
                    }
                    if sent.as_ref().is_some_and(|s| same(s, &frame)) {
                        continue; // e.g. silence: the screen already shows it
                    }
                    if out.send(Message::Spectrum(frame.clone())).await.is_err() {
                        break;
                    }
                    sent_count += 1;
                    sent = Some(frame);
                }
            }
        }
        std::future::pending::<()>().await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(levels: &[f32], peaks: &[f32]) -> Frame {
        Frame { levels: levels.to_vec(), peaks: peaks.to_vec(), ..Frame::default() }
    }

    #[test]
    fn frames_under_a_pixel_apart_are_the_same() {
        let a = frame(&[0.5, 0.2], &[0.6, 0.3]);
        assert!(same(&a, &frame(&[0.5004, 0.2], &[0.6, 0.3003])));
        assert!(!same(&a, &frame(&[0.51, 0.2], &[0.6, 0.3])), "a moving bar");
        assert!(!same(&a, &frame(&[0.5, 0.2], &[0.58, 0.3])), "a falling peak");
        assert!(!same(&a, &frame(&[0.5], &[0.6])), "another bar count");
    }

    #[test]
    fn modes_map_to_bands() {
        let cfg = |mode, lo_res| analyzer_config(&Visualizer { mode, lo_res, ..Visualizer::default() });
        assert_eq!(cfg(10, false).octave, None);
        assert_eq!(cfg(10, false).bands, GRAPH_POINTS);
        assert_eq!((cfg(0, false).bands, cfg(0, true).bands), (DISCRETE_BARS, DISCRETE_BARS / 2));
        assert_eq!(cfg(6, false).octave, Some(Octave { per_octave: 3, ansi: false }));
        assert_eq!(cfg(1, false).octave.map(|o| o.per_octave), Some(24));
        assert_eq!(cfg(8, false).octave.map(|o| o.per_octave), Some(1));
        let ansi = analyzer_config(&Visualizer { mode: 4, ansi_bands: true, ..Visualizer::default() });
        assert_eq!(ansi.octave, Some(Octave { per_octave: 6, ansi: true }));
    }

    #[test]
    fn options_map_to_the_analyzer() {
        let v = Visualizer {
            fft_size: 4096,
            min_freq: 50.0,
            max_freq: 9000.0,
            min_db: -90.0,
            max_db: -20.0,
            smoothing: 0.8,
            frequency_scale: FrequencyScale::Mel,
            weighting_filter: WeightingFilter::Z,
            linear_amplitude: true,
            linear_boost: 3.0,
            volume: 2.0,
            channel_layout: ChannelLayout::DualVertical,
            peak_hold_time: 200,
            peak_fade_time: 400,
            gravity: 5.0,
            max_fps: 60,
            ..Visualizer::default()
        };
        let c = analyzer_config(&v);
        assert_eq!((c.fft_size, c.min_freq, c.max_freq), (4096, 50.0, 9000.0));
        assert_eq!((c.min_db, c.max_db, c.smoothing), (-90.0, -20.0, 0.8));
        assert_eq!((c.scale, c.weighting), (FreqScale::Mel, Weighting::Z));
        assert!(c.linear_amplitude && c.dual);
        assert_eq!((c.linear_boost, c.volume, c.gravity), (3.0, 2.0, 5.0));
        assert_eq!((c.peak_hold_ms, c.peak_fade_ms), (200, 400));
        assert!((c.frame_ms - 1000.0 / 60.0).abs() < 1e-3);
        assert!(!analyzer_config(&Visualizer::default()).dual);
    }

    #[test]
    fn fps_and_peak_fading_follow_audiomotion() {
        let fps = |max_fps| analyzer_config(&Visualizer { max_fps, ..Visualizer::default() }).frame_ms;
        assert!((fps(0) - 1000.0 / 30.0).abs() < 1e-3, "default 30 fps");
        assert!((fps(10) - 100.0).abs() < 1e-3);
        assert!((fps(500) - 1000.0 / 60.0).abs() < 1e-3, "capped at 60");
        // Fading has no effect while the peak line shows (mode 10).
        let fade = |mode, peak_line| {
            analyzer_config(&Visualizer { mode, peak_line, fade_peaks: true, ..Visualizer::default() })
                .fade_peaks
        };
        assert!(!fade(10, true) && fade(10, false) && fade(4, true));
    }

    #[test]
    fn config_changes_are_noticed_once() {
        set_config(Config { smoothing: 0.11, ..Config::default() });
        let (g1, c1) = current_config();
        set_config(Config { smoothing: 0.11, ..Config::default() });
        assert_eq!(current_config().0, g1, "same options: same generation");
        set_config(Config { smoothing: 0.22, ..Config::default() });
        let (g2, c2) = current_config();
        assert!(g2 > g1 && c2.smoothing == 0.22 && c1.smoothing == 0.11);
    }
}
