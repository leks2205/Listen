//! Playlists: the Playlists page, the playlist page, the create / edit dialogs and the "Add to playlist"
//! picker. Behaviour follows Feishin (`features/playlists/*`); writes have a YouTube Music version
//! where its capabilities allow.
//!
//! | file | what |
//! | --- | --- |
//! | `mod.rs` | state, messages, the page's rows and actions |
//! | `rows.rs` | search, filters, sort and the albums view (pure) |
//! | `edit.rs` | edit mode (drag reorder, save and replace) and removing songs |
//! | `forms.rs` | the create / edit dialog |
//! | `picker.rs` | the "Add to playlist" page of the context menu |
//!
//! The UI is `ui/playlists.rs` (the list page) and `ui/playlist_page/`.

mod edit;
mod forms;
pub mod picker;
pub mod rows;

pub use edit::Edit;
pub use forms::{Form, FormKind, PendingImage};

use super::album_page::{PagePlay, TrackSort};
use super::list::{ListData, ListId};
use super::menu::{MenuAction, MenuMsg, MenuTarget};
use super::*;
use listen_core::settings::PlaylistTarget;
use listen_source::Capabilities;
use rand::seq::SliceRandom;
use rows::{FilterKind, SongFilters};
use std::borrow::Cow;
use std::collections::HashMap;

/// What the Playlists page can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaylistSort {
    #[default]
    Name,
    SongCount,
    Duration,
    Owner,
    Visibility,
}

impl PlaylistSort {
    pub const ALL: [PlaylistSort; 5] = [
        PlaylistSort::Name,
        PlaylistSort::SongCount,
        PlaylistSort::Duration,
        PlaylistSort::Owner,
        PlaylistSort::Visibility,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlaylistSort::Name => "Name",
            PlaylistSort::SongCount => "Song count",
            PlaylistSort::Duration => "Duration",
            PlaylistSort::Owner => "Owner",
            PlaylistSort::Visibility => "Visibility",
        }
    }

    /// The order a sort starts in when picked.
    pub fn default_desc(self) -> bool {
        matches!(self, PlaylistSort::SongCount | PlaylistSort::Duration)
    }
}

