//! Item lists (tables, grids, detail lists): play-button behaviour and each list's display type, table
//! columns, row size, card size and pagination. Names and defaults are Feishin's `lists` settings
//! (store/settings.store.ts, `ItemListConfig`). Only changed lists are stored (`Lists::lists`); the rest
//! use [`ListKind::default_config`].

use super::*;

/// What the main Play buttons, the default context-menu Play, a double-click
/// on a song and the `listPlayDefault` hotkey do (Feishin `playButtonBehavior`).
/// Shuffle play, the Auto-DJ and the AI DJ are never affected by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayBehavior {
    /// Replace the queue and start playing.
    #[default]
    Now,
    /// Add right after the playing song.
    Next,
    /// Add to the end of the queue.
    Last,
}

impl PlayBehavior {
    pub const ALL: [PlayBehavior; 3] = [PlayBehavior::Now, PlayBehavior::Next, PlayBehavior::Last];

    /// The text of a main Play button.
    pub fn button_label(self) -> &'static str {
        match self {
            PlayBehavior::Now => "Play",
            PlayBehavior::Next => "Add next",
            PlayBehavior::Last => "Add last",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PlayBehavior::Now => "Play now",
            PlayBehavior::Next => "Add to queue next",
            PlayBehavior::Last => "Add to queue last",
        }
    }
}

/// How a list shows its items (Feishin `ListDisplayType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayType {
    #[default]
    Table,
    /// Poster cards.
    Grid,
    /// Every item with its own block (albums: cover, facts and the track list).
    Detail,
}

impl DisplayType {
    pub fn label(self) -> &'static str {
        match self {
            DisplayType::Table => "Table",
            DisplayType::Grid => "Grid",
            DisplayType::Detail => "Detail",
        }
    }
}

/// Feishin `ListPaginationType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pagination {
    #[default]
    Infinite,
    Paginated,
}

/// Table row height: compact 40 px, default 64 px, large 88 px.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowSize {
    Compact,
    #[default]
    Default,
    Large,
}

impl RowSize {
    pub const ALL: [RowSize; 3] = [RowSize::Compact, RowSize::Default, RowSize::Large];

    pub fn height(self) -> f32 {
        match self {
            RowSize::Compact => 40.0,
            RowSize::Default => 64.0,
            RowSize::Large => 88.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RowSize::Compact => "Compact",
            RowSize::Default => "Default",
            RowSize::Large => "Large",
        }
    }
}

/// Grid card size: compact shows only the cover, default is the poster card,
/// large has fewer, bigger cards (Feishin multiplies the columns by 0.75).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardSize {
    Compact,
    #[default]
    Default,
    Large,
}

impl CardSize {
    pub const ALL: [CardSize; 3] = [CardSize::Compact, CardSize::Default, CardSize::Large];

    pub fn label(self) -> &'static str {
        match self {
            CardSize::Compact => "Compact",
            CardSize::Default => "Default",
            CardSize::Large => "Large",
        }
    }
}

/// Space between cards (Mantine spacing: xs 10, sm 12, md 16, lg 20, xl 32 px).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gap {
    Xs,
    #[default]
    Sm,
    Md,
    Lg,
    Xl,
}

impl Gap {
    pub const ALL: [Gap; 5] = [Gap::Xs, Gap::Sm, Gap::Md, Gap::Lg, Gap::Xl];

    pub fn px(self) -> f32 {
        match self {
            Gap::Xs => 10.0,
            Gap::Sm => 12.0,
            Gap::Md => 16.0,
            Gap::Lg => 20.0,
            Gap::Xl => 32.0,
        }
    }

    pub fn larger(self) -> Gap {
        let i = Gap::ALL.iter().position(|g| *g == self).unwrap_or(1);
        Gap::ALL[(i + 1).min(4)]
    }

    pub fn smaller(self) -> Gap {
        let i = Gap::ALL.iter().position(|g| *g == self).unwrap_or(1);
        Gap::ALL[i.saturating_sub(1)]
    }
}

/// Cell / card-row alignment (Feishin `start` / `center` / `end`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

impl Align {
    pub fn next(self) -> Align {
        match self {
            Align::Start => Align::Center,
            Align::Center => Align::End,
            Align::End => Align::Start,
        }
    }
}

/// Which edge a column is pinned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pin {
    Left,
    Right,
}

/// What a list shows (the kind of its rows), which decides the available columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemType {
    Song,
    Album,
    Artist,
    Playlist,
    Genre,
}

