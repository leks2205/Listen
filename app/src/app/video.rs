//! Music videos for YouTube Music songs: a second mpv (no audio, encode mode) writes raw RGBA frames
//! to a bounded pipe, and the frame matching the audio position is shown. The audio is the clock;
//! frames it has passed are dropped. Seeks go to the running mpv over IPC. Each frame carries its
//! song time in a marker row (two extra lines on top, cut before display) so stale frames are recognised.

mod demo;

use super::{Listen, Message};
use iced::Task;
use listen_core::model::{Song, SourceKind};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::time::{Duration, Instant};

/// Frame size and rate of the pipe (other aspect ratios get black bars).
const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FPS: f64 = 30.0;
/// Extra lines on top of the picture that carry the marker; they are cut off before the frame is shown.
const MARK_ROWS: u32 = 2;
const FRAME_BYTES: usize = (WIDTH * (HEIGHT + MARK_ROWS) * 4) as usize;
const MARK_BYTES: usize = (WIDTH * MARK_ROWS * 4) as usize;
/// Seeks within this time of each other are one: only the last is sent.
const SEEK_DEBOUNCE: Duration = Duration::from_millis(300);
/// Decoded frames waiting in the pipe's queue (mpv blocks when it is full).
const QUEUE: usize = 6;
/// Seconds the picture may run ahead of the audio before mpv is restarted.
const MAX_AHEAD: f64 = 0.7;
/// Seconds the picture may trail the audio before mpv seeks; shorter stalls are left to catch up.
const MAX_BEHIND: f64 = 4.0;
/// Seconds behind the audio from which a frame is not shown (the last picture stays).
const LATE: f64 = 0.5;
/// After a start, seek or resume no drift correction happens for this long.
const SETTLE: Duration = Duration::from_secs(2);
/// Recovered = the picture runs at this share of the source rate within `RECOVER_DRIFT`.
const RECOVER_FPS: f64 = 0.9;
const RECOVER_DRIFT: f64 = 0.2;
/// A recovery that takes longer than this is reported even though it is still open.
const STALL: Duration = Duration::from_secs(4);
const RECOVER_WINDOW: Duration = Duration::from_millis(500);
/// Bits of the song time (in ms) in the marker row, `MARK_BLOCK` pixels wide each.
const MARK_BITS: usize = 24;
const MARK_BLOCK: usize = 16;
/// No frame at all after this long: give up.
const NO_FRAME: Duration = Duration::from_secs(25);
/// How often the alignment of the picture with an own copy of the song is worked out again.
const SHIFT_REFRESH: Duration = Duration::from_secs(3);
/// How often the `video-stats:` line is logged.
const STATS_EVERY: Duration = Duration::from_secs(5);

/// What `video-stats:` reports for the last window (test scripts parse the line).
struct Stats {
    since: Instant,
    /// Frames that reached the screen (the image handle was replaced).
    shown: u32,
    /// Frames taken from the pipe but never shown (the audio had already passed them).
    dropped: u32,
    /// Largest |video time - audio time| seen, in seconds.
    max_drift: f64,
    /// Seeks of the picture to the audio's position.
    seeks: u32,
    /// Slowest recovery after a start, seek or resume in the window, in ms.
    recover_ms: u64,
}

/// Time from a start, seek or resume until the picture runs normally again.
struct Recover {
    began: Instant,
    /// A window that is still all good: when it began and how many frames it has shown.
    probe: Option<(Instant, u32)>,
}

impl Stats {
    fn new() -> Self {
        Self { since: Instant::now(), shown: 0, dropped: 0, max_drift: 0.0, seeks: 0, recover_ms: 0 }
    }

    /// Starts the fps window over (frames and time), keeping the rest of the report.
    fn restart_window(&mut self) {
        self.since = Instant::now();
        self.shown = 0;
        self.dropped = 0;
    }

