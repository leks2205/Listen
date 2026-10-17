//! Sort, order, search and filters of the Tracks and Albums lists (Feishin's list headers): reloads,
//! "play all" of a query and saved queries (collections). UI in `ui/list/header.rs`; per-server options come
//! from the source (`MusicSource::album_sorts` / `album_filters`, ...).

use super::menu::{MenuAction, MenuMsg};
use super::*;
use crate::app::list::ListId;
use listen_core::settings::{Collection, CollectionList, PlayBehavior};
use listen_source::sorting::{self, QueryFilters, SortOption};
use listen_source::{AlbumQuery, AlbumSort, SongQuery, SongSort, SortOrder};

/// Rows fetched per request while loading everything for "play all".
const PLAY_ALL_PAGE: usize = 500;
/// A list longer than this is cut when played as a whole.
const PLAY_ALL_MAX: usize = 20_000;
/// Albums whose tracks are fetched at the same time for "play all" on albums.
const ALBUM_FETCHES: usize = 8;
/// The search box waits this long after the last key (Feishin debounces too).
pub const SEARCH_DEBOUNCE: Duration = Duration::from_millis(400);

/// Genre names and (id, name) of the album artists.
pub type FilterOptions = (Vec<String>, Vec<(String, String)>);

/// The album tags and the song tags of a server.
pub type TagOptions = (Vec<listen_source::TagFilter>, Vec<listen_source::TagFilter>);

/// Everything a filter panel can set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filters {
    pub favorite: Option<bool>,
    pub rated: Option<bool>,
    pub compilation: Option<bool>,
    pub recently_played: bool,
    pub genres: Vec<String>,
    /// (id, name)
    pub artists: Vec<(String, String)>,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    /// Custom tag filters: tag name and the picked value ids.
    pub tags: Vec<(String, Vec<String>)>,
}

impl Filters {
    /// True when any filter is set (the header's filter button lights up).
    pub fn active(&self) -> bool {
        *self != Filters::default()
    }

    /// Only what `supported` allows (a filter of another server is dropped).
    pub fn masked(&self, supported: QueryFilters) -> Filters {
        let mut f = self.clone();
        if !supported.contains(QueryFilters::FAVORITE) {
            f.favorite = None;
        }
        if !supported.contains(QueryFilters::RATED) {
            f.rated = None;
        }
        if !supported.contains(QueryFilters::COMPILATION) {
            f.compilation = None;
        }
        if !supported.contains(QueryFilters::RECENTLY_PLAYED) {
            f.recently_played = false;
        }
        if !supported.contains(QueryFilters::GENRES) {
            f.genres.clear();
        }
        if !supported.contains(QueryFilters::ARTISTS) {
            f.artists.clear();
        }
        if !supported.intersects(QueryFilters::YEAR | QueryFilters::YEAR_RANGE) {
            f.year_min = None;
            f.year_max = None;
        }
        if !supported.contains(QueryFilters::TAGS) {
            f.tags.clear();
        }
        if !supported.contains(QueryFilters::MULTI) {
            f.genres.truncate(1);
            f.artists.truncate(1);
        }
        if supported.contains(QueryFilters::EXCLUSIVE) {
            // The plain Subsonic lists take one of these at a time.
            f.keep_one_of_the_exclusive();
        }
        f
    }

    /// Which of favourite, artists, genres and year is set (in that priority).
    fn exclusive_in_use(&self) -> Option<Exclusive> {
        if self.favorite.is_some() {
            Some(Exclusive::Favorite)
        } else if !self.artists.is_empty() {
            Some(Exclusive::Artists)
        } else if !self.genres.is_empty() {
            Some(Exclusive::Genres)
        } else if self.year_min.is_some() || self.year_max.is_some() {
            Some(Exclusive::Year)
        } else {
            None
        }
    }

    /// Clears all but the first set of the mutually exclusive filters.
    fn keep_one_of_the_exclusive(&mut self) {
        let keep = self.exclusive_in_use();
        if keep != Some(Exclusive::Favorite) {
            self.favorite = None;
        }
        if keep != Some(Exclusive::Artists) {
            self.artists.clear();
        }
        if keep != Some(Exclusive::Genres) {
            self.genres.clear();
        }
        if keep != Some(Exclusive::Year) {
            self.year_min = None;
            self.year_max = None;
        }
    }

    /// Whether `which` cannot be edited now: on the plain Subsonic lists
    /// another of the exclusive filters is active (Feishin disables the rest).
    pub fn disabled(&self, supported: QueryFilters, which: Exclusive) -> bool {
        supported.contains(QueryFilters::EXCLUSIVE)
            && self.exclusive_in_use().is_some_and(|in_use| in_use != which)
    }
}

/// The filters the plain Subsonic album list can only set one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusive {
    Favorite,
    Artists,
    Genres,
    Year,
}

/// Sort, order, search and filters of one list.
#[derive(Debug, Clone, PartialEq)]
pub struct Query<S> {
    pub sort: S,
    pub order: SortOrder,
    pub search: String,
    pub filters: Filters,
}

impl<S: Copy> Query<S> {
    fn new(sort: S) -> Self {
        Self { sort, order: SortOrder::Ascending, search: String::new(), filters: Filters::default() }
    }
}

impl Query<AlbumSort> {
    pub fn to_source(&self, supported: QueryFilters, start: usize, limit: usize) -> AlbumQuery {
        let f = self.filters.masked(supported);
        AlbumQuery {
            sort: self.sort,
            order: self.order,
            favorite: f.favorite,
            has_rating: f.rated,
            compilation: f.compilation,
            recently_played: f.recently_played,
            genres: f.genres,
            artist_ids: f.artists.into_iter().map(|(id, _)| id).collect(),
            year_min: f.year_min,
            year_max: f.year_max,
            tags: f.tags,
            search: Some(self.search.trim().to_string()).filter(|s| !s.is_empty()),
            start,
            limit,
            ..Default::default()
        }
    }
}

impl Query<SongSort> {
    pub fn to_source(&self, supported: QueryFilters, start: usize, limit: usize) -> SongQuery {
        let f = self.filters.masked(supported);
        SongQuery {
            sort: self.sort,
            order: self.order,
            favorite: f.favorite,
            has_rating: f.rated,
            genres: f.genres,
            artist_ids: f.artists.into_iter().map(|(id, _)| id).collect(),
            year_min: f.year_min,
            year_max: f.year_max,
            tags: f.tags,
            search: Some(self.search.trim().to_string()).filter(|s| !s.is_empty()),
            start,
            limit,
            ..Default::default()
        }
    }
}

/// A floating list of the header (sort dropdown, collections).
#[derive(Debug, Clone, PartialEq)]
pub enum Popup {
    Sort { list: ListId, at: iced::Point },
    Collections { list: ListId, at: iced::Point },
}