/// A table column or a card row (Feishin `TableColumn`; the ids are the same
/// for grid card rows). Columns no source has data for are not offered; a
/// source without the data (composer on YouTube Music) leaves the cell empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnId {
    /// Songs: groups the rows of an album under a header with its cover,
    /// facts and favourite / rating (Feishin "album group").
    AlbumGroup,
    RowIndex,
    Image,
    /// The item's name (song title, album name, ...).
    Title,
    /// Cover, title and artist(s) in one cell.
    TitleCombined,
    /// Title and artist(s) in one cell.
    TitleArtist,
    Duration,
    Album,
    AlbumArtists,
    Artists,
    Composer,
    /// The date tag of a track.
    Date,
    Genres,
    GenreBadge,
    Year,
    ReleaseYear,
    ReleaseDate,
    LastPlayed,
    ReleaseType,
    DiscNumber,
    TrackNumber,
    BitDepth,
    BitRate,
    Codec,
    SampleRate,
    Comment,
    Channels,
    Bpm,
    DateAdded,
    Path,
    PlayCount,
    Size,
    SongCount,
    AlbumCount,
    Owner,
    UserFavorite,
    UserRating,
    Actions,
}

impl ColumnId {
    pub fn label(self) -> &'static str {
        match self {
            ColumnId::AlbumGroup => "Album group",
            ColumnId::RowIndex => "#",
            ColumnId::Image => "Image",
            ColumnId::Title => "Title",
            ColumnId::TitleCombined => "Title",
            ColumnId::TitleArtist => "Title",
            ColumnId::Duration => "Duration",
            ColumnId::Album => "Album",
            ColumnId::AlbumArtists => "Album artist",
            ColumnId::Artists => "Artist",
            ColumnId::Composer => "Composer",
            ColumnId::Date => "Date",
            ColumnId::Genres => "Genre",
            ColumnId::GenreBadge => "Genre",
            ColumnId::Year => "Year",
            ColumnId::ReleaseYear => "Release year",
            ColumnId::ReleaseDate => "Release date",
            ColumnId::LastPlayed => "Last played",
            ColumnId::ReleaseType => "Type",
            ColumnId::DiscNumber => "Disc",
            ColumnId::TrackNumber => "Track",
            ColumnId::BitDepth => "Bit depth",
            ColumnId::BitRate => "Bitrate",
            ColumnId::Codec => "Codec",
            ColumnId::SampleRate => "Sample rate",
            ColumnId::Comment => "Comment",
            ColumnId::Channels => "Channels",
            ColumnId::Bpm => "BPM",
            ColumnId::DateAdded => "Date added",
            ColumnId::Path => "Path",
            ColumnId::PlayCount => "Plays",
            ColumnId::Size => "Size",
            ColumnId::SongCount => "Songs",
            ColumnId::AlbumCount => "Albums",
            ColumnId::Owner => "Owner",
            ColumnId::UserFavorite => "Favorite",
            ColumnId::UserRating => "Rating",
            ColumnId::Actions => "Actions",
        }
    }

    /// The name in the column picker (the combined title columns are told apart).
    pub fn picker_label(self) -> &'static str {
        match self {
            ColumnId::TitleCombined => "Title + artist + cover",
            ColumnId::TitleArtist => "Title + artist",
            ColumnId::Title => "Title (name only)",
            ColumnId::Artists => "Artist",
            other => other.label(),
        }
    }

    /// Feishin's default width and alignment.
    fn default_shape(self) -> (f32, Align) {
        match self {
            ColumnId::AlbumGroup => (240.0, Align::Start),
            ColumnId::RowIndex => (60.0, Align::Center),
            ColumnId::Image => (70.0, Align::Center),
            ColumnId::Title | ColumnId::TitleCombined | ColumnId::TitleArtist => (300.0, Align::Start),
            ColumnId::Duration => (100.0, Align::Center),
            ColumnId::Album
            | ColumnId::AlbumArtists
            | ColumnId::Artists
            | ColumnId::Genres
            | ColumnId::GenreBadge
            | ColumnId::Comment
            | ColumnId::Path
            | ColumnId::Composer
            | ColumnId::Owner => (300.0, Align::Start),
            ColumnId::Year | ColumnId::ReleaseYear => (200.0, Align::Center),
            ColumnId::Date | ColumnId::ReleaseDate => (240.0, Align::Center),
            ColumnId::LastPlayed => (140.0, Align::Center),
            ColumnId::DateAdded => (120.0, Align::Center),
            ColumnId::UserFavorite | ColumnId::Actions => (60.0, Align::Center),
            ColumnId::ReleaseType => (140.0, Align::Start),
            _ => (100.0, Align::Center),
        }
    }
}