    /// The `video-stats:` line for the window so far; starts the next window.
    fn line(&mut self, drift: f64, recovering: Option<Duration>) -> String {
        // A recovery that has not finished yet counts with what it has taken so far.
        if let Some(open) = recovering {
            self.recover_ms = self.recover_ms.max(open.as_millis() as u64);
        }
        let secs = self.since.elapsed().as_secs_f64().max(0.001);
        let line = format!(
            "video-stats: fps={:.1} source_fps={FPS:.0} dropped={} drift_ms={} max_drift_ms={} seeks={} recover_ms={}",
            f64::from(self.shown) / secs,
            self.dropped,
            (drift * 1000.0).round() as i64,
            (self.max_drift * 1000.0).round() as i64,
            self.seeks,
            self.recover_ms,
        );
        *self = Self::new();
        line
    }
}

/// A running video mpv and its frames.
struct Session {
    child: Child,
    /// mpv's IPC socket: seeks go through it.
    socket: PathBuf,
    /// Frames with their song time (read from the marker row).
    frames: Receiver<(f64, Vec<u8>)>,
    song_id: String,
    url: String,
    /// The next frame, not yet due.
    pending: Option<(f64, Vec<u8>)>,
    /// Song time of the frame on screen.
    shown: Option<f64>,
    /// After a seek to this time: frames outside its neighbourhood are leftovers and dropped.
    expect: Option<f64>,
    /// Added to the marker time; set from a fresh mpv's first frame when its filter clock starts at zero.
    offset: f64,
    offset_known: bool,
    /// No drift correction before this.
    settle_until: Instant,
    spawned: Instant,
    first_frame: Option<Instant>,
    /// mpv closed the pipe (end of the video, or it died).
    ended: bool,
    /// The picture on screen belongs to the current position (drift counts only then).
    measuring: bool,
    recover: Option<Recover>,
    stats: Stats,
    /// Since when the cover has been shown while no smooth picture is available.
    cold_since: Option<Instant>,
    /// A picture has been shown; from then on a seek keeps the last picture instead of the cover.
    had_picture: bool,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

pub struct Video {
    /// The song/video switch: off by default.
    pub on: bool,
    /// The frame to show.
    pub frame: Option<iced::widget::image::Handle>,
    session: Option<Session>,
    /// The song whose video address is being looked up.
    resolving: Option<String>,
    /// The next song's video, pre-started so its first frames wait in the pipe.
    next: Option<Session>,
    resolving_next: Option<String>,
    /// The last audio position reported and when (reported about once a second).
    pos_seen: (Duration, Instant),
    /// The user sought: the picture follows to the new audio position at once.
    pub seeked: bool,
    /// Since when a seek waits to be sent to the picture (the last seek restarts the wait).
    seek_pending: Option<Instant>,
    /// Whether the audio was playing at the last tick (a resume starts a settle period).
    was_playing: bool,
    /// Picture time minus audio time: non-zero when the audio is an own copy of the song that starts
    /// at another moment than the YouTube Music file the picture belongs to.
    shift: f64,
    /// The song (and own copy) `shift` was worked out for, and when.
    shift_for: Option<((String, Option<String>), Instant)>,
}

impl Default for Video {
    fn default() -> Self {
        Self {
            on: false,
            frame: None,
            session: None,
            resolving: None,
            next: None,
            resolving_next: None,
            pos_seen: (Duration::ZERO, Instant::now()),
            seeked: false,
            seek_pending: None,
            was_playing: false,
            shift: 0.0,
            shift_for: None,
        }
    }
}

/// What the picture needs, given where it is and where the audio is.
#[derive(Debug, PartialEq)]
enum Verdict {
    Fine,
    Resync(&'static str),
}

/// `shown`/`next` are the song times of the frame on screen and the next one waiting.
fn verdict(shown: Option<f64>, next: Option<f64>, target: f64, ended: bool) -> Verdict {
    let ahead = shown.into_iter().chain(next).any(|t| t - target > MAX_AHEAD);
    if ahead {
        return Verdict::Resync("picture ahead of the audio");
    }
    if let Some(t) = shown
        && !ended
        && next.is_none()
        && target - t > MAX_BEHIND
    {
        return Verdict::Resync("picture behind the audio");
    }
    Verdict::Fine
}

fn log_path() -> Option<PathBuf> {
    let dir = listen_core::config::Paths::discover().ok()?.log_dir();
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("video-mpv.log"))
}

/// `listen-mpv-<pid>-v<n>.sock` next to the audio player's sockets (same stale-socket cleanup).
/// Windows: the named pipe `\\.\pipe\listen-mpv-<pid>-v<n>`.
fn socket_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if cfg!(windows) {
        return PathBuf::from(format!(r"\\.\pipe\listen-mpv-{}-v{n}", std::process::id()));
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join(format!("listen-mpv-{}-v{n}.sock", std::process::id()))
}

/// Sends one JSON line to mpv's IPC (Unix socket, or named pipe on Windows) and closes the connection.
fn send_ipc(socket: &std::path::Path, line: &str) -> bool {
    #[cfg(unix)]
    {
        let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket) else { return false };
        let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
        stream.write_all(line.as_bytes()).is_ok()
    }
    #[cfg(windows)]
    {
        // A named pipe opens like a file.
        let Ok(mut pipe) = std::fs::OpenOptions::new().read(true).write(true).open(socket) else {
            return false;
        };
        pipe.write_all(line.as_bytes()).is_ok()
    }
}