/// Which filter changed (the filter modal).
#[derive(Debug, Clone, PartialEq)]
pub enum FilterEdit {
    Favorite(Option<bool>),
    Rated(Option<bool>),
    Compilation(Option<bool>),
    RecentlyPlayed(bool),
    /// Toggle a genre (single mode: it replaces the others).
    Genre(String),
    Artist(String, String),
    YearFrom(String),
    YearTo(String),
    /// The one-year field (Navidrome).
    Year(String),
    /// Toggle one value (id) of a custom tag.
    Tag(String, String),
    /// The tag whose values the modal lists.
    TagPick(String),
    MultiGenre(bool),
    MultiArtist(bool),
    OptionSearch(String),
    Reset,
}

#[derive(Debug, Clone)]
pub enum ListQueryMsg {
    /// The nth option of the source's sort list.
    Sort(ListId, usize),
    ToggleOrder(ListId),
    SearchInput(ListId, String),
    /// The debounce of the search box ran out (generation).
    SearchApply(ListId, u64),
    ClearSearch(ListId),
    /// The Refresh button: the source forgets its library cache, then the list reloads.
    Refresh(ListId),
    /// Reload after the source forgot its cache.
    Reload(ListId),
    OpenSort(ListId, iced::Point),
    OpenCollections(ListId, iced::Point),
    ClosePopup,
    OpenFilters(ListId),
    Filter(ListId, FilterEdit),
    /// Genre names and (id, name) of the album artists for the filter lists.
    OptionsLoaded(Result<FilterOptions, String>),
    /// The tags albums and songs can be filtered by.
    TagsLoaded(Result<TagOptions, String>),
    PlayAll(ListId, PlayBehavior, bool),
    CollectionName(String),
    SaveCollection(ListId),
    ApplyCollection(usize),
    DeleteCollection(usize),
}

/// `Listen::list_queries`.
#[derive(Debug)]
pub struct ListQueries {
    pub albums: Query<AlbumSort>,
    pub songs: Query<SongSort>,
    /// The albums and the tracks of the open genre page.
    pub genre_albums: Query<AlbumSort>,
    pub genre_songs: Query<SongSort>,
    pub popup: Option<Popup>,
    /// The list the filter modal edits.
    pub filter_list: Option<ListId>,
    pub genre_options: Vec<String>,
    pub artist_options: Vec<(String, String)>,
    pub options_loaded: bool,
    /// The tags the server can filter albums / songs by, once loaded.
    pub album_tags: Vec<listen_source::TagFilter>,
    pub song_tags: Vec<listen_source::TagFilter>,
    pub tags_loaded: bool,
    /// The tag whose values the filter modal lists.
    pub tag_pick: Option<String>,
    pub option_search: String,
    pub multi_genre: bool,
    pub multi_artist: bool,
    /// Text of the year fields (kept as typed).
    pub year_from: String,
    pub year_to: String,
    pub collection_name: String,
    /// Bumped per change: older loads and search timers are dropped.
    pub albums_gen: u64,
    pub songs_gen: u64,
    /// The same for the open genre page.
    pub genre_gen: u64,
    search_gen: u64,
    /// The queries of the servers that are not active (they come back with their library).
    parked: std::collections::HashMap<ServerId, ListQueries>,
}

impl Default for ListQueries {
    fn default() -> Self {
        Self {
            albums: Query::new(AlbumSort::Name),
            songs: Query::new(SongSort::Name),
            genre_albums: Query::new(AlbumSort::Name),
            genre_songs: Query::new(SongSort::Name),
            popup: None,
            filter_list: None,
            genre_options: vec![],
            artist_options: vec![],
            options_loaded: false,
            album_tags: vec![],
            song_tags: vec![],
            tags_loaded: false,
            tag_pick: None,
            option_search: String::new(),
            multi_genre: true,
            multi_artist: true,
            year_from: String::new(),
            year_to: String::new(),
            collection_name: String::new(),
            albums_gen: 0,
            songs_gen: 0,
            genre_gen: 0,
            search_gen: 0,
            parked: Default::default(),
        }
    }
}

/// The saved sort name of a sort (its variant name).
pub fn sort_key<S: std::fmt::Debug>(s: S) -> String {
    format!("{s:?}")
}

/// The sort named `key` among `options`.
pub fn sort_from_key<S: std::fmt::Debug + Copy>(options: &[SortOption<S>], key: &str) -> Option<S> {
    options.iter().map(|o| o.sort).find(|s| sort_key(*s) == key)
}

fn parse_year(text: &str) -> Option<i32> {
    text.trim().parse::<i32>().ok().filter(|y| (0..=5000).contains(y))
}

/// A list of songs (the Tracks page, the tracks of a genre).
pub fn is_songs(id: &ListId) -> bool {
    matches!(id, ListId::Songs | ListId::GenreSongs(_))
}

/// A list on a genre page: its genre is fixed, so it has no genre filter.
pub fn is_genre_list(id: &ListId) -> bool {
    matches!(id, ListId::Genre(_) | ListId::GenreSongs(_))
}

impl ListQueries {
    fn album_query(&self, id: &ListId) -> &Query<AlbumSort> {
        if is_genre_list(id) { &self.genre_albums } else { &self.albums }
    }

    fn album_query_mut(&mut self, id: &ListId) -> &mut Query<AlbumSort> {
        if is_genre_list(id) { &mut self.genre_albums } else { &mut self.albums }
    }

    fn song_query(&self, id: &ListId) -> &Query<SongSort> {
        if is_genre_list(id) { &self.genre_songs } else { &self.songs }
    }

    fn song_query_mut(&mut self, id: &ListId) -> &mut Query<SongSort> {
        if is_genre_list(id) { &mut self.genre_songs } else { &mut self.songs }
    }

    pub fn filters(&self, id: &ListId) -> &Filters {
        if is_songs(id) { &self.song_query(id).filters } else { &self.album_query(id).filters }
    }

    pub fn search(&self, id: &ListId) -> &str {
        if is_songs(id) { &self.song_query(id).search } else { &self.album_query(id).search }
    }

    pub fn order(&self, id: &ListId) -> SortOrder {
        if is_songs(id) { self.song_query(id).order } else { self.album_query(id).order }
    }

    fn filters_mut(&mut self, id: &ListId) -> &mut Filters {
        if is_songs(id) {
            &mut self.song_query_mut(id).filters
        } else {
            &mut self.album_query_mut(id).filters
        }
    }

    /// A genre page opens: its lists start with the sort and the filters they had last
    /// time, but without a search.
    pub fn open_genre(&mut self) {
        self.genre_albums.search.clear();
        self.genre_songs.search.clear();
    }

