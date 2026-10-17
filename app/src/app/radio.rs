//! Internet radio, the Related (similar songs) list and the Jukebox player.
//!
//! * Radio (Feishin `features/radio`): station CRUD and streams in mpv; the queue idles meanwhile and
//!   the ICY title is the now-playing line.
//! * Related (Feishin `similar-songs-list.tsx`): `similar_songs` of the playing song for the full-screen tab.
//! * Jukebox (Feishin `jukebox-player.tsx`): hands playback to the server's `jukeboxControl`.

use super::fullscreen_state::FullscreenMsg;
use super::{Listen, Message, err};
use iced::Task;
use listen_core::model::Song;
use listen_core::settings::AudioPlayer;
use listen_player::Track;
use listen_source::{Capabilities, InternetRadio, JukeboxCmd, JukeboxStatus};
use std::sync::Arc;
use std::time::Duration;

/// Key of the create / edit station dialog.
pub const FORM_MODAL: &str = "radio-form";
/// How many similar songs the Related list asks for.
pub const RELATED_COUNT: usize = 50;
/// Songs handed to the jukebox at a time.
const JUKEBOX_BATCH: usize = 100;
/// How often the jukebox is asked where it is.
const JUKEBOX_POLL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioSort {
    Name,
    Id,
}

impl RadioSort {
    pub const ALL: [RadioSort; 2] = [RadioSort::Name, RadioSort::Id];
}

impl std::fmt::Display for RadioSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RadioSort::Name => "Name",
            RadioSort::Id => "ID",
        })
    }
}

#[derive(Debug, Clone)]
pub enum RadioMsg {
    Refresh,
    Loaded(Result<Vec<InternetRadio>, String>),
    Search(String),
    Sort(RadioSort),
    Order,
    Play(String),
    Stop,
    OpenCreate,
    OpenEdit(String),
    FormName(String),
    FormStream(String),
    FormHome(String),
    FormSubmit,
    Saved(Result<Option<InternetRadio>, String>),
    AskDelete(String),
    Delete(String),
    Deleted(String, Result<(), String>),
    // Related
    /// Loads the Related list of the playing song now.
    RelatedRefresh,
    RelatedLoaded(String, Result<Vec<Song>, String>),
    // Jukebox
    SetAudioPlayer(AudioPlayer),
    JukeboxProbe,
    JukeboxProbed(bool),
    JukeboxPoll,
    JukeboxDone(Result<JukeboxStatus, String>),
}

/// The station being edited or created.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StationForm {
    /// `None` when creating.
    pub id: Option<String>,
    pub name: String,
    pub stream_url: String,
    pub home_page_url: String,
    pub busy: bool,
}

impl StationForm {
    pub fn is_create(&self) -> bool {
        self.id.is_none()
    }

    /// A name and a stream address that looks like one.
    pub fn submittable(&self) -> bool {
        !self.busy && !self.name.trim().is_empty() && valid_url(&self.stream_url)
    }

    fn station(&self) -> InternetRadio {
        let home = self.home_page_url.trim();
        InternetRadio {
            id: self.id.clone().unwrap_or_default(),
            name: self.name.trim().to_string(),
            stream_url: self.stream_url.trim().to_string(),
            home_page_url: (!home.is_empty()).then(|| home.to_string()),
            image_id: None,
        }
    }
}

fn valid_url(s: &str) -> bool {
    let s = s.trim();
    (s.starts_with("http://") || s.starts_with("https://")) && s.len() > 8
}

/// The station that plays now.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveStation {
    pub station: InternetRadio,
    /// The stream's current ICY title (`Artist - Title`).
    pub title: Option<String>,
}

