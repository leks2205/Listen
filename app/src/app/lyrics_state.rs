//! Lyrics: the lookup chain (server / YouTube Music first, then the providers),
//! the per-song override and offset, the search and export dialogs, translation,
//! and following the active line. The view is `ui::lyrics`.

use super::*;
use listen_core::model::Lyrics;
use listen_core::settings::LyricsSettings;
use listen_lyrics::fetch::{self, Options};
use listen_lyrics::providers::{Query, SearchHit, Source};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Keys of the modals (`ui::overlay`).
pub const SEARCH_MODAL: &str = "lyrics-search";
pub const EXPORT_MODAL: &str = "lyrics-export";
pub const SETTINGS_MODAL: &str = "lyrics-settings";
const OVERRIDES_FILE: &str = "lyrics_overrides.json";

/// The result a user picked for a song, remembered across sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Override {
    pub source: String,
    pub id: String,
    pub artist: String,
    pub name: String,
}

impl Override {
    fn hit(&self) -> Option<SearchHit> {
        Some(SearchHit {
            source: Source::from_name(&self.source)?,
            id: self.id.clone(),
            artist: self.artist.clone(),
            name: self.name.clone(),
            synced: None,
            score: 0.0,
        })
    }
}

/// The search dialog.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub name: String,
    pub artist: String,
    pub loading: bool,
    pub searched: bool,
    pub hits: Vec<SearchHit>,
    pub selected: Option<usize>,
    pub preview: Option<Lyrics>,
    pub preview_loading: bool,
    /// Tells a stale answer from the current search / preview.
    pub token: u64,
}

/// The export dialog.
#[derive(Debug, Clone)]
pub struct ExportState {
    pub lyrics: Lyrics,
    pub name: String,
    pub artist: String,
    pub synced: bool,
    pub offset_ms: i32,
    pub offset_text: String,
}

impl ExportState {
    /// The file's text.
    pub fn text(&self) -> String {
        if self.synced && self.lyrics.synced {
            listen_lyrics::export::to_lrc(&self.lyrics, &self.name, &self.artist, self.offset_ms)
        } else {
            listen_lyrics::export::to_txt(&self.lyrics)
        }
    }

    pub fn file_name(&self) -> String {
        listen_lyrics::export::file_name(&self.artist, &self.name, self.synced && self.lyrics.synced)
    }
}

pub struct LyricsUi {
    overrides: HashMap<String, Override>,
    file: Option<PathBuf>,
    /// The song the state below belongs to (`server:id`).
    pub key: String,
    /// Session offsets per song (the setting is only the default).
    offsets: HashMap<String, i32>,
    /// What the number input shows while typing (`-`, empty ...).
    pub offset_text: Option<String>,
    /// Songs whose provider lookup the user cleared.
    suppressed: HashSet<String>,
    /// Everything found for the song; the shown lyrics are `selected`.
    pub candidates: Vec<Lyrics>,
    pub selected: usize,
    /// Online translation, line for line.
    pub translated: Vec<Option<String>>,
    pub translating: bool,
    pub show_translation: bool,
    pub search: Option<SearchState>,
    pub export: Option<ExportState>,
    /// Playback position plus offset, for the word highlight.
    pub pos_ms: u64,
    /// Size of the lyrics viewport, once the scrollable reported it.
    pub viewport: Option<iced::Size>,
    /// Bumped when the shown lyrics change (MPRIS publishes them).
    pub rev: u64,
    token: u64,
}

