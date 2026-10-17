//! Casting to a DLNA / UPnP renderer (Feishin's `dlna-cast-button.tsx`, `main/features/core/player/dlna`).
//! While casting mpv stays paused, transport and volume go to the renderer as SOAP actions, and
//! `GetPositionInfo` / `GetTransportInfo` are polled each second. DJ transitions and the AI DJ's
//! voice are off while casting.

use super::{Listen, Message, err};
use iced::Task;
use listen_core::model::{Song, SourceKind};
use listen_dlna::soap::TrackMeta;
use listen_dlna::{PositionInfo, Proxy, Renderer, Source, TransportState};
use listen_player::PlayerEvent;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the renderer is asked where it is.
const POLL: Duration = Duration::from_secs(1);
/// How long a search for renderers listens.
const SCAN_FOR: Duration = Duration::from_secs(3);
/// Consecutive failed polls after which the device counts as gone.
const MAX_FAILS: u8 = 5;
/// A song is not seeked into below this (the click that started it was a start).
const MIN_RESUME: Duration = Duration::from_secs(3);

/// What the cast button and its popover show.
#[derive(Default)]
pub struct DlnaState {
    pub renderers: Vec<Renderer>,
    pub scanning: bool,
    pub open: bool,
    /// The name of the device being connected to.
    pub connecting: Option<String>,
    pub cast: Option<Cast>,
    /// Where the next load starts in the song (a hand-over from local playback).
    resume: Option<Duration>,
}

/// An open connection to a renderer.
pub struct Cast {
    pub renderer: Renderer,
    proxy: Arc<Proxy>,
    /// The local volume and mute, back when casting stops.
    local_volume: (u8, bool),
    /// The renderer has the current song loaded.
    pub loaded: bool,
    loading: bool,
    /// The renderer has reported `PLAYING` for this song (so `STOPPED` means it ended).
    seen_playing: bool,
    polling: bool,
    fails: u8,
    vol_busy: bool,
    vol_pending: Option<u8>,
    loaded_at: Option<Instant>,
}

/// What connecting found out.
#[derive(Debug, Clone)]
pub struct Connected {
    renderer: Renderer,
    proxy: Arc<Proxy>,
    volume: u8,
    muted: bool,
}

/// One poll's answer.
#[derive(Debug, Clone)]
pub struct Polled {
    state: TransportState,
    info: PositionInfo,
}

#[derive(Debug, Clone)]
pub enum DlnaMsg {
    /// The cast button: open / close the popover (and search when it opens).
    Toggle,
    Close,
    /// Search the network again.
    Scan,
    Scanned(Vec<Renderer>),
    /// Cast to the renderer at this index of the list.
    Connect(usize),
    Connected(Result<Box<Connected>, String>),
    /// "This computer": stop casting, continue here.
    Disconnect,
    SetEnabled(bool),
    Loaded {
        token: u64,
        result: Result<(), String>,
    },
    Poll,
    Polled {
        token: u64,
        result: Result<Box<Polled>, String>,
    },
    /// A fire-and-forget action finished.
    Done(Result<(), String>),
    VolumeDone,
}

/// Where a song's audio comes from for the renderer.
#[derive(Debug, Clone, PartialEq)]
pub enum Delivery {
    /// A plain `http://` address the renderer can fetch itself.
    Direct(String),
    /// A local file, served by the proxy.
    File(std::path::PathBuf),
    /// An address that needs TLS or headers: the proxy fetches it.
    Remote { url: String, headers: Vec<(String, String)> },
}

/// How to hand a resolved stream to a renderer: renderers rarely do TLS and
/// cannot send headers, so only a bare `http://` URL goes out directly.
pub fn delivery(url: &str, headers: &[(String, String)]) -> Delivery {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Delivery::File(std::path::PathBuf::from(url.strip_prefix("file://").unwrap_or(url)));
    }
    if url.starts_with("http://") && headers.is_empty() {
        return Delivery::Direct(url.to_string());
    }
    Delivery::Remote { url: url.to_string(), headers: headers.to_vec() }
}

/// The audio MIME type of a song: googlevideo says it in the URL, the song
/// knows its suffix, else the file or URL ends in an extension.
pub fn mime_of(song: &Song, url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url)
        && let Some((_, mime)) = parsed.query_pairs().find(|(k, _)| k == "mime")
        && mime.starts_with("audio/")
    {
        return mime.into_owned();
    }
    let ext = song.suffix.clone().filter(|s| !s.is_empty()).or_else(|| {
        let path = url::Url::parse(url).map(|u| u.path().to_string()).unwrap_or_else(|_| url.to_string());
        path.rsplit_once('.').map(|(_, e)| e.to_string())
    });
    listen_dlna::proxy::mime_for(&format!("x.{}", ext.unwrap_or_default())).to_string()
}