/// The song time (ms) of a frame, from the marker row: bit k is the brightness of block k.
fn frame_time(frame: &[u8]) -> f64 {
    let mut ms = 0u64;
    for k in 0..MARK_BITS {
        let x = k * MARK_BLOCK + MARK_BLOCK / 2;
        if (u32::from(frame[x * 4]) + u32::from(frame[x * 4 + 1]) + u32::from(frame[x * 4 + 2])) / 3 > 127 {
            ms |= 1 << k;
        }
    }
    ms as f64 / 1000.0
}

/// Starts mpv at `start` seconds. Its log stays in the app's log dir (`video-mpv.log`).
fn spawn(url: &str, start: f64, song_id: &str) -> Result<Session, String> {
    // geq writes the frame time (T, ms) as bits into the two extra top lines, before the RGBA conversion.
    let filters = format!(
        "--vf=lavfi=[fps={FPS},scale={WIDTH}:{HEIGHT}:force_original_aspect_ratio=decrease,\
         pad={WIDTH}:{HEIGHT}:(ow-iw)/2:(oh-ih)/2,pad={WIDTH}:{}:0:{MARK_ROWS},format=yuv420p,\
         geq=lum='if(lt(Y,2),255*mod(floor(floor(T*1000)/pow(2,floor(X/{MARK_BLOCK}))),2),lum(X,Y))':cb='if(lt(Y,1),128,cb(X,Y))':cr='if(lt(Y,1),128,cr(X,Y))',\
         format=rgba]",
        HEIGHT + MARK_ROWS
    );
    let socket = socket_path();
    let mut cmd = Command::new(listen_core::platform::tool("mpv"));
    listen_core::platform::no_window(&mut cmd);
    cmd.args([
        "--no-config",
        "--load-scripts=no",
        "--terminal=no",
        "--ytdl=no",
        "--no-audio",
        "--sid=no",
        "--hwdec=no",
        // Less probing: the container is known (mp4/webm from YouTube, mkv demo).
        "--demuxer-lavf-analyzeduration=0.3",
        "--demuxer-lavf-probesize=262144",
        "--vd-lavc-fast",
        // Keep played data so a short seek back is served from memory.
        "--demuxer-max-back-bytes=33554432",
        // Bounded read-ahead while a pre-started video waits in the full pipe.
        "--cache-secs=20",
        "--demuxer-max-bytes=25165824",
        "--network-timeout=10",
        "--stream-lavf-o=reconnect=1,reconnect_streamed=1,reconnect_delay_max=5",
        "--msg-level=all=v",
        // Windows has no /dev/stdout; ffmpeg's `pipe:1` is the process's stdout on both.
        if cfg!(windows) { "--o=pipe:1" } else { "--o=/dev/stdout" },
        "--of=rawvideo",
        "--ovc=rawvideo",
        "--ofopts=flush_packets=1",
    ])
    .arg(format!("--input-ipc-server={}", socket.display()))
    .args(cfg!(windows).then_some("--media-controls=no")) // only Listen's own media session
    .arg(filters)
    .arg("--hr-seek=yes")
    .arg(format!("--start={start:.2}"));
    if let Some(log) = log_path() {
        cmd.arg(format!("--log-file={}", log.display()));
    }
    let mut child = cmd
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start mpv ({e}); is the `mpv` package installed?"))?;
    let mut out = child.stdout.take().ok_or("mpv gave no output")?;
    let (tx, frames) = sync_channel(QUEUE);
    std::thread::spawn(move || {
        loop {
            let mut frame = vec![0u8; FRAME_BYTES];
            if out.read_exact(&mut frame).is_err() || tx.send((frame_time(&frame), frame)).is_err() {
                break;
            }
        }
    });
    tracing::info!("video: mpv started at {start:.2}s ({song_id})");
    Ok(Session {
        child,
        socket,
        frames,
        song_id: song_id.to_string(),
        url: url.to_string(),
        pending: None,
        shown: None,
        expect: Some(start),
        offset: 0.0,
        offset_known: false,
        settle_until: Instant::now() + SETTLE,
        spawned: Instant::now(),
        first_frame: None,
        ended: false,
        measuring: false,
        recover: Some(Recover::begin()),
        stats: Stats::new(),
        cold_since: Some(Instant::now()),
        had_picture: false,
    })
}

