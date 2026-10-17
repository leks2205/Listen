//! State and messages of the item lists (`ui::list`): selection, scroll, pagination,
//! drag and drop, table configuration and play actions, per page keyed by [`ListId`].
//! Adoption steps for a page are in [`crate::ui::list`].

use super::artist_page::ArtistView;
use super::menu::{MenuAction, MenuMsg, MenuTarget};
use super::*;
use crate::ui::list::selection::{Mods, Move, Selection};
use crate::ui::list::{self, columns::ListItem};
use iced::widget::scrollable::AbsoluteOffset;
use listen_core::settings::{
    AlbumGroupItem, Align, CardSize, ColumnId, DisplayType, Gap, ListKind, Pagination, Pin, PlayBehavior,
    RowSize,
};
use listen_source::{Capabilities, ItemKind};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// The server of rows that belong to the one shown (genres).
static NO_SERVER: ServerId = ServerId(String::new());

/// Tabs of the search page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SearchTab {
    #[default]
    Songs,
    Albums,
    Artists,
    Playlists,
}

/// Tabs of the favourites page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FavoritesTab {
    Songs,
    Albums,
    Artists,
}

/// The three scrollables that scroll the unpinned columns together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HPart {
    Header,
    Body,
    Bar,
}

/// Which list. Selection, scroll position and the page of every list are
/// kept under its id; the look (columns, row size, ...) is shared by all lists
/// of the same [`ListKind`] (`ListId::kind`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ListId {
    Albums,
    Songs,
    Artists,
    AlbumArtists,
    Genres,
    Playlists,
    /// The songs of one playlist.
    Playlist(String),
    /// The songs of one playlist grouped into albums.
    PlaylistAlbums(String),
    /// The tracks of one album.
    Album(String),
    /// The albums of a genre.
    Genre(String),
    /// The songs of a genre.
    GenreSongs(String),
    /// The song list of an artist's sub-page (all tracks, top or favorite songs).
    Artist(String),
    /// The albums of an artist.
    ArtistAlbums(String),
    /// The queue on the Now Playing page.
    Queue,
    /// The queue in the side panel, the drawer and the full-screen player.
    SideQueue,
    Search(SearchTab),
    Favorites(FavoritesTab),
    /// Recently played.
    Recent,
    /// The songs of the open folder.
    Folder,
}

impl ListId {
    /// One of the queue lists (the page or the panel).
    pub fn is_queue(&self) -> bool {
        matches!(self, ListId::Queue | ListId::SideQueue)
    }

    /// The settings group of this list.
    pub fn kind(&self) -> ListKind {
        match self {
            ListId::Albums | ListId::Genre(_) | ListId::ArtistAlbums(_) | ListId::PlaylistAlbums(_) => {
                ListKind::Albums
            }
            ListId::Songs => ListKind::Songs,
            ListId::Artists => ListKind::Artists,
            ListId::AlbumArtists => ListKind::AlbumArtists,
            ListId::Genres => ListKind::Genres,
            ListId::Playlists => ListKind::Playlists,
            ListId::Playlist(_) => ListKind::PlaylistSongs,
            ListId::Album(_) => ListKind::AlbumSongs,
            ListId::GenreSongs(_) => ListKind::GenreSongs,
            ListId::Artist(_) => ListKind::ArtistSongs,
            ListId::Queue => ListKind::Queue,
            ListId::SideQueue => ListKind::SideQueue,
            ListId::Search(SearchTab::Songs) => ListKind::SearchSongs,
            ListId::Search(SearchTab::Albums) => ListKind::Albums,
            ListId::Search(SearchTab::Artists) => ListKind::Artists,
            ListId::Search(SearchTab::Playlists) => ListKind::Playlists,
            ListId::Favorites(FavoritesTab::Songs) => ListKind::FavoriteSongs,
            ListId::Favorites(FavoritesTab::Albums) => ListKind::Albums,
            ListId::Favorites(FavoritesTab::Artists) => ListKind::Artists,
            ListId::Recent => ListKind::History,
            ListId::Folder => ListKind::FolderSongs,
        }
    }

    /// The scrollable widget of this list.
    pub fn scroll_id(&self) -> iced::widget::Id {
        iced::widget::Id::from(format!("list-{self:?}"))
    }

    /// One of the horizontal scrollables of a table with pinned columns.
    pub fn hscroll_id(&self, part: HPart) -> iced::widget::Id {
        iced::widget::Id::from(format!("list-{self:?}-h-{part:?}"))
    }

    /// The list a route shows, if its page uses the list component. **Pages add
    /// their route here.**
    pub fn of_route(route: &Route) -> Option<ListId> {
        match route {
            Route::Tracks => Some(ListId::Songs),
            Route::Albums => Some(ListId::Albums),
            Route::Album(id) => Some(ListId::Album(id.clone())),
            Route::AlbumArtists => Some(ListId::AlbumArtists),
            Route::Artists => Some(ListId::Artists),
            Route::ArtistList(id, ArtistView::Discography) => Some(ListId::ArtistAlbums(id.clone())),
            Route::ArtistList(id, _) => Some(ListId::Artist(id.clone())),
            Route::NowPlaying => Some(ListId::Queue),
            _ => None,
        }
    }
}

