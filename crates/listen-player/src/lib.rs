//! mpv playback, Feishin's model: the queue lives in the app and mpv holds two entries,
//! current (0) and next (1). mpv prefetches 1 and moves to it gaplessly; the app then
//! drops 0 and appends the new next ([`Player::advanced`]).

pub mod mpv;

use mpv::{Mpv, MpvEvent};
pub use mpv::{MpvError, MpvOptions};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Something the player can open.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub url: String,
    /// Extra HTTP headers (`http-header-fields`), e.g. for YouTube.
    pub headers: Vec<(String, String)>,
    /// mpv options for this file only (e.g. no EQ for the AI DJ's voice).
    pub options: Vec<(String, String)>,
}

impl Track {
    pub fn url(url: impl Into<String>) -> Self {
        Self { url: url.into(), headers: Vec::new(), options: Vec::new() }
    }

    /// A local voice clip between songs, played without ReplayGain and the EQ/filter chain.
    pub fn voice_clip(path: impl Into<String>) -> Self {
        let options = vec![("replaygain".into(), "no".into()), ("af".into(), String::new())];
        Self { url: path.into(), headers: Vec::new(), options }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    Position(Duration),
    Duration(Duration),
    Paused(bool),
    Volume(f32),
    Muted(bool),
    Codec(String),
    /// The stream's ICY title changed (`artist - title` of a radio stream);
    /// `None` when the stream has none.
    StreamTitle(Option<String>),
    /// mpv moved on to the prefetched next track by itself.
    Advanced,
    /// The last track finished; nothing left to play.
    Ended,
    /// A track failed to load or play.
    Error(String),
    /// mpv's `playback-restart`: a load or seek finished (a paused deck is ready on cue).
    Restarted,
    /// The mpv process went away.
    Crashed,
}

/// One `audio-pts` reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PtsSample {
    /// Seconds into the file at the output (what is heard, as far as mpv knows).
    pub pts: f64,
    /// Best guess of when mpv read it: the middle of the IPC round trip.
    pub at: Instant,
    /// The round trip; a large one makes `at` uncertain for phase measurement.
    pub rtt: Duration,
}

impl PtsSample {
    /// Extrapolates the position to `when`, assuming playback at `speed`.
    pub fn pts_at(&self, when: Instant, speed: f64) -> f64 {
        let dt = if when >= self.at {
            when.duration_since(self.at).as_secs_f64()
        } else {
            -self.at.duration_since(when).as_secs_f64()
        };
        self.pts + dt * speed
    }
}

/// Audio settings applied through mpv properties.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioSettings {
    /// `no`, `yes` or `weak`.
    pub gapless: String,
    /// `no`, `track` or `album`.
    pub replay_gain: String,
    pub replay_gain_preamp_db: f32,
    pub replay_gain_clip: bool,
    /// mpv `audio-device` (`auto` for the default).
    pub audio_device: String,
    /// Full `af` filter chain (EQ etc.); empty for none.
    pub audio_filters: String,
    /// mpv `audio-pitch-correction`: keep the pitch when `speed` is not 1.
    pub pitch_correction: bool,
    /// mpv `replaygain-fallback` (dB, for songs without tags).
    pub replay_gain_fallback_db: f32,
    /// mpv `audio-exclusive`.
    pub audio_exclusive: bool,
    /// mpv `audio-samplerate` in Hz; below 8000 = the device default.
    pub audio_samplerate: u32,
    /// mpv `audio-format` (`s16`, `float`, ...); empty = automatic.
    pub audio_format: String,
}