impl Recover {
    fn begin() -> Self {
        Self { began: Instant::now(), probe: None }
    }
}

impl Session {
    /// Takes the frames the audio has reached; returns the newest if close enough, the rest count as dropped.
    fn advance(&mut self, target: f64) -> Option<Vec<u8>> {
        let mut newest = None;
        let mut due = 0u32;
        loop {
            if self.pending.is_none() {
                match self.frames.try_recv() {
                    Ok((at, frame)) => {
                        if !self.offset_known {
                            self.offset_known = true;
                            if let Some(want) = self.expect
                                && (at - want).abs() > 3.0
                            {
                                self.offset = want - at;
                            }
                            tracing::info!("video: first marker {at:.3}s, offset {:.3}s", self.offset);
                        }
                        let at = at + self.offset;
                        if let Some(want) = self.expect {
                            // Left over from before the seek (or the start): dropped unseen.
                            if at < want - 0.5 || at > want + 3.0 {
                                continue;
                            }
                            self.expect = None;
                        }
                        self.first_frame.get_or_insert_with(Instant::now);
                        self.pending = Some((at, frame));
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.ended = true;
                        break;
                    }
                }
            }
            match self.pending.take() {
                Some((at, frame)) if at <= target => {
                    newest = Some((at, frame));
                    due += 1;
                }
                other => {
                    self.pending = other;
                    break;
                }
            }
        }
        let (at, frame) = newest?;
        if target - at > LATE {
            // Still catching up (buffering after a start or seek): the last picture stays.
            self.stats.dropped += due;
            return None;
        }
        self.stats.dropped += due.saturating_sub(1);
        self.shown = Some(at);
        self.measuring = true;
        self.had_picture = true;
        let mut frame = frame;
        frame.drain(..MARK_BYTES);
        Some(frame)
    }

    fn verdict(&self, target: f64) -> Verdict {
        verdict(self.shown, self.pending.as_ref().map(|p| p.0), target, self.ended)
    }