/// The rows of a list as the actions see them. **Pages add their list to
/// [`Listen::list_data`].** Owned rows (`Cow::Owned`) work too, for lists
/// that are built on the fly (the queue).
#[derive(Debug, Clone)]
pub enum ListData<'a> {
    Songs(Cow<'a, [Song]>),
    Albums(Cow<'a, [Album]>),
    Artists(Cow<'a, [Artist]>),
    Playlists(Cow<'a, [Playlist]>),
    Genres(Cow<'a, [listen_core::model::Genre]>),
}

/// A row taken out of a list (what a drag carries, what an action acts on).
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Picked {
    Song(Song),
    Album(Album),
    Artist(Artist),
    Playlist(Playlist),
    Genre(listen_core::model::Genre),
}

impl Picked {
    pub fn server(&self) -> &ServerId {
        match self {
            Picked::Song(s) => &s.server_id,
            Picked::Album(a) => &a.server_id,
            Picked::Artist(a) => &a.server_id,
            Picked::Playlist(p) => &p.server_id,
            // Genres belong to the server shown.
            Picked::Genre(_) => &NO_SERVER,
        }
    }

    /// The page the item opens (double-click on albums, artists, playlists).
    pub fn route(&self) -> Option<Route> {
        match self {
            Picked::Song(_) => None,
            Picked::Album(a) => Some(Route::Album(a.id.clone())),
            Picked::Artist(a) => Some(Route::Artist(a.id.clone())),
            Picked::Playlist(p) => Some(Route::Playlist(p.id.clone())),
            Picked::Genre(g) => Some(Route::Genre(g.name.clone())),
        }
    }

    /// The context-menu target of this one row.
    pub fn menu_target(&self) -> Option<MenuTarget> {
        MenuTarget::from_picks(std::slice::from_ref(self))
    }
}

impl ListData<'_> {
    pub fn len(&self) -> usize {
        match self {
            ListData::Songs(v) => v.len(),
            ListData::Albums(v) => v.len(),
            ListData::Artists(v) => v.len(),
            ListData::Playlists(v) => v.len(),
            ListData::Genres(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<Picked> {
        match self {
            ListData::Songs(v) => v.get(index).cloned().map(Picked::Song),
            ListData::Albums(v) => v.get(index).cloned().map(Picked::Album),
            ListData::Artists(v) => v.get(index).cloned().map(Picked::Artist),
            ListData::Playlists(v) => v.get(index).cloned().map(Picked::Playlist),
            ListData::Genres(v) => v.get(index).cloned().map(Picked::Genre),
        }
    }

    /// The rows at `indices` (ascending order kept).
    pub fn pick(&self, indices: &[usize]) -> Vec<Picked> {
        indices.iter().filter_map(|i| self.get(*i)).collect()
    }

    pub fn item_type(&self) -> listen_core::settings::ItemType {
        use listen_core::settings::ItemType;
        match self {
            ListData::Songs(_) => ItemType::Song,
            ListData::Albums(_) => ItemType::Album,
            ListData::Artists(_) => ItemType::Artist,
            ListData::Playlists(_) => ItemType::Playlist,
            ListData::Genres(_) => ItemType::Genre,
        }
    }

    /// Plain text of a column of row `index` (for fitting a column to its content).
    pub fn text(&self, index: usize, col: ColumnId) -> Option<String> {
        match self {
            ListData::Songs(v) => v.get(index)?.text(col, index),
            ListData::Albums(v) => v.get(index)?.text(col, index),
            ListData::Artists(v) => v.get(index)?.text(col, index),
            ListData::Playlists(v) => v.get(index)?.text(col, index),
            ListData::Genres(v) => v.get(index)?.text(col, index),
        }
    }
}

/// Where dragged rows can be dropped (see `ui::list::drop_target`).
#[derive(Debug, Clone, PartialEq)]
pub enum DropTarget {
    /// The end of the play queue.
    Queue,
    /// Before row `n` of the queue as it is shown (`len` = the end): dragged
    /// queue rows move there, anything else is inserted there.
    QueueAt(usize),
    /// The empty queue: what is dropped plays right away.
    QueueEmpty,
    /// A playlist of a server.
    Playlist { server: ServerId, id: String, name: String },
}

/// What a drag from a list carries.
#[derive(Debug, Clone, PartialEq)]
pub struct DragPayload {
    pub from: ListId,
    pub items: Vec<Picked>,
}

impl DragPayload {
    /// The songs among the items.
    pub fn songs(&self) -> Vec<Song> {
        self.items
            .iter()
            .filter_map(|i| match i {
                Picked::Song(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    /// "3 songs", "1 album".
    pub fn label(&self) -> String {
        let n = self.items.len();
        let (one, many) = match self.items.first() {
            Some(Picked::Album(_)) => ("album", "albums"),
            Some(Picked::Artist(_)) => ("artist", "artists"),
            Some(Picked::Playlist(_)) => ("playlist", "playlists"),
            _ => ("song", "songs"),
        };
        format!("{n} {}", if n == 1 { one } else { many })
    }
}

/// An active drag (after the pointer moved a few px with the button down).
#[derive(Debug, Clone)]
pub struct Drag {
    pub payload: DragPayload,
    pub at: iced::Point,
}

/// The button is down on a row; a drag starts once the pointer moves.
#[derive(Debug, Clone)]
pub struct Press {
    pub list: ListId,
    pub index: usize,
    origin: Option<iced::Point>,
}

/// A header being dragged to another place.
#[derive(Debug, Clone)]
pub struct ColumnDrag {
    pub list: ListId,
    pub column: ColumnId,
    pub over: Option<ColumnId>,
}

/// A column being resized by its right edge.
#[derive(Debug, Clone)]
pub struct Resize {
    pub list: ListId,
    pub column: ColumnId,
    origin_x: Option<f32>,
    start_width: f32,
}

/// A popover of the lists.
#[derive(Debug, Clone, PartialEq)]
pub enum ListMenu {
    /// The play options of a row: now, next, last.
    Play { list: ListId, index: usize, at: iced::Point },
    /// Display, columns and card options of a list.
    Config { list: ListId, at: iced::Point },
}

/// The play options that open above a row's play button when the pointer rests
/// on it (Feishin `RowPlayControlPopover`: opens after 150 ms, closes 150 ms
/// after the pointer left both the button and the popover).
#[derive(Debug, Clone, PartialEq)]
pub struct HoverPop {
    pub list: ListId,
    pub index: usize,
    /// Where the button is (window coordinates).
    pub rect: iced::Rectangle,
    pub open: bool,
}

/// Feishin's delays for [`HoverPop`].
pub const HOVER_OPEN: Duration = Duration::from_millis(150);
/// How long a plain click on a row waits before it opens the album, so the
/// first click of a double-click (play) does not leave the page.
pub const CLICK_OPEN_DELAY: Duration = Duration::from_millis(320);
pub const HOVER_CLOSE: Duration = Duration::from_millis(150);

/// What one list remembers.
#[derive(Debug, Default)]
pub struct ListState {
    pub selection: Selection,
    pub scroll: ScrollState,
    /// The shown page (paginated lists), counted from 0.
    pub page: usize,
    /// The row under the pointer.
    pub hover: Option<usize>,
    /// The part of the hovered row the pointer is in (a row with pinned columns
    /// has up to three; each reports entering and leaving on its own).
    hover_part: u8,
    /// Horizontal scroll of the table's unpinned columns, in px.
    pub hscroll: f32,
    /// The offset to restore after the page was recreated; the scrollable's
    /// first event (at offset 0) must not overwrite it.
    restore: Option<f32>,
}

/// All lists' state. `Listen::lists`.
#[derive(Debug, Default)]
pub struct Lists {
    states: HashMap<ListId, ListState>,
    pub press: Option<Press>,
    pub drag: Option<Drag>,
    pub column_drag: Option<ColumnDrag>,
    pub resize: Option<Resize>,
    pub menu: Option<ListMenu>,
    pub hover_pop: Option<HoverPop>,
    /// The album card expanded into a panel under its row (one at a time, as in Feishin).
    pub expanded: Option<(ListId, usize)>,
    /// Bumped per hover event: an older timer does nothing.
    pub hover_gen: u64,
    /// Bumped per row click, double-click and drag: a pending plain-click
    /// "open" from an older one does nothing.
    pub click_gen: u64,
    /// Tracks of the albums shown in the detail view, by (server, album id).
    pub detail: HashMap<(ServerId, String), Vec<Song>>,
    detail_pending: HashSet<(ServerId, String)>,
    /// The albums behind the album group headers (their favourite and rating).
    pub albums: HashMap<(ServerId, String), Album>,
    albums_pending: HashSet<(ServerId, String)>,
}

static EMPTY: std::sync::LazyLock<ListState> = std::sync::LazyLock::new(ListState::default);

impl Lists {
    /// The state of a list (empty when it was never used).
    pub fn get(&self, id: &ListId) -> &ListState {
        self.states.get(id).unwrap_or(&EMPTY)
    }

    pub fn entry(&mut self, id: &ListId) -> &mut ListState {
        self.states.entry(id.clone()).or_default()
    }

    pub fn scroll(&self, id: &ListId) -> ScrollState {
        self.get(id).scroll
    }

    /// The album an album group header shows the favourite and rating of.
    pub fn album_cache(&self, server: &ServerId, id: &str) -> Option<&Album> {
        self.albums.get(&(server.clone(), id.to_string()))
    }

    /// A list got new rows (another sort or filter): the selection and the
    /// page start over.
    pub fn reset(&mut self, id: &ListId) {
        if self.expanded.as_ref().is_some_and(|(l, _)| l == id) {
            self.expanded = None;
        }
        let s = self.entry(id);
        s.selection.clear();
        s.page = 0;
        s.hover = None;
    }
}

/// Settings of the table / grid edited from the config popover.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigEdit {
    Display(DisplayType),
    RowSize(RowSize),
    Header(bool),
    RowHover(bool),
    AlternateRows(bool),
    HorizontalBorders(bool),
    VerticalBorders(bool),
    AutoFit(bool),
    ColumnEnabled(ColumnId, bool),
    ColumnShift(ColumnId, i32),
    ColumnPin(ColumnId, Option<Pin>),
    ColumnAlign(ColumnId, Align),
    ColumnAutoSize(ColumnId, bool),
    CardSize(CardSize),
    Gap(Gap),
    ItemsPerRowEnabled(bool),
    ItemsPerRow(u32),
    CardRowEnabled(ColumnId, bool),
    CardRowShift(ColumnId, i32),
    CardRowAlign(ColumnId, Align),
    Pagination(Pagination),
    ItemsPerPage(u32),
    /// Cover size of the album group header (0: none).
    GroupImageSize(u32),
    GroupItemEnabled(AlbumGroupItem, bool),
    GroupItemShift(AlbumGroupItem, i32),
    GroupFavoriteRating(bool),
    GroupVertical(bool),
    /// Back to Feishin's defaults.
    Reset,
}

#[derive(Debug, Clone)]
pub enum ListMsg {
    /// The list's scrollable moved.
    Scrolled(ListId, ScrollState),
    /// An unpinned-columns scrollable (header, rows or the bar) moved to `x`.
    HScroll(ListId, HPart, f32),
    /// Button down on a row: selects (Ctrl / Shift as held now) and arms a drag.
    Press(ListId, usize),
    DoubleClick(ListId, usize),
    /// A plain click's "open the album" after the double-click window passed.
    ClickOpen(ListId, usize, u64),
    Hover(ListId, usize),
    /// Hover of one part (0 left, 1 middle, 2 right) of a row with pinned columns.
    HoverPart(ListId, usize, u8),
    UnhoverPart(ListId, usize, u8),
    /// The pointer left a row (ignored when another row was entered meanwhile).
    Unhover(ListId, usize),
    /// Right-click on a row.
    Context(ListId, usize),
    /// The "⋮" button of a selected row: the selection's menu at the button.
    ContextAt(ListId, usize, iced::Point),
    /// A keyboard move of the selection in the page's list.
    Move(Move),
    SelectAll(ListId),
    Clear(ListId),
    /// Play one row with a behaviour (hover controls of rows and cards).
    PlayRow(ListId, usize, PlayBehavior),
    /// Play the selection with a behaviour.
    PlaySelection(ListId, PlayBehavior),
    /// Open the page of a row (album, artist, playlist).
    Open(ListId, usize),
    /// Like / unlike one row (the heart cell).
    Favorite(ListId, usize),
    /// Rate a row 1..=5 (the same stars again clears).
    Rate(ListId, usize, u8),
    /// "More" of the selection bar was pressed at a point.
    SelectionMenu(ListId, iced::Point),
    /// The play options popover of a row.
    PlayMenu(ListId, usize),
    /// The Expand control of an album card: opens (or closes) the album's panel under its row.
    Expand(ListId, usize),
    /// Closes the expanded album panel.
    Collapse,
    /// The pointer entered a row's play button (its bounds): the popover opens after a moment.
    PlayHover(ListId, usize, iced::Rectangle),
    /// The pointer left the play button or the popover: it closes after a moment.
    PlayHoverLeave,
    /// The pointer is on the popover: it stays.
    PlayHoverKeep,
    /// A timer of the popover ran out (generation, open or close).
    PlayHoverTimer(u64, bool),
    ConfigMenu(ListId, iced::Point),
    CloseMenu,
    Config(ListId, ConfigEdit),
    SetPage(ListId, usize),
    ColumnDragStart(ListId, ColumnId),
    ColumnDragOver(ColumnId),
    ColumnResizeStart(ListId, ColumnId),
    /// The pointer moved while something is being dragged or resized.
    PointerMoved(iced::Point),
    /// The button went up (or the pointer left the window).
    PointerReleased,
    Drop(DropTarget),
    /// The tracks of an album for the detail view.
    DetailLoaded((ServerId, String), Result<Vec<Song>, String>),
    /// A track of a detail block: play its album from there.
    PlayDetailTrack(ListId, usize, usize),
    PlayBehavior(PlayBehavior),
    GridMultiSelect(bool),
    /// An album group header's album arrived (its favourite and rating).
    GroupLoaded((ServerId, String), Result<Box<Album>, String>),
    /// The heart of an album group header.
    GroupFavorite(ServerId, String),
    /// A star of an album group header (the same star again clears).
    GroupRate(ServerId, String, u8),
    /// Play `count` rows from `first` (the Play button of an album group header).
    GroupPlay(ListId, usize, usize, PlayBehavior),
}

impl Listen {
    /// What the main Play buttons do (settings: Lists → Play button).
    pub fn play_behavior(&self) -> PlayBehavior {
        self.config.settings.lists.play_behavior
    }

    /// The rows of `id`. **Add an arm for every page that uses the component.**
    pub fn list_data(&self, id: &ListId) -> Option<ListData<'_>> {
        Some(match id {
            ListId::Songs => ListData::Songs(Cow::Borrowed(&self.library.songs)),
            ListId::Albums => ListData::Albums(Cow::Borrowed(&self.library.albums)),
            ListId::Album(id) => ListData::Songs(Cow::Borrowed(self.album_rows(id))),
            ListId::Search(SearchTab::Songs) => ListData::Songs(Cow::Borrowed(&self.search.results.songs)),
            ListId::Search(SearchTab::Albums) => ListData::Albums(Cow::Borrowed(&self.search.results.albums)),
            ListId::Search(SearchTab::Artists) => {
                ListData::Artists(Cow::Borrowed(&self.search.results.artists))
            }
            ListId::Search(SearchTab::Playlists) => {
                ListData::Playlists(Cow::Borrowed(&self.search.results.playlists))
            }
            ListId::AlbumArtists | ListId::Artists => ListData::Artists(Cow::Borrowed(&self.library.artists)),
            ListId::Queue | ListId::SideQueue => ListData::Songs(Cow::Owned(self.queue_row_songs())),
            ListId::Artist(_) | ListId::ArtistAlbums(_) => return self.artist_list_data(id),
            ListId::Playlists | ListId::Playlist(_) | ListId::PlaylistAlbums(_) => {
                return self.playlist_list_data(id);
            }
            // Genres, folders and favourites: `library_extra.rs`.
            ListId::Genres
            | ListId::Genre(_)
            | ListId::GenreSongs(_)
            | ListId::Folder
            | ListId::Favorites(_) => {
                return self.extra_list_data(id);
            }
            _ => return None,
        })
    }

    /// Rows of `id` the visible window needs data for (images, more pages).
    /// **Add an arm for every page that loads lazily.**
    pub(in crate::app) fn list_page_visible(&mut self, id: &ListId) -> Task<Message> {
        let groups = self.load_album_groups(id);
        Task::batch([groups, self.list_page_data(id)])
    }

    fn list_page_data(&mut self, id: &ListId) -> Task<Message> {
        match id {
            ListId::Songs => self.songs_visible(),
            ListId::Albums => self.albums_visible(),
            ListId::Search(tab) => self.search_visible(*tab),
            ListId::AlbumArtists | ListId::Artists => self.artists_visible(id),
            ListId::Artist(_) | ListId::ArtistAlbums(_) => self.artist_sub_visible(id),
            ListId::Playlists | ListId::Playlist(_) | ListId::PlaylistAlbums(_) => self.playlist_visible(id),
            ListId::Genres
            | ListId::Genre(_)
            | ListId::GenreSongs(_)
            | ListId::Folder
            | ListId::Favorites(_) => self.extra_visible(id),
            ListId::Queue | ListId::SideQueue => self.queue_list_visible(id),
            _ => Task::none(),
        }
    }

    /// `[first, last)` rows (cards) that are on screen, with a little overscan.
    pub fn list_range(&self, id: &ListId, total: usize) -> (usize, usize) {
        let config = self.config.settings.lists.config(id.kind());
        let scroll = self.lists.scroll(id);
        let scroll = ScrollState {
            width: if scroll.width > 0.0 { scroll.width } else { self.page_width() },
            ..scroll
        };
        let page = self.lists.get(id).page;
        let (start, end) = list::page_bounds(&config, page, total);
        let (first, last) = match config.display {
            DisplayType::Table => list::table::visible_rows(scroll, config.table.size.height(), end - start),
            DisplayType::Grid => {
                list::grid::GridLayout::new(scroll.width, &config.grid).visible_items(scroll, end - start)
            }
            DisplayType::Detail => {
                let albums = match self.list_data(id) {
                    Some(ListData::Albums(a)) => a,
                    _ => return (start, start),
                };
                list::detail::visible_blocks(scroll, &albums[start..end])
            }
        };
        (start + first, start + last)
    }

    pub(in crate::app) fn selection_total(&self, id: &ListId) -> usize {
        self.list_data(id).map_or(0, |d| d.len())
    }

    /// A list page is shown again: its selection is gone, its scroll offset comes back.
    pub(super) fn list_enter(&mut self, route: &Route) -> Task<Message> {
        let Some(id) = self.list_for_route(route) else { return Task::none() };
        let state = self.lists.entry(&id);
        state.selection.clear();
        state.hover = None;
        let offset = state.scroll.offset;
        if offset > 0.0 {
            state.restore = Some(offset);
            self.scroll_list_to(&id, offset)
        } else {
            state.restore = None;
            Task::none()
        }
    }

    pub(in crate::app) fn scroll_list_to(&self, id: &ListId, y: f32) -> Task<Message> {
        iced::widget::operation::scroll_to(id.scroll_id(), AbsoluteOffset { x: 0.0, y })
    }

    /// Scrolls so that `index` is fully visible, if it is not.
    fn scroll_row_into_view(&mut self, id: &ListId, index: usize) -> Task<Message> {
        let config = self.config.settings.lists.config(id.kind());
        let state = self.lists.get(id);
        let scroll = ScrollState {
            width: if state.scroll.width > 0.0 { state.scroll.width } else { self.page_width() },
            ..state.scroll
        };
        let (start, _) = list::page_bounds(&config, state.page, self.selection_total(id));
        let local = index.saturating_sub(start);
        let (top, height) = match config.display {
            DisplayType::Table => (local as f32 * config.table.size.height(), config.table.size.height()),
            DisplayType::Grid => {
                let layout = list::grid::GridLayout::new(scroll.width, &config.grid);
                (list::grid::PAD + (local / layout.columns) as f32 * layout.row_height, layout.row_height)
            }
            DisplayType::Detail => {
                let albums = match self.list_data(id) {
                    Some(ListData::Albums(a)) => a,
                    _ => return Task::none(),
                };
                list::detail::block_span(&albums[start..], local)
            }
        };
        match list::scroll_into_view(scroll.offset, scroll.height, top, height) {
            Some(y) => {
                self.lists.entry(id).scroll.offset = y;
                self.scroll_list_to(id, y)
            }
            None => Task::none(),
        }
    }

    pub(super) fn on_list(&mut self, msg: ListMsg) -> Task<Message> {
        match msg {
            ListMsg::Scrolled(id, new) => {
                self.lists.hover_pop = None;
                let state = self.lists.entry(&id);
                if let Some(target) = state.restore {
                    // The recreated scrollable reports offset 0 before the restore lands.
                    if new.offset < 1.0 && target >= 1.0 {
                        state.scroll.height = new.height;
                        state.scroll.width = new.width;
                        return self.list_page_visible(&id);
                    }
                    state.restore = None;
                }
                state.scroll = new;
                return self.list_page_visible(&id);
            }
            // Unlike Feishin, only Ctrl/Shift+click selects; a plain click opens the row's
            // album (or the item), a double-click plays.
            ListMsg::Press(id, index) => {
                let mods = Mods::from_iced(self.modifiers);
                self.lists.menu = None;
                self.lists.click_gen += 1;
                self.lists.press = Some(Press { list: id.clone(), index, origin: None });
                if mods.toggle || mods.range {
                    let total = self.selection_total(&id);
                    self.lists.entry(&id).selection.click(index, mods, total);
                    return Task::none();
                }
                self.lists.entry(&id).selection.clear();
                let generation = self.lists.click_gen;
                return Task::perform(
                    async move {
                        tokio::time::sleep(CLICK_OPEN_DELAY).await;
                        generation
                    },
                    move |g| Message::List(ListMsg::ClickOpen(id.clone(), index, g)),
                );
            }
            ListMsg::DoubleClick(id, index) => {
                self.lists.click_gen += 1;
                self.lists.press = None;
                return self.activate_row(&id, index);
            }
            ListMsg::ClickOpen(id, index, generation) => {
                if generation != self.lists.click_gen || self.lists.drag.is_some() {
                    return Task::none();
                }
                if let Some(route) = self.click_route(&id, index)
                    && route != self.route
                {
                    return self.navigate(route);
                }
            }
            ListMsg::HScroll(id, from, x) => {
                let state = self.lists.entry(&id);
                if (state.hscroll - x).abs() < 0.5 {
                    return Task::none();
                }
                state.hscroll = x;
                let to = iced::widget::operation::scroll_to(
                    id.hscroll_id(HPart::Header),
                    AbsoluteOffset { x, y: 0.0 },
                );
                let rows = iced::widget::operation::scroll_to(
                    id.hscroll_id(HPart::Body),
                    AbsoluteOffset { x, y: 0.0 },
                );
                let bar = iced::widget::operation::scroll_to(
                    id.hscroll_id(HPart::Bar),
                    AbsoluteOffset { x, y: 0.0 },
                );
                return Task::batch(
                    [(HPart::Header, to), (HPart::Body, rows), (HPart::Bar, bar)]
                        .into_iter()
                        .filter(|(p, _)| *p != from)
                        .map(|(_, t)| t),
                );
            }
            ListMsg::HoverPart(id, row, part) => {
                let state = self.lists.entry(&id);
                state.hover_part = part;
                return self.on_list(ListMsg::Hover(id, row));
            }
            ListMsg::UnhoverPart(id, row, part) => {
                // Moving between two parts of a row reports the new one before or
                // after the old one is left; only leaving the current part counts.
                if self.lists.get(&id).hover_part == part {
                    return self.on_list(ListMsg::Unhover(id, row));
                }
            }
            ListMsg::Hover(id, row) => {
                self.lists.entry(&id).hover = Some(row);
                // Edit mode of a playlist: the dragged row is over this one.
                self.playlist_drag_over(row);
            }
            ListMsg::Unhover(id, row) => {
                let state = self.lists.entry(&id);
                if state.hover == Some(row) {
                    state.hover = None;
                }
            }
            ListMsg::Context(id, index) => return self.context(&id, index, None),
            ListMsg::ContextAt(id, index, at) => return self.context(&id, index, Some(at)),
            ListMsg::Move(mv) => {
                let Some(id) = self.list_for_route(&self.route) else { return Task::none() };
                return self.move_selection(&id, mv);
            }
            ListMsg::SelectAll(id) => {
                let total = self.selection_total(&id);
                self.lists.entry(&id).selection.toggle_all(total);
            }
            ListMsg::Clear(id) => {
                self.lists.entry(&id).selection.clear();
                self.lists.menu = None;
            }
            ListMsg::PlayRow(id, index, behavior) => {
                self.lists.menu = None;
                self.lists.hover_pop = None;
                if id.is_queue() && behavior == PlayBehavior::Now {
                    return self.queue_activate(index);
                }
                let picks = self.list_data(&id).map(|d| d.pick(&[index])).unwrap_or_default();
                return self.play_items(picks, behavior);
            }
            ListMsg::PlaySelection(id, behavior) => {
                self.lists.menu = None;
                return self.play_selection(&id, behavior);
            }
            ListMsg::Open(id, index) => {
                if let Some(route) = self.list_data(&id).and_then(|d| d.get(index)).and_then(|p| p.route()) {
                    return self.navigate(route);
                }
            }
            ListMsg::Favorite(id, index) => return self.favorite_row(&id, index),
            ListMsg::Rate(id, index, stars) => return self.rate_row(&id, index, stars),
            ListMsg::SelectionMenu(id, at) => {
                self.lists.menu = None;
                if let Some(target) = self.selection_target(&id) {
                    self.open_menu(target, Some(at), Some(id));
                }
            }
            ListMsg::PlayHover(id, index, rect) => {
                self.lists.hover_gen += 1;
                let open =
                    self.lists.hover_pop.as_ref().is_some_and(|p| p.open && p.list == id && p.index == index);
                self.lists.hover_pop = Some(HoverPop { list: id, index, rect, open });
                if open {
                    return Task::none();
                }
                return self.hover_timer(true);
            }
            ListMsg::PlayHoverLeave => {
                self.lists.hover_gen += 1;
                if self.lists.hover_pop.is_some() {
                    return self.hover_timer(false);
                }
            }
            ListMsg::PlayHoverKeep => self.lists.hover_gen += 1,
            ListMsg::PlayHoverTimer(generation, open) => {
                if generation == self.lists.hover_gen {
                    if open {
                        if let Some(p) = &mut self.lists.hover_pop {
                            p.open = true;
                        }
                    } else {
                        self.lists.hover_pop = None;
                    }
                }
            }
            ListMsg::Expand(id, index) => {
                if self.lists.expanded == Some((id.clone(), index)) {
                    self.lists.expanded = None;
                    return Task::none();
                }
                self.lists.expanded = Some((id.clone(), index));
                if let Some(Picked::Album(album)) = self.list_data(&id).and_then(|d| d.get(index)) {
                    return self.fetch_tracks(&album);
                }
            }
            ListMsg::Collapse => self.lists.expanded = None,
            ListMsg::PlayMenu(id, index) => {
                self.menu = None;
                self.lists.menu = Some(ListMenu::Play { list: id, index, at: self.right_click_at });
            }
            ListMsg::ConfigMenu(id, at) => {
                self.menu = None;
                self.lists.menu = Some(ListMenu::Config { list: id, at });
            }
            ListMsg::CloseMenu => self.lists.menu = None,
            ListMsg::Config(id, edit) => return self.edit_config(&id, edit),
            ListMsg::SetPage(id, page) => {
                let config = self.config.settings.lists.config(id.kind());
                let total = self.selection_total(&id);
                let pages = list::page_count(&config, total).max(1);
                let state = self.lists.entry(&id);
                state.page = page.min(pages - 1);
                state.selection.clear();
                state.hover = None;
                state.scroll.offset = 0.0;
                let scroll = self.scroll_list_to(&id, 0.0);
                return Task::batch([scroll, self.list_page_visible(&id)]);
            }
            ListMsg::ColumnDragStart(id, column) => {
                self.lists.column_drag = Some(ColumnDrag { list: id, column, over: None });
            }
            ListMsg::ColumnDragOver(column) => {
                if let Some(d) = &mut self.lists.column_drag {
                    d.over = Some(column);
                }
            }
            ListMsg::ColumnResizeStart(id, column) => {
                // Freeze the widths as shown, so the drag moves the edge 1:1.
                let kind = id.kind();
                let shown = self.shown_widths(&id);
                self.config.settings.lists.edit(kind, |c| {
                    for (col, w) in &shown {
                        if let Some(cc) = c.table.column_mut(*col) {
                            cc.width = *w;
                        }
                    }
                });
                let start_width = shown.iter().find(|(c, _)| *c == column).map_or(100.0, |(_, w)| *w);
                self.lists.column_drag = None;
                self.lists.resize = Some(Resize { list: id, column, origin_x: None, start_width });
            }
            ListMsg::PointerMoved(at) => self.pointer_moved(at),
            ListMsg::PointerReleased => return self.pointer_released(),
            ListMsg::Drop(target) => {
                if let Some(drag) = self.lists.drag.take() {
                    self.lists.press = None;
                    return self.drop_on(target, drag.payload);
                }
            }
            ListMsg::DetailLoaded(key, result) => {
                self.lists.detail_pending.remove(&key);
                match result {
                    Ok(songs) => {
                        self.lists.detail.insert(key, songs);
                    }
                    Err(e) => self.toast_error(format!("Could not load the album: {e}")),
                }
            }
            ListMsg::PlayDetailTrack(id, album, track) => {
                if let Some(Picked::Album(a)) = self.list_data(&id).and_then(|d| d.get(album)) {
                    let songs = list::detail::album_tracks(self, &a);
                    return self.play_picked(songs.to_vec(), track);
                }
            }
            ListMsg::PlayBehavior(b) => {
                self.config.settings.lists.play_behavior = b;
                self.save_config();
            }
            ListMsg::GridMultiSelect(on) => {
                self.config.settings.lists.grid_multi_select = on;
                self.save_config();
            }
            ListMsg::GroupLoaded(key, result) => {
                self.lists.albums_pending.remove(&key);
                if let Ok(album) = result {
                    self.lists.albums.insert(key, Album { songs: Vec::new(), ..*album });
                }
            }
            ListMsg::GroupFavorite(server, album) => {
                let on = self.lists.album_cache(&server, &album).is_some_and(|a| a.favorite);
                return self.set_item_favorite(ItemKind::Album, &server, &album, !on);
            }
            ListMsg::GroupRate(server, album, stars) => {
                let current = self.lists.album_cache(&server, &album).and_then(|a| a.rating);
                let stars = if current == Some(stars) { 0 } else { stars.min(5) };
                return self.set_item_rating(ItemKind::Album, &server, &album, stars);
            }
            ListMsg::GroupPlay(id, first, count, behavior) => {
                let indices: Vec<usize> = (first..first + count).collect();
                let picks = self.list_data(&id).map(|d| d.pick(&indices)).unwrap_or_default();
                return self.play_items(picks, behavior);
            }
        }
        Task::none()
    }

    // ---- actions -------------------------------------------------------------

    /// The page a plain click on a row opens: a song's album, or the item.
    fn click_route(&self, id: &ListId, index: usize) -> Option<Route> {
        match self.list_data(id)?.get(index)? {
            Picked::Song(s) => s.album_id.clone().map(Route::Album),
            other => other.route(),
        }
    }

    /// Double-click / Enter on a row: songs play per the play-button behaviour (±50 songs
    /// around it, like Feishin); anything else opens.
    fn activate_row(&mut self, id: &ListId, index: usize) -> Task<Message> {
        // A row of the queue jumps to that song; the queue is not replaced.
        if id.is_queue() {
            return self.queue_activate(index);
        }
        let behavior = self.play_behavior();
        match self.list_data(id) {
            Some(ListData::Songs(songs)) => {
                if index >= songs.len() {
                    return Task::none();
                }
                match behavior {
                    PlayBehavior::Now => {
                        // Feishin queues 50 songs before and after the clicked one.
                        let from = index.saturating_sub(50);
                        let to = (index + 51).min(songs.len());
                        let window = songs[from..to].to_vec();
                        self.play_picked(window, index - from)
                    }
                    other => {
                        let song = songs[index].clone();
                        self.play_items(vec![Picked::Song(song)], other)
                    }
                }
            }
            Some(data) => match data.get(index).and_then(|p| p.route()) {
                Some(route) => self.navigate(route),
                None => Task::none(),
            },
            None => Task::none(),
        }
    }

    /// Plays or queues rows with a behaviour. Songs go straight to the
    /// queue actions of the context menu; albums, playlists and artists
    /// fetch their songs first.
    pub fn play_items(&mut self, picks: Vec<Picked>, behavior: PlayBehavior) -> Task<Message> {
        if picks.is_empty() {
            return Task::none();
        }
        let action = match behavior {
            PlayBehavior::Now => MenuAction::Play,
            PlayBehavior::Next => MenuAction::PlayNext,
            PlayBehavior::Last => MenuAction::AddToQueue,
        };
        self.apply_to_picks(action, picks)
    }

    /// `action` (Play, PlayNext, AddToQueue, AddToPlaylist) on rows.
    pub(in crate::app) fn apply_to_picks(&mut self, action: MenuAction, picks: Vec<Picked>) -> Task<Message> {
        match MenuTarget::from_picks(&picks) {
            Some(target) => self.apply_to_target(action, target),
            None => Task::none(),
        }
    }

    /// The selected rows of a list as one context-menu target.
    pub fn selection_target(&self, id: &ListId) -> Option<MenuTarget> {
        if id.is_queue() {
            return self.queue_selection_target(id);
        }
        let indices = self.lists.get(id).selection.indices();
        let picks = self.list_data(id)?.pick(&indices);
        MenuTarget::from_picks(&picks)
    }

    fn play_selection(&mut self, id: &ListId, behavior: PlayBehavior) -> Task<Message> {
        let indices = self.lists.get(id).selection.indices();
        if id.is_queue() && behavior == PlayBehavior::Now {
            return match indices.first() {
                Some(&i) => self.queue_activate(i),
                None => Task::none(),
            };
        }
        let picks = self.list_data(id).map(|d| d.pick(&indices)).unwrap_or_default();
        self.play_items(picks, behavior)
    }

    fn context(&mut self, id: &ListId, index: usize, at: Option<iced::Point>) -> Task<Message> {
        let total = self.selection_total(id);
        if index >= total {
            return Task::none();
        }
        let state = self.lists.entry(id);
        // Feishin: a right-click on an unselected row selects just that row.
        if !state.selection.contains(index) {
            state.selection.set_only(index);
        }
        self.lists.menu = None;
        // The menu acts on the whole selection (this row is part of it).
        match self.selection_target(id) {
            Some(target) => self.on_menu(MenuMsg::OpenIn { target, at, list: id.clone() }),
            None => Task::none(),
        }
    }

    fn move_selection(&mut self, id: &ListId, mv: Move) -> Task<Message> {
        let config = self.config.settings.lists.config(id.kind());
        let total = self.selection_total(id);
        if total == 0 {
            return Task::none();
        }
        let state = self.lists.get(id);
        let width = if state.scroll.width > 0.0 { state.scroll.width } else { self.page_width() };
        let (columns, page_rows) = match config.display {
            DisplayType::Grid => {
                let layout = list::grid::GridLayout::new(width, &config.grid);
                (layout.columns, ((state.scroll.height / layout.row_height) as usize).max(1))
            }
            DisplayType::Table => (1, ((state.scroll.height / config.table.size.height()) as usize).max(1)),
            DisplayType::Detail => (1, 3),
        };
        let (start, end) = list::page_bounds(&config, state.page, total);
        let state = self.lists.entry(id);
        // The cursor lives on the shown page.
        if let Some(last) = state.selection.last()
            && !(start..end).contains(&last)
        {
            state.selection.clear();
        }
        let local_total = end - start;
        let local_last_selected = state.selection.last().map(|l| l - start);
        let mut local = Selection::default();
        if let Some(l) = local_last_selected {
            local.set_only(l);
        }
        let Some(to) = local.step(mv, local_total, columns, page_rows) else { return Task::none() };
        let to = start + to;
        self.lists.entry(id).selection.set_only(to);
        self.scroll_row_into_view(id, to)
    }

    fn favorite_row(&mut self, id: &ListId, index: usize) -> Task<Message> {
        match self.list_data(id).and_then(|d| d.get(index)) {
            Some(Picked::Song(song)) => self.set_song_favorite(&song, !song.favorite),
            Some(Picked::Album(album)) => {
                self.set_item_favorite(ItemKind::Album, &album.server_id, &album.id, !album.favorite)
            }
            Some(Picked::Artist(artist)) => {
                self.set_item_favorite(ItemKind::Artist, &artist.server_id, &artist.id, !artist.favorite)
            }
            _ => Task::none(),
        }
    }

    fn rate_row(&mut self, id: &ListId, index: usize, stars: u8) -> Task<Message> {
        match self.list_data(id).and_then(|d| d.get(index)) {
            Some(Picked::Song(song)) => {
                let stars = if song.rating == Some(stars) { 0 } else { stars };
                self.rate_song(&song, stars)
            }
            Some(Picked::Album(album)) => {
                let stars = if album.rating == Some(stars) { 0 } else { stars.min(5) };
                self.set_item_rating(ItemKind::Album, &album.server_id, &album.id, stars)
            }
            Some(Picked::Artist(artist)) => {
                let stars = if artist.rating == Some(stars) { 0 } else { stars.min(5) };
                self.set_item_rating(ItemKind::Artist, &artist.server_id, &artist.id, stars)
            }
            _ => Task::none(),
        }
    }

    /// Whether the rating column / stars are offered for a source.
    pub fn can_rate(&self, server: &ServerId, kind: SourceKind) -> bool {
        kind != SourceKind::YouTubeMusic
            && self.sources.get(server).is_some_and(|s| s.capabilities().contains(Capabilities::RATINGS))
    }

    // ---- hotkeys -------------------------------------------------------------

    /// `HotkeysMsg::List`: the hotkeys act on the selection of the shown list.
    pub(super) fn on_list_hotkey(&mut self, key: hotkeys::ListHotkey) -> Task<Message> {
        use hotkeys::ListHotkey as K;
        let active = self.list_for_route(&self.route).filter(|id| self.list_data(id).is_some());
        match key {
            K::ShowPlayingSong => {
                if let Some(id) = &active
                    && let Some(task) = self.show_playing_song(id)
                {
                    return task;
                }
                if let Some(id) = self.queue.current().and_then(|i| i.song.album_id.clone()) {
                    return self.handle(Message::OpenAlbum(id));
                }
                Task::none()
            }
            K::PlayDefault | K::PlayNow | K::PlayNext | K::PlayLast => {
                let Some(id) = active else { return Task::none() };
                let behavior = match key {
                    K::PlayNow => PlayBehavior::Now,
                    K::PlayNext => PlayBehavior::Next,
                    K::PlayLast => PlayBehavior::Last,
                    _ => self.play_behavior(),
                };
                // Feishin plays the first selected row; Listen plays them all (the same for one).
                self.play_selection(&id, behavior)
            }
            K::NavigateToPage => {
                let Some(id) = active else { return Task::none() };
                let first = self.lists.get(&id).selection.first();
                match first.and_then(|i| self.list_data(&id).and_then(|d| d.get(i))).and_then(|p| p.route()) {
                    Some(route) => self.navigate(route),
                    None => Task::none(),
                }
            }
        }
    }

    /// Selects and scrolls to the playing song if the list holds it.
    fn show_playing_song(&mut self, id: &ListId) -> Option<Task<Message>> {
        let current = self.queue.current()?.song.clone();
        let index = match self.list_data(id)? {
            ListData::Songs(songs) => {
                songs.iter().position(|s| s.id == current.id && s.server_id == current.server_id)?
            }
            _ => return None,
        };
        // Paginated lists jump to the page of the song.
        let config = self.config.settings.lists.config(id.kind());
        if config.pagination == Pagination::Paginated {
            let page = index / config.items_per_page.max(1) as usize;
            self.lists.entry(id).page = page;
        }
        self.lists.entry(id).selection.set_only(index);
        Some(self.scroll_row_into_view(id, index))
    }

    /// Keys for the shown list that are not configurable hotkeys: arrows, Home / End,
    /// Page Up / Down and Ctrl+A. `true` when the key was used.
    pub(super) fn list_key(
        &mut self,
        key: &iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        let id = self.list_for_route(&self.route)?;
        self.list_data(&id).filter(|d| !d.is_empty())?;
        if self.overlay_blocks_list() {
            return None;
        }
        let grid = self.config.settings.lists.config(id.kind()).display == DisplayType::Grid;
        match list::hotkeys::key_action(key, modifiers, grid)? {
            list::hotkeys::KeyAction::SelectAll => Some(self.on_list(ListMsg::SelectAll(id))),
            list::hotkeys::KeyAction::Move(mv) => Some(self.on_list(ListMsg::Move(mv))),
        }
    }

    fn overlay_blocks_list(&self) -> bool {
        self.menu.is_some() || self.fullscreen
    }

    // ---- configuration -------------------------------------------------------

    /// The widths the columns are shown at (their share of the free space).
    fn shown_widths(&self, id: &ListId) -> Vec<(ColumnId, f32)> {
        let config = self.config.settings.lists.config(id.kind());
        let scroll = self.lists.scroll(id);
        let width = if scroll.width > 0.0 { scroll.width } else { self.page_width() };
        let data = self.list_data(id);
        // Rows with album headers cannot scroll sideways, so their columns shrink to fit.
        let sideways = !config.table.column(ColumnId::AlbumGroup).is_some_and(|c| c.enabled);
        let shape =
            list::table::Shape::new(&config.table, list::table::inner_width(width), sideways, |col| {
                data.as_ref().and_then(|d| list::columns::content_width(d, col))
            });
        shape.columns.iter().map(|c| c.id).zip(shape.widths.iter().copied()).collect()
    }

    fn edit_config(&mut self, id: &ListId, edit: ConfigEdit) -> Task<Message> {
        let kind = id.kind();
        // The album group header is one look for every song table.
        let group = &mut self.config.settings.lists.album_group;
        match &edit {
            ConfigEdit::GroupImageSize(px) => group.image_size = (*px).min(256),
            ConfigEdit::GroupItemEnabled(item, on) => {
                if let Some(e) = group.items.iter_mut().find(|e| e.item == *item) {
                    e.enabled = *on;
                }
            }
            ConfigEdit::GroupItemShift(item, by) => group.shift(*item, *by),
            ConfigEdit::GroupFavoriteRating(on) => group.show_favorite_rating = *on,
            ConfigEdit::GroupVertical(on) => group.vertical_layout = *on,
            _ => {}
        }
        if matches!(
            edit,
            ConfigEdit::GroupImageSize(_)
                | ConfigEdit::GroupItemEnabled(..)
                | ConfigEdit::GroupItemShift(..)
                | ConfigEdit::GroupFavoriteRating(_)
                | ConfigEdit::GroupVertical(_)
        ) {
            self.save_config();
            return self.list_page_visible(id);
        }
        let autosize = if let ConfigEdit::ColumnAutoSize(col, true) = &edit {
            self.list_data(id).and_then(|d| list::columns::content_width(&d, *col))
        } else {
            None
        };
        self.config.settings.lists.edit(kind, |c| match edit {
            ConfigEdit::Display(d) => c.display = d,
            ConfigEdit::RowSize(s) => c.table.size = s,
            ConfigEdit::Header(v) => c.table.header = v,
            ConfigEdit::RowHover(v) => c.table.row_hover_highlight = v,
            ConfigEdit::AlternateRows(v) => c.table.alternate_rows = v,
            ConfigEdit::HorizontalBorders(v) => c.table.horizontal_borders = v,
            ConfigEdit::VerticalBorders(v) => c.table.vertical_borders = v,
            ConfigEdit::AutoFit(v) => c.table.auto_fit = v,
            ConfigEdit::ColumnEnabled(col, on) => {
                if let Some(cc) = c.table.column_mut(col) {
                    cc.enabled = on;
                }
            }
            ConfigEdit::ColumnShift(col, by) => c.table.shift(col, by),
            ConfigEdit::ColumnPin(col, pin) => {
                if let Some(cc) = c.table.column_mut(col) {
                    cc.pinned = pin;
                }
            }
            ConfigEdit::ColumnAlign(col, align) => {
                if let Some(cc) = c.table.column_mut(col) {
                    cc.align = align;
                }
            }
            ConfigEdit::ColumnAutoSize(col, on) => {
                if let Some(cc) = c.table.column_mut(col) {
                    cc.auto_size = on;
                    if let Some(w) = autosize.filter(|_| on) {
                        cc.width = w;
                    }
                }
            }
            ConfigEdit::CardSize(s) => c.grid.size = s,
            ConfigEdit::Gap(g) => c.grid.gap = g,
            ConfigEdit::ItemsPerRowEnabled(v) => c.grid.items_per_row_enabled = v,
            ConfigEdit::ItemsPerRow(n) => c.grid.items_per_row = n.clamp(1, 20),
            ConfigEdit::CardRowEnabled(col, on) => {
                if let Some(r) = c.grid.rows.iter_mut().find(|r| r.id == col) {
                    r.enabled = on;
                }
            }
            ConfigEdit::CardRowShift(col, by) => c.grid.shift(col, by),
            ConfigEdit::CardRowAlign(col, align) => {
                if let Some(r) = c.grid.rows.iter_mut().find(|r| r.id == col) {
                    r.align = align;
                }
            }
            ConfigEdit::GroupImageSize(_)
            | ConfigEdit::GroupItemEnabled(..)
            | ConfigEdit::GroupItemShift(..)
            | ConfigEdit::GroupFavoriteRating(_)
            | ConfigEdit::GroupVertical(_) => {}
            ConfigEdit::Pagination(p) => c.pagination = p,
            ConfigEdit::ItemsPerPage(n) => c.items_per_page = n,
            ConfigEdit::Reset => *c = kind.default_config(),
        });
        self.save_config();
        // The page, the columns or the display type changed what is on screen.
        let state = self.lists.entry(id);
        state.page = 0;
        self.list_page_visible(id)
    }

    // ---- pointer: drag, column drag and resize ---------------------------------

    fn pointer_moved(&mut self, at: iced::Point) {
        if let Some(drag) = &mut self.lists.drag {
            drag.at = at;
            return;
        }
        if let Some(resize) = &mut self.lists.resize {
            let origin = *resize.origin_x.get_or_insert(at.x);
            let width = (resize.start_width + at.x - origin).clamp(40.0, 1200.0);
            let (id, column) = (resize.list.clone(), resize.column);
            self.config.settings.lists.edit(id.kind(), |c| {
                if let Some(cc) = c.table.column_mut(column) {
                    cc.width = width;
                    cc.auto_size = false;
                }
            });
            return;
        }
        if let Some(press) = &mut self.lists.press {
            let origin = *press.origin.get_or_insert(at);
            if (at.x - origin.x).hypot(at.y - origin.y) < 6.0 {
                return;
            }
            let (id, index) = (press.list.clone(), press.index);
            self.lists.press = None;
            self.lists.click_gen += 1;
            // Dragging an unselected row drags just that row.
            let state = self.lists.entry(&id);
            if !state.selection.contains(index) {
                state.selection.set_only(index);
            }
            let indices = state.selection.indices();
            let items = self.list_data(&id).map(|d| d.pick(&indices)).unwrap_or_default();
            if !items.is_empty() {
                self.lists.drag = Some(Drag { payload: DragPayload { from: id, items }, at });
            }
        }
    }

    fn pointer_released(&mut self) -> Task<Message> {
        self.lists.press = None;
        // A drop target takes the payload in its own `Drop` message first; whatever
        // is left was dropped on nothing.
        self.lists.drag = None;
        if self.lists.resize.take().is_some() {
            self.save_config();
        }
        if let Some(drag) = self.lists.column_drag.take()
            && let Some(over) = drag.over.filter(|o| *o != drag.column)
        {
            self.config.settings.lists.edit(drag.list.kind(), |c| c.table.reorder(drag.column, over));
            self.save_config();
        }
        Task::none()
    }

    /// Dropped rows: the queue takes songs (albums and playlists are fetched), a
    /// playlist gets the songs added.
    pub fn drop_on(&mut self, target: DropTarget, payload: DragPayload) -> Task<Message> {
        match target {
            DropTarget::Queue => self.apply_to_picks(MenuAction::AddToQueue, payload.items),
            DropTarget::QueueAt(row) => self.queue_drop(row, payload),
            DropTarget::QueueEmpty => self.apply_to_picks(MenuAction::Play, payload.items),
            DropTarget::Playlist { id, name, .. } => {
                self.apply_to_picks(MenuAction::AddToPlaylist(id, name), payload.items)
            }
        }
    }

    /// The subscription that feeds the pointer to a drag, a column drag or resize.
    pub(super) fn list_subscription(&self) -> Option<Subscription<Message>> {
        let l = &self.lists;
        (l.press.is_some() || l.drag.is_some() || l.column_drag.is_some() || l.resize.is_some()).then(|| {
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::List(ListMsg::PointerMoved(position)))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
                | iced::Event::Mouse(iced::mouse::Event::CursorLeft) => {
                    Some(Message::List(ListMsg::PointerReleased))
                }
                _ => None,
            })
        })
    }

    /// A timer for the play popover; `demo` mode (tests) fires it at once.
    fn hover_timer(&self, open: bool) -> Task<Message> {
        let generation = self.lists.hover_gen;
        let wait = if open { HOVER_OPEN } else { HOVER_CLOSE };
        Task::perform(
            async move {
                tokio::time::sleep(wait).await;
                generation
            },
            move |g| Message::List(ListMsg::PlayHoverTimer(g, open)),
        )
    }

    // ---- album group headers -------------------------------------------------

    /// Fetches the albums of the album group headers in view (their favourite
    /// and rating) once each.
    pub(super) fn load_album_groups(&mut self, id: &ListId) -> Task<Message> {
        let config = self.config.settings.lists.config(id.kind());
        let on = config.display == DisplayType::Table
            && config.table.column(ColumnId::AlbumGroup).is_some_and(|c| c.enabled);
        if !on || !self.config.settings.lists.album_group.show_favorite_rating {
            return Task::none();
        }
        let Some(ListData::Songs(songs)) = self.list_data(id) else { return Task::none() };
        let total = songs.len();
        let (first, last) = self.list_range(id, total);
        let wanted: Vec<(ServerId, String)> = list::album_group::runs(&songs, first, last)
            .into_iter()
            .filter_map(|(i, _)| Some((songs[i].server_id.clone(), songs[i].album_id.clone()?)))
            .collect();
        drop(songs);
        let mut tasks = Vec::new();
        for key in wanted {
            if self.lists.albums.contains_key(&key) || self.lists.albums_pending.contains(&key) {
                continue;
            }
            if self.demo {
                if let Some(a) = demo::albums().into_iter().find(|a| a.id == key.1) {
                    self.lists.albums.insert(key, Album { songs: Vec::new(), ..a });
                }
                continue;
            }
            let Some(source) = self.sources.get(&key.0).cloned() else { continue };
            self.lists.albums_pending.insert(key.clone());
            tasks.push(Task::perform(
                {
                    let album = key.1.clone();
                    async move { source.album(&album).await.map(Box::new).map_err(err) }
                },
                move |r| Message::List(ListMsg::GroupLoaded(key.clone(), r)),
            ));
        }
        Task::batch(tasks)
    }

    // ---- detail view ---------------------------------------------------------

    /// Fetches the tracks of the albums in `first..last` the detail view shows.
    pub(super) fn load_detail(&mut self, id: &ListId, first: usize, last: usize) -> Task<Message> {
        if self.config.settings.lists.config(id.kind()).display != DisplayType::Detail {
            return Task::none();
        }
        let albums = match self.list_data(id) {
            Some(ListData::Albums(a)) => a[first.min(a.len())..last.min(a.len())].to_vec(),
            _ => return Task::none(),
        };
        Task::batch(albums.iter().map(|album| self.fetch_tracks(album)))
    }

    /// Fetches the tracks of an album for the detail view or its expanded panel,
    /// unless it has them, they are known or on their way.
    pub(super) fn fetch_tracks(&mut self, album: &Album) -> Task<Message> {
        let key = (album.server_id.clone(), album.id.clone());
        if !album.songs.is_empty()
            || self.lists.detail.contains_key(&key)
            || self.lists.detail_pending.contains(&key)
        {
            return Task::none();
        }
        let Some(source) = self.sources.get(&album.server_id).cloned() else { return Task::none() };
        self.lists.detail_pending.insert(key.clone());
        let id = album.id.clone();
        Task::perform(async move { source.album(&id).await.map(|a| a.songs).map_err(err) }, move |r| {
            Message::List(ListMsg::DetailLoaded(key.clone(), r))
        })
    }
}