impl ItemType {
    /// The columns (and card rows) that exist for this kind of row, in
    /// Feishin's default order.
    pub fn columns(self) -> &'static [ColumnId] {
        use ColumnId::*;
        match self {
            ItemType::Song => &[
                AlbumGroup,
                RowIndex,
                Image,
                Title,
                TitleCombined,
                TitleArtist,
                Duration,
                Album,
                AlbumArtists,
                Artists,
                Composer,
                Date,
                Genres,
                GenreBadge,
                Year,
                ReleaseYear,
                ReleaseDate,
                DiscNumber,
                TrackNumber,
                BitDepth,
                BitRate,
                Codec,
                SampleRate,
                LastPlayed,
                Comment,
                Channels,
                Bpm,
                DateAdded,
                Path,
                PlayCount,
                Size,
                UserFavorite,
                UserRating,
                Actions,
            ],
            ItemType::Album => &[
                RowIndex,
                Image,
                Title,
                Duration,
                AlbumArtists,
                Genres,
                GenreBadge,
                ReleaseYear,
                ReleaseDate,
                ReleaseType,
                SongCount,
                LastPlayed,
                PlayCount,
                UserFavorite,
                UserRating,
                Actions,
            ],
            ItemType::Artist => {
                &[RowIndex, Image, Title, AlbumCount, SongCount, UserFavorite, UserRating, Actions]
            }
            ItemType::Playlist => &[RowIndex, Image, Title, SongCount, Duration, Owner, Actions],
            ItemType::Genre => &[RowIndex, Title, AlbumCount, SongCount, Actions],
        }
    }

    /// Display types the list kind offers.
    pub fn display_types(self) -> &'static [DisplayType] {
        match self {
            ItemType::Song => &[DisplayType::Table],
            ItemType::Album => &[DisplayType::Table, DisplayType::Grid, DisplayType::Detail],
            ItemType::Artist | ItemType::Playlist => &[DisplayType::Table, DisplayType::Grid],
            ItemType::Genre => &[DisplayType::Table],
        }
    }

    fn enabled_by_default(self) -> &'static [ColumnId] {
        use ColumnId::*;
        match self {
            ItemType::Song => &[RowIndex, TitleCombined, Duration, Album, Genres, Year, UserFavorite],
            ItemType::Album => &[RowIndex, Image, Title, AlbumArtists, ReleaseYear, SongCount, UserFavorite],
            ItemType::Artist => &[RowIndex, Image, Title, AlbumCount, SongCount, UserFavorite],
            ItemType::Playlist => &[RowIndex, Image, Title, SongCount, Duration, Owner],
            ItemType::Genre => &[RowIndex, Title, AlbumCount, SongCount],
        }
    }

    /// The lines under a card by default (Feishin: name, artists, release year).
    fn card_rows(self) -> &'static [ColumnId] {
        use ColumnId::*;
        match self {
            ItemType::Song => &[Title, Artists, Album],
            ItemType::Album => &[Title, AlbumArtists, ReleaseYear],
            ItemType::Artist => &[Title, AlbumCount, SongCount],
            ItemType::Playlist => &[Title, SongCount, Owner],
            ItemType::Genre => &[Title, AlbumCount, SongCount],
        }
    }
}

/// One column of a table.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnConfig {
    pub id: ColumnId,
    pub enabled: bool,
    /// Width in px; with `TableConfig::auto_fit` it is the share of the free space.
    pub width: f32,
    pub align: Align,
    pub pinned: Option<Pin>,
    /// Fits its content (the width follows the longest loaded value).
    pub auto_size: bool,
}

impl Default for ColumnConfig {
    fn default() -> Self {
        Self::new(ColumnId::Title)
    }
}

impl ColumnConfig {
    pub fn new(id: ColumnId) -> Self {
        let (width, align) = id.default_shape();
        // The album group sits at the left edge (Feishin pins it there).
        let pinned = (id == ColumnId::AlbumGroup).then_some(Pin::Left);
        Self { id, enabled: false, width, align, pinned, auto_size: false }
    }
}

/// The table look and its columns (Feishin `ItemTableListProps`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TableConfig {
    pub columns: Vec<ColumnConfig>,
    /// Columns stretch to fill the width (otherwise they keep their width,
    /// shrinking only when they would not fit).
    pub auto_fit: bool,
    pub size: RowSize,
    pub header: bool,
    pub row_hover_highlight: bool,
    pub alternate_rows: bool,
    pub horizontal_borders: bool,
    pub vertical_borders: bool,
}

impl Default for TableConfig {
    fn default() -> Self {
        Self::for_item(ItemType::Song)
    }
}

impl TableConfig {
    pub fn for_item(item: ItemType) -> Self {
        let on = item.enabled_by_default();
        let columns = item
            .columns()
            .iter()
            .map(|id| ColumnConfig { enabled: on.contains(id), ..ColumnConfig::new(*id) })
            .collect();
        Self {
            columns,
            auto_fit: true,
            size: RowSize::Default,
            header: true,
            row_hover_highlight: true,
            alternate_rows: false,
            horizontal_borders: false,
            vertical_borders: false,
        }
    }