#[derive(Debug, Default)]
pub struct Related {
    /// The song the list belongs to (`server:id`).
    pub for_song: Option<String>,
    /// That song's id, the key the Related tab uses.
    pub song_id: Option<String>,
    pub songs: Vec<Song>,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct Jukebox {
    /// Whether the server answered `jukeboxControl`; `None` = not asked yet.
    pub available: Option<bool>,
    /// Index in the jukebox queue of the song the app believes is playing.
    pub index: i64,
}

#[derive(Debug, Default)]
pub struct RadioState {
    pub stations: Vec<InternetRadio>,
    pub loaded: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub search: String,
    pub sort: Option<RadioSort>,
    pub desc: bool,
    pub form: Option<StationForm>,
    pub active: Option<ActiveStation>,
    pub related: Related,
    pub jukebox: Jukebox,
    /// Demo mode keeps the stations in memory; ids come from here.
    demo_next: u32,
}

impl RadioState {
    pub fn sort(&self) -> RadioSort {
        self.sort.unwrap_or(RadioSort::Name)
    }

    /// The stations the page shows: filtered by the search, sorted.
    pub fn visible(&self) -> Vec<&InternetRadio> {
        let mut list: Vec<&InternetRadio> =
            self.stations.iter().filter(|s| matches_search(s, &self.search)).collect();
        match self.sort() {
            RadioSort::Name => list.sort_by_key(|s| s.name.to_lowercase()),
            RadioSort::Id => list.sort_by_key(|s| natural_id(&s.id)),
        }
        if self.desc {
            list.reverse();
        }
        list
    }
}

/// Ids sort numerically when they are numbers (`2` before `10`).
fn natural_id(id: &str) -> (u64, String) {
    (id.parse().unwrap_or(u64::MAX), id.to_string())
}

fn matches_search(s: &InternetRadio, term: &str) -> bool {
    let term = term.trim().to_lowercase();
    term.is_empty()
        || s.name.to_lowercase().contains(&term)
        || s.stream_url.to_lowercase().contains(&term)
        || s.home_page_url.as_deref().is_some_and(|h| h.to_lowercase().contains(&term))
}

/// The stations demo mode starts with.
pub fn demo_stations() -> Vec<InternetRadio> {
    [
        ("d1", "Jazz Cafe", "https://radio.example/jazz", Some("https://jazz.example")),
        ("d2", "Night Drive FM", "https://radio.example/night", None),
        ("d3", "Classical Morning", "https://radio.example/classic", Some("https://classic.example")),
    ]
    .into_iter()
    .map(|(id, name, url, home)| InternetRadio {
        id: id.into(),
        name: name.into(),
        stream_url: url.into(),
        home_page_url: home.map(String::from),
        image_id: None,
    })
    .collect()
}

/// A song's key for the Related list.
fn song_key(song: &Song) -> String {
    format!("{}:{}", song.server_id.0, song.id)
}

impl Listen {
    // ---- queries used by the views ------------------------------------------------

    /// The browsed server has internet radio (or this is the demo).
    pub fn radio_supported(&self) -> bool {
        self.demo
            || self.source.as_ref().is_some_and(|s| s.capabilities().contains(Capabilities::INTERNET_RADIO))
    }

    pub fn radio_active(&self) -> bool {
        self.radio.active.is_some()
    }

    /// The Related list can be fetched for the playing song.
    pub fn related_supported(&self) -> bool {
        self.demo
            || self
                .queue
                .current()
                .and_then(|i| self.sources.get(&i.song.server_id))
                .is_some_and(|s| s.capabilities().contains(Capabilities::SIMILAR_SONGS))
    }

    /// The jukebox replaces mpv.
    pub fn jukebox_active(&self) -> bool {
        !self.demo
            && self.config.settings.playback.audio_player == AudioPlayer::Jukebox
            && self.radio.jukebox.available != Some(false)
            && self.source.as_ref().is_some_and(|s| s.capabilities().contains(Capabilities::JUKEBOX))
    }

    /// The "Audio player" choice is offered (the server answered the jukebox).
    pub fn jukebox_offered(&self) -> bool {
        self.radio.jukebox.available == Some(true)
            || self.config.settings.playback.audio_player == AudioPlayer::Jukebox
    }

    // ---- entering pages -----------------------------------------------------------

    pub(super) fn radio_enter(&mut self) -> Task<Message> {
        if self.radio.loaded || self.radio.loading {
            return Task::none();
        }
        self.radio_load()
    }

