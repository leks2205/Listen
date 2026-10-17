//! Windows System Media Transport Controls through `souvlaki`, with the same interface as the Linux
//! `mpris` module ([`Message::Mpris`] in, [`MprisHandle`] out).
//! SMTC needs a window handle: a hidden window on its own message-loop thread keeps it alive in the tray.

use crate::app::Message;
use iced::Subscription;
use iced::futures::{SinkExt, Stream, StreamExt, channel::mpsc};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use std::time::Duration;
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

pub use crate::media_types::*;

/// Seconds the fast-forward and rewind buttons jump.
const SEEK_STEP: f64 = 10.0;

/// Maps what SMTC reports to the app's commands.
pub fn command(event: MediaControlEvent) -> Option<MprisCmd> {
    let sign = |d: SeekDirection| if d == SeekDirection::Forward { 1.0 } else { -1.0 };
    Some(match event {
        MediaControlEvent::Play => MprisCmd::Play,
        MediaControlEvent::Pause => MprisCmd::Pause,
        MediaControlEvent::Toggle => MprisCmd::PlayPause,
        MediaControlEvent::Next => MprisCmd::Next,
        MediaControlEvent::Previous => MprisCmd::Previous,
        MediaControlEvent::Stop => MprisCmd::Stop,
        MediaControlEvent::Seek(d) => MprisCmd::SeekBy(sign(d) * SEEK_STEP),
        MediaControlEvent::SeekBy(d, by) => MprisCmd::SeekBy(sign(d) * by.as_secs_f64()),
        MediaControlEvent::SetPosition(MediaPosition(p)) => MprisCmd::SetPosition(p.as_secs_f64()),
        MediaControlEvent::SetVolume(v) => MprisCmd::Volume(v),
        MediaControlEvent::Raise => MprisCmd::Raise,
        MediaControlEvent::OpenUri(_) | MediaControlEvent::Quit => return None,
    })
}

enum Update {
    State(Box<MprisState>),
    Seeked(f64),
}

/// Lets the app publish state; cheap to clone.
#[derive(Clone)]
pub struct MprisHandle {
    state: Arc<Mutex<MprisState>>,
    updates: std_mpsc::Sender<Update>,
    thread_id: u32,
}

impl std::fmt::Debug for MprisHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MprisHandle")
    }
}

impl MprisHandle {
    fn wake(&self, update: Update) {
        if self.updates.send(update).is_ok() {
            // SAFETY: plain Win32 call; a thread that is gone just fails.
            unsafe { wm::PostThreadMessageW(self.thread_id, wm::WM_APP, 0, 0) };
        }
    }

    /// Stores the new state and hands it to the SMTC thread.
    pub async fn publish(self, new: MprisState) {
        *self.state.lock().expect("smtc state") = new.clone();
        self.wake(Update::State(Box::new(new)));
    }

    /// Tells the controls the position jumped (after a seek).
    pub async fn seeked(self, position_secs: f64) {
        self.state.lock().expect("smtc state").position_secs = position_secs;
        self.wake(Update::Seeked(position_secs));
    }

    /// Updates the position without telling the controls (they extrapolate it).
    pub fn set_position(&self, position_secs: f64) {
        self.state.lock().expect("smtc state").position_secs = position_secs;
    }
}

#[derive(Debug, Clone)]
pub enum MprisMsg {
    Ready(MprisHandle),
    Command(MprisCmd),
}

fn duration(secs: f64) -> Duration {
    Duration::from_secs_f64(secs.max(0.0))
}

fn playback(state: &MprisState) -> MediaPlayback {
    let progress = Some(MediaPosition(duration(state.position_secs)));
    match state.status {
        Status::Playing => MediaPlayback::Playing { progress },
        Status::Paused => MediaPlayback::Paused { progress },
        Status::Stopped => MediaPlayback::Stopped,
    }
}

/// The metadata text SMTC shows for `track` (none: cleared).
fn texts(track: Option<&Track>) -> (String, String, String) {
    match track {
        Some(t) => (t.title.clone(), t.artists.join(", "), t.album.clone().unwrap_or_default()),
        None => Default::default(),
    }
}

fn apply(controls: &mut MediaControls, old: &MprisState, new: &MprisState, force: bool) {
    if force || old.track != new.track {
        let (title, artist, album) = texts(new.track.as_ref());
        let cover = new.track.as_ref().and_then(|t| t.art_url.as_deref());
        let length = new.track.as_ref().map(|t| duration(t.length_secs));
        let meta = |cover_url| MediaMetadata {
            title: Some(&title),
            artist: Some(&artist),
            album: Some(&album),
            cover_url,
            duration: length,
        };
        if let Err(e) = controls.set_metadata(meta(cover)) {
            // A cover that can't be loaded must not cost the text.
            tracing::debug!("smtc cover failed: {e:?}");
            if let Err(e) = controls.set_metadata(meta(None)) {
                tracing::debug!("smtc metadata failed: {e:?}");
            }
        }
    }
    if (force || old.track != new.track || old.status != new.status)
        && let Err(e) = controls.set_playback(playback(new))
    {
        tracing::debug!("smtc playback failed: {e:?}");
    }
}