/// A short file name with an extension for the proxy URL.
fn proxy_name(song: &Song, mime: &str) -> String {
    let ext = match mime {
        "audio/flac" => "flac",
        "audio/mp4" => "m4a",
        "audio/ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/wav" => "wav",
        "audio/webm" => "webm",
        _ => "mp3",
    };
    format!("{}.{ext}", song.id.chars().filter(char::is_ascii_alphanumeric).take(16).collect::<String>())
}

/// What the renderer shows about the song.
fn meta_of(song: &Song) -> TrackMeta {
    TrackMeta {
        title: song.title.clone(),
        artist: song.artist_line(),
        album: song.album.clone().unwrap_or_default(),
        art_url: None,
        duration: Some(song.duration).filter(|d| !d.is_zero()),
    }
}

/// Resolves the song and registers what the renderer must fetch. Returns its
/// address and MIME type.
pub async fn prepare(
    sources: &super::Sources,
    proxy: &Proxy,
    song: &Song,
) -> Result<(String, String), String> {
    // YouTube Music: the local copy (also what the DJ analyses). The resolved
    // googlevideo address expires and wants headers; the file does not.
    if let Some(source) = sources.get(&song.server_id)
        && source.kind() == SourceKind::YouTubeMusic
        && let Ok(Some(path)) = source.local_copy(song).await
    {
        let mime = mime_of(song, &path.to_string_lossy());
        let url = proxy.register(Source::File { path }, &proxy_name(song, &mime));
        return Ok((url, mime));
    }
    let track = super::stream_track(sources, song).await?;
    let mime = mime_of(song, &track.url);
    // Only the stream's own headers go on; never the account's cookies.
    let headers: Vec<(String, String)> =
        track.headers.into_iter().filter(|(k, _)| !k.eq_ignore_ascii_case("cookie")).collect();
    let url = match delivery(&track.url, &headers) {
        Delivery::Direct(url) => url,
        Delivery::File(path) => proxy.register(Source::File { path }, &proxy_name(song, &mime)),
        Delivery::Remote { url, headers } => {
            proxy.register(Source::Remote { url, headers }, &proxy_name(song, &mime))
        }
    };
    Ok((url, mime))
}

/// Loads the song on the renderer, plays it and moves to `start_at`.
async fn load(
    sources: super::Sources,
    proxy: Arc<Proxy>,
    renderer: Renderer,
    song: Song,
    start_at: Duration,
) -> Result<(), String> {
    let (url, mime) = prepare(&sources, &proxy, &song).await?;
    tracing::info!("casting “{}” to {} ({})", song.title, renderer.name, url.split('?').next().unwrap_or(""));
    renderer.set_uri(&url, &mime, &meta_of(&song)).await.map_err(err)?;
    renderer.play().await.map_err(err)?;
    if start_at >= MIN_RESUME {
        // Renderers only accept a seek once they are playing.
        for _ in 0..25 {
            match renderer.transport_state().await {
                Ok(TransportState::Playing | TransportState::Paused) => break,
                _ => tokio::time::sleep(Duration::from_millis(300)).await,
            }
        }
        renderer.seek(start_at).await.map_err(err)?;
    }
    Ok(())
}

impl Listen {
    /// Playback goes to a renderer.
    pub fn dlna_casting(&self) -> bool {
        self.dlna.cast.is_some()
    }

    /// The name of the device being cast to.
    pub fn dlna_device(&self) -> Option<&str> {
        self.dlna.cast.as_ref().map(|c| c.renderer.name.as_str())
    }

    /// The cast button is on the player bar: renderers were found (or one is in use).
    pub fn dlna_visible(&self) -> bool {
        self.dlna_casting() || (self.config.settings.playback.dlna_enabled && !self.dlna.renderers.is_empty())
    }

    /// Test hook: the renderers a search found.
    #[doc(hidden)]
    pub fn dlna_found_for_test(&mut self, renderers: Vec<Renderer>) {
        self.dlna.renderers = renderers;
    }

    /// Test hook: one poll of the current song failed.
    #[doc(hidden)]
    pub fn dlna_poll_failed_for_test(&mut self) -> Task<Message> {
        let token = self.play_token;
        self.dlna_polled(token, Err("timeout".into()))
    }