    /// The enabled columns in display order: left-pinned first, right-pinned last.
    pub fn visible(&self) -> Vec<ColumnConfig> {
        let enabled = || self.columns.iter().filter(|c| c.enabled);
        enabled()
            .filter(|c| c.pinned == Some(Pin::Left))
            .chain(enabled().filter(|c| c.pinned.is_none()))
            .chain(enabled().filter(|c| c.pinned == Some(Pin::Right)))
            .copied()
            .collect()
    }

    pub fn column(&self, id: ColumnId) -> Option<&ColumnConfig> {
        self.columns.iter().find(|c| c.id == id)
    }

    pub fn column_mut(&mut self, id: ColumnId) -> Option<&mut ColumnConfig> {
        self.columns.iter_mut().find(|c| c.id == id)
    }

    /// Moves `from` to where `to` is (Feishin: dropping a header on another header).
    pub fn reorder(&mut self, from: ColumnId, to: ColumnId) {
        let (Some(a), Some(b)) =
            (self.columns.iter().position(|c| c.id == from), self.columns.iter().position(|c| c.id == to))
        else {
            return;
        };
        if a != b {
            let col = self.columns.remove(a);
            self.columns.insert(b, col);
        }
    }

    /// Moves a column one step up (-1) or down (+1) among all columns.
    pub fn shift(&mut self, id: ColumnId, by: i32) {
        let Some(i) = self.columns.iter().position(|c| c.id == id) else { return };
        let j = (i as i32 + by).clamp(0, self.columns.len() as i32 - 1) as usize;
        self.columns.swap(i, j);
    }

    /// Adds the columns of `item` a saved config does not have yet (new
    /// versions), and drops the ones that do not exist for it.
    pub fn normalize(&mut self, item: ItemType) {
        self.columns.retain(|c| item.columns().contains(&c.id));
        for id in item.columns() {
            if !self.columns.iter().any(|c| c.id == *id) {
                self.columns.push(ColumnConfig::new(*id));
            }
        }
    }
}

/// One line under a card.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CardRow {
    pub id: ColumnId,
    pub enabled: bool,
    pub align: Align,
}

impl Default for CardRow {
    fn default() -> Self {
        Self { id: ColumnId::Title, enabled: false, align: Align::Start }
    }
}

/// Grid look (Feishin `grid`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridConfig {
    pub size: CardSize,
    pub gap: Gap,
    /// A fixed number of cards per row (otherwise by the width of the list).
    pub items_per_row_enabled: bool,
    pub items_per_row: u32,
    pub rows: Vec<CardRow>,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self::for_item(ItemType::Album)
    }
}

impl GridConfig {
    pub fn for_item(item: ItemType) -> Self {
        let defaults = item.card_rows();
        // The default lines first (enabled), then the other available columns.
        let mut rows: Vec<CardRow> =
            defaults.iter().map(|id| CardRow { id: *id, enabled: true, align: Align::Start }).collect();
        for id in item.columns() {
            let useful = !matches!(
                id,
                ColumnId::RowIndex
                    | ColumnId::Image
                    | ColumnId::TitleCombined
                    | ColumnId::TitleArtist
                    | ColumnId::AlbumGroup
                    | ColumnId::Actions
                    | ColumnId::UserFavorite
                    | ColumnId::UserRating
            );
            if useful && !rows.iter().any(|r| r.id == *id) {
                rows.push(CardRow { id: *id, enabled: false, align: Align::Start });
            }
        }
        Self { size: CardSize::Default, gap: Gap::Sm, items_per_row_enabled: false, items_per_row: 6, rows }
    }

    pub fn enabled_rows(&self) -> impl Iterator<Item = &CardRow> {
        self.rows.iter().filter(|r| r.enabled)
    }

    pub fn shift(&mut self, id: ColumnId, by: i32) {
        let Some(i) = self.rows.iter().position(|c| c.id == id) else { return };
        let j = (i as i32 + by).clamp(0, self.rows.len() as i32 - 1) as usize;
        self.rows.swap(i, j);
    }

    pub fn normalize(&mut self, item: ItemType) {
        self.rows.retain(|c| item.columns().contains(&c.id));
        self.items_per_row = self.items_per_row.clamp(1, 20);
    }
}

/// The settings of one list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ListConfig {
    pub display: DisplayType,
    pub pagination: Pagination,
    pub items_per_page: u32,
    pub table: TableConfig,
    pub grid: GridConfig,
}

impl Default for ListConfig {
    fn default() -> Self {
        ListKind::Songs.default_config()
    }
}