    fn radio_load(&mut self) -> Task<Message> {
        if self.demo {
            if !self.radio.loaded {
                self.radio.stations = demo_stations();
                self.radio.demo_next = 4;
            }
            self.radio.loaded = true;
            return Task::none();
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        self.radio.loading = true;
        self.radio.error = None;
        Task::perform(async move { source.internet_radios().await.map_err(err) }, |r| {
            Message::Radio(RadioMsg::Loaded(r))
        })
    }

    // ---- messages -----------------------------------------------------------------

    pub(super) fn on_radio(&mut self, msg: RadioMsg) -> Task<Message> {
        match msg {
            RadioMsg::Refresh => {
                self.radio.loaded = false;
                return self.radio_load();
            }
            RadioMsg::Loaded(result) => {
                self.radio.loading = false;
                match result {
                    Ok(list) => {
                        self.radio.stations = list;
                        self.radio.loaded = true;
                        let refs: Vec<_> = self
                            .radio
                            .stations
                            .iter()
                            .filter_map(|s| s.image_id.clone().map(listen_core::model::ImageRef::Id))
                            .collect();
                        return Task::batch(
                            refs.iter().map(|r| self.request_image(Some(r))).collect::<Vec<_>>(),
                        );
                    }
                    Err(e) => self.radio.error = Some(e),
                }
            }
            RadioMsg::Search(s) => self.radio.search = s,
            RadioMsg::Sort(s) => self.radio.sort = Some(s),
            RadioMsg::Order => self.radio.desc = !self.radio.desc,
            RadioMsg::Play(id) => return self.radio_play(&id),
            RadioMsg::Stop => return self.radio_stop(),
            RadioMsg::OpenCreate => {
                self.radio.form = Some(StationForm::default());
                self.open_modal(FORM_MODAL);
            }
            RadioMsg::OpenEdit(id) => {
                if let Some(s) = self.radio.stations.iter().find(|s| s.id == id) {
                    self.radio.form = Some(StationForm {
                        id: Some(s.id.clone()),
                        name: s.name.clone(),
                        stream_url: s.stream_url.clone(),
                        home_page_url: s.home_page_url.clone().unwrap_or_default(),
                        busy: false,
                    });
                    self.open_modal(FORM_MODAL);
                }
            }
            RadioMsg::FormName(v) => self.radio_form(|f| f.name = v),
            RadioMsg::FormStream(v) => self.radio_form(|f| f.stream_url = v),
            RadioMsg::FormHome(v) => self.radio_form(|f| f.home_page_url = v),
            RadioMsg::FormSubmit => return self.radio_submit(),
            RadioMsg::Saved(result) => return self.radio_saved(result),
            RadioMsg::AskDelete(id) => {
                let Some(name) = self.radio.stations.iter().find(|s| s.id == id).map(|s| s.name.clone())
                else {
                    return Task::none();
                };
                self.confirm(
                    super::overlay::Confirm::new(
                        "Delete radio station",
                        format!("Delete “{name}”? This cannot be undone."),
                        Message::Radio(RadioMsg::Delete(id)),
                    )
                    .labels("Delete", "Cancel")
                    .danger(),
                );
            }
            RadioMsg::Delete(id) => return self.radio_delete(id),
            RadioMsg::Deleted(id, result) => match result {
                Ok(()) => {
                    self.radio.stations.retain(|s| s.id != id);
                    if self.radio.active.as_ref().is_some_and(|a| a.station.id == id) {
                        return self.radio_stop();
                    }
                }
                Err(e) => self.toast_error(format!("Could not delete the station: {e}")),
            },
            RadioMsg::RelatedRefresh => return self.related_load(true),
            RadioMsg::RelatedLoaded(key, result) => {
                // A late answer for a song that is no longer playing.
                if self.radio.related.for_song.as_deref() != Some(key.as_str()) {
                    return Task::none();
                }
                self.radio.related.loading = false;
                let id = self.radio.related.song_id.clone().unwrap_or_default();
                match result {
                    Ok(songs) => {
                        self.radio.related.songs = songs.clone();
                        self.radio.related.error = None;
                        return self.related_push(&id, songs);
                    }
                    Err(e) => {
                        self.radio.related.error = Some(e);
                        return self.related_push(&id, Vec::new());
                    }
                }
            }
            RadioMsg::SetAudioPlayer(p) => return self.set_audio_player(p),
            RadioMsg::JukeboxProbe => return self.jukebox_probe(),
            RadioMsg::JukeboxProbed(ok) => {
                self.radio.jukebox.available = Some(ok);
                if !ok && self.config.settings.playback.audio_player == AudioPlayer::Jukebox {
                    self.config.settings.playback.audio_player = AudioPlayer::Mpv;
                    self.toast_warning("The server has no jukebox; playing with mpv again.".into());
                    self.save_config();
                }
            }
            RadioMsg::JukeboxPoll => {
                if self.jukebox_active() && self.playback.playing {
                    return self.jukebox_task(JukeboxCmd::Status);
                }
            }
            RadioMsg::JukeboxDone(result) => return self.jukebox_done(result),
        }
        Task::none()
    }

    fn radio_form(&mut self, f: impl FnOnce(&mut StationForm)) {
        if let Some(form) = self.radio.form.as_mut().filter(|f| !f.busy) {
            f(form);
        }
    }

    fn radio_submit(&mut self) -> Task<Message> {
        let Some(form) = self.radio.form.as_mut().filter(|f| f.submittable()) else { return Task::none() };
        let station = form.station();
        let creating = form.is_create();
        form.busy = true;
        if self.demo {
            let saved = if creating {
                self.radio.demo_next += 1;
                InternetRadio { id: format!("d{}", self.radio.demo_next), ..station }
            } else {
                station
            };
            return Task::done(Message::Radio(RadioMsg::Saved(Ok(Some(saved)))));
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        Task::perform(
            async move {
                if creating {
                    source.create_internet_radio(&station).await.map(Some).map_err(err)
                } else {
                    source.update_internet_radio(&station).await.map(|()| Some(station)).map_err(err)
                }
            },
            |r| Message::Radio(RadioMsg::Saved(r)),
        )
    }

    fn radio_saved(&mut self, result: Result<Option<InternetRadio>, String>) -> Task<Message> {
        match result {
            Ok(saved) => {
                self.radio.form = None;
                self.close_modal();
                if let Some(saved) = saved {
                    match self.radio.stations.iter_mut().find(|s| !saved.id.is_empty() && s.id == saved.id) {
                        Some(slot) => *slot = saved.clone(),
                        None if !saved.id.is_empty() => self.radio.stations.push(saved.clone()),
                        None => {}
                    }
                    if let Some(a) = self.radio.active.as_mut().filter(|a| a.station.id == saved.id) {
                        a.station = saved;
                    }
                }
                // The server may have changed more than we know (a new id).
                self.radio_load_after_save()
            }
            Err(e) => {
                if let Some(f) = self.radio.form.as_mut() {
                    f.busy = false;
                }
                self.toast_error(format!("Could not save the station: {e}"));
                Task::none()
            }
        }
    }

    fn radio_load_after_save(&mut self) -> Task<Message> {
        if self.demo {
            return Task::none();
        }
        self.radio.loaded = false;
        self.radio_load()
    }

    fn radio_delete(&mut self, id: String) -> Task<Message> {
        if self.demo {
            return Task::done(Message::Radio(RadioMsg::Deleted(id, Ok(()))));
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        Task::perform(
            async move {
                let r = source.delete_internet_radio(&id).await.map_err(err);
                (id, r)
            },
            |(id, r)| Message::Radio(RadioMsg::Deleted(id, r)),
        )
    }

    // ---- playing a station ----------------------------------------------------------

    fn radio_play(&mut self, id: &str) -> Task<Message> {
        let Some(station) = self.radio.stations.iter().find(|s| s.id == id).cloned() else {
            return Task::none();
        };
        // Clicking the playing station stops it (Feishin).
        if self.radio.active.as_ref().is_some_and(|a| a.station.id == station.id) {
            return self.radio_stop();
        }
        // A station plays on this computer: casting ends.
        let uncast = self.dlna_release();
        let cancel = Task::batch([uncast, self.mix_cancel("a radio station started")]);
        self.dj_interrupt();
        self.restored = None;
        self.bump_token();
        self.playback.position = Duration::ZERO;
        self.playback.duration = None;
        self.playback.reset_scrobble();
        self.seek_preview = None;
        self.fader.pos_at = None;
        self.playback.playing = true;
        let track = Track::url(station.stream_url.clone());
        self.radio.active = Some(ActiveStation { station, title: None });
        if self.mock_playback() {
            return cancel;
        }
        Task::batch([cancel, self.player_task(move |p| async move { p.play(&track, None).await })])
    }

    pub(super) fn radio_stop(&mut self) -> Task<Message> {
        self.radio.active = None;
        self.bump_token();
        self.stop_player()
    }

    /// Player events while a station plays (they concern the stream). `Some` = handled.
    pub(super) fn radio_on_player_event(
        &mut self,
        event: &listen_player::PlayerEvent,
    ) -> Option<Task<Message>> {
        use listen_player::PlayerEvent as E;
        if let E::StreamTitle(title) = event {
            if let Some(a) = self.radio.active.as_mut() {
                a.title = title.clone();
            }
            return Some(Task::none());
        }
        self.radio.active.as_ref()?;
        match event {
            E::Position(pos) => {
                self.playback.position = *pos;
                Some(Task::none())
            }
            E::Duration(_) | E::Advanced | E::Restarted => Some(Task::none()),
            E::Error(e) => {
                let name = self.radio.active.take().map(|a| a.station.name).unwrap_or_default();
                self.playback.playing = false;
                self.toast_error(format!("The stream of “{name}” failed: {e}"));
                Some(Task::none())
            }
            E::Ended => {
                self.playback.playing = false;
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// Transport messages while a station or the jukebox owns playback. `Some` = handled.
    pub(super) fn playback_intercept(&mut self, message: &Message) -> Option<Task<Message>> {
        if let Some(task) = self.dlna_intercept(message) {
            return Some(task); // a DLNA renderer plays
        }
        if self.radio.active.is_some() {
            return match message {
                Message::TogglePlay => {
                    let pause = self.playback.playing;
                    self.playback.playing = !pause;
                    if self.mock_playback() {
                        return Some(Task::none());
                    }
                    Some(self.player_task(move |p| async move { p.set_paused(pause).await }))
                }
                Message::Stop => Some(self.radio_stop()),
                // A stream has no queue to skip through and nothing to seek.
                Message::Next
                | Message::Previous
                | Message::NextClick
                | Message::PreviousClick
                | Message::NextAlbum
                | Message::PreviousAlbum
                | Message::Seek(_)
                | Message::SkipForward
                | Message::SkipBackward => Some(Task::none()),
                _ => None,
            };
        }
        if !self.jukebox_active() {
            return None;
        }
        match message {
            Message::TogglePlay if self.queue.current().is_some() => {
                if self.playback.position == Duration::ZERO && !self.playback.playing {
                    return Some(self.start_current());
                }
                let cmd = if self.playback.playing { JukeboxCmd::Stop } else { JukeboxCmd::Start };
                self.playback.playing = !self.playback.playing;
                Some(self.jukebox_task(cmd))
            }
            Message::Stop => {
                self.playback.playing = false;
                self.playback.position = Duration::ZERO;
                Some(self.jukebox_task(JukeboxCmd::Stop))
            }
            Message::Seek(secs) => {
                let index = self.radio.jukebox.index.max(0) as usize;
                self.playback.position = Duration::from_secs_f32(secs.max(0.0));
                Some(self.jukebox_task(JukeboxCmd::Skip { index, offset_secs: secs.max(0.0) as u32 }))
            }
            Message::Volume(v) => {
                self.playback.volume = v.round().clamp(0.0, 100.0) as u8;
                self.config.settings.playback.volume = self.playback.volume;
                let gain = f32::from(self.playback.volume) / 100.0;
                Some(self.jukebox_task(JukeboxCmd::SetGain(gain)))
            }
            _ => None,
        }
    }

    // ---- Related --------------------------------------------------------------------

    /// Called when the playing song changed: the open Related tab follows it.
    pub(super) fn related_track_changed(&mut self) -> Task<Message> {
        if self.fullscreen && self.fullscreen_tab == super::FullscreenTab::Related {
            self.related_load(false)
        } else {
            Task::none()
        }
    }

    /// Hands the list to the full-screen Related tab.
    fn related_push(&mut self, song_id: &str, songs: Vec<Song>) -> Task<Message> {
        self.on_fullscreen(FullscreenMsg::SetRelated(song_id.to_string(), songs))
    }

    fn related_load(&mut self, force: bool) -> Task<Message> {
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else {
            self.radio.related = Related::default();
            return Task::none();
        };
        let key = song_key(&song);
        let r = &mut self.radio.related;
        if !force && r.for_song.as_deref() == Some(key.as_str()) && (r.loading || r.error.is_none()) {
            return Task::none();
        }
        r.for_song = Some(key.clone());
        r.song_id = Some(song.id.clone());
        r.songs.clear();
        r.error = None;
        if self.demo {
            // No server: the other demo albums stand in.
            let songs: Vec<Song> = crate::demo::albums()
                .into_iter()
                .flat_map(|a| a.songs)
                .filter(|s| s.id != song.id)
                .take(12)
                .collect();
            self.radio.related.songs = songs.clone();
            return self.related_push(&song.id, songs);
        }
        let Some(source) = self.sources.get(&song.server_id).cloned() else { return Task::none() };
        if !source.capabilities().contains(Capabilities::SIMILAR_SONGS) {
            self.radio.related.error = Some("This source has no similar songs.".into());
            return self.related_push(&song.id, Vec::new());
        }
        self.radio.related.loading = true;
        let _ = self.on_fullscreen(FullscreenMsg::RelatedLoading(true));
        Task::perform(related_songs(source, song), move |r| {
            Message::Radio(RadioMsg::RelatedLoaded(key.clone(), r))
        })
    }

    // ---- Jukebox --------------------------------------------------------------------

    fn set_audio_player(&mut self, player: AudioPlayer) -> Task<Message> {
        if self.config.settings.playback.audio_player == player {
            return Task::none();
        }
        let was_jukebox = self.jukebox_active();
        self.config.settings.playback.audio_player = player;
        self.save_config();
        if was_jukebox {
            // Back to mpv: the server stops; the song starts again here.
            self.playback.playing = false;
            return self.jukebox_task(JukeboxCmd::Stop);
        }
        if player == AudioPlayer::Jukebox {
            // Hand the playing song over.
            self.playback.playing = false;
            let stop = self.stop_player();
            if self.queue.current().is_some() {
                return Task::batch([stop, self.start_current()]);
            }
            return stop;
        }
        Task::none()
    }

    pub(super) fn jukebox_probe(&mut self) -> Task<Message> {
        if self.demo || self.radio.jukebox.available.is_some() {
            return Task::none();
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        if !source.capabilities().contains(Capabilities::JUKEBOX) {
            self.radio.jukebox.available = Some(false);
            return Task::none();
        }
        Task::perform(async move { source.jukebox(&JukeboxCmd::Status).await.is_ok() }, |ok| {
            Message::Radio(RadioMsg::JukeboxProbed(ok))
        })
    }

    fn jukebox_task(&self, cmd: JukeboxCmd) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        Task::perform(async move { source.jukebox(&cmd).await.map_err(err) }, |r| {
            Message::Radio(RadioMsg::JukeboxDone(r))
        })
    }

    /// Hands the queue (from the current song on) to the server and starts it.
    pub(super) fn jukebox_start(&mut self) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        let Some(current) = self.queue.current().map(|i| i.song.server_id.clone()) else {
            return Task::none();
        };
        let at = self.queue.current_position().unwrap_or(0);
        let ids: Vec<String> = self
            .queue
            .iter()
            .skip(at)
            .filter(|i| i.song.server_id == current)
            .take(JUKEBOX_BATCH)
            .map(|i| i.song.id.clone())
            .collect();
        self.radio.jukebox.index = 0;
        self.playback.playing = true;
        self.playback.position = Duration::ZERO;
        Task::perform(
            async move {
                source.jukebox(&JukeboxCmd::Set(ids)).await?;
                source.jukebox(&JukeboxCmd::Start).await
            },
            |r| Message::Radio(RadioMsg::JukeboxDone(r.map_err(err))),
        )
    }

    fn jukebox_done(&mut self, result: Result<JukeboxStatus, String>) -> Task<Message> {
        let status = match result {
            Ok(s) => s,
            Err(e) => {
                if self.jukebox_active() {
                    self.config.settings.playback.audio_player = AudioPlayer::Mpv;
                    self.radio.jukebox.available = Some(false);
                    self.playback.playing = false;
                    self.toast_error(format!("The jukebox failed ({e}); using mpv again."));
                    self.save_config();
                }
                return Task::none();
            }
        };
        self.radio.jukebox.available = Some(true);
        if !self.jukebox_active() {
            return Task::none();
        }
        self.playback.playing = status.playing;
        self.playback.position = Duration::from_secs(u64::from(status.position_secs));
        if status.current_index > self.radio.jukebox.index {
            // The server went on to the next song of the batch.
            let steps = (status.current_index - self.radio.jukebox.index) as usize;
            self.radio.jukebox.index = status.current_index;
            for _ in 0..steps {
                self.queue.advance();
            }
            self.bump_token();
            self.playback.reset_scrobble();
            return self.on_track_changed();
        }
        if !status.playing && status.current_index < 0 && self.playback.position == Duration::ZERO {
            self.playback.playing = false;
        }
        Task::none()
    }

    /// The timers radio and the jukebox need.
    pub(super) fn radio_subscriptions(&self) -> Vec<iced::Subscription<Message>> {
        let mut subs = Vec::new();
        if self.jukebox_active() && self.playback.playing {
            subs.push(iced::time::every(JUKEBOX_POLL).map(|_| Message::Radio(RadioMsg::JukeboxPoll)));
        }
        subs
    }
}

/// Similar songs of `song` from its source; the song itself is dropped.
/// YouTube Music's radio starts from the song, so it comes back too.
async fn related_songs(source: Arc<dyn listen_source::MusicSource>, song: Song) -> Result<Vec<Song>, String> {
    let mut songs = source.similar_songs(&song, RELATED_COUNT).await.map_err(err)?;
    songs.retain(|s| s.id != song.id);
    songs.truncate(RELATED_COUNT);
    Ok(songs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station(id: &str, name: &str) -> InternetRadio {
        InternetRadio {
            id: id.into(),
            name: name.into(),
            stream_url: format!("http://x/{id}"),
            home_page_url: None,
            image_id: None,
        }
    }

    #[test]
    fn filters_and_sorts_stations() {
        let mut s = RadioState {
            stations: vec![station("10", "beta"), station("2", "Alpha"), station("3", "gamma")],
            ..Default::default()
        };
        let names = |s: &RadioState| s.visible().iter().map(|x| x.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&s), ["Alpha", "beta", "gamma"]);
        s.desc = true;
        assert_eq!(names(&s), ["gamma", "beta", "Alpha"]);
        s.desc = false;
        s.sort = Some(RadioSort::Id);
        assert_eq!(names(&s), ["Alpha", "gamma", "beta"], "ids sort as numbers");
        s.search = "GAM".into();
        assert_eq!(names(&s), ["gamma"]);
        s.search = "x/10".into();
        assert_eq!(names(&s), ["beta"], "the stream address is searched too");
    }

    #[test]
    fn the_form_needs_a_name_and_a_stream_address() {
        let mut f = StationForm::default();
        assert!(!f.submittable());
        f.name = " Jazz ".into();
        f.stream_url = "ftp://x".into();
        assert!(!f.submittable());
        f.stream_url = "https://radio.example/jazz".into();
        assert!(f.submittable());
        let st = f.station();
        assert_eq!(st.name, "Jazz");
        assert_eq!(st.home_page_url, None);
        f.home_page_url = " https://home ".into();
        assert_eq!(f.station().home_page_url.as_deref(), Some("https://home"));
        f.busy = true;
        assert!(!f.submittable());
    }
}