    /// Applies one edit of the filter modal.
    pub fn edit(&mut self, id: &ListId, edit: FilterEdit) {
        let (multi_genre, multi_artist) = (self.multi_genre, self.multi_artist);
        match edit {
            FilterEdit::OptionSearch(t) => {
                self.option_search = t;
                return;
            }
            FilterEdit::MultiGenre(on) => {
                self.multi_genre = on;
                if !on {
                    self.filters_mut(id).genres.truncate(1);
                }
                return;
            }
            FilterEdit::MultiArtist(on) => {
                self.multi_artist = on;
                if !on {
                    self.filters_mut(id).artists.truncate(1);
                }
                return;
            }
            FilterEdit::YearFrom(t) => {
                self.year_from = t.clone();
                self.filters_mut(id).year_min = parse_year(&t);
                return;
            }
            FilterEdit::YearTo(t) => {
                self.year_to = t.clone();
                self.filters_mut(id).year_max = parse_year(&t);
                return;
            }
            FilterEdit::Year(t) => {
                self.year_from = t.clone();
                self.year_to.clear();
                let y = parse_year(&t);
                let f = self.filters_mut(id);
                f.year_min = y;
                f.year_max = y;
                return;
            }
            FilterEdit::TagPick(name) => {
                self.tag_pick = Some(name);
                return;
            }
            FilterEdit::Reset => {
                *self.filters_mut(id) = Filters::default();
                self.year_from.clear();
                self.year_to.clear();
                return;
            }
            _ => {}
        }
        let f = self.filters_mut(id);
        match edit {
            FilterEdit::Tag(name, value) => match f.tags.iter().position(|(n, _)| *n == name) {
                Some(i) => {
                    let ids = &mut f.tags[i].1;
                    match ids.iter().position(|v| *v == value) {
                        Some(j) => {
                            ids.remove(j);
                        }
                        None => ids.push(value),
                    }
                    if f.tags[i].1.is_empty() {
                        f.tags.remove(i);
                    }
                }
                None => f.tags.push((name, vec![value])),
            },
            FilterEdit::Favorite(v) => f.favorite = v,
            FilterEdit::Rated(v) => f.rated = v,
            FilterEdit::Compilation(v) => f.compilation = v,
            FilterEdit::RecentlyPlayed(v) => f.recently_played = v,
            FilterEdit::Genre(name) => toggle(&mut f.genres, name, multi_genre),
            FilterEdit::Artist(aid, name) => {
                if let Some(i) = f.artists.iter().position(|(i, _)| *i == aid) {
                    f.artists.remove(i);
                } else {
                    if !multi_artist {
                        f.artists.clear();
                    }
                    f.artists.push((aid, name));
                }
            }
            _ => {}
        }
    }

    /// The collection for the current query of `id`.
    pub fn to_collection(&self, id: &ListId, name: String) -> Collection {
        if is_songs(id) {
            let q = self.song_query(id);
            collection_of(CollectionList::Songs, name, sort_key(q.sort), q)
        } else {
            let q = self.album_query(id);
            collection_of(CollectionList::Albums, name, sort_key(q.sort), q)
        }
    }

    /// Makes `c` the current query of its list; the list it is for.
    pub fn apply_collection(
        &mut self,
        c: &Collection,
        albums: &[SortOption<AlbumSort>],
        songs: &[SortOption<SongSort>],
    ) -> ListId {
        let (year_from, year_to) = year_texts(c.year_min, c.year_max);
        self.year_from = year_from;
        self.year_to = year_to;
        match c.list {
            CollectionList::Albums => {
                self.albums = query_of(c, |k| sort_from_key(albums, k), AlbumSort::Name);
                ListId::Albums
            }
            CollectionList::Songs => {
                self.songs = query_of(c, |k| sort_from_key(songs, k), SongSort::Name);
                ListId::Songs
            }
        }
    }

    /// The queries the lists had when the app was last closed.
    pub fn from_settings(lists: &listen_core::settings::Lists) -> Self {
        let mut q = Self::default();
        let get = |key: &str| lists.saved_queries.get(key);
        // Any server's sorts: one a server lacks falls back to its first when it is used.
        if let Some(c) = get("albums") {
            q.albums = query_of(c, |k| sort_from_key(sorting::NAVIDROME_ALBUMS, k), AlbumSort::Name);
        }
        if let Some(c) = get("songs") {
            q.songs = query_of(c, |k| sort_from_key(sorting::NAVIDROME_SONGS, k), SongSort::Name);
        }
        if let Some(c) = get("genre_albums") {
            q.genre_albums = query_of(c, |k| sort_from_key(sorting::NAVIDROME_ALBUMS, k), AlbumSort::Name);
        }
        if let Some(c) = get("genre_songs") {
            q.genre_songs = query_of(c, |k| sort_from_key(sorting::NAVIDROME_SONGS, k), SongSort::Name);
        }
        q
    }

    /// Writes the sort, order and filters of every list into the settings (a
    /// list with the defaults is not stored). Returns whether anything changed.
    pub fn store(&self, lists: &mut listen_core::settings::Lists) -> bool {
        let before = lists.saved_queries.clone();
        let default = Self::default();
        let mut put = |key: &str, c: Collection, is_default: bool| {
            if is_default {
                lists.saved_queries.remove(key);
            } else {
                lists.saved_queries.insert(key.to_string(), c);
            }
        };
        let a = Query { search: String::new(), ..self.albums.clone() };
        let g = Query { search: String::new(), ..self.genre_albums.clone() };
        let s = Query { search: String::new(), ..self.songs.clone() };
        let gs = Query { search: String::new(), ..self.genre_songs.clone() };
        let blank = |q: &Query<AlbumSort>| Query { search: String::new(), ..q.clone() };
        let blank_s = |q: &Query<SongSort>| Query { search: String::new(), ..q.clone() };
        put(
            "albums",
            collection_of(CollectionList::Albums, String::new(), sort_key(a.sort), &a),
            a == blank(&default.albums),
        );
        put(
            "genre_albums",
            collection_of(CollectionList::Albums, String::new(), sort_key(g.sort), &g),
            g == blank(&default.genre_albums),
        );
        put(
            "songs",
            collection_of(CollectionList::Songs, String::new(), sort_key(s.sort), &s),
            s == blank_s(&default.songs),
        );
        put(
            "genre_songs",
            collection_of(CollectionList::Songs, String::new(), sort_key(gs.sort), &gs),
            gs == blank_s(&default.genre_songs),
        );
        before != lists.saved_queries
    }
}

/// The text of the year fields for a range.
fn year_texts(min: Option<i32>, max: Option<i32>) -> (String, String) {
    let from = min.map(|y| y.to_string()).unwrap_or_default();
    let to = match (min, max) {
        (a, b) if a == b => String::new(),
        (_, b) => b.map(|y| y.to_string()).unwrap_or_default(),
    };
    (from, to)
}

fn filters_of(c: &Collection) -> Filters {
    Filters {
        favorite: c.favorite,
        rated: c.rated,
        compilation: c.compilation,
        recently_played: c.recently_played,
        genres: c.genres.clone(),
        artists: c.artists.clone(),
        year_min: c.year_min,
        year_max: c.year_max,
        tags: c.tags.clone(),
    }
}