    /// Seeks the running mpv to `t`. False if mpv can't be reached (ended, or no socket yet).
    fn seek_to(&mut self, t: f64) -> bool {
        if self.ended || !matches!(self.child.try_wait(), Ok(None)) {
            return false;
        }
        let command = format!("{{\"command\":[\"seek\",\"{t:.3}\",\"absolute+exact\"]}}\n");
        if !send_ipc(&self.socket, &command) {
            return false;
        }
        self.pending = None;
        self.shown = None;
        self.expect = Some(t);
        self.settle();
        self.measuring = false;
        self.stats.seeks += 1;
        true
    }

    /// A start, seek or resume: no drift correction for a while, and the recovery time is measured.
    fn settle(&mut self) {
        self.settle_until = Instant::now() + SETTLE;
        self.recover = Some(Recover::begin());
    }

    /// Tracks recovery: done once the picture runs at 0.9x the source rate within 200 ms of the audio.
    fn track_recovery(&mut self, target: f64, shown_now: bool) {
        let Some(recover) = self.recover.as_mut() else { return };
        let in_step = self.measuring && self.shown.is_some_and(|t| (t - target).abs() <= RECOVER_DRIFT);
        if !in_step {
            recover.probe = None;
            return;
        }
        let (since, frames) = recover.probe.get_or_insert((Instant::now(), 0));
        *frames += u32::from(shown_now);
        let secs = since.elapsed();
        if secs >= RECOVER_WINDOW {
            if f64::from(*frames) / secs.as_secs_f64() >= RECOVER_FPS * FPS {
                let ms = since.duration_since(recover.began).as_millis() as u64;
                self.stats.recover_ms = self.stats.recover_ms.max(ms);
                // Catch-up time goes to recover_ms; the fps window starts with the steady picture.
                self.stats.restart_window();
                tracing::info!("video: recovered after {ms} ms");
                self.recover = None;
            } else {
                recover.probe = None;
            }
        }
    }
}

impl Listen {
    /// Whether the song has a music video: YouTube Music songs, and demo songs (a generated picture).
    pub fn has_video(&self, song: &Song) -> bool {
        matches!(song.source, SourceKind::YouTubeMusic) || (self.demo_audio && song.server_id.0 == "demo")
    }

    /// The audio position now in seconds, extrapolated from the last (about once a second) report.
    fn video_target(&mut self) -> f64 {
        if self.video.pos_seen.0 != self.playback.position {
            self.video.pos_seen = (self.playback.position, Instant::now());
        }
        let (pos, seen) = self.video.pos_seen;
        let audio =
            pos.as_secs_f64() + if self.playback.playing { seen.elapsed().as_secs_f64() } else { 0.0 };
        (audio + self.video.shift).max(0.0)
    }

    /// Works out how the picture lines up with the audio when that is an own copy of the song: from
    /// the two analyses where both exist (they may arrive while the song plays), else 0.
    fn video_shift_update(&mut self, song: &Song) {
        let own = self.substitutes.used(song);
        let key = (song.id.clone(), own.as_ref().map(|o| o.id.clone()));
        let due = self
            .video
            .shift_for
            .as_ref()
            .is_none_or(|(k, at)| *k != key || (own.is_some() && at.elapsed() > SHIFT_REFRESH));
        if !due {
            return;
        }
        let shift = own.as_ref().map_or(0.0, |o| self.substitute_shift(song, o));
        if (shift - self.video.shift).abs() > 0.15
            && self.video.session.as_ref().is_some_and(|s| s.song_id == song.id)
        {
            // The picture moves to the new alignment like after a user seek.
            self.video.seeked = true;
        }
        self.video.shift = shift;
        self.video.shift_for = Some((key, Instant::now()));
    }