    /// The first search, a moment after start-up.
    pub(super) fn dlna_boot(&mut self) -> Task<Message> {
        if self.demo || self.ephemeral || !self.config.settings.playback.dlna_enabled {
            return Task::none();
        }
        self.dlna.scanning = true;
        Task::perform(
            async {
                tokio::time::sleep(Duration::from_secs(4)).await;
                listen_dlna::discover(SCAN_FOR).await
            },
            |r| Message::Dlna(DlnaMsg::Scanned(r)),
        )
    }

    fn dlna_scan(&mut self) -> Task<Message> {
        if self.dlna.scanning {
            return Task::none();
        }
        self.dlna.scanning = true;
        Task::perform(listen_dlna::discover(SCAN_FOR), |r| Message::Dlna(DlnaMsg::Scanned(r)))
    }

    pub(super) fn dlna_subscriptions(&self) -> Vec<iced::Subscription<Message>> {
        match &self.dlna.cast {
            Some(c) if c.loaded && !c.loading => {
                vec![iced::time::every(POLL).map(|_| Message::Dlna(DlnaMsg::Poll))]
            }
            _ => Vec::new(),
        }
    }

    fn dlna_done(result: Result<(), listen_dlna::Error>) -> Message {
        Message::Dlna(DlnaMsg::Done(result.map_err(err)))
    }

    /// Runs an action on the connected renderer.
    fn dlna_call<F, Fut>(&self, f: F) -> Task<Message>
    where
        F: FnOnce(Renderer) -> Fut,
        Fut: std::future::Future<Output = Result<(), listen_dlna::Error>> + Send + 'static,
    {
        match &self.dlna.cast {
            Some(c) => Task::perform(f(c.renderer.clone()), Self::dlna_done),
            None => Task::none(),
        }
    }

    pub(super) fn on_dlna(&mut self, msg: DlnaMsg) -> Task<Message> {
        match msg {
            DlnaMsg::Toggle => {
                self.dlna.open = !self.dlna.open;
                if self.dlna.open {
                    return self.dlna_scan();
                }
            }
            DlnaMsg::Close => self.dlna.open = false,
            DlnaMsg::Scan => return self.dlna_scan(),
            DlnaMsg::SetEnabled(on) => {
                self.config.settings.playback.dlna_enabled = on;
                self.save_config();
                if on {
                    return self.dlna_scan();
                }
                self.dlna.open = false;
                self.dlna.renderers.clear();
                if self.dlna_casting() {
                    return self.dlna_disconnect();
                }
            }
            DlnaMsg::Scanned(found) => {
                self.dlna.scanning = false;
                let casting = self.dlna.cast.as_ref().map(|c| c.renderer.clone());
                self.dlna.renderers = found;
                // The device in use stays listed even when it missed this search.
                if let Some(r) = casting
                    && !self.dlna.renderers.iter().any(|x| x.udn == r.udn)
                {
                    self.dlna.renderers.push(r);
                }
            }
            DlnaMsg::Connect(i) => {
                let Some(renderer) = self.dlna.renderers.get(i).cloned() else { return Task::none() };
                if self.dlna.connecting.is_some()
                    || self.dlna.cast.as_ref().is_some_and(|c| c.renderer == renderer)
                {
                    return Task::none();
                }
                self.dlna.connecting = Some(renderer.name.clone());
                return Task::perform(connect(renderer), |r| {
                    Message::Dlna(DlnaMsg::Connected(r.map(Box::new)))
                });
            }
            DlnaMsg::Connected(result) => return self.dlna_connected(result),
            DlnaMsg::Disconnect => {
                self.dlna.open = false;
                return self.dlna_disconnect();
            }
            DlnaMsg::Loaded { token, result } => {
                if token != self.play_token {
                    return Task::none(); // another song was started meanwhile
                }
                let Some(cast) = self.dlna.cast.as_mut() else { return Task::none() };
                cast.loading = false;
                match result {
                    Ok(()) => {
                        cast.loaded = true;
                        cast.loaded_at = Some(Instant::now());
                        cast.fails = 0;
                        self.playback.playing = true;
                    }
                    Err(e) => {
                        let name = cast.renderer.name.clone();
                        self.playback.playing = false;
                        self.toast_error(format!("{name} could not play this song: {e}"));
                    }
                }
            }
            DlnaMsg::Poll => {
                let Some(cast) = self.dlna.cast.as_mut() else { return Task::none() };
                if cast.polling || !cast.loaded || cast.loading {
                    return Task::none();
                }
                cast.polling = true;
                let (renderer, token) = (cast.renderer.clone(), self.play_token);
                return Task::perform(
                    async move {
                        let state = renderer.transport_state().await.map_err(err)?;
                        let info = renderer.position_info().await.map_err(err)?;
                        Ok(Box::new(Polled { state, info }))
                    },
                    move |result| Message::Dlna(DlnaMsg::Polled { token, result }),
                );
            }
            DlnaMsg::Polled { token, result } => return self.dlna_polled(token, result),
            DlnaMsg::Done(result) => {
                if let Err(e) = result {
                    tracing::warn!("DLNA: {e}");
                    let name = self.dlna_device().unwrap_or("The device").to_string();
                    self.toast_warning(format!("{name}: {e}"));
                }
            }
            DlnaMsg::VolumeDone => {
                // Volume changes while the last one was in flight: send the newest.
                let pending = self.dlna.cast.as_mut().and_then(|c| {
                    c.vol_busy = false;
                    c.vol_pending.take()
                });
                if let Some(v) = pending {
                    return self.dlna_send_volume(v);
                }
            }
        }
        Task::none()
    }