impl LyricsUi {
    /// `data_dir` makes the overrides persistent; `None` keeps them in memory.
    pub fn new(data_dir: Option<&Path>) -> Self {
        let file = data_dir.map(|d| d.join(OVERRIDES_FILE));
        let overrides = file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            overrides,
            file,
            key: String::new(),
            offsets: HashMap::new(),
            offset_text: None,
            suppressed: HashSet::new(),
            candidates: Vec::new(),
            selected: 0,
            translated: Vec::new(),
            translating: false,
            show_translation: true,
            search: None,
            export: None,
            pos_ms: 0,
            viewport: None,
            rev: 0,
            token: 0,
        }
    }

    pub fn override_for(&self, key: &str) -> Option<&Override> {
        self.overrides.get(key)
    }

    fn set_override(&mut self, key: &str, o: Option<Override>) {
        match o {
            Some(o) => self.overrides.insert(key.to_string(), o),
            None => self.overrides.remove(key),
        };
        let Some(file) = &self.file else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(&self.overrides) {
            Ok(json) => {
                if let Err(e) = std::fs::write(file, json) {
                    tracing::warn!("saving lyric overrides: {e}");
                }
            }
            Err(e) => tracing::warn!("saving lyric overrides: {e}"),
        }
    }

    /// The offset of the current song, ms.
    pub fn offset_ms(&self, settings: &LyricsSettings) -> i32 {
        self.offsets.get(&self.key).copied().unwrap_or(settings.delay_ms)
    }

    fn shown_translation<'a>(&'a self, line: &'a listen_core::model::LyricLine, i: usize) -> Option<&'a str> {
        if !self.show_translation {
            return None;
        }
        line.translation.as_deref().or_else(|| self.translated.get(i).and_then(|t| t.as_deref()))
    }

    /// The translation to show under line `i` of `lyrics`.
    pub fn translation_of<'a>(&'a self, lyrics: &'a Lyrics, i: usize) -> Option<&'a str> {
        lyrics.lines.get(i).and_then(|l| self.shown_translation(l, i))
    }

    /// Whether the lyrics have a translation to toggle (from the provider or online).
    pub fn has_translation(&self, lyrics: &Lyrics) -> bool {
        listen_lyrics::has_translation(lyrics) || self.translated.iter().any(Option::is_some)
    }

    fn next_token(&mut self) -> u64 {
        self.token += 1;
        self.token
    }
}

/// A change in the lyrics settings.
#[derive(Debug, Clone, PartialEq)]
pub enum LyricsSetting {
    Follow(bool),
    PreferLocal(bool),
    Romaji(bool),
    Furigana(bool),
    NeteaseTranslation(bool),
    AutoTranslation(bool),
    TranslationProvider(String),
    TranslationKey(String),
    TranslationLanguage(String),
    DelayMs(i32),
    Alignment(String),
    FontSize(f32),
    FontSizeUnsync(f32),
    Gap(f32),
    GapUnsync(f32),
    PaddingLeft(f32),
    PaddingRight(f32),
    FollowAlignment(i32),
    LeadMs(u32),
    Opacity(f32),
    Scale(f32),
    ShowMatch(bool),
    ShowProvider(bool),
    /// A provider on or off (turning on appends it to the order).
    Source(String, bool),
    /// A provider one place up (-1) or down (1) in the order.
    MoveSource(String, i32),
}