    /// Brings the video in line with the switch, the current song and the audio. Runs on every tick.
    pub(super) fn video_sync(&mut self) -> Task<Message> {
        if self.mock_playback() {
            return Task::none();
        }
        let song = self.queue.current().map(|i| i.song.clone()).filter(|_| self.video.on);
        let Some(song) = song else {
            self.video.session = None;
            self.video.resolving = None;
            self.video.next = None;
            self.video.resolving_next = None;
            self.video.frame = None;
            return Task::none();
        };
        if self.dj_airing() {
            return Task::none();
        }
        self.video_shift_update(&song);
        let resumed = self.playback.playing && !self.video.was_playing;
        self.video.was_playing = self.playback.playing;
        if resumed {
            // Paused: restart the extrapolation clock so the pause isn't added to the target.
            self.video.pos_seen.1 = Instant::now();
        }
        let target = self.video_target();
        if self.video.session.as_ref().is_some_and(|s| s.song_id != song.id) {
            self.video.session = None;
            self.video.frame = None;
        }
        if self.video.session.is_none()
            && let Some(mut next) = self.video.next.take()
            && next.song_id == song.id
            && target <= 2.0
            && matches!(next.child.try_wait(), Ok(None))
        {
            // The pre-started video: its first frames are already waiting.
            tracing::info!("video: using the pre-started video of {}", song.id);
            next.spawned = Instant::now();
            next.cold_since = None;
            next.settle();
            self.video.resolving = None;
            self.video.session = Some(next);
        }
        let Some(session) = self.video.session.as_mut() else {
            if self.video.resolving.as_deref() == Some(song.id.as_str()) {
                return Task::none();
            }
            self.video.resolving = Some(song.id.clone());
            return self.video_resolve(song);
        };
        if resumed {
            session.settle();
        }
        let seek_tick = std::mem::take(&mut self.video.seeked);
        if seek_tick {
            self.video.seek_pending = Some(Instant::now());
        }
        let steady = session.recover.is_none();
        let shown_now = if let Some(frame) = session.advance(target) {
            self.video.frame = Some(iced::widget::image::Handle::from_rgba(WIDTH, HEIGHT, frame));
            session.stats.shown += 1;
            true
        } else {
            false
        };
        if shown_now && let Some(since) = session.cold_since.take() {
            let ms = since.elapsed().as_millis();
            if ms >= 250 {
                tracing::info!("video: cold start, cover until first frame after {ms} ms");
            }
        }
        if !shown_now
            && !session.had_picture
            && session.recover.as_ref().is_some_and(|r| r.began.elapsed() > Duration::from_millis(300))
        {
            // No smooth picture yet: the cover, never the wrong or catching-up picture.
            self.video.frame = None;
        }
        if self.playback.playing {
            session.track_recovery(target, shown_now);
        } else {
            // Paused: no frames are due, so the window would only read as a stall.
            session.stats = Stats::new();
            session.recover = None;
        }
        // Between a user seek and the first fresh frame the gap to the audio is the seek, not drift.
        let seeking = seek_tick || self.video.seek_pending.is_some() || session.expect.is_some();
        let drift =
            session.shown.filter(|_| session.measuring && !seeking).map_or(0.0, |shown| shown - target);
        session.stats.max_drift = session.stats.max_drift.max(drift.abs());
        // Not reported while catching up after a start, seek or resume (a stall past `STALL` still is).
        let catching_up = session.recover.as_ref().is_some_and(|r| r.began.elapsed() < STALL);
        if self.playback.playing && !catching_up && session.stats.since.elapsed() >= STATS_EVERY {
            let open = session.recover.as_ref().map(|r| r.began.elapsed());
            tracing::info!(target: "listen::app::video", "{}", session.stats.line(drift, open));
        }
        if session.first_frame.is_none() {
            if session.ended {
                return self.video_ended(Some("the video could not be decoded (see video-mpv.log)".into()));
            }
            if session.spawned.elapsed() > NO_FRAME {
                return self.video_ended(Some("the video did not start (see video-mpv.log)".into()));
            }
            // A seek before the first picture: restart at the target rather than open twice.
            if self.video.seek_pending.is_some_and(|at| at.elapsed() >= SEEK_DEBOUNCE) {
                self.video.seek_pending = None;
                return self.video_respawn(target, "seek before the first picture");
            }
        } else if session.expect.is_some() && session.ended {
            // The seek did not work (mpv has gone): start again at the audio.
            return self.video_respawn(target, "seek failed");
        } else if session.expect.is_some() && session.settle_until + NO_FRAME < Instant::now() {
            return self.video_ended(Some("the video did not follow a seek (see video-mpv.log)".into()));
        } else if let Some(why) = resync_reason(&mut self.video.seek_pending, session, target) {
            tracing::info!("video: seeking to {target:.2}s ({why})");
            if !session.seek_to(target) {
                return self.video_respawn(target, why);
            }
        }
        self.video_prestart(steady)
    }