    fn dlna_send_volume(&mut self, volume: u8) -> Task<Message> {
        let Some(cast) = self.dlna.cast.as_mut() else { return Task::none() };
        if cast.vol_busy {
            cast.vol_pending = Some(volume);
            return Task::none();
        }
        if !cast.renderer.has_volume() {
            return Task::none();
        }
        cast.vol_busy = true;
        let renderer = cast.renderer.clone();
        Task::perform(
            async move {
                if let Err(e) = renderer.set_volume(volume).await {
                    tracing::warn!("DLNA volume: {e}");
                }
            },
            |()| Message::Dlna(DlnaMsg::VolumeDone),
        )
    }

    fn dlna_connected(&mut self, result: Result<Box<Connected>, String>) -> Task<Message> {
        self.dlna.connecting = None;
        let c = match result {
            Ok(c) => c,
            Err(e) => {
                self.toast_error(format!("Could not connect to the device: {e}"));
                return Task::none();
            }
        };
        self.dlna.open = false;
        let was_playing = self.playback.playing && !self.dlna.cast.as_ref().is_some_and(|c| !c.loaded);
        let at = self.restored.take().unwrap_or(self.playback.position);
        // Switching devices: the old one stops, the local volume stays remembered.
        let (local_volume, old) = match self.dlna.cast.take() {
            Some(old) => (old.local_volume, Some(old.renderer)),
            None => ((self.playback.volume, self.playback.muted), None),
        };
        let mut tasks = Vec::new();
        if let Some(old) = old {
            tasks.push(Task::perform(async move { old.stop().await }, Self::dlna_done));
        } else {
            // mpv rests while casting.
            tasks.push(self.mix_cancel("casting started"));
            self.dj_interrupt();
            tasks.push(self.player_task(|p| async move { p.set_paused(true).await }));
            if self.video.on {
                self.video.on = false;
                tasks.push(self.video_sync());
            }
        }
        self.playback.volume = c.volume;
        self.playback.muted = c.muted;
        self.dlna.cast = Some(Cast {
            renderer: c.renderer.clone(),
            proxy: c.proxy,
            local_volume,
            loaded: false,
            loading: false,
            seen_playing: false,
            polling: false,
            fails: 0,
            vol_busy: false,
            vol_pending: None,
            loaded_at: None,
        });
        self.toast_info(format!("Casting to {}", c.renderer.name));
        if self.queue.current().is_some() && was_playing {
            // The song goes on where it was.
            self.dlna.resume = Some(at).filter(|a| *a >= MIN_RESUME);
            tasks.push(self.start_current());
        } else {
            self.playback.playing = false;
            self.playback.position = at;
            self.dlna.resume = Some(at).filter(|a| *a >= MIN_RESUME);
        }
        Task::batch(tasks)
    }

    /// Hand the current song to the renderer (called when a song starts while casting).
    pub(super) fn dlna_load(&mut self) -> Task<Message> {
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else { return Task::none() };
        let start_at = self.dlna.resume.take().unwrap_or(Duration::ZERO);
        let Some(cast) = self.dlna.cast.as_mut() else { return Task::none() };
        cast.loaded = false;
        cast.loading = true;
        cast.seen_playing = false;
        cast.fails = 0;
        cast.polling = false;
        if start_at > Duration::ZERO {
            self.playback.position = start_at;
        }
        self.playback.playing = true;
        let token = self.play_token;
        let (sources, proxy, renderer) = (self.sources.clone(), cast.proxy.clone(), cast.renderer.clone());
        Task::perform(load(sources, proxy, renderer, song, start_at), move |result| {
            Message::Dlna(DlnaMsg::Loaded { token, result })
        })
    }

