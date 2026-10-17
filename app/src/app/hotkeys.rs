//! Hotkey actions: which message each Feishin action runs, the editor's messages
//! (record / clear / global flag / media keys) and the recording of a combo.

use super::*;
use listen_core::model::SourceKind;
use listen_source::{Capabilities, ItemKind};

/// Zoom steps from the `zoomIn` / `zoomOut` hotkeys; handled by the appearance settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomStep {
    In,
    Out,
    Reset,
}

/// A list hotkey acting on the selected rows (the item lists handle it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListHotkey {
    PlayDefault,
    PlayNow,
    PlayNext,
    PlayLast,
    NavigateToPage,
    ShowPlayingSong,
}

/// The song a favorite hotkey is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SongRef {
    Current,
    /// The queue entry before the current one.
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FavoriteMode {
    Add,
    Remove,
    Toggle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HotkeysMsg {
    /// Edit button of a row: the next key combo becomes its binding (again: cancel).
    Record(String),
    /// Clear button: removes the binding and its global flag.
    Clear(String),
    /// The row's "global" checkbox.
    SetGlobal(String, bool),
    /// Grab the hardware media keys through the portal.
    MediaKeys(bool),
    /// `play` (does nothing when already playing).
    Play,
    /// `pause` (does nothing when already paused).
    Pause,
    /// `rate0` .. `rate5` for the current song.
    Rate(u8),
    Favorite(SongRef, FavoriteMode),
    List(ListHotkey),
}

impl Listen {
    /// The message Feishin's action `action` runs, if Listen handles it in any window state.
    /// Shared by window hotkeys, system-wide shortcuts and the media keys.
    pub fn hotkey_message(&self, action: &str) -> Option<Message> {
        use FavoriteMode::{Add, Remove, Toggle};
        use SongRef::{Current, Previous};
        let hk = |m| Some(Message::Hotkeys(m));
        match action {
            "playPause" => Some(Message::TogglePlay),
            "play" => hk(HotkeysMsg::Play),
            "pause" => hk(HotkeysMsg::Pause),
            "next" => Some(Message::Next),
            "previous" => Some(Message::Previous),
            "nextAlbum" => Some(Message::NextAlbum),
            "previousAlbum" => Some(Message::PreviousAlbum),
            "stop" => Some(Message::Stop),
            "skipForward" => Some(Message::SkipForward),
            "skipBackward" => Some(Message::SkipBackward),
            "toggleShuffle" => Some(Message::ToggleShuffle),
            "toggleRepeat" => Some(Message::CycleRepeat),
            "volumeUp" => Some(Message::VolumeWheel(1.0)),
            "volumeDown" => Some(Message::VolumeWheel(-1.0)),
            "volumeMute" => Some(Message::ToggleMute),
            "favoriteCurrentAdd" => hk(HotkeysMsg::Favorite(Current, Add)),
            "favoriteCurrentRemove" => hk(HotkeysMsg::Favorite(Current, Remove)),
            "favoriteCurrentToggle" => Some(Message::ToggleFavorite),
            "favoritePreviousAdd" => hk(HotkeysMsg::Favorite(Previous, Add)),
            "favoritePreviousRemove" => hk(HotkeysMsg::Favorite(Previous, Remove)),
            "favoritePreviousToggle" => hk(HotkeysMsg::Favorite(Previous, Toggle)),
            "rate0" => hk(HotkeysMsg::Rate(0)),
            "rate1" => hk(HotkeysMsg::Rate(1)),
            "rate2" => hk(HotkeysMsg::Rate(2)),
            "rate3" => hk(HotkeysMsg::Rate(3)),
            "rate4" => hk(HotkeysMsg::Rate(4)),
            "rate5" => hk(HotkeysMsg::Rate(5)),
            "zoomIn" => Some(Message::Zoom(ZoomStep::In)),
            "zoomOut" => Some(Message::Zoom(ZoomStep::Out)),
            "toggleQueue" => Some(Message::ToggleQueue),
            "toggleFullscreenPlayer" => Some(Message::ToggleFullscreen),
            "browserBack" => Some(Message::Back),
            "browserForward" => Some(Message::Forward),
            "navigateHome" => Some(Message::Navigate(Route::Home)),
            "listPlayDefault" => hk(HotkeysMsg::List(ListHotkey::PlayDefault)),
            "listPlayNow" => hk(HotkeysMsg::List(ListHotkey::PlayNow)),
            "listPlayNext" => hk(HotkeysMsg::List(ListHotkey::PlayNext)),
            "listPlayLast" => hk(HotkeysMsg::List(ListHotkey::PlayLast)),
            "listNavigateToPage" => hk(HotkeysMsg::List(ListHotkey::NavigateToPage)),
            "listShowPlayingSong" => hk(HotkeysMsg::List(ListHotkey::ShowPlayingSong)),
            _ => None,
        }
    }

    /// Actions whose hotkey the desktop portal handles instead of the window.
    pub(super) fn portal_active(&self) -> bool {
        cfg!(target_os = "linux") && !self.demo && self.screenshot.is_none() && !self.ephemeral
    }

    #[cfg(target_os = "linux")]
    /// Whether the portal should be running: some action is global or the media keys are grabbed.
    pub fn portal_wanted(&self) -> crate::global_shortcuts::Wanted {
        if self.portal_active() {
            crate::global_shortcuts::wanted(
                &self.config.settings,
                crate::global_shortcuts::session_handles_media_keys(),
            )
        } else {
            Vec::new()
        }
    }

    /// The queue entry before the current one.
    fn previous_song(&self) -> Option<Song> {
        let p = self.queue.current_position()?.checked_sub(1)?;
        self.queue.iter().nth(p).map(|i| i.song.clone())
    }

    /// A key press while a row is recording; `true` when it was consumed.
    pub(super) fn record_hotkey(
        &mut self,
        key: &iced::keyboard::Key,
        physical: &iced::keyboard::key::Physical,
        modifiers: iced::keyboard::Modifiers,
    ) -> bool {
        let Some(action) = self.hotkey_recording.clone() else { return false };
        if *key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) {
            self.hotkey_recording = None;
            return true;
        }
        let name = crate::hotkeys::press_name(key, physical);
        if let Some(combo) = crate::hotkeys::record(name.as_deref(), modifiers) {
            self.hotkey_recording = None;
            self.config.settings.hotkeys.insert(action, combo);
            self.hotkeys_changed();
        }
        true
    }

    fn hotkeys_changed(&mut self) {
        self.global_error = None;
        self.global_triggers.clear();
        self.save_config();
    }

    pub(super) fn on_hotkeys(&mut self, msg: HotkeysMsg) -> Task<Message> {
        match msg {
            HotkeysMsg::Record(action) => {
                self.hotkey_recording = (self.hotkey_recording.as_deref() != Some(&action)).then_some(action);
            }
            HotkeysMsg::Clear(action) => {
                let s = &mut self.config.settings;
                s.hotkeys.remove(&action);
                s.hotkeys_global.remove(&action);
                if self.hotkey_recording.as_deref() == Some(&action) {
                    self.hotkey_recording = None;
                }
                self.hotkeys_changed();
            }
            HotkeysMsg::SetGlobal(action, on) => {
                let s = &mut self.config.settings;
                let allowed = listen_core::settings::hotkey_action(&action).is_some_and(|a| a.allow_global);
                if on && allowed && s.hotkeys.get(&action).is_some_and(|h| !h.is_empty()) {
                    s.hotkeys_global.insert(action);
                } else {
                    s.hotkeys_global.remove(&action);
                }
                self.hotkeys_changed();
            }
            HotkeysMsg::MediaKeys(on) => {
                self.config.settings.global_media_keys = on;
                self.hotkeys_changed();
            }
            HotkeysMsg::Play => {
                if !self.playback.playing {
                    return self.handle(Message::TogglePlay);
                }
            }
            HotkeysMsg::Pause => {
                if self.playback.playing {
                    return self.handle(Message::TogglePlay);
                }
            }
            HotkeysMsg::Rate(stars) => return self.rate_current(stars),
            HotkeysMsg::Favorite(which, mode) => {
                let song = match which {
                    SongRef::Current => self.queue.current().map(|i| i.song.clone()),
                    SongRef::Previous => self.previous_song(),
                };
                let Some(song) = song else { return Task::none() };
                let favorite = match mode {
                    FavoriteMode::Add => true,
                    FavoriteMode::Remove => false,
                    FavoriteMode::Toggle => !song.favorite,
                };
                if favorite != song.favorite {
                    return self.set_song_favorite(&song, favorite);
                }
            }
            // The item lists act on their selection (`app/list.rs`).
            HotkeysMsg::List(key) => return self.on_list_hotkey(key),
        }
        Task::none()
    }

    /// Rates the playing song; sources without ratings (YouTube Music) ignore it.
    fn rate_current(&mut self, stars: u8) -> Task<Message> {
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else { return Task::none() };
        self.rate_song(&song, stars)
    }

    /// Rates a song (0 clears) everywhere it is shown; sources without ratings ignore it.
    pub(super) fn rate_song(&mut self, song: &Song, stars: u8) -> Task<Message> {
        let song = song.clone();
        let Some(source) = self.sources.get(&song.server_id).cloned() else { return Task::none() };
        if song.source == SourceKind::YouTubeMusic || !source.capabilities().contains(Capabilities::RATINGS) {
            return Task::none();
        }
        let stars = stars.min(5);
        let (server, id) = (song.server_id.clone(), song.id.clone());
        let rate = |s: &mut Song| {
            if s.server_id == server && s.id == id {
                s.rating = (stars > 0).then_some(stars);
            }
        };
        self.queue.for_each_song_mut(rate);
        let lib = &mut self.library;
        for list in [
            Some(&mut lib.songs),
            Some(&mut lib.favorites),
            lib.album.as_mut().map(|a| &mut a.songs),
            lib.playlist.as_mut().map(|p| &mut p.songs),
            lib.artist.as_mut().map(|a| &mut a.top_songs),
        ]
        .into_iter()
        .flatten()
        {
            list.iter_mut().for_each(rate);
        }
        self.album_rows_sync();
        let text = if stars == 0 { "Rating cleared.".to_string() } else { format!("Rated {stars} / 5.") };
        Task::perform(
            async move {
                source
                    .set_item_rating(ItemKind::Song, &id, stars)
                    .await
                    .map(|_| text)
                    .map_err(|e| format!("Could not rate the song: {e}"))
            },
            |r| Message::Menu(menu::MenuMsg::Done(r)),
        )
    }
}