    /// Looks up the video address of `song` (the answer arrives as `VideoUrl`).
    fn video_resolve(&self, song: Song) -> Task<Message> {
        if song.server_id.0 == "demo" {
            return Task::future(async move {
                let url = match tokio::task::spawn_blocking(demo::url).await {
                    Ok(url) => url,
                    Err(e) => Err(e.to_string()),
                };
                Message::VideoUrl(song.id, url)
            });
        }
        let Some(source) = self.sources.get(&song.server_id).cloned() else { return Task::none() };
        tracing::info!("video: loading the video of {} (id {})", song.title, song.id);
        Task::future(async move {
            let url = match source.video_stream(&song).await {
                Ok(Some(stream)) => Ok(stream.url),
                Ok(None) => Err("this song has no video".to_string()),
                Err(e) => Err(e.to_string()),
            };
            Message::VideoUrl(song.id, url)
        })
    }

    /// While the picture of the current song runs steadily, starts the next song's video in the background.
    fn video_prestart(&mut self, steady: bool) -> Task<Message> {
        let current = self.queue.current().map(|i| i.song.id.clone());
        let next = self
            .queue
            .peek_next()
            .map(|i| i.song.clone())
            .filter(|s| Some(&s.id) != current.as_ref() && self.has_video(s));
        let Some(song) = next else {
            self.video.next = None;
            self.video.resolving_next = None;
            return Task::none();
        };
        if self.video.next.as_ref().is_some_and(|n| n.song_id == song.id) {
            return Task::none();
        }
        self.video.next = None;
        if !steady || self.video.resolving_next.as_deref() == Some(song.id.as_str()) {
            return Task::none();
        }
        self.video.resolving_next = Some(song.id.clone());
        self.video_resolve(song)
    }

    /// Last resort: a new mpv at `target` (mpv ended, e.g. at the end of the video, or has no socket).
    fn video_respawn(&mut self, target: f64, why: &str) -> Task<Message> {
        let Some(old) = self.video.session.take() else { return Task::none() };
        tracing::info!("video: restarting at {target:.2}s ({why})");
        let (url, id) = (old.url.clone(), old.song_id.clone());
        let mut stats = Stats::new();
        stats.seeks = old.stats.seeks + 1;
        let had_picture = old.had_picture;
        drop(old);
        match spawn(&url, target, &id) {
            Ok(mut new) => {
                new.stats = stats;
                new.had_picture = had_picture;
                self.video.session = Some(new);
                Task::none()
            }
            Err(why) => self.video_ended(Some(why)),
        }
    }

    /// The video address of a song arrived: start mpv there.
    pub(super) fn video_url(&mut self, song_id: String, url: Result<String, String>) -> Task<Message> {
        if self.video.resolving_next.as_deref() == Some(song_id.as_str()) {
            self.video.resolving_next = None;
            // A failed look-up of the next video leaves the current one alone; it is retried at its turn.
            if let Ok(url) = url
                && self.video.on
                && self.queue.peek_next().is_some_and(|i| i.song.id == song_id)
                && let Ok(session) = spawn(&url, 0.0, &song_id)
            {
                tracing::info!("video: pre-starting the video of {song_id}");
                self.video.next = Some(session);
            }
            return Task::none();
        }
        if self.video.resolving.as_deref() != Some(song_id.as_str()) || self.video.session.is_some() {
            return Task::none();
        }
        self.video.resolving = None;
        let url = match url {
            Ok(url) => url,
            Err(why) => return self.video_ended(Some(why)),
        };
        if !self.video.on || self.queue.current().is_none_or(|i| i.song.id != song_id) {
            return Task::none();
        }
        let target = self.video_target();
        match spawn(&url, target, &song_id) {
            Ok(session) => {
                self.video.seeked = false;
                self.video.seek_pending = None;
                self.video.session = Some(session);
            }
            Err(why) => return self.video_ended(Some(why)),
        }
        Task::none()
    }