fn query_of<S: Copy>(c: &Collection, parse: impl Fn(&str) -> Option<S>, default: S) -> Query<S> {
    Query {
        sort: parse(&c.sort).unwrap_or(default),
        order: if c.descending { SortOrder::Descending } else { SortOrder::Ascending },
        search: c.search.clone(),
        filters: filters_of(c),
    }
}

fn collection_of<S: Copy>(list: CollectionList, name: String, sort: String, q: &Query<S>) -> Collection {
    let f = &q.filters;
    Collection {
        name,
        list,
        sort,
        descending: q.order == SortOrder::Descending,
        search: q.search.clone(),
        favorite: f.favorite,
        rated: f.rated,
        compilation: f.compilation,
        recently_played: f.recently_played,
        genres: f.genres.clone(),
        artists: f.artists.clone(),
        year_min: f.year_min,
        year_max: f.year_max,
        tags: f.tags.clone(),
    }
}

fn toggle(list: &mut Vec<String>, name: String, multi: bool) {
    if let Some(i) = list.iter().position(|g| *g == name) {
        list.remove(i);
    } else {
        if !multi {
            list.clear();
        }
        list.push(name);
    }
}

/// The lists whose header has these controls.
pub fn has_controls(id: &ListId) -> bool {
    matches!(id, ListId::Songs | ListId::Albums | ListId::Genre(_) | ListId::GenreSongs(_))
}

/// Saved collections belong to the Albums and Tracks pages.
pub fn has_collections(id: &ListId) -> bool {
    matches!(id, ListId::Songs | ListId::Albums)
}

impl Listen {
    /// The sort options the active server offers for `id`, as (label, selected).
    /// Without a server (demo) the richest set, Navidrome's, is shown.
    pub fn sort_labels(&self, id: &ListId) -> Vec<(&'static str, bool)> {
        if is_songs(id) {
            let cur = self.list_queries.song_query(id).sort;
            self.song_sort_options().iter().map(|o| (o.label, o.sort == cur)).collect()
        } else {
            let cur = self.list_queries.album_query(id).sort;
            self.album_sort_options().iter().map(|o| (o.label, o.sort == cur)).collect()
        }
    }