/// The SMTC thread: a hidden window, the controls and a message loop.
fn run_thread(
    commands: mpsc::UnboundedSender<MprisCmd>,
    updates: std_mpsc::Receiver<Update>,
    ready: std_mpsc::Sender<Result<u32, String>>,
) {
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    let title: Vec<u16> = "Listen media controls\0".encode_utf16().collect();
    // SAFETY: plain Win32 calls with valid, NUL-terminated strings.
    let (hwnd, thread_id) = unsafe {
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let hwnd = wm::CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            wm::WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        // Creating the thread's message queue before anyone posts to it.
        let mut msg = std::mem::zeroed();
        wm::PeekMessageW(&mut msg, std::ptr::null_mut(), wm::WM_USER, wm::WM_USER, wm::PM_NOREMOVE);
        (hwnd, GetCurrentThreadId())
    };
    if hwnd.is_null() {
        let _ = ready.send(Err("can't create the media controls window".into()));
        return;
    }
    let config = PlatformConfig {
        display_name: "Listen",
        dbus_name: listen_core::config::APP_ID,
        hwnd: Some(hwnd.cast()),
    };
    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(format!("{e:?}")));
            return;
        }
    };
    let attached = controls.attach(move |event| {
        if let Some(cmd) = command(event) {
            let _ = commands.unbounded_send(cmd);
        }
    });
    if let Err(e) = attached {
        let _ = ready.send(Err(format!("{e:?}")));
        return;
    }
    let _ = ready.send(Ok(thread_id));
    let mut current = MprisState::default();
    apply(&mut controls, &current, &current.clone(), true);
    // SAFETY: the standard message loop; `msg` is only used by these calls.
    unsafe {
        let mut msg = std::mem::zeroed();
        while wm::GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() && msg.message == wm::WM_APP {
                while let Ok(update) = updates.try_recv() {
                    match update {
                        Update::State(new) => {
                            apply(&mut controls, &current, &new, false);
                            current = *new;
                        }
                        Update::Seeked(secs) => {
                            current.position_secs = secs;
                            let _ = controls.set_playback(playback(&current));
                        }
                    }
                }
            } else {
                wm::TranslateMessage(&msg);
                wm::DispatchMessageW(&msg);
            }
        }
    }
}

pub fn subscription() -> Subscription<Message> {
    Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Message> {
    iced::stream::channel(32, async move |mut out: mpsc::Sender<Message>| {
        let (tx, mut rx) = mpsc::unbounded();
        let (update_tx, update_rx) = std_mpsc::channel();
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let state = Arc::new(Mutex::new(MprisState::default()));
        let spawned = std::thread::Builder::new()
            .name("smtc".into())
            .spawn(move || run_thread(tx, update_rx, ready_tx));
        let ready = match spawned {
            Ok(_) => tokio::task::spawn_blocking(move || ready_rx.recv()).await.ok().and_then(Result::ok),
            Err(e) => Some(Err(e.to_string())),
        };
        match ready {
            Some(Ok(thread_id)) => {
                let handle = MprisHandle { state, updates: update_tx, thread_id };
                let _ = out.send(Message::Mpris(MprisMsg::Ready(handle))).await;
                while let Some(cmd) = rx.next().await {
                    if out.send(Message::Mpris(MprisMsg::Command(cmd))).await.is_err() {
                        break;
                    }
                }
            }
            Some(Err(e)) => tracing::warn!("media controls (SMTC) unavailable: {e}"),
            None => tracing::warn!("media controls (SMTC) unavailable"),
        }
        std::future::pending::<()>().await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_become_commands() {
        assert!(matches!(command(MediaControlEvent::Toggle), Some(MprisCmd::PlayPause)));
        assert!(matches!(command(MediaControlEvent::Next), Some(MprisCmd::Next)));
        assert!(matches!(
            command(MediaControlEvent::Seek(SeekDirection::Backward)),
            Some(MprisCmd::SeekBy(d)) if d < 0.0
        ));
        assert!(matches!(
            command(MediaControlEvent::SetPosition(MediaPosition(Duration::from_secs(5)))),
            Some(MprisCmd::SetPosition(p)) if (p - 5.0).abs() < 1e-9
        ));
        assert!(command(MediaControlEvent::Quit).is_none());
    }
}
