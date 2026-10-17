//! MPRIS (Linux media keys, KDE/GNOME widgets): a D-Bus server inside an iced subscription.
//! Commands arrive as [`Message::Mpris`]; state goes out through [`MprisHandle`].

use crate::app::Message;
use iced::Subscription;
use iced::futures::{SinkExt, Stream, StreamExt, channel::mpsc};
use mpris_server::zbus::{Result as ZResult, fdo};
use mpris_server::{
    LoopStatus as MprisLoop, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property,
    RootInterface, Server, Time, TrackId, Volume,
};
use std::sync::{Arc, Mutex};

pub use crate::media_types::*;

struct Imp {
    state: Arc<Mutex<MprisState>>,
    commands: mpsc::UnboundedSender<MprisCmd>,
}

impl Imp {
    fn send(&self, cmd: MprisCmd) -> fdo::Result<()> {
        self.commands.unbounded_send(cmd).map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    fn with<R>(&self, f: impl FnOnce(&MprisState) -> R) -> R {
        f(&self.state.lock().expect("mpris state"))
    }
}

fn track_id(uid: u64) -> TrackId {
    TrackId::try_from(format!("/io/github/leks2205/Listen/track/{uid}")).unwrap_or(TrackId::NO_TRACK)
}

fn metadata(state: &MprisState) -> Metadata {
    let Some(t) = &state.track else { return Metadata::new() };
    let mut builder = Metadata::builder()
        .trackid(track_id(t.uid))
        .title(t.title.clone())
        .artist(t.artists.clone())
        .album_artist(t.album_artists.clone())
        .length(Time::from_micros((t.length_secs * 1e6) as i64));
    if let Some(text) = t.extra.lyrics.clone().filter(|l| !l.is_empty()) {
        builder = builder.other("xesam:asText", text);
    }
    let mut m = builder.build();
    if let Some(album) = &t.album {
        m.set_album(Some(album.clone()));
    }
    if let Some(url) = &t.art_url {
        m.set_art_url(Some(url.clone()));
    }
    if let Some(n) = t.track_number {
        m.set_track_number(Some(n));
    }
    let x = &t.extra;
    if !x.genres.is_empty() {
        m.set_genre(Some(x.genres.clone()));
    }
    m.set_disc_number(x.disc_number);
    m.set_audio_bpm(x.bpm);
    m.set_comment(x.comment.as_ref().map(|c| [c.clone()]));
    m.set_content_created(x.content_created.clone());
    m.set_last_used(x.last_used.clone());
    m.set_use_count(x.use_count);
    m.set_user_rating(x.user_rating);
    m
}

fn playback_status(s: Status) -> PlaybackStatus {
    match s {
        Status::Playing => PlaybackStatus::Playing,
        Status::Paused => PlaybackStatus::Paused,
        Status::Stopped => PlaybackStatus::Stopped,
    }
}

fn loop_status(r: RepeatMode) -> MprisLoop {
    match r {
        RepeatMode::None => MprisLoop::None,
        RepeatMode::Track => MprisLoop::Track,
        RepeatMode::Playlist => MprisLoop::Playlist,
    }
}

impl RootInterface for Imp {
    async fn raise(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Raise)
    }
    async fn quit(&self) -> fdo::Result<()> {
        Ok(())
    }
    async fn can_quit(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn set_fullscreen(&self, _: bool) -> ZResult<()> {
        Ok(())
    }
    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn can_raise(&self) -> fdo::Result<bool> {
        Ok(true)
    }
    async fn has_track_list(&self) -> fdo::Result<bool> {
        Ok(false)
    }
    async fn identity(&self) -> fdo::Result<String> {
        Ok("Listen".into())
    }
    async fn desktop_entry(&self) -> fdo::Result<String> {
        Ok(listen_core::config::APP_ID.into())
    }
    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        Ok(vec![])
    }
    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        Ok(vec![])
    }
}