impl AudioSettings {
    /// The mpv properties these settings set, in order. Not `speed`: the DJ transitions
    /// own it (see `Player::set_speed`).
    pub fn properties(&self) -> Vec<(&'static str, Value)> {
        let samplerate = if self.audio_samplerate >= 8000 { self.audio_samplerate } else { 0 };
        let mut props = vec![
            ("gapless-audio", json!(self.gapless)),
            ("replaygain", json!(self.replay_gain)),
            ("replaygain-preamp", json!(self.replay_gain_preamp_db)),
            ("replaygain-clip", json!(self.replay_gain_clip)),
            ("replaygain-fallback", json!(self.replay_gain_fallback_db)),
            ("audio-device", json!(self.audio_device)),
            ("audio-exclusive", json!(self.audio_exclusive)),
            ("audio-samplerate", json!(samplerate)),
            ("audio-pitch-correction", json!(self.pitch_correction)),
            ("af", json!(self.audio_filters)),
        ];
        // mpv refuses resetting `audio-format` at run time, so empty is not sent;
        // "Reload mpv" clears it.
        let format = self.audio_format.trim();
        if !format.is_empty() {
            props.push(("audio-format", json!(format)));
        }
        props
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            gapless: "weak".into(),
            replay_gain: "no".into(),
            replay_gain_preamp_db: 0.0,
            replay_gain_clip: true,
            audio_device: "auto".into(),
            audio_filters: String::new(),
            pitch_correction: true,
            replay_gain_fallback_db: 0.0,
            audio_exclusive: false,
            audio_samplerate: 0,
            audio_format: String::new(),
        }
    }
}

/// Cheap-to-clone handle to the player.
#[derive(Clone)]
pub struct Player {
    mpv: Arc<Mpv>,
    /// True while mpv plays something (`idle-active == false`); tells "queue ended" from
    /// start-up idle and explicit stops.
    active: Arc<AtomicBool>,
    /// Set when a track failed: mpv's own follow-up (next entry or idle) is swallowed
    /// until the next `play`, since the app decides what plays instead.
    failed: Arc<AtomicBool>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player").finish_non_exhaustive()
    }
}

impl Player {
    pub async fn spawn(opts: &MpvOptions) -> Result<(Self, mpsc::UnboundedReceiver<PlayerEvent>), MpvError> {
        let (mpv, raw) = Mpv::spawn(opts).await?;
        let active = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(translate(raw, tx, active.clone(), failed.clone()));
        Ok((Self { mpv: Arc::new(mpv), active, failed }, rx))
    }

    /// A player without mpv (app tests): deck-gated code runs, every command fails.
    #[doc(hidden)]
    pub fn inert() -> Self {
        Self { mpv: Arc::new(Mpv::inert()), active: Arc::default(), failed: Arc::default() }
    }

    /// Replaces everything: starts `current`, prefetches `next`.
    pub async fn play(&self, current: &Track, next: Option<&Track>) -> Result<(), MpvError> {
        self.play_from(current, next, None).await
    }

    /// Like [`Player::play`], starting `current` at `start` (e.g. resuming after a retry).
    pub async fn play_from(
        &self,
        current: &Track,
        next: Option<&Track>,
        start: Option<Duration>,
    ) -> Result<(), MpvError> {
        self.failed.store(false, Ordering::SeqCst);
        let mut options = file_options(current);
        if let Some(start) = start {
            options.push(("start".into(), format!("{:.1}", start.as_secs_f64())));
        }
        self.mpv.loadfile(&current.url, false, &options).await?;
        if let Some(next) = next {
            self.mpv.loadfile(&next.url, true, &file_options(next)).await?;
        }
        self.mpv.set_property("pause", json!(false)).await
    }

    /// Replaces the prefetched next track. Removes what follows the *playing* entry, since
    /// the finished one stays at index 0 until [`Player::advanced`] drops it.
    pub async fn set_next(&self, next: Option<&Track>) -> Result<(), MpvError> {
        let pos = self.mpv.get_property("playlist-pos").await?.as_i64().unwrap_or(0).max(0) as u64;
        let count = self.mpv.get_property("playlist-count").await?.as_u64().unwrap_or(0);
        for _ in pos + 1..count {
            self.mpv.command(json!(["playlist-remove", pos + 1])).await?;
        }
        if let Some(next) = next {
            self.mpv.loadfile(&next.url, true, &file_options(next)).await?;
        }
        Ok(())
    }