    /// Stops the renderer and gives the local volume back. No local playback follows.
    pub(super) fn dlna_release(&mut self) -> Task<Message> {
        let Some(cast) = self.dlna.cast.take() else { return Task::none() };
        self.playback.volume = cast.local_volume.0;
        self.playback.muted = cast.local_volume.1;
        self.dlna.resume = None;
        let renderer = cast.renderer;
        Task::perform(async move { renderer.stop().await }, Self::dlna_done)
    }

    /// "This computer": the song goes on here at the same position.
    fn dlna_disconnect(&mut self) -> Task<Message> {
        let position = self.playback.position;
        let was_playing = self.playback.playing && self.dlna.cast.as_ref().is_some_and(|c| c.loaded);
        let stop = self.dlna_release();
        self.toast_info("Playing on this computer again.".into());
        if self.queue.current().is_none() {
            self.playback.playing = false;
            return stop;
        }
        if was_playing {
            let start = self.start_current();
            if position >= MIN_RESUME {
                self.resume_at = Some((self.play_token, position));
            }
            return Task::batch([stop, start]);
        }
        // Paused: it waits here, and the next Play starts it at that position.
        self.playback.playing = false;
        self.playback.position = position;
        self.restored = Some(position);
        stop
    }

    fn dlna_polled(&mut self, token: u64, result: Result<Box<Polled>, String>) -> Task<Message> {
        let Some(cast) = self.dlna.cast.as_mut() else { return Task::none() };
        cast.polling = false;
        if token != self.play_token || !cast.loaded {
            return Task::none();
        }
        let polled = match result {
            Ok(p) => {
                cast.fails = 0;
                p
            }
            Err(e) => {
                cast.fails += 1;
                tracing::warn!("DLNA poll failed ({}): {e}", cast.fails);
                if cast.fails >= MAX_FAILS {
                    let name = cast.renderer.name.clone();
                    self.toast_error(format!("Lost the connection to {name}; playing on this computer."));
                    return self.dlna_disconnect();
                }
                return Task::none();
            }
        };
        let Polled { state, info } = *polled;
        let settled = cast.loaded_at.is_none_or(|t| t.elapsed() > Duration::from_secs(2));
        let ended = matches!(state, TransportState::Stopped | TransportState::NoMedia)
            && cast.seen_playing
            && settled;
        if matches!(state, TransportState::Playing | TransportState::Paused) {
            cast.seen_playing = true;
        }
        let mut tasks = Vec::new();
        if let Some(d) = info.duration {
            tasks.push(self.apply_player_event(PlayerEvent::Duration(d)));
        }
        match state {
            TransportState::Playing => {
                self.playback.playing = true;
                if let Some(pos) = info.position {
                    tasks.push(self.apply_player_event(PlayerEvent::Position(pos)));
                }
            }
            TransportState::Paused => {
                self.playback.playing = false;
                if let Some(pos) = info.position {
                    self.playback.position = pos;
                }
            }
            _ if ended => tasks.push(self.dlna_song_ended()),
            _ => {}
        }
        Task::batch(tasks)
    }

    /// The renderer finished the song: the next one, or the end of the queue.
    fn dlna_song_ended(&mut self) -> Task<Message> {
        if let Some(cast) = self.dlna.cast.as_mut() {
            cast.seen_playing = false;
            cast.loaded = false;
        }
        if self.queue.peek_next().is_none() {
            self.playback.playing = false;
            self.playback.position = Duration::ZERO;
            return Task::none();
        }
        self.queue.advance();
        let sleep = self.sleep_advanced();
        if !self.playback.playing {
            // The sleep timer ended it: the next song waits, paused.
            self.bump_token();
            self.playback.position = Duration::ZERO;
            self.playback.reset_scrobble();
            return Task::batch([sleep, self.on_track_changed()]);
        }
        Task::batch([sleep, self.start_current()])
    }