impl LyricsSetting {
    pub fn apply(self, s: &mut LyricsSettings) {
        match self {
            Self::Follow(v) => s.follow = v,
            Self::PreferLocal(v) => s.prefer_local = v,
            Self::Romaji(v) => {
                s.enable_romaji = v;
                if v {
                    listen_lyrics::furigana::warm_up();
                }
            }
            Self::Furigana(v) => {
                s.enable_furigana = v;
                if v {
                    listen_lyrics::furigana::warm_up();
                }
            }
            Self::NeteaseTranslation(v) => s.enable_netease_translation = v,
            Self::AutoTranslation(v) => s.enable_auto_translation = v,
            Self::TranslationProvider(v) => s.translation_api_provider = v,
            Self::TranslationKey(v) => s.translation_api_key = v,
            Self::TranslationLanguage(v) => s.translation_target_language = v,
            Self::DelayMs(v) => s.delay_ms = v.clamp(-10_000, 10_000),
            Self::Alignment(v) => s.alignment = v,
            Self::FontSize(v) => s.font_size = v.clamp(8.0, 72.0).round(),
            Self::FontSizeUnsync(v) => s.font_size_unsync = v.clamp(8.0, 72.0).round(),
            Self::Gap(v) => s.gap = v.clamp(0.0, 50.0).round(),
            Self::GapUnsync(v) => s.gap_unsync = v.clamp(0.0, 50.0).round(),
            Self::PaddingLeft(v) => s.padding_left = v.clamp(0.0, 50.0).round(),
            Self::PaddingRight(v) => s.padding_right = v.clamp(0.0, 50.0).round(),
            Self::FollowAlignment(v) => s.follow_scroll_alignment = v.clamp(-50, 50),
            Self::LeadMs(v) => s.line_lead_time_ms = v.min(3000),
            Self::Opacity(v) => s.opacity_non_active = (v.clamp(0.0, 1.0) * 100.0).round() / 100.0,
            Self::Scale(v) => s.scale_non_active = (v.clamp(0.5, 1.0) * 100.0).round() / 100.0,
            Self::ShowMatch(v) => s.show_match = v,
            Self::ShowProvider(v) => s.show_provider = v,
            Self::Source(name, on) => {
                s.sources.retain(|n| *n != name);
                if on && Source::from_name(&name).is_some() {
                    s.sources.push(name);
                }
            }
            Self::MoveSource(name, by) => {
                if let Some(i) = s.sources.iter().position(|n| *n == name) {
                    let j = (i as i32 + by).clamp(0, s.sources.len() as i32 - 1) as usize;
                    s.sources.swap(i, j);
                }
            }
        }
    }
}

/// The lyrics view's messages.
#[derive(Debug, Clone)]
pub enum LyricsMsg {
    /// The lookup for a song finished: what it found, the shown one first.
    Loaded(String, Vec<Lyrics>),
    /// Offset buttons: ±50 ms.
    OffsetStep(i32),
    OffsetInput(String),
    /// The lyrics scrollable's size.
    Viewport(iced::Size),
    /// Remove the override and the provider lyrics.
    Clear,
    /// Look the song up again.
    Refresh,
    /// Pick one of the found lyrics (the language / version picker).
    Version(usize),
    /// The Translation button.
    Translation,
    Translated(String, Option<Vec<Option<String>>>),
    SearchOpen,
    SearchClose,
    SearchName(String),
    SearchArtist(String),
    SearchRun,
    SearchResults(u64, Vec<SearchHit>),
    SearchPick(usize),
    Preview(u64, Option<Lyrics>),
    SearchApply,
    /// Export the previewed result.
    SearchExport,
    ExportOpen,
    ExportSynced(bool),
    ExportOffset(String),
    ExportSave,
    SettingsOpen,
}

/// Where exports go: the user's Downloads folder.
fn export_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(Path::to_path_buf))
        .or_else(|| directories::UserDirs::new().map(|d| d.home_dir().to_path_buf()))
        .unwrap_or_else(std::env::temp_dir)
}