    /// Call after [`PlayerEvent::Advanced`]: drops the finished entry and
    /// queues the track after the new current one.
    pub async fn advanced(&self, next: Option<&Track>) -> Result<(), MpvError> {
        let pos = self.mpv.get_property("playlist-pos").await?.as_i64().unwrap_or(0);
        for _ in 0..pos.max(0) {
            self.mpv.command(json!(["playlist-remove", 0])).await?;
        }
        self.set_next(next).await
    }

    pub async fn set_paused(&self, paused: bool) -> Result<(), MpvError> {
        self.mpv.set_property("pause", json!(paused)).await
    }

    pub async fn stop(&self) -> Result<(), MpvError> {
        self.active.store(false, Ordering::SeqCst);
        self.mpv.command(json!(["stop"])).await.map(|_| ())
    }

    pub async fn seek(&self, position: Duration) -> Result<(), MpvError> {
        self.mpv.command(json!(["seek", position.as_secs_f64(), "absolute"])).await.map(|_| ())
    }

    /// Volume in percent (0–100).
    pub async fn set_volume(&self, volume: f32) -> Result<(), MpvError> {
        self.mpv.set_property("volume", json!(volume.clamp(0.0, 100.0))).await
    }

    pub async fn set_muted(&self, muted: bool) -> Result<(), MpvError> {
        self.mpv.set_property("mute", json!(muted)).await
    }