/// A list page. Settings are kept per kind: all playlists share one look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListKind {
    Songs,
    Albums,
    Artists,
    AlbumArtists,
    Playlists,
    Genres,
    /// The songs of one playlist.
    PlaylistSongs,
    /// The tracks of one album.
    AlbumSongs,
    GenreSongs,
    /// Songs of an artist (top / all songs).
    ArtistSongs,
    /// The queue on the Now Playing page.
    Queue,
    /// The queue in the side panel, the drawer and the full-screen player.
    SideQueue,
    SearchSongs,
    FavoriteSongs,
    /// The songs of the open folder.
    FolderSongs,
    History,
}

impl ListKind {
    /// The key in the config file.
    pub fn key(self) -> String {
        serde_json::to_value(self).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
    }

    pub fn item_type(self) -> ItemType {
        match self {
            ListKind::Songs
            | ListKind::PlaylistSongs
            | ListKind::AlbumSongs
            | ListKind::GenreSongs
            | ListKind::ArtistSongs
            | ListKind::Queue
            | ListKind::SideQueue
            | ListKind::SearchSongs
            | ListKind::FavoriteSongs
            | ListKind::FolderSongs
            | ListKind::History => ItemType::Song,
            ListKind::Albums => ItemType::Album,
            ListKind::Artists | ListKind::AlbumArtists => ItemType::Artist,
            ListKind::Playlists => ItemType::Playlist,
            ListKind::Genres => ItemType::Genre,
        }
    }

    /// Feishin's defaults per list: song lists in detail pages are compact tables,
    /// the library lists use the default row size; albums, artists and
    /// playlists open as grids.
    pub fn default_config(self) -> ListConfig {
        let item = self.item_type();
        let mut table = TableConfig::for_item(item);
        let mut display = DisplayType::Table;
        match self {
            ListKind::AlbumSongs => {
                // Track number, title, duration, favorite.
                table.size = RowSize::Compact;
                for c in &mut table.columns {
                    c.enabled = matches!(
                        c.id,
                        ColumnId::TrackNumber | ColumnId::Title | ColumnId::Duration | ColumnId::UserFavorite
                    );
                }
            }
            ListKind::Queue => {
                table.size = RowSize::Compact;
                for c in &mut table.columns {
                    c.enabled = matches!(
                        c.id,
                        ColumnId::RowIndex
                            | ColumnId::TitleCombined
                            | ColumnId::Album
                            | ColumnId::Duration
                            | ColumnId::UserFavorite
                            | ColumnId::Actions
                    );
                }
            }
            ListKind::SideQueue => {
                // The narrow panel: number, cover with title and artist, time, heart.
                table.header = false;
                for c in &mut table.columns {
                    c.enabled = matches!(
                        c.id,
                        ColumnId::RowIndex
                            | ColumnId::TitleCombined
                            | ColumnId::Duration
                            | ColumnId::UserFavorite
                    );
                    match c.id {
                        ColumnId::RowIndex => c.width = 44.0,
                        ColumnId::Duration => c.width = 70.0,
                        ColumnId::UserFavorite => c.width = 44.0,
                        _ => {}
                    }
                }
            }
            ListKind::Albums | ListKind::Artists | ListKind::AlbumArtists | ListKind::Playlists => {
                display = DisplayType::Grid;
            }
            _ => {}
        }
        ListConfig {
            display,
            pagination: Pagination::Infinite,
            items_per_page: 100,
            table,
            grid: GridConfig::for_item(item),
        }
    }
}

/// A fact the album group header can show (Feishin `AlbumGroupItem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlbumGroupItem {
    AlbumArtists,
    ReleaseDate,
    ReleaseYear,
    SongCount,
    Duration,
    ReleaseType,
    Genres,
    Size,
}

impl AlbumGroupItem {
    pub const ALL: [AlbumGroupItem; 8] = [
        AlbumGroupItem::AlbumArtists,
        AlbumGroupItem::ReleaseDate,
        AlbumGroupItem::ReleaseYear,
        AlbumGroupItem::SongCount,
        AlbumGroupItem::Duration,
        AlbumGroupItem::ReleaseType,
        AlbumGroupItem::Genres,
        AlbumGroupItem::Size,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AlbumGroupItem::AlbumArtists => "Album artists",
            AlbumGroupItem::ReleaseDate => "Release date",
            AlbumGroupItem::ReleaseYear => "Release year",
            AlbumGroupItem::SongCount => "Song count",
            AlbumGroupItem::Duration => "Duration",
            AlbumGroupItem::ReleaseType => "Release type",
            AlbumGroupItem::Genres => "Genres",
            AlbumGroupItem::Size => "Size",
        }
    }
}

/// One line of the album group header, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumGroupEntry {
    pub item: AlbumGroupItem,
    pub enabled: bool,
}

/// The header above an album's songs in a table with the "album group" column
/// (Feishin `albumGroupImageSize`, `albumGroupItems`,
/// `albumGroupShowFavoriteRating`, `albumGroupVerticalLayout`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlbumGroupConfig {
    /// Cover size in px; 0 shows no cover.
    pub image_size: u32,
    pub items: Vec<AlbumGroupEntry>,
    pub show_favorite_rating: bool,
    /// The cover above the facts (otherwise beside them).
    pub vertical_layout: bool,
}