impl PlayerInterface for Imp {
    async fn next(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Next)
    }
    async fn previous(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Previous)
    }
    async fn pause(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Pause)
    }
    async fn play_pause(&self) -> fdo::Result<()> {
        self.send(MprisCmd::PlayPause)
    }
    async fn stop(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Stop)
    }
    async fn play(&self) -> fdo::Result<()> {
        self.send(MprisCmd::Play)
    }
    async fn seek(&self, offset: Time) -> fdo::Result<()> {
        self.send(MprisCmd::SeekBy(offset.as_micros() as f64 / 1e6))
    }
    async fn set_position(&self, track: TrackId, position: Time) -> fdo::Result<()> {
        let current = self.with(|s| s.track.as_ref().map(|t| track_id(t.uid)));
        if current.as_ref() == Some(&track) {
            self.send(MprisCmd::SetPosition(position.as_micros() as f64 / 1e6))?;
        }
        Ok(())
    }
    async fn open_uri(&self, _: String) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("open_uri".into()))
    }
    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        Ok(self.with(|s| playback_status(s.status)))
    }
    async fn loop_status(&self) -> fdo::Result<MprisLoop> {
        Ok(self.with(|s| loop_status(s.repeat)))
    }
    async fn set_loop_status(&self, status: MprisLoop) -> ZResult<()> {
        let status = match status {
            MprisLoop::None => LoopStatus::None,
            MprisLoop::Track => LoopStatus::Track,
            MprisLoop::Playlist => LoopStatus::Playlist,
        };
        let _ = self.send(MprisCmd::Loop(status));
        Ok(())
    }
    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }
    async fn set_rate(&self, _: PlaybackRate) -> ZResult<()> {
        Ok(())
    }
    async fn shuffle(&self) -> fdo::Result<bool> {
        Ok(self.with(|s| s.shuffle))
    }
    async fn set_shuffle(&self, shuffle: bool) -> ZResult<()> {
        let _ = self.send(MprisCmd::Shuffle(shuffle));
        Ok(())
    }
    async fn metadata(&self) -> fdo::Result<Metadata> {
        Ok(self.with(metadata))
    }
    async fn volume(&self) -> fdo::Result<Volume> {
        Ok(self.with(|s| s.volume))
    }
    async fn set_volume(&self, volume: Volume) -> ZResult<()> {
        let _ = self.send(MprisCmd::Volume(volume));
        Ok(())
    }
    async fn position(&self) -> fdo::Result<Time> {
        Ok(self.with(|s| Time::from_micros((s.position_secs * 1e6) as i64)))
    }
    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }
    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }
    async fn can_go_next(&self) -> fdo::Result<bool> {
        Ok(self.with(|s| s.has_next))
    }
    async fn can_go_previous(&self) -> fdo::Result<bool> {
        Ok(self.with(|s| s.has_previous))
    }
    async fn can_play(&self) -> fdo::Result<bool> {
        // Always true: Plasma caches CanPlay, so false at startup would disable Play for the session.
        Ok(true)
    }
    async fn can_pause(&self) -> fdo::Result<bool> {
        Ok(true)
    }
    async fn can_seek(&self) -> fdo::Result<bool> {
        Ok(true)
    }
    async fn can_control(&self) -> fdo::Result<bool> {
        Ok(true)
    }
}

/// Lets the app publish state; cheap to clone.
#[derive(Clone)]
pub struct MprisHandle {
    state: Arc<Mutex<MprisState>>,
    server: Arc<Server<Imp>>,
}

impl std::fmt::Debug for MprisHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MprisHandle")
    }
}

impl MprisHandle {
    /// Stores the new state and emits `PropertiesChanged` for what differs.
    pub async fn publish(self, new: MprisState) {
        let old = std::mem::replace(&mut *self.state.lock().expect("mpris state"), new.clone());
        let mut changed = Vec::new();
        if old.status != new.status {
            changed.push(Property::PlaybackStatus(playback_status(new.status)));
        }
        if old.track != new.track {
            changed.push(Property::Metadata(metadata(&new)));
        }
        if old.shuffle != new.shuffle {
            changed.push(Property::Shuffle(new.shuffle));
        }
        if old.repeat != new.repeat {
            changed.push(Property::LoopStatus(loop_status(new.repeat)));
        }
        if (old.volume - new.volume).abs() > f64::EPSILON {
            changed.push(Property::Volume(new.volume));
        }
        if old.has_next != new.has_next {
            changed.push(Property::CanGoNext(new.has_next));
        }
        if old.has_previous != new.has_previous {
            changed.push(Property::CanGoPrevious(new.has_previous));
        }
        if !changed.is_empty()
            && let Err(e) = self.server.properties_changed(changed).await
        {
            tracing::debug!("mpris update failed: {e}");
        }
    }

    /// Tells clients the position jumped (after a seek).
    pub async fn seeked(self, position_secs: f64) {
        self.state.lock().expect("mpris state").position_secs = position_secs;
        let position = Time::from_micros((position_secs * 1e6) as i64);
        let _ = self.server.emit(mpris_server::Signal::Seeked { position }).await;
    }

    /// Updates the position without a signal (clients poll it).
    pub fn set_position(&self, position_secs: f64) {
        self.state.lock().expect("mpris state").position_secs = position_secs;
    }
}