    pub async fn apply(&self, s: &AudioSettings) -> Result<(), MpvError> {
        let mut first_error = None;
        for (name, value) in s.properties() {
            // One refused property (device gone, unsupported format) must not stop the rest.
            if let Err(e) = self.mpv.set_property(name, value).await {
                tracing::warn!("mpv rejected {name}: {e}");
                first_error.get_or_insert(e);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// `(name, description)` pairs from mpv's `audio-device-list`.
    pub async fn audio_devices(&self) -> Result<Vec<(String, String)>, MpvError> {
        let list = self.mpv.get_property("audio-device-list").await?;
        Ok(list
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|d| {
                        Some((d["name"].as_str()?.to_string(), d["description"].as_str()?.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    // ---- deck primitives (DJ mixing with two players) -----------------------------

    /// Loads `track` alone (no next slot) at `start`, sample-exact. `pause` is set before
    /// `loadfile`, so a paused deck stays silent; wait for [`PlayerEvent::Restarted`].
    pub async fn load(&self, track: &Track, start: Option<Duration>, paused: bool) -> Result<(), MpvError> {
        self.failed.store(false, Ordering::SeqCst);
        self.mpv.set_property("pause", json!(paused)).await?;
        let mut options = file_options(track);
        if let Some(start) = start {
            options.push(("start".into(), format!("{:.6}", start.as_secs_f64())));
        }
        self.mpv.loadfile(&track.url, false, &options).await
    }

    /// Playback speed (1.0 = normal). With a `scaletempo2`/`rubberband` filter in
    /// the chain (see `listen_core::settings::deck_audio_filters`) the pitch stays.
    pub async fn set_speed(&self, speed: f64) -> Result<(), MpvError> {
        self.mpv.set_property("speed", json!(speed)).await
    }

    /// Replaces the whole `af` chain.
    pub async fn set_af(&self, chain: &str) -> Result<(), MpvError> {
        self.mpv.set_property("af", json!(chain)).await
    }

    /// mpv `af-command`: `command arg` to the filter labelled `label`; in a `lavfi` graph
    /// `target` is the inner name (`bass@lo`), or a class name for all such filters.
    /// ffmpeg's biquads take `f`, `g`, `w` and `mix` live.
    pub async fn af_command(
        &self,
        label: &str,
        command: &str,
        arg: &str,
        target: &str,
    ) -> Result<(), MpvError> {
        self.mpv.command(json!(["af-command", label, command, arg, target])).await.map(|_| ())
    }

    /// The position being heard (`audio-pts`, net of output buffering), stamped with when
    /// mpv answered. `None` while nothing is loaded.
    pub async fn audio_pts(&self) -> Result<Option<PtsSample>, MpvError> {
        let sent = Instant::now();
        let reply = match self.mpv.get_property("audio-pts").await {
            Ok(v) => v,
            Err(MpvError::Command(e)) if e == "property unavailable" => return Ok(None),
            Err(e) => return Err(e),
        };
        let rtt = sent.elapsed();
        Ok(reply.as_f64().map(|pts| PtsSample { pts, at: sent + rtt / 2, rtt }))
    }

    /// Exact (sample-accurate) seek relative to the current position.
    pub async fn seek_relative_exact(&self, delta_secs: f64) -> Result<(), MpvError> {
        self.mpv.command(json!(["seek", delta_secs, "relative+exact"])).await.map(|_| ())
    }

    /// Exact seek to an absolute position.
    pub async fn seek_exact(&self, position: Duration) -> Result<(), MpvError> {
        self.mpv.command(json!(["seek", position.as_secs_f64(), "absolute+exact"])).await.map(|_| ())
    }

    /// Direct access for things not wrapped here.
    pub fn mpv(&self) -> &Mpv {
        &self.mpv
    }
}

fn file_options(track: &Track) -> Vec<(String, String)> {
    let mut options = track.options.clone();
    if track.headers.is_empty() {
        return options;
    }
    // mpv list option: entries separated by commas; escape commas in values.
    let fields = track
        .headers
        .iter()
        .map(|(k, v)| format!("{k}: {}", v.replace(',', "\\,")))
        .collect::<Vec<_>>()
        .join(",");
    options.push(("http-header-fields".into(), fields));
    options
}

/// The ICY title in mpv's `metadata` map (`icy-title`; the key's case varies
/// with the demuxer). Blank titles count as none.
pub fn icy_title(metadata: &Value) -> Option<String> {
    let map = metadata.as_object()?;
    map.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("icy-title") || k.eq_ignore_ascii_case("StreamTitle"))
        .and_then(|(_, v)| v.as_str())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Splits an ICY title (`Artist - Title`) at the first " - ".
pub fn split_icy_title(title: &str) -> (Option<&str>, &str) {
    match title.split_once(" - ") {
        Some((artist, song)) if !artist.trim().is_empty() && !song.trim().is_empty() => {
            (Some(artist.trim()), song.trim())
        }
        _ => (None, title.trim()),
    }
}

fn secs(v: &Value) -> Option<Duration> {
    v.as_f64().filter(|s| s.is_finite() && *s >= 0.0).map(Duration::from_secs_f64)
}

/// Position report interval; fades extrapolate 150 ms, the mix 250 ms past a report.
pub const POSITION_INTERVAL: Duration = Duration::from_millis(100);

/// Thins position reports to one per [`POSITION_INTERVAL`]; the first after a seek,
/// pause, restart or file change passes, as does any jump.
#[derive(Debug, Default)]
pub struct PositionThrottle {
    last: Option<(Instant, Duration)>,
}

impl PositionThrottle {
    /// Whether the report `pos` at `now` goes out.
    pub fn pass(&mut self, pos: Duration, now: Instant) -> bool {
        if let Some((at, last)) = self.last {
            let elapsed = now.saturating_duration_since(at);
            let jumped = pos < last || pos > last + elapsed + Duration::from_millis(500);
            if elapsed < POSITION_INTERVAL && !jumped {
                return false;
            }
        }
        self.last = Some((now, pos));
        true
    }

    /// The next report passes whatever its timing.
    pub fn reset(&mut self) {
        self.last = None;
    }
}

async fn translate(
    mut raw: mpsc::UnboundedReceiver<MpvEvent>,
    tx: mpsc::UnboundedSender<PlayerEvent>,
    active: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
) {
    let mut throttle = PositionThrottle::default();
    while let Some(event) = raw.recv().await {
        if let MpvEvent::EndFile { reason, error, playlist_entry_id } = &event {
            tracing::debug!("mpv end-file: reason={reason} error={error:?} entry={playlist_entry_id:?}");
        }
        // Seeks, pauses, loads and file changes reset the filter (not volume: fades).
        match &event {
            MpvEvent::Property { name, .. }
                if matches!(name.as_str(), "pause" | "duration" | "playlist-pos" | "idle-active") =>
            {
                throttle.reset()
            }
            MpvEvent::Property { .. } => {}
            _ => throttle.reset(),
        }
        let out = match event {
            MpvEvent::Property { name, value } => match name.as_str() {
                "time-pos" => {
                    secs(&value).filter(|&p| throttle.pass(p, Instant::now())).map(PlayerEvent::Position)
                }
                "duration" => secs(&value).map(PlayerEvent::Duration),
                "pause" => value.as_bool().map(PlayerEvent::Paused),
                "volume" => value.as_f64().map(|v| PlayerEvent::Volume(v as f32)),
                "mute" => value.as_bool().map(PlayerEvent::Muted),
                "audio-codec-name" => value.as_str().map(|c| PlayerEvent::Codec(c.to_string())),
                "metadata" => Some(PlayerEvent::StreamTitle(icy_title(&value))),
                // Our own `playlist-remove 0` resets this to 0, so only 1+ means mpv moved on.
                "playlist-pos" if value.as_i64().is_some_and(|p| p >= 1) => {
                    if failed.load(Ordering::SeqCst) {
                        tracing::debug!("ignoring mpv's move to the next entry after a failed track");
                        None
                    } else {
                        Some(PlayerEvent::Advanced)
                    }
                }
                "idle-active" => match value.as_bool() {
                    Some(false) => {
                        active.store(true, Ordering::SeqCst);
                        None
                    }
                    Some(true) if active.swap(false, Ordering::SeqCst) && !failed.load(Ordering::SeqCst) => {
                        Some(PlayerEvent::Ended)
                    }
                    _ => None,
                },
                _ => None,
            },
            MpvEvent::EndFile { reason, error, .. } if reason == "error" => {
                if failed.swap(true, Ordering::SeqCst) {
                    // The prefetched entry mpv jumped to failed as well; one report is enough.
                    None
                } else {
                    Some(PlayerEvent::Error(error.unwrap_or_else(|| "playback error".into())))
                }
            }
            MpvEvent::PlaybackRestart => Some(PlayerEvent::Restarted),
            MpvEvent::Shutdown => {
                let _ = tx.send(PlayerEvent::Crashed);
                break;
            }
            _ => None,
        };
        if let Some(out) = out
            && tx.send(out).is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_taken_at_different_times_extrapolate_to_the_same_position() {
        // Two decks read 3 ms apart agree once extrapolated to one instant.
        let t0 = Instant::now();
        let a = PtsSample { pts: 10.000, at: t0, rtt: Duration::from_micros(300) };
        let b = PtsSample { pts: 10.003, at: t0 + Duration::from_millis(3), rtt: Duration::from_micros(300) };
        let now = t0 + Duration::from_millis(10);
        assert!((a.pts_at(now, 1.0) - b.pts_at(now, 1.0)).abs() < 1e-9);
        // A faster deck moves further in the same time.
        assert!(a.pts_at(now, 1.1) > a.pts_at(now, 1.0));
    }

    #[test]
    fn audio_properties_are_built_from_the_settings() {
        let s = AudioSettings {
            replay_gain: "track".into(),
            replay_gain_preamp_db: -2.5,
            replay_gain_fallback_db: -6.0,
            audio_exclusive: true,
            audio_samplerate: 96_000,
            audio_format: " s32 ".into(),
            pitch_correction: false,
            audio_filters: "volume=-3dB".into(),
            ..Default::default()
        };
        let p: std::collections::HashMap<_, _> = s.properties().into_iter().collect();
        assert_eq!(p["replaygain"], json!("track"));
        assert_eq!(p["replaygain-preamp"], json!(-2.5));
        assert_eq!(p["replaygain-fallback"], json!(-6.0));
        assert_eq!(p["audio-exclusive"], json!(true));
        assert_eq!(p["audio-samplerate"], json!(96_000));
        assert_eq!(p["audio-format"], json!("s32"));
        assert_eq!(p["audio-pitch-correction"], json!(false));
        assert_eq!(p["af"], json!("volume=-3dB"));
        assert!(!p.contains_key("speed"), "the speed belongs to the DJ transitions or the player config");
    }

    #[test]
    fn unset_audio_options_mean_mpv_defaults() {
        let p: std::collections::HashMap<_, _> = AudioSettings::default().properties().into_iter().collect();
        assert_eq!(p["audio-samplerate"], json!(0));
        assert!(!p.contains_key("audio-format"), "mpv refuses to reset it");
        assert_eq!(p["audio-device"], json!("auto"));
        // Below 8 kHz is not a rate: the device default.
        let s = AudioSettings { audio_samplerate: 4000, ..Default::default() };
        assert!(s.properties().contains(&("audio-samplerate", json!(0))));
    }

    /// Real mpv (skipped when it is not installed): every property is accepted.
    #[tokio::test]
    async fn mpv_accepts_all_audio_properties() {
        let opts = MpvOptions { null_audio: true, ..Default::default() };
        let Ok((player, _events)) = Player::spawn(&opts).await else { return };
        let s = AudioSettings {
            replay_gain: "album".into(),
            audio_samplerate: 48_000,
            audio_format: "float".into(),
            audio_exclusive: false,
            pitch_correction: false,
            audio_filters: "lavfi=[acompressor=threshold=0.063096:ratio=4:attack=20:release=250:makeup=1.995262:knee=2.83]".into(),
            ..Default::default()
        };
        player.apply(&s).await.expect("mpv accepts the audio settings");
        // Back to the defaults (sample rate 0).
        let back = AudioSettings { replay_gain: "no".into(), ..Default::default() };
        player.apply(&back).await.expect("and the defaults");
        player.set_speed(1.5).await.unwrap();
        let devices = player.audio_devices().await.unwrap();
        assert!(devices.iter().any(|(n, _)| n == "auto"));
    }

    #[test]
    fn reads_the_icy_title() {
        assert_eq!(
            icy_title(&json!({"icy-name": "Jazz FM", "icy-title": " Miles Davis - So What "})),
            Some("Miles Davis - So What".into())
        );
        assert_eq!(icy_title(&json!({"ICY-TITLE": "X"})), Some("X".into()));
        assert_eq!(icy_title(&json!({"icy-title": "  "})), None);
        assert_eq!(icy_title(&json!({"title": "Song"})), None);
        assert_eq!(icy_title(&Value::Null), None);
    }

    #[test]
    fn splits_artist_and_title() {
        assert_eq!(split_icy_title("Miles Davis - So What"), (Some("Miles Davis"), "So What"));
        assert_eq!(split_icy_title("Just a title"), (None, "Just a title"));
        assert_eq!(split_icy_title("A - B - C"), (Some("A"), "B - C"));
        assert_eq!(split_icy_title(" - x"), (None, "- x"));
    }

    #[test]
    fn header_options_escape_commas() {
        let t = Track {
            url: "https://x".into(),
            headers: vec![("User-Agent".into(), "a, b".into()), ("X".into(), "1".into())],
            options: Vec::new(),
        };
        assert_eq!(
            file_options(&t),
            vec![("http-header-fields".to_string(), "User-Agent: a\\, b,X: 1".to_string())]
        );
        assert!(file_options(&Track::url("u")).is_empty());
        assert_eq!(file_options(&Track::voice_clip("/tmp/dj.wav")).len(), 2);
    }

    #[test]
    fn positions_are_thinned_out_but_jumps_and_resets_pass() {
        let ms = Duration::from_millis;
        let t0 = Instant::now();
        let mut th = PositionThrottle::default();
        assert!(th.pass(ms(1000), t0), "the first report");
        assert!(!th.pass(ms(1020), t0 + ms(20)));
        assert!(!th.pass(ms(1090), t0 + ms(90)));
        assert!(th.pass(ms(1100), t0 + ms(100)), "after the interval");
        assert!(!th.pass(ms(1150), t0 + ms(150)));
        assert!(th.pass(ms(500), t0 + ms(160)), "a seek back");
        assert!(th.pass(ms(60_000), t0 + ms(170)), "a seek forward");
        assert!(!th.pass(ms(60_010), t0 + ms(180)));
        th.reset();
        assert!(th.pass(ms(60_020), t0 + ms(190)), "after a pause / restart / new file");
    }
}