    pub(super) fn video_ended(&mut self, why: Option<String>) -> Task<Message> {
        self.video.on = false;
        self.video.frame = None;
        self.video.session = None;
        self.video.resolving = None;
        if let Some(why) = why {
            tracing::warn!("video: ended: {why}");
            self.toast_error(format!("No video: {why}"));
        }
        Task::none()
    }
}

/// Why the picture must be moved now: an explicit user seek, or the drift rule (never while settling).
fn resync_reason(pending: &mut Option<Instant>, session: &Session, target: f64) -> Option<&'static str> {
    if let Some(at) = *pending {
        // Seeks in quick succession: only the last one is sent.
        if at.elapsed() < SEEK_DEBOUNCE {
            return None;
        }
        *pending = None;
        return Some("seek");
    }
    if session.expect.is_some() || Instant::now() < session.settle_until {
        return None;
    }
    match session.verdict(target) {
        Verdict::Resync(why) => Some(why),
        Verdict::Fine => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_line_has_the_agreed_format() {
        let mut stats = Stats::new();
        stats.shown = 0;
        stats.dropped = 2;
        stats.max_drift = 0.0334;
        stats.seeks = 1;
        stats.recover_ms = 400;
        let line = stats.line(-0.0334, None);
        let want = "video-stats: fps=0.0 source_fps=30 dropped=2 drift_ms=-33 max_drift_ms=33 seeks=1 recover_ms=400";
        assert_eq!(line, want);
        assert_eq!((stats.dropped, stats.seeks, stats.recover_ms), (0, 0, 0));
    }

    #[test]
    fn an_open_recovery_counts_with_its_time_so_far() {
        let mut stats = Stats::new();
        let line = stats.line(0.0, Some(Duration::from_millis(2500)));
        assert!(line.ends_with("recover_ms=2500"), "{line}");
    }

    #[test]
    fn frame_time_reads_the_marker_row() {
        let mut frame = vec![0u8; FRAME_BYTES];
        // 59433 ms = 0b1110_0111_0010_1001
        for k in 0..MARK_BITS {
            if (59433u64 >> k) & 1 == 1 {
                for x in k * MARK_BLOCK..(k + 1) * MARK_BLOCK {
                    frame[x * 4..x * 4 + 3].fill(255);
                }
            }
        }
        assert!((frame_time(&frame) - 59.433).abs() < 1e-9);
    }

    #[test]
    fn in_step_is_fine() {
        assert_eq!(verdict(Some(10.0), Some(10.03), 10.1, false), Verdict::Fine);
        assert_eq!(verdict(Some(10.0), None, 10.5, false), Verdict::Fine);
        // A short stall is not worth a seek.
        assert_eq!(verdict(Some(10.0), None, 12.5, false), Verdict::Fine);
    }

    #[test]
    fn far_ahead_resyncs() {
        assert!(matches!(verdict(Some(60.0), None, 20.0, false), Verdict::Resync(_)));
        assert!(matches!(verdict(None, Some(60.0), 20.0, true), Verdict::Resync(_)));
    }

    #[test]
    fn far_behind_resyncs_only_while_running() {
        assert!(matches!(verdict(Some(10.0), None, 30.0, false), Verdict::Resync(_)));
        assert_eq!(verdict(Some(10.0), None, 30.0, true), Verdict::Fine);
        assert_eq!(verdict(Some(10.0), Some(10.03), 30.0, false), Verdict::Fine);
    }
}