    pub fn album_sort_options(&self) -> &'static [SortOption<AlbumSort>] {
        self.source.as_ref().map_or(sorting::NAVIDROME_ALBUMS, |s| s.album_sorts())
    }

    pub fn song_sort_options(&self) -> &'static [SortOption<SongSort>] {
        self.source.as_ref().map_or(sorting::NAVIDROME_SONGS, |s| s.song_sorts())
    }

    /// The label of the current sort ("Name"); a sort the server lacks shows the first option.
    pub fn sort_label(&self, id: &ListId) -> &'static str {
        if is_songs(id) {
            sorting::find(self.song_sort_options(), self.list_queries.song_query(id).sort).label
        } else {
            sorting::find(self.album_sort_options(), self.list_queries.album_query(id).sort).label
        }
    }

    /// What filters the active server supports on `id`.
    pub fn query_filters(&self, id: &ListId) -> QueryFilters {
        let supported = match &self.source {
            None => {
                QueryFilters::FAVORITE
                    | QueryFilters::RATED
                    | QueryFilters::GENRES
                    | QueryFilters::ARTISTS
                    | QueryFilters::YEAR_RANGE
                    | QueryFilters::MULTI
                    | QueryFilters::TAGS
                    | if is_songs(id) { QueryFilters::empty() } else { QueryFilters::COMPILATION }
            }
            Some(source) if is_songs(id) => source.song_filters(),
            Some(source) => source.album_filters(),
        };
        // A genre page has its genre already.
        if is_genre_list(id) { supported - QueryFilters::GENRES } else { supported }
    }

    /// One page of a genre's albums under the genre page's sort, filters and search.
    pub(super) fn genre_album_query(&self, genre: &str, start: usize, limit: usize) -> AlbumQuery {
        let id = ListId::Genre(genre.to_string());
        let mut q = self.list_queries.genre_albums.to_source(self.query_filters(&id), start, limit);
        q.sort = sorting::find(self.album_sort_options(), q.sort).sort;
        q.genre = Some(genre.to_string());
        q
    }

    pub(super) fn genre_song_query(&self, genre: &str, start: usize, limit: usize) -> SongQuery {
        let id = ListId::GenreSongs(genre.to_string());
        let mut q = self.list_queries.genre_songs.to_source(self.query_filters(&id), start, limit);
        q.sort = sorting::find(self.song_sort_options(), q.sort).sort;
        q.genre = Some(genre.to_string());
        q
    }

    /// The album query for one page of the Albums list.
    pub(super) fn album_page_query(&self, start: usize) -> AlbumQuery {
        let supported = self.query_filters(&ListId::Albums);
        let mut q = self.list_queries.albums.to_source(supported, start, ALBUM_PAGE);
        q.sort = sorting::find(self.album_sort_options(), q.sort).sort;
        q
    }

    pub(super) fn song_page_query(&self, start: usize) -> SongQuery {
        let supported = self.query_filters(&ListId::Songs);
        let mut q = self.list_queries.songs.to_source(supported, start, SONG_PAGE);
        q.sort = sorting::find(self.song_sort_options(), q.sort).sort;
        q
    }

    /// Another server becomes active: sort and filters belong to a server, so the
    /// one we leave is parked and the new one's comes back (defaults when new).
    pub(super) fn swap_list_queries(&mut self, to: &ServerId, from: Option<ServerId>) {
        let mut parked = std::mem::take(&mut self.list_queries.parked);
        if let Some(from) = from {
            parked.insert(from, std::mem::take(&mut self.list_queries));
        }
        self.list_queries =
            parked.remove(to).unwrap_or_else(|| ListQueries::from_settings(&self.config.settings.lists));
        self.list_queries.parked = parked;
    }

    /// Remembers the sort, order and filters of the lists across restarts.
    pub(super) fn save_queries(&mut self) {
        if self.list_queries.store(&mut self.config.settings.lists) {
            self.save_config();
        }
    }

    /// The rows of `id` are replaced: forget what was loaded and load again.
    pub fn reload_list(&mut self, id: &ListId) -> Task<Message> {
        let scroll = iced::widget::operation::scroll_to(
            id.scroll_id(),
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
        );
        self.lists.reset(id);
        self.lists.entry(id).scroll.offset = 0.0;
        match id {
            ListId::Genre(name) | ListId::GenreSongs(name) => {
                let songs = matches!(id, ListId::GenreSongs(_));
                let name = name.clone();
                let Some(g) = self.extra.genre.as_mut().filter(|g| g.name == name) else {
                    return scroll;
                };
                if songs {
                    g.songs.clear();
                    g.songs_load = crate::app::library_extra::Load::Idle;
                } else {
                    g.albums.clear();
                    g.albums_load = crate::app::library_extra::Load::Idle;
                }
                self.list_queries.genre_gen += 1;
                if self.demo {
                    self.demo_genre(&name, songs);
                    return scroll;
                }
                Task::batch([scroll, self.load_genre_target()])
            }
            ListId::Songs => {
                self.list_queries.songs_gen += 1;
                self.library.songs.clear();
                self.library.songs_done = false;
                self.library.songs_loading = false;
                if self.demo {
                    self.demo_songs();
                    return scroll;
                }
                Task::batch([scroll, self.load_songs()])
            }
            _ => {
                self.list_queries.albums_gen += 1;
                self.library.albums.clear();
                self.library.albums_done = false;
                self.library.albums_loading = false;
                if self.demo {
                    self.demo_albums();
                    return scroll;
                }
                Task::batch([scroll, self.load_albums()])
            }
        }
    }

    /// The demo genre page under its sort, filters and search (client side).
    pub(super) fn demo_genre(&mut self, name: &str, songs: bool) {
        use crate::app::library_extra::Load;
        if songs {
            let q = self.genre_song_query(name, 0, usize::MAX / 2);
            let mut all: Vec<Song> = (0..demo::ALBUMS.len()).flat_map(demo::album_songs).collect();
            for s in &mut all {
                for r in &mut s.artists {
                    r.id = Some(r.name.clone());
                }
            }
            let rows = listen_source::clientside::songs(all, &SongQuery { genre: None, ..q });
            if let Some(g) = self.extra.genre.as_mut().filter(|g| g.name == name) {
                g.songs = rows;
                g.songs_load = Load::Done;
            }
        } else {
            let q = self.genre_album_query(name, 0, usize::MAX / 2);
            let mut all = demo::albums();
            for a in &mut all {
                for r in &mut a.album_artists {
                    r.id = Some(r.name.clone());
                }
            }
            let rows = listen_source::clientside::albums(all, &AlbumQuery { genre: None, ..q });
            if let Some(g) = self.extra.genre.as_mut().filter(|g| g.name == name) {
                g.albums = rows;
                g.albums_load = Load::Done;
            }
        }
    }

    /// Demo mode has no server: filter and sort the demo rows in the client.
    fn demo_albums(&mut self) {
        let q = self.album_page_query(0);
        let mut all = demo::albums();
        for a in &mut all {
            for r in &mut a.album_artists {
                r.id = Some(r.name.clone());
            }
        }
        self.library.albums = listen_source::clientside::albums(all, &q);
        self.library.albums_done = true;
    }

    fn demo_songs(&mut self) {
        let q = self.song_page_query(0);
        let mut all: Vec<Song> = (0..demo::ALBUMS.len()).flat_map(demo::album_songs).collect();
        for s in &mut all {
            for r in &mut s.artists {
                r.id = Some(r.name.clone());
            }
        }
        self.library.songs = listen_source::clientside::songs(all, &q);
        self.library.songs_done = true;
    }

    /// The tags the server can filter by, for the custom tag filters (once per server).
    fn load_tag_filters(&mut self, id: &ListId) -> Task<Message> {
        let supported = self.query_filters(id);
        if self.list_queries.tags_loaded || !supported.contains(QueryFilters::TAGS) {
            return Task::none();
        }
        if self.demo {
            self.list_queries.tags_loaded = true;
            let tag = |name: &str, values: &[&str]| listen_source::TagFilter {
                name: name.into(),
                options: values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (format!("{name}{i}"), (*v).into()))
                    .collect(),
            };
            let tags = vec![
                tag("mood", &["Dreamy", "Nocturnal", "Upbeat"]),
                tag("recordlabel", &["Midnight Tapes", "Harbour Records"]),
            ];
            self.list_queries.album_tags = tags.clone();
            self.list_queries.song_tags = tags;
            return Task::none();
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        Task::perform(
            async move {
                let albums = source.tag_filters(listen_source::ItemKind::Album).await.map_err(err)?;
                let songs = source.tag_filters(listen_source::ItemKind::Song).await.unwrap_or_default();
                Ok((albums, songs))
            },
            self.scoped(|r| Message::ListQuery(ListQueryMsg::TagsLoaded(r))),
        )
    }

    /// Genres and album artists for the filter lists (once per server).
    fn load_filter_options(&mut self) -> Task<Message> {
        if self.list_queries.options_loaded {
            return Task::none();
        }
        if self.demo {
            self.list_queries.options_loaded = true;
            let mut genres: Vec<String> = demo::albums().iter().flat_map(|a| a.genres.clone()).collect();
            genres.sort();
            genres.dedup();
            self.list_queries.genre_options = genres;
            let mut artists: Vec<(String, String)> = demo::albums()
                .iter()
                .flat_map(|a| a.album_artists.iter().map(|r| (r.name.clone(), r.name.clone())))
                .collect();
            artists.sort_by_key(|(_, n)| n.to_lowercase());
            artists.dedup();
            self.list_queries.artist_options = artists;
            return Task::none();
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        Task::perform(
            async move {
                let genres = source
                    .genres()
                    .await
                    .map(|g| g.into_iter().map(|g| g.name).collect())
                    .unwrap_or_default();
                let artists = source
                    .artists_query(&listen_source::ArtistQuery { limit: 5000, ..Default::default() })
                    .await
                    .map_err(err)?
                    .items
                    .into_iter()
                    .map(|a| (a.id, a.name))
                    .collect();
                Ok((genres, artists))
            },
            self.scoped(|r| Message::ListQuery(ListQueryMsg::OptionsLoaded(r))),
        )
    }

    pub(super) fn on_list_query(&mut self, msg: ListQueryMsg) -> Task<Message> {
        match msg {
            ListQueryMsg::Sort(id, index) => {
                self.list_queries.popup = None;
                if is_songs(&id) {
                    let Some(o) = self.song_sort_options().get(index).copied() else {
                        return Task::none();
                    };
                    let q = self.list_queries.song_query_mut(&id);
                    q.sort = o.sort;
                    q.order = o.default_order;
                } else {
                    let Some(o) = self.album_sort_options().get(index).copied() else {
                        return Task::none();
                    };
                    let q = self.list_queries.album_query_mut(&id);
                    q.sort = o.sort;
                    q.order = o.default_order;
                }
                self.save_queries();
                return self.reload_list(&id);
            }
            ListQueryMsg::ToggleOrder(id) => {
                let flip = |o: SortOrder| match o {
                    SortOrder::Ascending => SortOrder::Descending,
                    SortOrder::Descending => SortOrder::Ascending,
                };
                if is_songs(&id) {
                    let q = self.list_queries.song_query_mut(&id);
                    q.order = flip(q.order);
                } else {
                    let q = self.list_queries.album_query_mut(&id);
                    q.order = flip(q.order);
                }
                self.save_queries();
                return self.reload_list(&id);
            }
            ListQueryMsg::SearchInput(id, text) => {
                if is_songs(&id) {
                    self.list_queries.song_query_mut(&id).search = text;
                } else {
                    self.list_queries.album_query_mut(&id).search = text;
                }
                self.list_queries.search_gen += 1;
                let gen_ = self.list_queries.search_gen;
                if self.demo {
                    // No timer in demo (tests): the list follows at once.
                    return self.on_list_query(ListQueryMsg::SearchApply(id, gen_));
                }
                return Task::perform(
                    async move {
                        tokio::time::sleep(SEARCH_DEBOUNCE).await;
                        (id, gen_)
                    },
                    |(id, gen_)| Message::ListQuery(ListQueryMsg::SearchApply(id, gen_)),
                );
            }
            ListQueryMsg::SearchApply(id, gen_) => {
                if gen_ == self.list_queries.search_gen {
                    return self.reload_list(&id);
                }
            }
            ListQueryMsg::ClearSearch(id) => {
                if is_songs(&id) {
                    self.list_queries.song_query_mut(&id).search.clear();
                } else {
                    self.list_queries.album_query_mut(&id).search.clear();
                }
                self.list_queries.search_gen += 1;
                return self.reload_list(&id);
            }
            ListQueryMsg::Refresh(id) => {
                // An explicit refresh bypasses what the source caches (YouTube Music's
                // library); the reload that follows is the one request burst.
                let Some(source) = self.source.clone().filter(|_| !self.demo) else {
                    return self.reload_list(&id);
                };
                return Task::perform(async move { source.refresh().await }, move |()| {
                    Message::ListQuery(ListQueryMsg::Reload(id.clone()))
                });
            }
            ListQueryMsg::Reload(id) => return self.reload_list(&id),
            ListQueryMsg::OpenSort(list, at) => self.list_queries.popup = Some(Popup::Sort { list, at }),
            ListQueryMsg::OpenCollections(list, at) => {
                self.list_queries.popup = Some(Popup::Collections { list, at })
            }
            ListQueryMsg::ClosePopup => self.list_queries.popup = None,
            ListQueryMsg::OpenFilters(id) => {
                self.list_queries.popup = None;
                let f = self.list_queries.filters(&id).clone();
                let (from, to) = year_texts(f.year_min, f.year_max);
                self.list_queries.year_from = from;
                self.list_queries.year_to = to;
                self.list_queries.filter_list = Some(id.clone());
                self.list_queries.tag_pick = None;
                self.list_queries.option_search.clear();
                self.open_modal("list-filters");
                return Task::batch([self.load_filter_options(), self.load_tag_filters(&id)]);
            }
            ListQueryMsg::Filter(id, edit) => {
                let reload = !matches!(
                    edit,
                    FilterEdit::OptionSearch(_) | FilterEdit::MultiGenre(_) | FilterEdit::MultiArtist(_)
                );
                self.list_queries.edit(&id, edit);
                if reload {
                    self.save_queries();
                    return self.reload_list(&id);
                }
            }
            ListQueryMsg::OptionsLoaded(result) => match result {
                Ok((genres, artists)) => {
                    self.list_queries.genre_options = genres;
                    self.list_queries.artist_options = artists;
                    self.list_queries.options_loaded = true;
                }
                Err(e) => tracing::warn!("filter options: {e}"),
            },
            ListQueryMsg::TagsLoaded(result) => match result {
                Ok((albums, songs)) => {
                    self.list_queries.album_tags = albums;
                    self.list_queries.song_tags = songs;
                    self.list_queries.tags_loaded = true;
                }
                Err(e) => tracing::warn!("tag filters: {e}"),
            },
            ListQueryMsg::PlayAll(id, behavior, shuffle) => return self.play_all(&id, behavior, shuffle),
            ListQueryMsg::CollectionName(name) => self.list_queries.collection_name = name,
            ListQueryMsg::SaveCollection(id) => {
                let name = self.list_queries.collection_name.trim().to_string();
                if name.is_empty() {
                    self.toast_warning("Give the collection a name.".into());
                    return Task::none();
                }
                let c = self.list_queries.to_collection(&id, name.clone());
                let list = &mut self.config.settings.lists.collections;
                // Same name and list: override (Feishin's "override existing").
                match list.iter_mut().find(|x| x.name == name && x.list == c.list) {
                    Some(existing) => *existing = c,
                    None => list.push(c),
                }
                self.save_config();
                self.list_queries.collection_name.clear();
                self.list_queries.popup = None;
                self.toast_success(format!("Saved collection {name}."));
            }
            ListQueryMsg::ApplyCollection(index) => {
                self.list_queries.popup = None;
                let Some(c) = self.config.settings.lists.collections.get(index).cloned() else {
                    return Task::none();
                };
                let (albums, songs) = (self.album_sort_options(), self.song_sort_options());
                let id = self.list_queries.apply_collection(&c, albums, songs);
                let route = if id == ListId::Songs { Route::Tracks } else { Route::Albums };
                let go = if self.route == route { Task::none() } else { self.navigate(route) };
                return Task::batch([go, self.reload_list(&id)]);
            }
            ListQueryMsg::DeleteCollection(index) => {
                let list = &mut self.config.settings.lists.collections;
                if index < list.len() {
                    list.remove(index);
                    self.save_config();
                }
            }
        }
        Task::none()
    }

    /// The header Play button: everything the current sort, filters and search match.
    fn play_all(&mut self, id: &ListId, behavior: PlayBehavior, shuffle: bool) -> Task<Message> {
        let action = match (behavior, shuffle) {
            (PlayBehavior::Now, true) => MenuAction::Shuffle,
            (PlayBehavior::Now, false) => MenuAction::Play,
            (PlayBehavior::Next, _) => MenuAction::PlayNext,
            (PlayBehavior::Last, _) => MenuAction::AddToQueue,
        };
        // Next / last shuffled: the songs are shuffled here (Shuffle itself is Listen's shuffle play).
        let pre_shuffle = shuffle && !matches!(action, MenuAction::Shuffle);
        if self.demo {
            let genre = self.extra.genre.as_ref();
            let songs = match id {
                ListId::Songs => self.library.songs.clone(),
                ListId::GenreSongs(_) => genre.map(|g| g.songs.clone()).unwrap_or_default(),
                ListId::Genre(_) => genre
                    .map(|g| g.albums.iter().flat_map(|a| demo_album_songs(&a.id)).collect())
                    .unwrap_or_default(),
                _ => self.library.albums.iter().flat_map(|a| a.songs.clone()).collect(),
            };
            let songs = if songs.is_empty() && *id == ListId::Albums {
                self.library.albums.iter().flat_map(|a| demo_album_songs(&a.id)).collect()
            } else {
                songs
            };
            return self.apply_to_songs(action, songs);
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        let kind = id.clone();
        let (aq, sq) = match id {
            ListId::Genre(name) | ListId::GenreSongs(name) => (
                self.genre_album_query(name, 0, PLAY_ALL_PAGE),
                self.genre_song_query(name, 0, PLAY_ALL_PAGE),
            ),
            _ => (self.album_page_query(0), self.song_page_query(0)),
        };
        self.toast_info("Loading the list…".into());
        Task::perform(
            async move {
                let mut songs = if is_songs(&kind) {
                    fetch_all_songs(&*source, sq).await?
                } else {
                    fetch_all_album_songs(&source, aq).await?
                };
                if pre_shuffle {
                    use rand::seq::SliceRandom;
                    songs.shuffle(&mut rand::rng());
                }
                Ok(songs)
            },
            move |r: Result<Vec<Song>, String>| Message::Menu(MenuMsg::Songs(action.clone(), r)),
        )
    }
}

fn demo_album_songs(id: &str) -> Vec<Song> {
    demo::albums().iter().position(|a| a.id == id).map(demo::album_songs).unwrap_or_default()
}

async fn fetch_all_songs(source: &dyn MusicSource, mut q: SongQuery) -> Result<Vec<Song>, String> {
    let mut out = Vec::new();
    q.limit = PLAY_ALL_PAGE;
    loop {
        q.start = out.len();
        let page = source.songs(&q).await.map_err(err)?;
        let got = page.items.len();
        let more = page.has_more() && got > 0;
        out.extend(page.items);
        if !more || got < PLAY_ALL_PAGE.min(100) || out.len() >= PLAY_ALL_MAX {
            break;
        }
    }
    out.truncate(PLAY_ALL_MAX);
    Ok(out)
}

async fn fetch_all_album_songs(
    source: &Arc<dyn MusicSource>,
    mut q: AlbumQuery,
) -> Result<Vec<Song>, String> {
    let mut albums: Vec<Album> = Vec::new();
    q.limit = PLAY_ALL_PAGE;
    loop {
        q.start = albums.len();
        let page = source.albums(&q).await.map_err(err)?;
        let got = page.items.len();
        let more = page.has_more() && got > 0;
        albums.extend(page.items);
        if !more || got < PLAY_ALL_PAGE.min(100) || albums.len() >= PLAY_ALL_MAX {
            break;
        }
    }
    let mut songs = Vec::new();
    for chunk in albums.chunks(ALBUM_FETCHES) {
        let handles: Vec<_> = chunk
            .iter()
            .map(|a| {
                let (source, id) = (source.clone(), a.id.clone());
                tokio::spawn(async move { source.album(&id).await })
            })
            .collect();
        for h in handles {
            match h.await {
                Ok(Ok(album)) => songs.extend(album.songs),
                Ok(Err(e)) => return Err(err(e)),
                Err(e) => return Err(e.to_string()),
            }
        }
        if songs.len() >= PLAY_ALL_MAX {
            break;
        }
    }
    songs.truncate(PLAY_ALL_MAX);
    Ok(songs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_masked_per_server() {
        let f = Filters {
            favorite: Some(true),
            rated: Some(true),
            compilation: Some(false),
            recently_played: true,
            genres: vec!["a".into(), "b".into()],
            artists: vec![("1".into(), "x".into()), ("2".into(), "y".into())],
            year_min: Some(1990),
            year_max: Some(1999),
            tags: vec![("mood".into(), vec!["t1".into()])],
        };
        assert!(f.active() && !Filters::default().active());
        let only_fav = f.masked(QueryFilters::FAVORITE);
        assert_eq!(only_fav.favorite, Some(true));
        assert_eq!((only_fav.rated, only_fav.compilation, only_fav.recently_played), (None, None, false));
        assert!(only_fav.genres.is_empty() && only_fav.artists.is_empty() && only_fav.year_min.is_none());
        // Without MULTI one genre / artist is kept.
        let single = f.masked(QueryFilters::GENRES | QueryFilters::ARTISTS | QueryFilters::YEAR_RANGE);
        assert_eq!((single.genres.len(), single.artists.len()), (1, 1));
        assert_eq!(single.year_max, Some(1999));
        // Tags only where the server has them.
        assert!(only_fav.tags.is_empty());
        assert_eq!(f.masked(QueryFilters::TAGS).tags.len(), 1);
    }

    #[test]
    fn plain_subsonic_takes_one_of_favorite_artists_genres_year() {
        let plain = QueryFilters::FAVORITE
            | QueryFilters::GENRES
            | QueryFilters::ARTISTS
            | QueryFilters::YEAR_RANGE
            | QueryFilters::EXCLUSIVE;
        let mut f = Filters { genres: vec!["Rock".into()], ..Default::default() };
        // With genres picked the other three are off (Feishin disables them).
        assert!(f.disabled(plain, Exclusive::Favorite));
        assert!(f.disabled(plain, Exclusive::Artists));
        assert!(f.disabled(plain, Exclusive::Year));
        assert!(!f.disabled(plain, Exclusive::Genres), "the active one stays editable");
        // Navidrome has no such rule.
        assert!(!f.disabled(QueryFilters::GENRES | QueryFilters::FAVORITE, Exclusive::Favorite));
        // Nothing set: nothing disabled.
        assert!(!Filters::default().disabled(plain, Exclusive::Year));
        // A saved state with several is cut down to one when it is used.
        f.favorite = Some(true);
        f.year_min = Some(1990);
        f.artists = vec![("1".into(), "A".into())];
        let m = f.masked(plain);
        assert_eq!(m.favorite, Some(true), "favourite wins");
        assert!(m.genres.is_empty() && m.artists.is_empty() && m.year_min.is_none());
    }

    #[test]
    fn tag_values_toggle_and_reach_the_source_query() {
        let mut lq = ListQueries::default();
        lq.edit(&ListId::Albums, FilterEdit::Tag("mood".into(), "a".into()));
        lq.edit(&ListId::Albums, FilterEdit::Tag("mood".into(), "b".into()));
        lq.edit(&ListId::Albums, FilterEdit::Tag("recordlabel".into(), "x".into()));
        assert_eq!(lq.albums.filters.tags.len(), 2);
        let q = lq.albums.to_source(QueryFilters::TAGS, 0, 10);
        assert_eq!(q.tags[0], ("mood".to_string(), vec!["a".to_string(), "b".to_string()]));
        lq.edit(&ListId::Albums, FilterEdit::Tag("mood".into(), "a".into()));
        lq.edit(&ListId::Albums, FilterEdit::Tag("mood".into(), "b".into()));
        assert_eq!(lq.albums.filters.tags.len(), 1, "a tag with no values is dropped");
        assert!(lq.albums.filters.active());
        // Not sent to a server without tags; the Tracks list has its own.
        assert!(lq.albums.to_source(QueryFilters::empty(), 0, 10).tags.is_empty());
        assert!(!lq.songs.filters.active());
        lq.edit(&ListId::Albums, FilterEdit::Reset);
        assert!(!lq.albums.filters.active());
    }

    #[test]
    fn queries_survive_a_restart_through_the_settings() {
        let mut lq = ListQueries::default();
        let mut lists = listen_core::settings::Lists::default();
        assert!(!lq.store(&mut lists), "defaults store nothing");
        assert!(lists.saved_queries.is_empty());
        lq.albums.sort = AlbumSort::Year;
        lq.albums.order = SortOrder::Descending;
        lq.albums.search = "not saved".into();
        lq.edit(&ListId::Albums, FilterEdit::Favorite(Some(true)));
        lq.edit(&ListId::Albums, FilterEdit::Tag("mood".into(), "t".into()));
        lq.songs.sort = SongSort::Bpm;
        lq.genre_albums.order = SortOrder::Descending;
        assert!(lq.store(&mut lists));
        assert_eq!(lists.saved_queries.len(), 3, "the untouched genre songs list is not stored");
        // Through the file and back.
        let text = toml::to_string_pretty(&lists).unwrap();
        let back: listen_core::settings::Lists = toml::from_str(&text).unwrap();
        let restored = ListQueries::from_settings(&back);
        assert_eq!(restored.albums.sort, AlbumSort::Year);
        assert_eq!(restored.albums.order, SortOrder::Descending);
        assert_eq!(restored.albums.filters.favorite, Some(true));
        assert_eq!(restored.albums.filters.tags.len(), 1);
        assert!(restored.albums.search.is_empty(), "the search is per session");
        assert_eq!(restored.songs.sort, SongSort::Bpm);
        assert_eq!(restored.genre_albums.order, SortOrder::Descending);
        assert_eq!(restored.genre_songs, Query::new(SongSort::Name));
        // Going back to the defaults removes the entry again.
        lq.albums = Query::new(AlbumSort::Name);
        lq.songs = Query::new(SongSort::Name);
        lq.genre_albums = Query::new(AlbumSort::Name);
        assert!(lq.store(&mut lists));
        assert!(lists.saved_queries.is_empty());
        // An old config without the section is the defaults.
        let old: listen_core::settings::Lists = toml::from_str("").unwrap();
        assert_eq!(ListQueries::from_settings(&old).albums, Query::new(AlbumSort::Name));
    }

    #[test]
    fn query_becomes_a_source_query() {
        let mut q = Query::new(AlbumSort::Year);
        q.order = SortOrder::Descending;
        q.search = "  abba ".into();
        q.filters.artists = vec![("7".into(), "ABBA".into())];
        q.filters.favorite = Some(false);
        let s = q.to_source(QueryFilters::FAVORITE | QueryFilters::ARTISTS | QueryFilters::MULTI, 200, 100);
        assert_eq!((s.sort, s.order, s.start, s.limit), (AlbumSort::Year, SortOrder::Descending, 200, 100));
        assert_eq!(s.search.as_deref(), Some("abba"));
        assert_eq!((s.artist_ids, s.favorite), (vec!["7".to_string()], Some(false)));
        assert!(Query::new(SongSort::Name).to_source(QueryFilters::empty(), 0, 10).search.is_none());
    }

    #[test]
    fn edits_follow_single_and_multi_mode() {
        let mut lq = ListQueries::default();
        lq.edit(&ListId::Albums, FilterEdit::Genre("Rock".into()));
        lq.edit(&ListId::Albums, FilterEdit::Genre("Pop".into()));
        assert_eq!(lq.albums.filters.genres, ["Rock", "Pop"]);
        lq.edit(&ListId::Albums, FilterEdit::Genre("Rock".into()));
        assert_eq!(lq.albums.filters.genres, ["Pop"]);
        lq.edit(&ListId::Albums, FilterEdit::MultiGenre(false));
        lq.edit(&ListId::Albums, FilterEdit::Genre("Jazz".into()));
        assert_eq!(lq.albums.filters.genres, ["Jazz"]);
        lq.edit(&ListId::Albums, FilterEdit::Artist("1".into(), "A".into()));
        lq.edit(&ListId::Albums, FilterEdit::Artist("2".into(), "B".into()));
        assert_eq!(lq.albums.filters.artists.len(), 2);
        lq.edit(&ListId::Albums, FilterEdit::MultiArtist(false));
        assert_eq!(lq.albums.filters.artists.len(), 1);
        // The Tracks list has its own filters.
        assert!(!lq.songs.filters.active());
        lq.edit(&ListId::Songs, FilterEdit::YearFrom("1980".into()));
        lq.edit(&ListId::Songs, FilterEdit::YearTo("abc".into()));
        assert_eq!((lq.songs.filters.year_min, lq.songs.filters.year_max), (Some(1980), None));
        lq.edit(&ListId::Songs, FilterEdit::Year("1999".into()));
        assert_eq!((lq.songs.filters.year_min, lq.songs.filters.year_max), (Some(1999), Some(1999)));
        lq.edit(&ListId::Albums, FilterEdit::Reset);
        assert!(!lq.albums.filters.active());
    }

    #[test]
    fn collections_round_trip() {
        let mut lq = ListQueries::default();
        lq.albums.sort = AlbumSort::RecentlyAdded;
        lq.albums.order = SortOrder::Descending;
        lq.albums.search = "x".into();
        lq.edit(&ListId::Albums, FilterEdit::Favorite(Some(true)));
        lq.edit(&ListId::Albums, FilterEdit::YearFrom("1990".into()));
        let c = lq.to_collection(&ListId::Albums, "Faves".into());
        assert_eq!((c.sort.as_str(), c.descending, c.list), ("RecentlyAdded", true, CollectionList::Albums));
        let mut other = ListQueries::default();
        let id = other.apply_collection(&c, sorting::NAVIDROME_ALBUMS, sorting::NAVIDROME_SONGS);
        assert_eq!(id, ListId::Albums);
        assert_eq!(other.albums, lq.albums);
        assert_eq!(other.year_from, "1990");
        // A sort the server lacks falls back to the name.
        let mut c2 = c.clone();
        c2.sort = "Rating".into();
        other.apply_collection(&c2, sorting::SUBSONIC_ALBUMS, sorting::SUBSONIC_SONGS);
        assert_eq!(other.albums.sort, AlbumSort::Name);
    }

    #[test]
    fn sort_keys_are_variant_names() {
        assert_eq!(sort_key(SongSort::RecentlyPlayed), "RecentlyPlayed");
        assert_eq!(sort_from_key(sorting::NAVIDROME_SONGS, "Bpm"), Some(SongSort::Bpm));
        assert_eq!(sort_from_key(sorting::SUBSONIC_SONGS, "Bpm"), None);
    }
}