#[derive(Debug, Clone)]
pub enum MprisMsg {
    Ready(MprisHandle),
    Command(MprisCmd),
}

pub fn subscription() -> Subscription<Message> {
    Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Message> {
    iced::stream::channel(32, async move |mut out: mpsc::Sender<Message>| {
        let (tx, mut rx) = mpsc::unbounded();
        let state = Arc::new(Mutex::new(MprisState::default()));
        let imp = Imp { state: state.clone(), commands: tx };
        let suffix = format!("Listen.instance{}", std::process::id());
        match Server::new(&suffix, imp).await {
            Ok(server) => {
                let handle = MprisHandle { state, server: Arc::new(server) };
                let _ = out.send(Message::Mpris(MprisMsg::Ready(handle))).await;
                while let Some(cmd) = rx.next().await {
                    if out.send(Message::Mpris(MprisMsg::Command(cmd))).await.is_err() {
                        break;
                    }
                }
            }
            Err(e) => tracing::warn!("MPRIS unavailable: {e}"),
        }
        std::future::pending::<()>().await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(extra: Extra) -> MprisState {
        MprisState {
            track: Some(Track {
                uid: 1,
                title: "T".into(),
                artists: vec!["A".into()],
                album: Some("Al".into()),
                album_artists: vec![],
                length_secs: 10.0,
                art_url: None,
                track_number: Some(3),
                extra,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn extra_fields_are_published() {
        let m = metadata(&track(Extra {
            genres: vec!["Pop".into(), "K-Pop".into()],
            disc_number: Some(2),
            bpm: Some(128),
            comment: Some("nice".into()),
            content_created: Some("2019-01-01T00:00:00Z".into()),
            last_used: Some("2026-10-01T12:00:00+00:00".into()),
            use_count: Some(7),
            user_rating: Some(0.8),
            lyrics: None,
        }));
        assert_eq!(m.genre(), Some(vec!["Pop".to_string(), "K-Pop".to_string()]));
        assert_eq!(m.disc_number(), Some(2));
        assert_eq!(m.audio_bpm(), Some(128));
        assert_eq!(m.comment(), Some(vec!["nice".to_string()]));
        assert_eq!(m.content_created().as_deref(), Some("2019-01-01T00:00:00Z"));
        assert_eq!(m.last_used().as_deref(), Some("2026-10-01T12:00:00+00:00"));
        assert_eq!(m.use_count(), Some(7));
        assert_eq!(m.user_rating(), Some(0.8));
        assert_eq!(m.track_number(), Some(3));
    }

    #[test]
    fn lyrics_are_published_as_text() {
        let m = metadata(&track(Extra { lyrics: Some("[00:01.00]Hi\n".into()), ..Extra::default() }));
        let text = m.get_value("xesam:asText").and_then(|v| v.downcast_ref::<&str>().ok().map(String::from));
        assert_eq!(text.as_deref(), Some("[00:01.00]Hi\n"));
        let none = metadata(&track(Extra::default()));
        assert!(none.get_value("xesam:asText").is_none());
        let empty = metadata(&track(Extra { lyrics: Some(String::new()), ..Extra::default() }));
        assert!(empty.get_value("xesam:asText").is_none());
    }

    fn imp() -> (Imp, mpsc::UnboundedReceiver<MprisCmd>) {
        let (tx, rx) = mpsc::unbounded();
        (Imp { state: Arc::new(Mutex::new(MprisState::default())), commands: tx }, rx)
    }

    #[test]
    fn can_play_is_true_even_without_a_track() {
        let (imp, _rx) = imp();
        assert!(iced::futures::executor::block_on(imp.can_play()).unwrap());
        assert!(iced::futures::executor::block_on(imp.can_pause()).unwrap());
    }

    #[test]
    fn play_and_play_pause_reach_the_app() {
        let (imp, mut rx) = imp();
        iced::futures::executor::block_on(async {
            imp.play().await.unwrap();
            imp.play_pause().await.unwrap();
        });
        assert!(matches!(rx.try_recv(), Ok(MprisCmd::Play)));
        assert!(matches!(rx.try_recv(), Ok(MprisCmd::PlayPause)));
    }

    #[test]
    fn missing_extras_are_left_out() {
        let m = metadata(&track(Extra::default()));
        assert_eq!(m.genre(), None);
        assert_eq!(m.disc_number(), None);
        assert_eq!(m.audio_bpm(), None);
        assert_eq!(m.comment(), None);
        assert_eq!(m.user_rating(), None);
        assert_eq!(m.title(), Some("T"));
    }
}