    /// Transport messages while casting. `Some` = handled here.
    pub(super) fn dlna_intercept(&mut self, message: &Message) -> Option<Task<Message>> {
        let cast = self.dlna.cast.as_ref()?;
        let (loaded, loading) = (cast.loaded, cast.loading);
        match message {
            Message::TogglePlay if self.queue.current().is_some() => {
                if loading {
                    return Some(Task::none());
                }
                if !loaded {
                    // Nothing on the device yet (connected while paused, or the song ended).
                    let at = self.restored.take().unwrap_or(self.playback.position);
                    self.dlna.resume = Some(at).filter(|a| *a >= MIN_RESUME);
                    return Some(self.start_current());
                }
                let pause = self.playback.playing;
                self.playback.playing = !pause;
                Some(
                    self.dlna_call(
                        move |r| async move { if pause { r.pause().await } else { r.play().await } },
                    ),
                )
            }
            Message::Stop => {
                self.playback.playing = false;
                self.playback.position = Duration::ZERO;
                if let Some(c) = self.dlna.cast.as_mut() {
                    c.seen_playing = false;
                }
                Some(self.dlna_call(|r| async move { r.stop().await }))
            }
            Message::Seek(secs) => {
                let pos = Duration::from_secs_f32(secs.max(0.0));
                self.playback.position = pos;
                if !loaded {
                    self.dlna.resume = Some(pos).filter(|a| *a >= MIN_RESUME);
                    return Some(Task::none());
                }
                Some(self.dlna_call(move |r| async move { r.seek(pos).await }))
            }
            Message::Volume(v) => {
                self.playback.volume = v.round().clamp(0.0, 100.0) as u8;
                if self.playback.muted {
                    self.playback.muted = false;
                    let _ = self.dlna_call(|r| async move { r.set_mute(false).await });
                }
                Some(self.dlna_send_volume(self.playback.volume))
            }
            Message::ToggleMute => {
                self.playback.muted = !self.playback.muted;
                let muted = self.playback.muted;
                Some(self.dlna_call(move |r| async move { r.set_mute(muted).await }))
            }
            _ => None,
        }
    }

    /// Stops the renderer when the queue ends (called from `stop_player`).
    pub(super) fn dlna_stop_task(&mut self) -> Option<Task<Message>> {
        self.dlna.cast.as_ref()?;
        if let Some(c) = self.dlna.cast.as_mut() {
            c.seen_playing = false;
            c.loaded = false;
        }
        Some(self.dlna_call(|r| async move { r.stop().await }))
    }
}

/// Starts the proxy on the LAN address that reaches the device, checks it answers, reads its volume.
async fn connect(renderer: Renderer) -> Result<Connected, String> {
    let ip = renderer.ip().ok_or("the device has no address")?;
    let local = listen_dlna::local_ip_for(ip).ok_or("no network route to the device")?;
    let proxy =
        Proxy::start(SocketAddr::new(local, 0)).await.map_err(|e| format!("the stream server: {e}"))?;
    renderer.transport_state().await.map_err(err)?;
    let (volume, muted) = if renderer.has_volume() {
        (renderer.volume().await.unwrap_or(50), renderer.muted().await.unwrap_or(false))
    } else {
        (50, false)
    };
    Ok(Connected { renderer, proxy: Arc::new(proxy), volume, muted })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(suffix: Option<&str>) -> Song {
        let mut s = crate::demo::album_songs(0).remove(0);
        s.suffix = suffix.map(str::to_string);
        s
    }

    #[test]
    fn only_bare_http_goes_out_directly() {
        assert_eq!(
            delivery("http://nas/stream?id=1", &[]),
            Delivery::Direct("http://nas/stream?id=1".into())
        );
        assert!(matches!(delivery("https://nas/stream?id=1", &[]), Delivery::Remote { .. }));
        let h = vec![("User-Agent".to_string(), "x".to_string())];
        assert!(matches!(delivery("http://x/a", &h), Delivery::Remote { .. }));
        assert_eq!(delivery("/home/me/a.flac", &[]), Delivery::File("/home/me/a.flac".into()));
        assert_eq!(delivery("file:///home/me/a.flac", &[]), Delivery::File("/home/me/a.flac".into()));
    }

    #[test]
    fn mime_comes_from_the_url_then_the_suffix_then_the_extension() {
        let url = "https://rr1.googlevideo.com/videoplayback?mime=audio%2Fwebm&id=1";
        assert_eq!(mime_of(&song(None), url), "audio/webm");
        assert_eq!(mime_of(&song(Some("flac")), "http://nas/stream?id=1"), "audio/flac");
        assert_eq!(mime_of(&song(None), "/music/a.m4a"), "audio/mp4");
        assert_eq!(mime_of(&song(None), "http://nas/stream?id=1"), "audio/mpeg");
    }
}
