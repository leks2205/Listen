//! The full-screen About tab: facts about the playing song with their web sources (Gemini with
//! Google Search grounding, the AI DJ's key and text model) and its credits from MusicBrainz
//! (`listen_lyrics::card`). Loaded when the tab is open, cached per song and app language.

use super::{FullscreenTab, Listen, Message};
use iced::Task;
use listen_lyrics::card::{Card, CardCache, SongInfo};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum AboutMsg {
    /// The card for `key` (song and language).
    Loaded(String, Result<Card, String>),
    /// Opens a source link in the browser.
    Open(String),
    /// Asks again, past the cache.
    Refresh,
}

#[derive(Debug, Default)]
pub struct AboutState {
    /// "server:song|lang" of the card shown or loading.
    pub key: String,
    pub card: Option<Card>,
    pub loading: bool,
    pub error: Option<String>,
}

/// The card's song details.
pub fn song_info(song: &listen_core::model::Song) -> SongInfo {
    SongInfo {
        title: song.title.clone(),
        artists: song.artists.iter().map(|a| a.name.clone()).collect(),
        album: song.album.clone(),
        year: song.year.and_then(|y| u32::try_from(y).ok()).filter(|y| *y > 0),
        recording_mbid: song.mbz.recording.clone(),
    }
}

/// The card language: the app language (German or English).
fn language() -> (&'static str, &'static str) {
    if crate::i18n::language() == "de" { ("de", "German") } else { ("en", "English") }
}

impl Listen {
    fn about_dir(&self) -> Option<PathBuf> {
        self.paths.as_ref().map(|p| p.cache_dir.join("song-cards"))
    }

    /// Called when the playing song changed or the panel opened: the open About tab follows.
    pub(super) fn about_track_changed(&mut self) -> Task<Message> {
        if self.fullscreen && self.fullscreen_tab == FullscreenTab::About {
            self.about_load(false)
        } else {
            Task::none()
        }
    }

    /// Whether a card can be asked for (the AI DJ's Gemini key is set).
    pub fn about_available(&self) -> bool {
        self.gemini_key().is_some()
    }

    fn about_load(&mut self, force: bool) -> Task<Message> {
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else {
            self.about = AboutState::default();
            return Task::none();
        };
        let (lang, language) = language();
        let song_key = format!("{}:{}", song.server_id.0, song.id);
        let key = format!("{song_key}|{lang}");
        if !force && self.about.key == key && (self.about.card.is_some() || self.about.loading) {
            return Task::none();
        }
        self.about = AboutState { key: key.clone(), ..AboutState::default() };
        if self.demo {
            return Task::none();
        }
        let now = listen_core::history::now_secs();
        if !force && let Some(card) = self.about_dir().and_then(|d| CardCache::load(&d, &song_key, lang, now))
        {
            tracing::info!("song card ({lang}) from the cache");
            self.about.card = Some(card);
            return Task::none();
        }
        let Some(api) = self.gemini_key() else { return Task::none() };
        let model = self.config.settings.ai_dj.text_model.clone();
        let http = self.http.clone();
        let info = song_info(&song);
        self.about.loading = true;
        Task::perform(
            async move { listen_lyrics::card::card(&http, &api, &model, language, &info, now).await },
            move |r| Message::About(AboutMsg::Loaded(key.clone(), r)),
        )
    }

    pub(super) fn on_about(&mut self, msg: AboutMsg) -> Task<Message> {
        match msg {
            AboutMsg::Loaded(key, result) => {
                if key != self.about.key {
                    return Task::none();
                }
                self.about.loading = false;
                match result {
                    Ok(card) => {
                        let (song_key, lang) = key.rsplit_once('|').unwrap_or((key.as_str(), ""));
                        if !card.is_empty()
                            && let Some(dir) = self.about_dir()
                            && let Err(e) = CardCache::save(&dir, song_key, lang, &card)
                        {
                            tracing::warn!("saving the song card: {e}");
                        }
                        self.about.card = Some(card);
                    }
                    Err(e) => {
                        tracing::warn!("song card: {e}");
                        self.about.error = Some(e);
                    }
                }
            }
            AboutMsg::Open(url) => super::album_page::open_url(&url),
            AboutMsg::Refresh => return self.about_load(true),
        }
        Task::none()
    }
}