impl std::fmt::Display for PlaylistSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// `playlists` matching every word of `search` in their name, owner or description.
pub fn filter_playlists(playlists: &[Playlist], search: &str) -> Vec<Playlist> {
    let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
    playlists
        .iter()
        .filter(|p| {
            let hay = format!(
                "{} {} {}",
                p.name,
                p.owner.as_deref().unwrap_or_default(),
                p.description.as_deref().unwrap_or_default()
            )
            .to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .cloned()
        .collect()
}

/// `list` in `sort` order (ties by name).
pub fn sort_playlists(list: &mut [Playlist], sort: PlaylistSort, desc: bool) {
    list.sort_by_cached_key(|p| p.name.to_lowercase());
    match sort {
        PlaylistSort::Name => {}
        PlaylistSort::SongCount => list.sort_by_key(|p| p.song_count),
        PlaylistSort::Duration => list.sort_by_key(|p| p.duration),
        PlaylistSort::Owner => {
            list.sort_by_cached_key(|p| p.owner.as_deref().unwrap_or_default().to_lowercase())
        }
        PlaylistSort::Visibility => list.sort_by_key(|p| p.public),
    }
    if desc {
        list.reverse();
    }
}

/// A change of the playlist filters.
#[derive(Debug, Clone)]
pub enum FilterEdit {
    Favorite(Option<bool>),
    Rated(Option<bool>),
    /// Toggle a value of a list filter.
    Toggle(FilterKind, String),
    /// Match all (true) or any of the values.
    MatchAll(FilterKind, bool),
    YearMin(String),
    YearMax(String),
}

#[derive(Debug, Clone)]
pub enum PlaylistPageMsg {
    // ---- the playlist page
    Search(String),
    Sort(TrackSort),
    ToggleOrder,
    Filter(FilterEdit),
    ResetFilters,
    OpenFilters,
    Target(PlaylistTarget),
    Play(PagePlay),
    TogglePlayMenu,
    /// The "..." button at a point.
    More(iced::Point),
    ToggleComment,
    ToggleCollapsed,
    Refresh,
    // ---- edit mode and removing
    EditStart,
    EditCancel,
    /// "Save and replace": asks first.
    EditSave,
    EditSaveConfirmed,
    EditSaved(String, Result<(), String>),
    /// Ask before removing the entries at these positions of the playlist.
    Remove(String, Vec<usize>),
    RemoveConfirmed(String, Vec<usize>),
    Removed(String, Vec<usize>, Result<(), String>),
    // ---- the Playlists page
    ListSearch(String),
    ListSort(PlaylistSort),
    ListOrder,
    ListRefresh,
    ListPlay,
    // ---- the create / edit dialog
    /// Opens the create dialog (with songs to start the playlist with).
    Create(Vec<Song>),
    EditPlaylist(Playlist),
    FormName(String),
    FormComment(String),
    FormPublic(bool),
    FormPickImage,
    FormImagePicked(Option<PendingImage>),
    FormRemoveImage,
    FormSubmit,
    FormDone(Result<forms::FormDone, String>),
    // ---- the cover on the playlist page's header
    CoverPick,
    CoverPicked(Option<PendingImage>),
    CoverDelete,
    CoverDone(Result<String, String>),
    // ---- the picker
    PickerSearch(String),
    SkipDuplicates(bool),
    /// Songs were added to a playlist (server, id of a playlist that was created, notice).
    Added(ServerId, Option<String>, Result<String, String>),
}

/// State of the Playlists page and the playlist page.
#[derive(Debug, Default)]
pub struct PlaylistPage {
    /// The playlist the state below belongs to.
    pub id: Option<String>,
    pub search: String,
    pub sort: TrackSort,
    pub desc: bool,
    seed: u64,
    pub filters: SongFilters,
    pub comment_open: bool,
    pub play_menu: bool,
    pub collapsed: bool,
    /// The songs as listed: searched, filtered and sorted.
    pub rows: Vec<Song>,
    /// The position in the playlist of each row.
    pub pos: Vec<usize>,
    /// The albums of the songs (the Albums view).
    pub albums: Vec<Album>,
    pub edit: Option<Edit>,
    // ---- the Playlists page
    pub list_search: String,
    pub list_sort: PlaylistSort,
    pub list_desc: bool,
    /// The playlists as listed.
    pub list: Vec<Playlist>,
    // ---- dialog and picker
    pub form: Option<Form>,
    /// The playlist songs were last added to, per server (pinned in the picker).
    pub recent: HashMap<ServerId, String>,
}

impl PlaylistPage {
    /// Whether rows are searched, filtered or sorted (the playlist's own order is gone).
    pub fn narrowed(&self) -> bool {
        !self.search.trim().is_empty() || self.filters.active() || self.sort != TrackSort::Id || self.desc
    }
}

impl Listen {
    /// The songs the playlist list shows (edit mode shows the order being edited).
    pub fn playlist_rows(&self, id: &str) -> &[Song] {
        if self.playlist_page.id.as_deref() != Some(id) {
            return &[];
        }
        match &self.playlist_page.edit {
            Some(edit) => &edit.rows,
            None => &self.playlist_page.rows,
        }
    }

    /// The list the playlist page shows: its songs, or their albums.
    pub fn playlist_list_id(&self, id: &str) -> ListId {
        match self.config.settings.lists.playlist_target {
            PlaylistTarget::Tracks => ListId::Playlist(id.to_string()),
            PlaylistTarget::Albums => ListId::PlaylistAlbums(id.to_string()),
        }
    }

    /// Data of the playlist lists (`Listen::list_data`).
    pub(super) fn playlist_list_data(&self, id: &ListId) -> Option<ListData<'_>> {
        Some(match id {
            ListId::Playlists => ListData::Playlists(Cow::Borrowed(&self.playlist_page.list)),
            ListId::Playlist(p) => ListData::Songs(Cow::Borrowed(self.playlist_rows(p))),
            ListId::PlaylistAlbums(p) if self.playlist_page.id.as_deref() == Some(p.as_str()) => {
                ListData::Albums(Cow::Borrowed(&self.playlist_page.albums))
            }
            ListId::PlaylistAlbums(_) => ListData::Albums(Cow::Borrowed(&[])),
            _ => return None,
        })
    }

    /// Covers for the visible rows of the playlist lists.
    pub(super) fn playlist_visible(&mut self, id: &ListId) -> Task<Message> {
        let refs: Vec<ImageRef> = match self.playlist_list_data(id) {
            Some(ListData::Playlists(p)) => {
                let (a, b) = self.list_range(id, p.len());
                p[a.min(p.len())..b.min(p.len())].iter().filter_map(|x| x.image.clone()).collect()
            }
            Some(ListData::Songs(s)) => {
                let (a, b) = self.list_range(id, s.len());
                s[a.min(s.len())..b.min(s.len())].iter().filter_map(|x| x.image.clone()).collect()
            }
            Some(ListData::Albums(s)) => {
                let (a, b) = self.list_range(id, s.len());
                s[a.min(s.len())..b.min(s.len())].iter().filter_map(|x| x.image.clone()).collect()
            }
            _ => Vec::new(),
        };
        self.request_images(refs.iter().map(Some))
    }

    /// The source of a playlist's server (else the one being browsed).
    pub fn playlist_source(&self, server: &ServerId) -> Option<Arc<dyn MusicSource>> {
        self.sources.get(server).cloned().or_else(|| self.source.clone())
    }

    /// What a playlist's server can do. Demo playlists behave like their source kind.
    pub fn playlist_caps(&self, p: &Playlist) -> Capabilities {
        if let Some(source) = self.sources.get(&p.server_id) {
            return source.capabilities();
        }
        if let Some(source) = self.source.as_ref().filter(|_| !self.demo) {
            return source.capabilities();
        }
        if self.demo {
            return match p.source {
                SourceKind::YouTubeMusic => {
                    Capabilities::PLAYLIST_EDIT
                        | Capabilities::PLAYLIST_RENAME
                        | Capabilities::PLAYLIST_REMOVE
                        | Capabilities::PLAYLIST_REORDER
                        | Capabilities::PLAYLIST_UPDATE
                }
                _ => {
                    Capabilities::PLAYLIST_EDIT
                        | Capabilities::PLAYLIST_RENAME
                        | Capabilities::PLAYLIST_REMOVE
                        | Capabilities::PLAYLIST_REORDER
                        | Capabilities::PLAYLIST_UPDATE
                        | Capabilities::PLAYLIST_IMAGE
                }
            };
        }
        Capabilities::empty()
    }

    /// The browsed server can create playlists (the Create playlist buttons).
    pub fn can_create_playlist(&self) -> bool {
        self.demo
            || self.source.as_ref().is_some_and(|s| s.capabilities().contains(Capabilities::PLAYLIST_CREATE))
    }

    /// The open playlist.
    pub fn open_playlist_detail(&self) -> Option<&PlaylistDetail> {
        self.library
            .playlist
            .as_ref()
            .filter(|d| self.playlist_page.id.as_deref() == Some(d.playlist.id.as_str()))
    }

    // ---- rows ------------------------------------------------------------------

    /// Builds the rows again from the loaded playlist (after a load, a search, a filter or a sort).
    pub(super) fn playlist_rows_sync(&mut self) {
        let Some(detail) = self.library.playlist.as_ref() else {
            self.playlist_page.rows.clear();
            self.playlist_page.pos.clear();
            self.playlist_page.albums.clear();
            return;
        };
        let page = &self.playlist_page;
        let (rows, pos) =
            rows::visible(&detail.songs, &page.search, &page.filters, page.sort, page.desc, page.seed);
        let albums = rows::albums_of(&rows);
        let id = detail.playlist.id.clone();
        self.playlist_page.rows = rows;
        self.playlist_page.pos = pos;
        self.playlist_page.albums = albums;
        let total = self.playlist_list_data(&self.playlist_list_id(&id)).map_or(0, |d| d.len());
        self.lists.entry(&self.playlist_list_id(&id)).selection.retain_below(total);
    }

    /// The rows changed (search, filter, sort): the selection and page start over.
    fn playlist_rows_changed(&mut self) -> Task<Message> {
        self.playlist_rows_sync();
        if let Some(id) = self.playlist_page.id.clone() {
            self.lists.reset(&ListId::Playlist(id.clone()));
            self.lists.reset(&ListId::PlaylistAlbums(id));
        }
        self.playlist_list_visible()
    }

    pub(in crate::app) fn playlist_list_visible(&mut self) -> Task<Message> {
        match self.playlist_page.id.clone() {
            Some(id) => {
                let list = self.playlist_list_id(&id);
                self.playlist_visible(&list)
            }
            None => Task::none(),
        }
    }

    /// The playlist (or a page of it) was loaded.
    pub(super) fn playlist_page_loaded(&mut self) -> Task<Message> {
        let Some(detail) = self.library.playlist.as_ref() else { return Task::none() };
        let id = detail.playlist.id.clone();
        let mut tasks = Vec::new();
        if self.playlist_page.id.as_deref() != Some(id.as_str()) {
            let page = std::mem::take(&mut self.playlist_page);
            self.playlist_page = PlaylistPage {
                id: Some(id.clone()),
                recent: page.recent,
                list: page.list,
                list_search: page.list_search,
                list_sort: page.list_sort,
                list_desc: page.list_desc,
                ..PlaylistPage::default()
            };
            for list in [ListId::Playlist(id.clone()), ListId::PlaylistAlbums(id)] {
                self.lists.reset(&list);
                self.lists.entry(&list).scroll.offset = 0.0;
                tasks.push(iced::widget::operation::scroll_to(
                    list.scroll_id(),
                    iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
                ));
            }
        }
        self.playlist_rows_sync();
        tasks.push(self.playlist_list_visible());
        Task::batch(tasks)
    }

    // ---- the Playlists page ----------------------------------------------------

    /// Builds the rows of the Playlists page again.
    pub(super) fn playlists_list_sync(&mut self) {
        let page = &self.playlist_page;
        let mut list = filter_playlists(&self.library.playlists, &page.list_search);
        sort_playlists(&mut list, page.list_sort, page.list_desc);
        self.playlist_page.list = list;
        let total = self.playlist_page.list.len();
        self.lists.entry(&ListId::Playlists).selection.retain_below(total);
    }

    /// The Playlists page is shown.
    pub(super) fn playlists_enter(&mut self) -> Task<Message> {
        self.playlists_list_sync();
        Task::batch([self.load_playlists(), self.playlist_visible(&ListId::Playlists)])
    }

    /// Plays (per the Play button setting) the playlists the page lists.
    fn play_listed_playlists(&mut self) -> Task<Message> {
        const MAX: usize = 50;
        let shown = self.playlist_page.list.clone();
        if shown.is_empty() {
            return Task::none();
        }
        if shown.len() > MAX {
            self.toast_warning(format!(
                "That is {} playlists. Search or select fewer than {MAX}.",
                shown.len()
            ));
            return Task::none();
        }
        let picks = shown.into_iter().map(list::Picked::Playlist).collect();
        let behavior = self.play_behavior();
        self.play_items(picks, behavior)
    }

    // ---- the playlist page -----------------------------------------------------

    /// Plays or queues the playlist per the header buttons.
    fn playlist_play(&mut self, kind: PagePlay) -> Task<Message> {
        self.playlist_page.play_menu = false;
        let mut songs = self.playlist_rows(&self.playlist_page.id.clone().unwrap_or_default()).to_vec();
        if songs.is_empty() {
            return Task::none();
        }
        let (action, shuffled) = match kind {
            PagePlay::Now => (MenuAction::Play, false),
            PagePlay::Shuffle => (MenuAction::Shuffle, false),
            PagePlay::Next => (MenuAction::PlayNext, false),
            PagePlay::Last => (MenuAction::AddToQueue, false),
            PagePlay::NextShuffled => (MenuAction::PlayNext, true),
            PagePlay::LastShuffled => (MenuAction::AddToQueue, true),
        };
        if shuffled {
            songs.shuffle(&mut rand::rng());
        }
        self.apply_to_songs(action, songs)
    }

    fn apply_filter(&mut self, edit: FilterEdit) {
        let f = &mut self.playlist_page.filters;
        let toggle = |list: &mut Vec<String>, value: String| match list.iter().position(|x| *x == value) {
            Some(i) => {
                list.remove(i);
            }
            None => list.push(value),
        };
        match edit {
            FilterEdit::Favorite(v) => f.favorite = v,
            FilterEdit::Rated(v) => f.rated = v,
            FilterEdit::Toggle(FilterKind::Genres, v) => toggle(&mut f.genres, v),
            FilterEdit::Toggle(FilterKind::Artists, v) => toggle(&mut f.artists, v),
            FilterEdit::Toggle(FilterKind::AlbumArtists, v) => toggle(&mut f.album_artists, v),
            FilterEdit::MatchAll(FilterKind::Genres, all) => f.genres_all = all,
            FilterEdit::MatchAll(FilterKind::Artists, all) => f.artists_all = all,
            FilterEdit::MatchAll(FilterKind::AlbumArtists, all) => f.album_artists_all = all,
            FilterEdit::YearMin(t) => f.year_min = t.trim().parse().ok(),
            FilterEdit::YearMax(t) => f.year_max = t.trim().parse().ok(),
        }
    }

    pub(super) fn on_playlist_page(&mut self, msg: PlaylistPageMsg) -> Task<Message> {
        use PlaylistPageMsg as P;
        match msg {
            P::Search(text) => {
                self.playlist_page.search = text;
                return self.playlist_rows_changed();
            }
            P::Sort(sort) => {
                self.playlist_page.sort = sort;
                self.playlist_page.desc = sort.default_desc();
                self.playlist_page.seed = rand::random();
                return self.playlist_rows_changed();
            }
            P::ToggleOrder => {
                self.playlist_page.desc = !self.playlist_page.desc;
                return self.playlist_rows_changed();
            }
            P::Filter(edit) => {
                self.apply_filter(edit);
                return self.playlist_rows_changed();
            }
            P::ResetFilters => {
                self.playlist_page.filters = SongFilters::default();
                return self.playlist_rows_changed();
            }
            P::OpenFilters => self.open_modal(FILTERS_MODAL),
            P::Target(target) => {
                self.config.settings.lists.playlist_target = target;
                self.save_config();
                if let Some(id) = self.playlist_page.id.clone() {
                    self.lists.reset(&self.playlist_list_id(&id));
                }
                return self.playlist_list_visible();
            }
            P::Play(kind) => return self.playlist_play(kind),
            P::TogglePlayMenu => self.playlist_page.play_menu = !self.playlist_page.play_menu,
            P::More(at) => {
                self.playlist_page.play_menu = false;
                if let Some(d) = self.open_playlist_detail() {
                    let target = MenuTarget::playlist(&d.playlist);
                    return self.on_menu(MenuMsg::OpenAt(target, at));
                }
            }
            P::ToggleComment => self.playlist_page.comment_open = !self.playlist_page.comment_open,
            P::ToggleCollapsed => self.playlist_page.collapsed = !self.playlist_page.collapsed,
            P::Refresh => {
                if let Some(id) = self.playlist_page.id.clone() {
                    return self.open_playlist(id);
                }
            }
            P::ListSearch(text) => {
                self.playlist_page.list_search = text;
                self.lists.reset(&ListId::Playlists);
                self.playlists_list_sync();
                return self.playlist_visible(&ListId::Playlists);
            }
            P::ListSort(sort) => {
                self.playlist_page.list_sort = sort;
                self.playlist_page.list_desc = sort.default_desc();
                self.lists.reset(&ListId::Playlists);
                self.playlists_list_sync();
                return self.playlist_visible(&ListId::Playlists);
            }
            P::ListOrder => {
                self.playlist_page.list_desc = !self.playlist_page.list_desc;
                self.lists.reset(&ListId::Playlists);
                self.playlists_list_sync();
                return self.playlist_visible(&ListId::Playlists);
            }
            P::ListRefresh => return self.load_playlists(),
            P::ListPlay => return self.play_listed_playlists(),
            P::PickerSearch(text) => {
                if let Some(menu::ContextMenu { page: menu::MenuPage::Playlists { search, .. }, .. }) =
                    &mut self.menu
                {
                    *search = text;
                }
            }
            P::Added(server, created, result) => match result {
                Ok(text) => {
                    self.toast_success(text);
                    if let Some(id) = created {
                        self.playlist_page.recent.insert(server, id);
                    }
                }
                Err(e) => self.toast_error(e),
            },
            P::SkipDuplicates(on) => {
                self.config.settings.lists.skip_duplicates = on;
                self.save_config();
            }
            other => return self.on_playlist_edit(other),
        }
        Task::none()
    }
}

/// The key of the filters modal (`ui::overlay::custom_modal`).
pub const FILTERS_MODAL: &str = "playlist-filters";
/// The key of the create / edit dialog.
pub const FORM_MODAL: &str = "playlist-form";

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist(name: &str, owner: &str, count: u32, secs: u64, public: bool) -> Playlist {
        Playlist {
            id: name.into(),
            server_id: ServerId("t".into()),
            source: SourceKind::Subsonic,
            name: name.into(),
            description: None,
            owner: Some(owner.into()),
            image: None,
            song_count: Some(count),
            duration: Duration::from_secs(secs),
            public,
            editable: true,
        }
    }

    fn names(l: &[Playlist]) -> Vec<&str> {
        l.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn playlists_sort_and_search() {
        let mut l = vec![
            playlist("b road", "zed", 10, 600, true),
            playlist("A gym", "amy", 30, 100, false),
            playlist("c chill", "amy", 10, 900, false),
        ];
        sort_playlists(&mut l, PlaylistSort::Name, false);
        assert_eq!(names(&l), ["A gym", "b road", "c chill"]);
        sort_playlists(&mut l, PlaylistSort::SongCount, true);
        assert_eq!(names(&l), ["A gym", "c chill", "b road"]);
        sort_playlists(&mut l, PlaylistSort::Duration, true);
        assert_eq!(names(&l), ["c chill", "b road", "A gym"]);
        sort_playlists(&mut l, PlaylistSort::Owner, false);
        assert_eq!(names(&l), ["A gym", "c chill", "b road"]);
        sort_playlists(&mut l, PlaylistSort::Visibility, true);
        assert_eq!(l[0].name, "b road");
        assert_eq!(filter_playlists(&l, "AMY").len(), 2);
        assert_eq!(names(&filter_playlists(&l, "amy chill")), ["c chill"]);
        assert_eq!(filter_playlists(&l, "").len(), 3);
    }
}