impl Default for AlbumGroupConfig {
    fn default() -> Self {
        Self {
            image_size: 0,
            items: AlbumGroupItem::ALL
                .iter()
                .map(|item| AlbumGroupEntry { item: *item, enabled: *item == AlbumGroupItem::AlbumArtists })
                .collect(),
            show_favorite_rating: true,
            vertical_layout: true,
        }
    }
}

impl AlbumGroupConfig {
    /// The enabled facts in order.
    pub fn shown(&self) -> impl Iterator<Item = AlbumGroupItem> + '_ {
        self.items.iter().filter(|e| e.enabled).map(|e| e.item)
    }

    /// Moves a fact one step up (-1) or down (+1).
    pub fn shift(&mut self, item: AlbumGroupItem, by: i32) {
        let Some(i) = self.items.iter().position(|e| e.item == item) else { return };
        let j = (i as i32 + by).clamp(0, self.items.len() as i32 - 1) as usize;
        self.items.swap(i, j);
    }

    /// Adds the facts a saved config does not have (newer versions).
    pub fn normalize(&mut self) {
        for item in AlbumGroupItem::ALL {
            if !self.items.iter().any(|e| e.item == item) {
                self.items.push(AlbumGroupEntry { item, enabled: false });
            }
        }
        self.image_size = self.image_size.min(256);
    }
}

/// What a genre page lists (Feishin `genreTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenreTarget {
    Albums,
    #[default]
    Tracks,
}

/// What a playlist page lists (Feishin `playlistTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistTarget {
    /// The playlist's songs.
    #[default]
    Tracks,
    /// The songs grouped into album cards.
    Albums,
}

/// `settings.lists`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lists {
    /// Feishin `playButtonBehavior`.
    pub play_behavior: PlayBehavior,
    /// Clicking a grid cover selects it (off: it opens the item's page). Feishin
    /// "Grid multi-select".
    pub grid_multi_select: bool,
    /// Hearts (favourite buttons and columns) are shown. Feishin `showFavorites`.
    pub show_favorites: bool,
    /// Star ratings are shown. Feishin `showRatings`.
    pub show_ratings: bool,
    /// What a genre page lists. Feishin `genreTarget`.
    pub genre_target: GenreTarget,
    /// What a playlist page lists. Feishin `playlistTarget`.
    pub playlist_target: PlaylistTarget,
    /// "Add to playlist" skips songs the playlist already has (Navidrome, Subsonic).
    pub skip_duplicates: bool,
    /// Lists whose look the user changed, by [`ListKind::key`].
    pub lists: BTreeMap<String, ListConfig>,
    /// Saved list queries (header "Save as collection").
    pub collections: Vec<Collection>,
    /// The header above the songs of an album in a table with the album group column.
    pub album_group: AlbumGroupConfig,
    /// Each list's sort, order and filters at close (`albums`, `songs`, `genre_albums`, `genre_songs`);
    /// lists at their defaults are not stored.
    pub saved_queries: BTreeMap<String, Collection>,
}

impl Default for Lists {
    fn default() -> Self {
        Self {
            play_behavior: PlayBehavior::Now,
            grid_multi_select: true,
            show_favorites: true,
            show_ratings: true,
            genre_target: GenreTarget::Tracks,
            playlist_target: PlaylistTarget::Tracks,
            skip_duplicates: false,
            lists: BTreeMap::new(),
            collections: Vec::new(),
            album_group: AlbumGroupConfig::default(),
            saved_queries: BTreeMap::new(),
        }
    }
}

impl Lists {
    /// The settings of a list as shown: the saved ones (or the defaults), repaired,
    /// without the heart / star columns while `show_favorites` / `show_ratings` are off.
    pub fn config(&self, kind: ListKind) -> ListConfig {
        let mut config = self.raw_config(kind);
        for (shown, column) in
            [(self.show_favorites, ColumnId::UserFavorite), (self.show_ratings, ColumnId::UserRating)]
        {
            if !shown && let Some(c) = config.table.column_mut(column) {
                c.enabled = false;
            }
        }
        config
    }

    /// The settings of a list as saved (or the defaults), repaired.
    fn raw_config(&self, kind: ListKind) -> ListConfig {
        let item = kind.item_type();
        let mut config = self.lists.get(&kind.key()).cloned().unwrap_or_else(|| kind.default_config());
        config.table.normalize(item);
        config.grid.normalize(item);
        if !item.display_types().contains(&config.display) {
            config.display = DisplayType::Table;
        }
        config.items_per_page = config.items_per_page.clamp(10, 1000);
        config
    }