/// Writes `text` as `dir/name`, or `name (2)` etc. when it exists.
pub fn write_export(dir: &Path, name: &str, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let mut path = dir.join(name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    std::fs::write(&path, text)?;
    Ok(path)
}

impl Listen {
    pub fn song_key(song: &Song) -> String {
        format!("{}:{}", song.server_id.0, song.id)
    }

    /// Looks the playing song's lyrics up, unless that already happened.
    pub(crate) fn load_lyrics(&mut self) -> Task<Message> {
        self.lyric_active = None;
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else {
            self.lyrics = None;
            self.lyrics_ui.candidates.clear();
            return Task::none();
        };
        if self.lyrics_song.as_deref() == Some(song.id.as_str()) {
            return Task::none();
        }
        self.lyrics_song = Some(song.id.clone());
        self.lookup_lyrics(song)
    }

    fn lookup_lyrics(&mut self, song: Song) -> Task<Message> {
        self.lyrics = None;
        self.lyric_active = None;
        let key = Self::song_key(&song);
        let ui = &mut self.lyrics_ui;
        if ui.key != key {
            ui.key = key.clone();
            ui.offset_text = None;
            ui.show_translation = true;
        }
        ui.candidates.clear();
        ui.selected = 0;
        ui.translated.clear();
        ui.translating = false;
        ui.rev += 1;
        let s = &self.config.settings.lyrics;
        let ovr = ui.override_for(&key).cloned();
        if self.demo || (!s.fetch && ovr.is_none()) {
            return Task::none();
        }
        self.lyrics_loading = true;
        let fetch_on = s.fetch && !ui.suppressed.contains(&key);
        let opts = Options {
            sources: Source::parse_list(&s.sources),
            netease_translation: s.enable_netease_translation,
        };
        let prefer_local = s.prefer_local;
        let source = self.sources.get(&song.server_id).cloned();
        let http = self.http.clone();
        let id = song.id.clone();
        Task::perform(
            async move {
                if let Some(hit) = ovr.as_ref().and_then(Override::hit)
                    && let Some(l) = fetch::get(&http, &hit, &opts).await
                {
                    return vec![l];
                }
                let server = match source {
                    Some(s) => s.lyrics(&song).await.ok().flatten().filter(|l| !l.lines.is_empty()),
                    None => None,
                };
                if !fetch_on || opts.sources.is_empty() || fetch::local_settles(server.as_ref(), prefer_local)
                {
                    return server.into_iter().collect();
                }
                let remote = fetch::auto(&http, &song, &opts).await;
                let mut found = Vec::new();
                if let Some(chosen) = fetch::choose(server.clone(), remote.clone(), prefer_local) {
                    found.push(chosen.clone());
                    found.extend([server, remote].into_iter().flatten().filter(|l| *l != chosen));
                }
                found
            },
            move |found| Message::Lyrics(LyricsMsg::Loaded(id.clone(), found)),
        )
    }

    /// The song playing now, if any.
    fn lyrics_song_now(&self) -> Option<Song> {
        self.queue.current().map(|i| i.song.clone())
    }

    /// Whether the lyrics are on screen (the full-screen tab; the panels add theirs).
    pub(crate) fn lyrics_visible(&self) -> bool {
        self.fullscreen && self.fullscreen_tab == FullscreenTab::Lyrics
    }

    /// Scrolls the lyrics so the active line sits where the settings say, in
    /// the full-screen tab and in the sidebar panel (both use `ui::lyrics`).
    pub(crate) fn scroll_lyrics(&self) -> Task<Message> {
        let s = &self.config.settings.lyrics;
        let Some(i) = self.lyric_active.filter(|_| s.follow) else { return Task::none() };
        let Some(y) = ui::lyrics::follow_offset(self, i) else { return Task::none() };
        let to = |id: &'static str| {
            iced::widget::operation::scroll_to(
                iced::widget::Id::new(id),
                scrollable::AbsoluteOffset { x: 0.0, y },
            )
        };
        let mut tasks = Vec::new();
        if self.lyrics_visible() {
            tasks.push(to(ui::lyrics::LYRICS_ID));
        }
        if self.queue_panel_lyrics_shown() {
            tasks.push(to(ui::queue::PANEL_LYRICS_ID));
        }
        Task::batch(tasks)
    }

    /// Follows the playback position: the active line, the word highlight.
    pub(crate) fn lyrics_tick(&mut self, pos: Duration) -> Option<Task<Message>> {
        let lyrics = self.lyrics.as_ref()?;
        let s = &self.config.settings.lyrics;
        let offset = self.lyrics_ui.offset_ms(s);
        let lead = Duration::from_millis(s.line_lead_time_ms.into());
        let pos = if offset >= 0 {
            pos + Duration::from_millis(offset as u64)
        } else {
            pos.saturating_sub(Duration::from_millis(offset.unsigned_abs().into()))
        };
        if listen_lyrics::has_word_sync(lyrics) {
            self.lyrics_ui.pos_ms = pos.as_millis() as u64;
        }
        let active = listen_lyrics::active_line(lyrics, pos, lead);
        if active == self.lyric_active {
            return None;
        }
        self.lyric_active = active;
        Some(self.scroll_lyrics())
    }

    /// The lyrics as one text for MPRIS (`xesam:asText`): LRC when synchronized.
    pub fn lyrics_mpris_text(&self) -> Option<String> {
        let l = self.lyrics.as_ref()?;
        let offset = self.lyrics_ui.offset_ms(&self.config.settings.lyrics);
        let song = self.queue.current().map(|i| &i.song);
        let (name, artist) = song
            .map(|s| (s.title.as_str(), s.artists.first().map(|a| a.name.as_str()).unwrap_or_default()))
            .unwrap_or_default();
        Some(if l.synced {
            listen_lyrics::export::to_lrc(l, name, artist, offset)
        } else {
            listen_lyrics::export::to_txt(l)
        })
    }

    fn show_candidate(&mut self, i: usize) {
        let ui = &mut self.lyrics_ui;
        if let Some(l) = ui.candidates.get(i) {
            ui.selected = i;
            self.lyrics = Some(l.clone());
            ui.translated.clear();
            ui.rev += 1;
            self.lyric_active = None;
        }
    }

    fn translation_configured(&self) -> bool {
        let s = &self.config.settings.lyrics;
        !s.translation_api_provider.is_empty() && !s.translation_api_key.trim().is_empty()
    }

    fn translate_lyrics(&mut self) -> Task<Message> {
        let Some(lyrics) = &self.lyrics else { return Task::none() };
        if self.demo || !self.translation_configured() || self.lyrics_ui.translating {
            return Task::none();
        }
        let s = &self.config.settings.lyrics;
        let (provider, key, target) = (
            s.translation_api_provider.clone(),
            s.translation_api_key.clone(),
            s.translation_target_language.clone(),
        );
        let lines: Vec<String> = lyrics.lines.iter().map(|l| l.text.clone()).collect();
        let http = self.http.clone();
        let id = self.lyrics_song.clone().unwrap_or_default();
        self.lyrics_ui.translating = true;
        Task::perform(
            async move { listen_lyrics::translate::translate(&http, &provider, &key, &target, &lines).await },
            move |r| Message::Lyrics(LyricsMsg::Translated(id.clone(), r)),
        )
    }

    pub(crate) fn on_lyrics(&mut self, msg: LyricsMsg) -> Task<Message> {
        match msg {
            LyricsMsg::Loaded(id, found) => {
                if self.lyrics_song.as_deref() != Some(id.as_str()) {
                    return Task::none();
                }
                self.lyrics_loading = false;
                self.lyrics_ui.candidates = found;
                self.lyrics = None;
                self.show_candidate(0);
                self.lyrics_ui.rev += 1;
                let auto = self.config.settings.lyrics.enable_auto_translation;
                let inline = self.lyrics.as_ref().is_some_and(listen_lyrics::has_translation);
                let mut tasks = vec![self.scroll_lyrics()];
                if auto && !inline {
                    tasks.push(self.translate_lyrics());
                }
                return Task::batch(tasks);
            }
            LyricsMsg::OffsetStep(delta) => {
                let cur = self.lyrics_ui.offset_ms(&self.config.settings.lyrics);
                let key = self.lyrics_ui.key.clone();
                self.lyrics_ui.offsets.insert(key, cur.saturating_add(delta).clamp(-60_000, 60_000));
                self.lyrics_ui.offset_text = None;
            }
            LyricsMsg::OffsetInput(text) => {
                let t = text.trim();
                if let Ok(v) = t.parse::<i32>() {
                    let key = self.lyrics_ui.key.clone();
                    self.lyrics_ui.offsets.insert(key, v.clamp(-60_000, 60_000));
                }
                if t.is_empty() || t == "-" || t.parse::<i32>().is_ok() {
                    self.lyrics_ui.offset_text = Some(text);
                }
            }
            LyricsMsg::Viewport(size) => self.lyrics_ui.viewport = Some(size),
            LyricsMsg::Clear => {
                let Some(song) = self.lyrics_song_now() else { return Task::none() };
                let key = Self::song_key(&song);
                self.lyrics_ui.set_override(&key, None);
                self.lyrics_ui.suppressed.insert(key);
                self.lyrics_song = Some(song.id.clone());
                return self.lookup_lyrics(song);
            }
            LyricsMsg::Refresh => {
                let Some(song) = self.lyrics_song_now() else { return Task::none() };
                self.lyrics_ui.suppressed.remove(&Self::song_key(&song));
                self.lyrics_song = Some(song.id.clone());
                return self.lookup_lyrics(song);
            }
            LyricsMsg::Version(i) => {
                self.show_candidate(i);
                return self.scroll_lyrics();
            }
            LyricsMsg::Translation => {
                let ui = &mut self.lyrics_ui;
                let has = self.lyrics.as_ref().is_some_and(|l| ui.has_translation(l));
                if has {
                    ui.show_translation = !ui.show_translation;
                } else if self.translation_configured() {
                    self.lyrics_ui.show_translation = true;
                    return self.translate_lyrics();
                } else {
                    self.toast_info(
                        "Choose a translation provider and enter its API key under Settings > Playback > Lyrics."
                            .into(),
                    );
                }
            }
            LyricsMsg::Translated(id, result) => {
                self.lyrics_ui.translating = false;
                if self.lyrics_song.as_deref() != Some(id.as_str()) {
                    return Task::none();
                }
                match result {
                    Some(lines) if lines.len() == self.lyrics.as_ref().map_or(0, |l| l.lines.len()) => {
                        self.lyrics_ui.translated = lines;
                        self.lyrics_ui.show_translation = true;
                    }
                    _ => self.toast_error(
                        "The translation did not work. Check the provider and the API key.".into(),
                    ),
                }
            }
            LyricsMsg::SearchOpen => {
                let (name, artist) = self
                    .lyrics_song_now()
                    .map(|s| (s.title.clone(), s.artists.first().map(|a| a.name.clone()).unwrap_or_default()))
                    .unwrap_or_default();
                self.lyrics_ui.search = Some(SearchState { name, artist, ..SearchState::default() });
                self.open_modal(SEARCH_MODAL);
                return self.run_lyrics_search();
            }
            LyricsMsg::SearchClose => {
                self.lyrics_ui.search = None;
                self.lyrics_ui.export = None;
                self.close_modal();
            }
            LyricsMsg::SearchName(v) => {
                if let Some(s) = &mut self.lyrics_ui.search {
                    s.name = v;
                }
            }
            LyricsMsg::SearchArtist(v) => {
                if let Some(s) = &mut self.lyrics_ui.search {
                    s.artist = v;
                }
            }
            LyricsMsg::SearchRun => return self.run_lyrics_search(),
            LyricsMsg::SearchResults(token, hits) => {
                if let Some(s) = self.lyrics_ui.search.as_mut().filter(|s| s.token == token) {
                    s.loading = false;
                    s.searched = true;
                    s.hits = hits;
                    s.selected = None;
                    s.preview = None;
                }
            }
            LyricsMsg::SearchPick(i) => {
                let Some(s) = self.lyrics_ui.search.as_mut() else { return Task::none() };
                let Some(hit) = s.hits.get(i).cloned() else { return Task::none() };
                s.selected = Some(i);
                s.preview = None;
                s.preview_loading = !self.demo;
                if self.demo {
                    return Task::none();
                }
                let token = self.lyrics_ui.next_token();
                if let Some(s) = self.lyrics_ui.search.as_mut() {
                    s.token = token;
                }
                let http = self.http.clone();
                let opts = Options {
                    sources: vec![],
                    netease_translation: self.config.settings.lyrics.enable_netease_translation,
                };
                return Task::perform(async move { fetch::get(&http, &hit, &opts).await }, move |l| {
                    Message::Lyrics(LyricsMsg::Preview(token, l))
                });
            }
            LyricsMsg::Preview(token, lyrics) => {
                if let Some(s) = self.lyrics_ui.search.as_mut().filter(|s| s.token == token) {
                    s.preview_loading = false;
                    s.preview = lyrics;
                }
            }
            LyricsMsg::SearchApply => {
                let Some(s) = self.lyrics_ui.search.take() else { return Task::none() };
                let (Some(hit), Some(song)) =
                    (s.selected.and_then(|i| s.hits.get(i)).cloned(), self.lyrics_song_now())
                else {
                    return Task::none();
                };
                let key = Self::song_key(&song);
                self.lyrics_ui.suppressed.remove(&key);
                self.lyrics_ui.set_override(
                    &key,
                    Some(Override {
                        source: hit.source.name().to_string(),
                        id: hit.id,
                        artist: hit.artist,
                        name: hit.name,
                    }),
                );
                self.close_modal();
                self.lyrics_song = Some(song.id.clone());
                // The preview is already the lyrics: show it right away.
                if let Some(l) = s.preview {
                    self.lyrics_ui.candidates = vec![l];
                    self.show_candidate(0);
                    return self.scroll_lyrics();
                }
                return self.lookup_lyrics(song);
            }
            LyricsMsg::SearchExport => {
                let Some(s) = &self.lyrics_ui.search else { return Task::none() };
                let (Some(l), Some(hit)) = (s.preview.clone(), s.selected.and_then(|i| s.hits.get(i))) else {
                    return Task::none();
                };
                let offset_ms = self.config.settings.lyrics.delay_ms;
                self.lyrics_ui.export = Some(ExportState {
                    synced: l.synced,
                    lyrics: l,
                    name: hit.name.clone(),
                    artist: hit.artist.clone(),
                    offset_ms,
                    offset_text: offset_ms.to_string(),
                });
                self.open_modal(EXPORT_MODAL);
            }
            LyricsMsg::ExportOpen => {
                let (Some(l), Some(song)) = (self.lyrics.clone(), self.lyrics_song_now()) else {
                    return Task::none();
                };
                let offset_ms = self.lyrics_ui.offset_ms(&self.config.settings.lyrics);
                self.lyrics_ui.export = Some(ExportState {
                    synced: l.synced,
                    lyrics: l,
                    name: song.title.clone(),
                    artist: song.artists.first().map(|a| a.name.clone()).unwrap_or_default(),
                    offset_ms,
                    offset_text: offset_ms.to_string(),
                });
                self.open_modal(EXPORT_MODAL);
            }
            LyricsMsg::ExportSynced(v) => {
                if let Some(e) = &mut self.lyrics_ui.export {
                    e.synced = v;
                }
            }
            LyricsMsg::ExportOffset(text) => {
                if let Some(e) = &mut self.lyrics_ui.export {
                    if let Ok(v) = text.trim().parse::<i32>() {
                        e.offset_ms = v;
                    }
                    e.offset_text = text;
                }
            }
            LyricsMsg::ExportSave => {
                let Some(e) = self.lyrics_ui.export.take() else { return Task::none() };
                let dir = if self.demo { std::env::temp_dir() } else { export_dir() };
                match write_export(&dir, &e.file_name(), &e.text()) {
                    Ok(path) => self.toast_success(format!("Saved {}", path.display())),
                    Err(err) => self.toast_error(format!("Could not save the lyrics: {err}")),
                }
                // Back to the search dialog when the export came from there.
                if self.lyrics_ui.search.is_some() {
                    self.open_modal(SEARCH_MODAL);
                } else {
                    self.close_modal();
                }
            }
            LyricsMsg::SettingsOpen => self.open_modal(SETTINGS_MODAL),
        }
        Task::none()
    }

    fn run_lyrics_search(&mut self) -> Task<Message> {
        let token = self.lyrics_ui.next_token();
        let Some(s) = self.lyrics_ui.search.as_mut() else { return Task::none() };
        s.token = token;
        s.hits.clear();
        s.selected = None;
        s.preview = None;
        s.loading = !self.demo;
        s.searched = self.demo;
        if self.demo {
            return Task::none();
        }
        let q = Query {
            name: s.name.trim().to_string(),
            artist: s.artist.trim().to_string(),
            ..Query::default()
        };
        let sources = Source::parse_list(&self.config.settings.lyrics.sources);
        let http = self.http.clone();
        Task::perform(async move { fetch::search_all(&http, &q, &sources).await }, move |hits| {
            Message::Lyrics(LyricsMsg::SearchResults(token, hits))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_toggle_and_order() {
        let mut s = LyricsSettings::default();
        assert_eq!(s.sources, ["NetEase", "lrclib.net"]);
        LyricsSetting::Source("Genius".into(), true).apply(&mut s);
        assert_eq!(s.sources, ["NetEase", "lrclib.net", "Genius"]);
        LyricsSetting::MoveSource("Genius".into(), -1).apply(&mut s);
        assert_eq!(s.sources, ["NetEase", "Genius", "lrclib.net"]);
        LyricsSetting::MoveSource("NetEase".into(), -1).apply(&mut s);
        assert_eq!(s.sources, ["NetEase", "Genius", "lrclib.net"]);
        LyricsSetting::MoveSource("lrclib.net".into(), 1).apply(&mut s);
        assert_eq!(s.sources, ["NetEase", "Genius", "lrclib.net"]);
        LyricsSetting::Source("NetEase".into(), false).apply(&mut s);
        assert_eq!(s.sources, ["Genius", "lrclib.net"]);
        LyricsSetting::Source("Nonsense".into(), true).apply(&mut s);
        assert_eq!(s.sources, ["Genius", "lrclib.net"]);
        // Turning an enabled one on again does not duplicate it.
        LyricsSetting::Source("Genius".into(), true).apply(&mut s);
        assert_eq!(s.sources, ["lrclib.net", "Genius"]);
    }

    #[test]
    fn display_settings_are_clamped() {
        let mut s = LyricsSettings::default();
        LyricsSetting::FontSize(200.0).apply(&mut s);
        LyricsSetting::Gap(-5.0).apply(&mut s);
        LyricsSetting::Opacity(0.456).apply(&mut s);
        LyricsSetting::Scale(0.1).apply(&mut s);
        LyricsSetting::FollowAlignment(99).apply(&mut s);
        LyricsSetting::LeadMs(99_999).apply(&mut s);
        assert_eq!((s.font_size, s.gap, s.opacity_non_active, s.scale_non_active), (72.0, 0.0, 0.46, 0.5));
        assert_eq!((s.follow_scroll_alignment, s.line_lead_time_ms), (50, 3000));
    }

    #[test]
    fn overrides_persist_per_song() {
        let dir = std::env::temp_dir().join(format!("listen-lyrics-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut ui = LyricsUi::new(Some(&dir));
        let o = Override { source: "NetEase".into(), id: "7".into(), artist: "A".into(), name: "T".into() };
        ui.set_override("srv:1", Some(o.clone()));
        let again = LyricsUi::new(Some(&dir));
        assert_eq!(again.override_for("srv:1"), Some(&o));
        assert_eq!(again.override_for("srv:2"), None);
        assert_eq!(o.hit().map(|h| h.source), Some(Source::NetEase));
        let mut ui = again;
        ui.set_override("srv:1", None);
        assert_eq!(LyricsUi::new(Some(&dir)).override_for("srv:1"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exports_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("listen-lyrics-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = write_export(&dir, "A - T.lrc", "one").unwrap();
        let b = write_export(&dir, "A - T.lrc", "two").unwrap();
        assert_eq!(a.file_name().unwrap(), "A - T.lrc");
        assert_eq!(b.file_name().unwrap(), "A - T (2).lrc");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offsets_default_to_the_setting() {
        let mut ui = LyricsUi::new(None);
        let mut s = LyricsSettings { delay_ms: 120, ..LyricsSettings::default() };
        ui.key = "a:1".into();
        assert_eq!(ui.offset_ms(&s), 120);
        ui.offsets.insert("a:1".into(), -50);
        assert_eq!(ui.offset_ms(&s), -50);
        ui.key = "a:2".into();
        assert_eq!(ui.offset_ms(&s), 120);
        s.delay_ms = 0;
        assert_eq!(ui.offset_ms(&s), 0);
    }
}