    /// Changes a list's settings; stores them when they differ from the defaults.
    pub fn edit(&mut self, kind: ListKind, f: impl FnOnce(&mut ListConfig)) {
        let mut config = self.raw_config(kind);
        f(&mut config);
        self.set(kind, config);
    }

    pub fn set(&mut self, kind: ListKind, config: ListConfig) {
        if config == kind.default_config() {
            self.lists.remove(&kind.key());
        } else {
            self.lists.insert(kind.key(), config);
        }
    }

    /// Back to Feishin's defaults for one list.
    pub fn reset(&mut self, kind: ListKind) {
        self.lists.remove(&kind.key());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hearts_and_stars_hide_with_the_toggles_and_come_back() {
        let mut lists = Lists::default();
        let shown = |l: &Lists, id| l.config(ListKind::Albums).table.visible().iter().any(|c| c.id == id);
        assert!(shown(&lists, ColumnId::UserFavorite));
        lists.edit(ListKind::Albums, |c| c.table.column_mut(ColumnId::UserRating).unwrap().enabled = true);
        assert!(shown(&lists, ColumnId::UserRating));
        lists.show_favorites = false;
        lists.show_ratings = false;
        assert!(!shown(&lists, ColumnId::UserFavorite) && !shown(&lists, ColumnId::UserRating));
        // Editing something else while hidden does not save the columns as off.
        lists.edit(ListKind::Albums, |c| c.table.header = false);
        lists.show_favorites = true;
        lists.show_ratings = true;
        assert!(shown(&lists, ColumnId::UserFavorite) && shown(&lists, ColumnId::UserRating));
    }

    #[test]
    fn new_settings_default_to_feishins() {
        let l: Lists = toml::from_str("").unwrap();
        assert!(l.show_favorites && l.show_ratings);
        assert_eq!(l.genre_target, GenreTarget::Tracks);
        assert_eq!(l.playlist_target, PlaylistTarget::Tracks);
        assert!(!l.skip_duplicates);
    }

    #[test]
    fn defaults_follow_feishin() {
        let songs = ListKind::Songs.default_config();
        let on: Vec<_> = songs.table.visible().iter().map(|c| c.id).collect();
        assert_eq!(
            on,
            [
                ColumnId::RowIndex,
                ColumnId::TitleCombined,
                ColumnId::Duration,
                ColumnId::Album,
                ColumnId::Genres,
                ColumnId::Year,
                ColumnId::UserFavorite
            ]
        );
        assert_eq!(songs.table.size.height(), 64.0);
        assert_eq!(ListKind::Albums.default_config().display, DisplayType::Grid);
        assert_eq!(ListKind::Songs.default_config().display, DisplayType::Table);
        assert_eq!(ListKind::AlbumSongs.default_config().table.size, RowSize::Compact);
        assert_eq!(songs.pagination, Pagination::Infinite);
        assert_eq!(songs.items_per_page, 100);
    }

    #[test]
    fn config_round_trips_through_toml() {
        let mut lists = Lists { play_behavior: PlayBehavior::Last, ..Lists::default() };
        lists.edit(ListKind::Songs, |c| {
            c.table.size = RowSize::Large;
            c.table.alternate_rows = true;
            let col = c.table.column_mut(ColumnId::Bpm).unwrap();
            col.enabled = true;
            col.pinned = Some(Pin::Right);
            col.align = Align::End;
            col.width = 123.0;
            c.table.reorder(ColumnId::Bpm, ColumnId::AlbumGroup);
        });
        lists.edit(ListKind::Albums, |c| {
            c.display = DisplayType::Detail;
            c.pagination = Pagination::Paginated;
            c.grid.size = CardSize::Large;
            c.grid.gap = Gap::Xl;
        });
        let text = toml::to_string_pretty(&lists).unwrap();
        let back: Lists = toml::from_str(&text).unwrap();
        assert_eq!(back, lists);
        let songs = back.config(ListKind::Songs);
        assert_eq!(songs.table.columns[0].id, ColumnId::Bpm);
        assert_eq!(songs.table.column(ColumnId::Bpm).unwrap().pinned, Some(Pin::Right));
        // Pinned right: last among the visible columns.
        assert_eq!(songs.table.visible().last().unwrap().id, ColumnId::Bpm);
        assert_eq!(back.config(ListKind::Albums).display, DisplayType::Detail);
        // Unchanged lists are not stored.
        assert_eq!(back.lists.len(), 2);
    }

    #[test]
    fn missing_fields_and_unknown_columns_are_repaired() {
        let lists: Lists = toml::from_str("[lists.songs]\ndisplay = \"grid\"\n").unwrap();
        // Songs have no grid.
        let config = lists.config(ListKind::Songs);
        assert_eq!(config.display, DisplayType::Table);
        assert!(config.table.header);
        // A saved config without a column (added later) gets it back, off.
        let mut partial = TableConfig::for_item(ItemType::Song);
        partial.columns.retain(|c| c.id != ColumnId::Bpm);
        partial.normalize(ItemType::Song);
        assert!(!partial.column(ColumnId::Bpm).unwrap().enabled);
        // Columns of another kind are dropped.
        partial.columns.push(ColumnConfig::new(ColumnId::AlbumCount));
        partial.normalize(ItemType::Song);
        assert!(partial.column(ColumnId::AlbumCount).is_none());
        // An empty section is the defaults.
        let empty: Lists = toml::from_str("").unwrap();
        assert_eq!(empty, Lists::default());
        assert_eq!(Lists::default().play_behavior, PlayBehavior::Now);
    }

    #[test]
    fn new_columns_exist_and_the_album_group_starts_pinned_left_and_off() {
        let songs = ListKind::Songs.default_config();
        for id in [
            ColumnId::AlbumGroup,
            ColumnId::Composer,
            ColumnId::Date,
            ColumnId::ReleaseDate,
            ColumnId::LastPlayed,
        ] {
            let c = songs.table.column(id).unwrap_or_else(|| panic!("{id:?} missing"));
            assert!(!c.enabled, "{id:?} is off by default");
        }
        assert_eq!(songs.table.column(ColumnId::AlbumGroup).unwrap().pinned, Some(Pin::Left));
        // Albums get the release date and the last play, not the song-only columns.
        let albums = ListKind::Albums.default_config();
        assert!(albums.table.column(ColumnId::ReleaseDate).is_some());
        assert!(albums.table.column(ColumnId::LastPlayed).is_some());
        assert!(albums.table.column(ColumnId::AlbumGroup).is_none());
        // Never offered as a line under a card.
        assert!(songs.grid.rows.iter().all(|r| r.id != ColumnId::AlbumGroup));
        // A config saved before they existed gets them back, off.
        let mut old = TableConfig::for_item(ItemType::Song);
        old.columns.retain(|c| !matches!(c.id, ColumnId::Composer | ColumnId::AlbumGroup));
        old.normalize(ItemType::Song);
        assert!(old.column(ColumnId::Composer).is_some_and(|c| !c.enabled));
    }

    #[test]
    fn the_album_group_header_has_feishins_defaults_and_round_trips() {
        let c = AlbumGroupConfig::default();
        assert_eq!(c.image_size, 0);
        assert!(c.show_favorite_rating && c.vertical_layout);
        assert_eq!(c.shown().collect::<Vec<_>>(), [AlbumGroupItem::AlbumArtists]);
        let mut c = c;
        c.shift(AlbumGroupItem::Genres, -9);
        c.items.iter_mut().find(|e| e.item == AlbumGroupItem::Genres).unwrap().enabled = true;
        assert_eq!(c.shown().next(), Some(AlbumGroupItem::Genres));
        let lists = Lists { album_group: c.clone(), ..Lists::default() };
        let back: Lists = toml::from_str(&toml::to_string_pretty(&lists).unwrap()).unwrap();
        assert_eq!(back.album_group, c);
        // A config from before the setting existed has the defaults.
        let old: Lists = toml::from_str("play_behavior = \"next\"\n").unwrap();
        assert_eq!(old.album_group, AlbumGroupConfig::default());
        // Missing facts (a newer version) come back, off.
        let mut partial = AlbumGroupConfig { items: vec![], ..Default::default() };
        partial.normalize();
        assert_eq!(partial.items.len(), AlbumGroupItem::ALL.len());
    }

    #[test]
    fn reordering_and_shifting() {
        let mut t = TableConfig::for_item(ItemType::Song);
        t.reorder(ColumnId::Duration, ColumnId::AlbumGroup);
        assert_eq!(t.columns[0].id, ColumnId::Duration);
        t.shift(ColumnId::Duration, 1);
        assert_eq!(t.columns[1].id, ColumnId::Duration);
        t.shift(ColumnId::Duration, -5);
        assert_eq!(t.columns[0].id, ColumnId::Duration);
        t.column_mut(ColumnId::Album).unwrap().pinned = Some(Pin::Left);
        assert_eq!(t.visible()[0].id, ColumnId::Album);
    }

    #[test]
    fn keys_are_stable() {
        assert_eq!(ListKind::Songs.key(), "songs");
        assert_eq!(ListKind::AlbumArtists.key(), "album_artists");
        assert_eq!(ListKind::PlaylistSongs.key(), "playlist_songs");
        assert_eq!(Gap::Md.larger(), Gap::Lg);
        assert_eq!(Gap::Xl.larger(), Gap::Xl);
        assert_eq!(Gap::Xs.smaller(), Gap::Xs);
    }
}
