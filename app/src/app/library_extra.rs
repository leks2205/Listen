//! Genres, the folder browser, the favourites page and favouriting / rating of albums and artists:
//! state ([`Extra`]), messages ([`ExtraMsg`]) and loading for `ui/browse.rs` and `ui/favorites.rs`.
//!
//! * `Listen::set_item_favorite` / `set_item_rating`: change an album or artist on its server and in
//!   every loaded copy; a failure reverts and shows a toast. `ExtraMsg::Favorite` / `Rate` wrap them.
//! * `ui::item_controls::{heart, stars}`: the controls, honouring the Show favourites / ratings settings.

use super::list::{FavoritesTab, ListData, ListId};
use super::menu::{MenuAction, MenuMsg};
use super::*;
use listen_core::model::{Folder, FolderEntry, Genre};
use listen_core::settings::{GenreTarget, PlayBehavior};
use listen_source::{Capabilities, ItemKind};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

const GENRE_PAGE: usize = 200;
/// Songs a recursive folder play collects at most, and folders it visits.
const FOLDER_SONG_CAP: usize = 3000;
const FOLDER_VISIT_CAP: usize = 400;
/// Songs a "play genre" fetches.
const GENRE_PLAY_CAP: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenreSort {
    Name,
    Albums,
    Songs,
}

impl GenreSort {
    pub const ALL: [GenreSort; 3] = [GenreSort::Name, GenreSort::Albums, GenreSort::Songs];

    pub fn label(self) -> &'static str {
        match self {
            GenreSort::Name => "Name",
            GenreSort::Albums => "Album count",
            GenreSort::Songs => "Song count",
        }
    }
}

/// Where a part of the data stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Load {
    #[default]
    Idle,
    Loading,
    Done,
}

/// The open genre's albums and songs, loaded page by page.
#[derive(Debug, Default)]
pub struct GenreView {
    pub name: String,
    pub albums: Vec<Album>,
    pub albums_load: Load,
    pub songs: Vec<Song>,
    pub songs_load: Load,
}

/// The folder tree beside the folder browser, loaded as nodes are opened. The root has the id `""`.
#[derive(Debug, Default)]
pub struct FolderTree {
    pub children: HashMap<String, Vec<FolderEntry>>,
    pub parents: HashMap<String, String>,
    pub names: HashMap<String, String>,
    pub expanded: HashSet<String>,
    pub loading: HashSet<String>,
}

impl FolderTree {
    /// Remembers the entries under `parent`.
    pub fn set_children(&mut self, parent: &str, entries: &[FolderEntry]) {
        for e in entries {
            self.parents.insert(e.id.clone(), parent.to_string());
            self.names.insert(e.id.clone(), e.name.clone());
        }
        self.children.insert(parent.to_string(), entries.to_vec());
    }

    /// The ids from the top down to (excluding) `id`.
    pub fn ancestors(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut at = id;
        while let Some(p) = self.parents.get(at) {
            // A parent without a name is not a node of the tree (a music folder).
            if p.is_empty() || out.len() > 32 || !self.names.contains_key(p) {
                break;
            }
            out.push(p.clone());
            at = p;
        }
        out.reverse();
        out
    }

    /// Opens every ancestor of `id` in the tree.
    pub fn reveal(&mut self, id: &str) {
        for a in self.ancestors(id) {
            self.expanded.insert(a);
        }
    }
}

#[derive(Debug)]
pub struct Extra {
    pub genres_view: Vec<Genre>,
    pub genre_sort: GenreSort,
    pub genre_descending: bool,
    pub genre_search: String,
    pub genre: Option<GenreView>,
    pub fav_tab: FavoritesTab,
    pub fav_albums: Vec<Album>,
    pub fav_albums_load: Load,
    pub fav_artists: Vec<Artist>,
    pub fav_artists_load: Load,
    /// Items that left the favourites lists when un-favourited, kept in case the server refuses.
    removed_albums: Vec<Album>,
    removed_artists: Vec<Artist>,
    pub folder: Option<Folder>,
    pub folder_loading: bool,
    pub tree: FolderTree,
}

impl Default for Extra {
    fn default() -> Self {
        Self {
            genres_view: Vec::new(),
            genre_sort: GenreSort::Name,
            genre_descending: false,
            genre_search: String::new(),
            genre: None,
            fav_tab: FavoritesTab::Songs,
            fav_albums: Vec::new(),
            fav_albums_load: Load::Idle,
            fav_artists: Vec::new(),
            fav_artists_load: Load::Idle,
            removed_albums: Vec::new(),
            removed_artists: Vec::new(),
            folder: None,
            folder_loading: false,
            tree: FolderTree::default(),
        }
    }
}

/// Genres filtered by the search text and sorted.
pub fn arrange_genres(all: &[Genre], search: &str, sort: GenreSort, descending: bool) -> Vec<Genre> {
    let needle = search.trim().to_lowercase();
    let mut out: Vec<Genre> = all
        .iter()
        .filter(|g| needle.is_empty() || g.name.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    match sort {
        GenreSort::Name => out.sort_by_key(|g| g.name.to_lowercase()),
        GenreSort::Albums => out.sort_by_key(|g| (g.album_count.unwrap_or(0), g.name.to_lowercase())),
        GenreSort::Songs => out.sort_by_key(|g| (g.song_count.unwrap_or(0), g.name.to_lowercase())),
    }
    if descending {
        out.reverse();
    }
    out
}

/// The crumbs above a folder: names from the top down to the folder itself,
/// the middle replaced by `None` ("…") when there are more than `max`.
pub fn breadcrumbs(tree: &FolderTree, id: &str, max: usize) -> Vec<Option<(String, String)>> {
    let mut chain: Vec<(String, String)> = tree
        .ancestors(id)
        .into_iter()
        .map(|a| (tree.names.get(&a).cloned().unwrap_or_else(|| a.clone()), a))
        .collect();
    if !id.is_empty() {
        chain.push((tree.names.get(id).cloned().unwrap_or_else(|| id.to_string()), id.to_string()));
    }
    let mut out: Vec<Option<(String, String)>> = chain.into_iter().map(Some).collect();
    if out.len() > max && max >= 3 {
        let keep_tail = max - 2;
        let tail = out.split_off(out.len() - keep_tail);
        out.truncate(1);
        out.push(None);
        out.extend(tail);
    }
    out
}

#[derive(Debug, Clone)]
pub enum ExtraMsg {
    // genres
    GenreSort(GenreSort),
    GenreOrder,
    GenreSearch(String),
    GenreTarget(GenreTarget),
    GenreAlbums {
        name: String,
        start: usize,
        /// `ListQueries::genre_gen` when asked: an older answer is dropped.
        generation: u64,
        result: Result<Vec<Album>, String>,
    },
    GenreSongs {
        name: String,
        start: usize,
        generation: u64,
        result: Result<Vec<Song>, String>,
    },
    /// The header Play button of a genre page.
    GenrePlay(String, PlayBehavior),
    // favourites
    FavTab(FavoritesTab),
    FavAlbums(Result<Vec<Album>, String>),
    FavArtists(Result<Vec<Artist>, String>),
    /// The header Play button of the favourites page (the shown tab).
    FavPlay(PlayBehavior),
    // folders
    TreeRoot(Result<Vec<FolderEntry>, String>),
    TreeToggle(String),
    TreeChildren(String, Result<Folder, String>),
    FolderLoaded(Option<String>, Result<Folder, String>),
    Ancestors(Result<Vec<Folder>, String>),
    FolderRefresh,
    /// Play, add next or add last the songs of a folder and everything below it.
    FolderPlay(String, PlayBehavior),
    FolderShuffle(String),
    // items
    Favorite(ItemKind, ServerId, String, bool),
    Rate(ItemKind, ServerId, String, u8),
    /// A change was refused: put the old value back.
    Revert {
        kind: ItemKind,
        server: ServerId,
        id: String,
        favorite: Option<bool>,
        rating: Option<u8>,
        error: String,
    },
    // settings
    ShowFavorites(bool),
    ShowRatings(bool),
}

/// The songs of a genre for "Play" (up to `GENRE_PLAY_CAP`).
pub async fn genre_songs(source: &dyn MusicSource, genre: &str) -> Result<Vec<Song>, String> {
    let mut out = Vec::new();
    while out.len() < GENRE_PLAY_CAP {
        let q = SongQuery {
            genre: Some(genre.to_string()),
            start: out.len(),
            limit: 500.min(GENRE_PLAY_CAP - out.len()),
            ..Default::default()
        };
        let page = source.songs(&q).await.map_err(err)?;
        let n = page.items.len();
        out.extend(page.items);
        if n < q.limit {
            break;
        }
    }
    Ok(out)
}

/// The songs under a folder, depth first, in order.
pub async fn folder_songs_recursive(source: &dyn MusicSource, id: &str) -> Result<Vec<Song>, String> {
    let mut out = Vec::new();
    let mut stack = vec![id.to_string()];
    let mut visited = 0;
    while let Some(at) = stack.pop() {
        if visited >= FOLDER_VISIT_CAP || out.len() >= FOLDER_SONG_CAP {
            break;
        }
        visited += 1;
        let folder = source.folder((!at.is_empty()).then_some(at.as_str())).await.map_err(err)?;
        out.extend(folder.songs);
        stack.extend(folder.folders.into_iter().rev().map(|f| f.id));
    }
    out.truncate(FOLDER_SONG_CAP);
    Ok(out)
}

impl Listen {
    // ---- which list a page shows -------------------------------------------------

    /// The list the route shows (pages whose list depends on a choice included).
    pub(super) fn list_for_route(&self, route: &Route) -> Option<ListId> {
        match route {
            Route::Genres => Some(ListId::Genres),
            Route::Genre(name) => Some(match self.config.settings.lists.genre_target {
                GenreTarget::Albums => ListId::Genre(name.clone()),
                GenreTarget::Tracks => ListId::GenreSongs(name.clone()),
            }),
            Route::Playlists => Some(ListId::Playlists),
            Route::Playlist(id) => Some(self.playlist_list_id(id)),
            Route::Folders | Route::Folder(_) => Some(ListId::Folder),
            Route::Favorites => Some(ListId::Favorites(self.extra.fav_tab)),
            Route::Search => Some(ListId::Search(self.search.tab)),
            other => ListId::of_route(other),
        }
    }

    /// The list the open page shows (keyboard, selection, scroll restore).
    pub fn active_list(&self) -> Option<ListId> {
        self.list_for_route(&self.route)
    }

    /// Number of rows of a list (tests).
    pub fn list_data_len(&self, id: &ListId) -> Option<usize> {
        self.list_data(id).map(|d| d.len())
    }

    pub(super) fn extra_list_data(&self, id: &ListId) -> Option<ListData<'_>> {
        let x = &self.extra;
        let genre = |name: &str| x.genre.as_ref().filter(|g| g.name == name);
        Some(match id {
            ListId::Genres => ListData::Genres(Cow::Borrowed(&x.genres_view)),
            ListId::Genre(name) => match genre(name) {
                Some(g) => ListData::Albums(Cow::Borrowed(&g.albums)),
                None => ListData::Albums(Cow::Owned(Vec::new())),
            },
            ListId::GenreSongs(name) => match genre(name) {
                Some(g) => ListData::Songs(Cow::Borrowed(&g.songs)),
                None => ListData::Songs(Cow::Owned(Vec::new())),
            },
            ListId::Folder => match &x.folder {
                Some(f) => ListData::Songs(Cow::Borrowed(&f.songs)),
                None => ListData::Songs(Cow::Owned(Vec::new())),
            },
            ListId::Favorites(FavoritesTab::Songs) => ListData::Songs(Cow::Borrowed(&self.library.favorites)),
            ListId::Favorites(FavoritesTab::Albums) => ListData::Albums(Cow::Borrowed(&x.fav_albums)),
            ListId::Favorites(FavoritesTab::Artists) => ListData::Artists(Cow::Borrowed(&x.fav_artists)),
            _ => return None,
        })
    }

    /// Covers of the rows on screen, and the next page of a genre near its end.
    pub(super) fn extra_visible(&mut self, id: &ListId) -> Task<Message> {
        let Some(data) = self.extra_list_data(id) else { return Task::none() };
        let total = data.len();
        let (first, last) = self.list_range(id, total);
        let refs: Vec<ImageRef> = match &data {
            ListData::Songs(v) => v[first..last].iter().filter_map(|s| s.image.clone()).collect(),
            ListData::Albums(v) => v[first..last].iter().filter_map(|a| a.image.clone()).collect(),
            ListData::Artists(v) => v[first..last].iter().filter_map(|a| a.image.clone()).collect(),
            _ => Vec::new(),
        };
        drop(data);
        let mut tasks: Vec<Task<Message>> = refs.iter().map(|r| self.request_image(Some(r))).collect();
        let near_end = last + 24 >= total;
        match id {
            ListId::Genre(_) if near_end => tasks.push(self.load_genre_page(true)),
            ListId::GenreSongs(_) if near_end => tasks.push(self.load_genre_page(false)),
            _ => {}
        }
        Task::batch(tasks)
    }

    // ---- entering the pages --------------------------------------------------------

    /// `Route::Genres`, `Genre`, `Folders`, `Folder` and `Favorites` were opened.
    pub(super) fn extra_enter(&mut self, route: Route) -> Task<Message> {
        if route == Route::Folders {
            return self.navigate(Route::Folder(None));
        }
        if self.demo {
            self.demo_extra(&route);
            return Task::none();
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        match route {
            Route::Genres => {
                if self.library.genres_loaded {
                    self.refresh_genres_view();
                    return Task::none();
                }
                Task::perform(
                    async move { source.genres().await.map_err(err) },
                    self.scoped(Message::GenresLoaded),
                )
            }
            Route::Genre(name) => {
                if self.extra.genre.as_ref().is_none_or(|g| g.name != name) {
                    self.extra.genre = Some(GenreView { name, ..Default::default() });
                    // Answers for a genre that is no longer open are dropped.
                    self.list_queries.genre_gen += 1;
                }
                self.list_queries.open_genre();
                self.lists.reset(&ListId::Genre(
                    self.extra.genre.as_ref().map(|g| g.name.clone()).unwrap_or_default(),
                ));
                self.load_genre_target()
            }
            Route::Favorites => {
                let caps = source.capabilities();
                let mut tasks = vec![self.load_favorites()];
                if caps.contains(Capabilities::FAVORITE_ITEMS) {
                    self.extra.fav_albums_load = Load::Loading;
                    self.extra.fav_artists_load = Load::Loading;
                    let (a, b) = (source.clone(), source);
                    tasks.push(Task::perform(
                        async move { a.favorite_albums().await.map_err(err) },
                        self.scoped(|r| Message::Extra(ExtraMsg::FavAlbums(r))),
                    ));
                    tasks.push(Task::perform(
                        async move { b.favorite_artists().await.map_err(err) },
                        self.scoped(|r| Message::Extra(ExtraMsg::FavArtists(r))),
                    ));
                } else if self.extra.fav_tab != FavoritesTab::Songs {
                    self.extra.fav_tab = FavoritesTab::Songs;
                }
                Task::batch(tasks)
            }
            Route::Folder(id) => {
                self.extra.folder_loading = true;
                self.lists.reset(&ListId::Folder);
                let mut tasks = vec![self.load_folder(source.clone(), id)];
                if !self.extra.tree.children.contains_key("") {
                    tasks.push(self.load_tree_root(source));
                }
                Task::batch(tasks)
            }
            _ => Task::none(),
        }
    }

    fn load_folder(&mut self, source: Arc<dyn MusicSource>, id: Option<String>) -> Task<Message> {
        let key = id.clone();
        Task::perform(
            async move { source.folder(id.as_deref()).await.map_err(err) },
            self.scoped(move |r| Message::Extra(ExtraMsg::FolderLoaded(key, r))),
        )
    }

    fn load_tree_root(&mut self, source: Arc<dyn MusicSource>) -> Task<Message> {
        self.extra.tree.loading.insert(String::new());
        Task::perform(
            async move {
                if source.capabilities().contains(Capabilities::FOLDER_TREE) {
                    source
                        .folder_tree()
                        .await
                        .map(|nodes| {
                            nodes.into_iter().map(|n| FolderEntry { id: n.id, name: n.name }).collect()
                        })
                        .map_err(err)
                } else {
                    source.folder(None).await.map(|f| f.folders).map_err(err)
                }
            },
            self.scoped(|r| Message::Extra(ExtraMsg::TreeRoot(r))),
        )
    }

    pub(super) fn refresh_genres_view(&mut self) {
        let x = &mut self.extra;
        x.genres_view =
            arrange_genres(&self.library.genres, &x.genre_search, x.genre_sort, x.genre_descending);
        self.lists.reset(&ListId::Genres);
    }

    pub(super) fn load_genre_target(&mut self) -> Task<Message> {
        match self.config.settings.lists.genre_target {
            GenreTarget::Albums => self.load_genre_page(true),
            GenreTarget::Tracks => self.load_genre_page(false),
        }
    }

    fn load_genre_page(&mut self, albums: bool) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        let Some(g) = &mut self.extra.genre else { return Task::none() };
        let name = g.name.clone();
        if albums {
            if g.albums_load != Load::Idle {
                return Task::none();
            }
            g.albums_load = Load::Loading;
            let start = g.albums.len();
            let q = self.genre_album_query(&name, start, GENRE_PAGE);
            let generation = self.list_queries.genre_gen;
            Task::perform(
                async move { source.albums(&q).await.map(|p| p.items).map_err(err) },
                self.scoped(move |result| {
                    Message::Extra(ExtraMsg::GenreAlbums { name, start, generation, result })
                }),
            )
        } else {
            if g.songs_load != Load::Idle {
                return Task::none();
            }
            g.songs_load = Load::Loading;
            let start = g.songs.len();
            let q = self.genre_song_query(&name, start, GENRE_PAGE);
            let generation = self.list_queries.genre_gen;
            Task::perform(
                async move { source.songs(&q).await.map(|p| p.items).map_err(err) },
                self.scoped(move |result| {
                    Message::Extra(ExtraMsg::GenreSongs { name, start, generation, result })
                }),
            )
        }
    }

    // ---- messages ----------------------------------------------------------------------

    pub(super) fn on_extra(&mut self, msg: ExtraMsg) -> Task<Message> {
        match msg {
            ExtraMsg::GenreSort(sort) => {
                self.extra.genre_sort = sort;
                self.refresh_genres_view();
            }
            ExtraMsg::GenreOrder => {
                self.extra.genre_descending = !self.extra.genre_descending;
                self.refresh_genres_view();
            }
            ExtraMsg::GenreSearch(text) => {
                self.extra.genre_search = text;
                self.refresh_genres_view();
            }
            ExtraMsg::GenreTarget(target) => {
                self.config.settings.lists.genre_target = target;
                self.save_config();
                return self.load_genre_target();
            }
            ExtraMsg::GenreAlbums { generation, .. } | ExtraMsg::GenreSongs { generation, .. }
                if generation != self.list_queries.genre_gen => {}
            ExtraMsg::GenreAlbums { name, start, result, .. } => {
                let Some(g) = self.extra.genre.as_mut().filter(|g| g.name == name && g.albums.len() == start)
                else {
                    return Task::none();
                };
                match result {
                    Ok(items) => {
                        g.albums_load = if items.len() < GENRE_PAGE { Load::Done } else { Load::Idle };
                        g.albums.extend(items);
                    }
                    Err(e) => {
                        g.albums_load = Load::Done;
                        self.toast_error(format!("Loading the genre failed: {e}"));
                    }
                }
                return self.extra_visible(&ListId::Genre(name));
            }
            ExtraMsg::GenreSongs { name, start, result, .. } => {
                let Some(g) = self.extra.genre.as_mut().filter(|g| g.name == name && g.songs.len() == start)
                else {
                    return Task::none();
                };
                match result {
                    Ok(items) => {
                        g.songs_load = if items.len() < GENRE_PAGE { Load::Done } else { Load::Idle };
                        g.songs.extend(items);
                    }
                    Err(e) => {
                        g.songs_load = Load::Done;
                        self.toast_error(format!("Loading the genre failed: {e}"));
                    }
                }
                return self.extra_visible(&ListId::GenreSongs(name));
            }
            ExtraMsg::GenrePlay(name, behavior) => {
                let genre = Genre { id: name.clone(), name, album_count: None, song_count: None };
                return self.play_items(vec![list::Picked::Genre(genre)], behavior);
            }
            ExtraMsg::FavTab(tab) => {
                self.extra.fav_tab = tab;
                let id = ListId::Favorites(tab);
                self.lists.reset(&id);
                return self.extra_visible(&id);
            }
            ExtraMsg::FavAlbums(result) => {
                self.extra.fav_albums_load = Load::Done;
                match result {
                    Ok(mut albums) => {
                        albums.iter_mut().for_each(|a| a.favorite = true);
                        self.extra.fav_albums = albums;
                    }
                    Err(e) => self.toast_error(format!("Loading favorite albums failed: {e}")),
                }
                return self.extra_visible(&ListId::Favorites(FavoritesTab::Albums));
            }
            ExtraMsg::FavArtists(result) => {
                self.extra.fav_artists_load = Load::Done;
                match result {
                    Ok(mut artists) => {
                        artists.iter_mut().for_each(|a| a.favorite = true);
                        self.extra.fav_artists = artists;
                    }
                    Err(e) => self.toast_error(format!("Loading favorite artists failed: {e}")),
                }
                return self.extra_visible(&ListId::Favorites(FavoritesTab::Artists));
            }
            ExtraMsg::FavPlay(behavior) => {
                let id = ListId::Favorites(self.extra.fav_tab);
                let Some(data) = self.list_data(&id) else { return Task::none() };
                let all: Vec<usize> = (0..data.len()).collect();
                let picks = data.pick(&all);
                return self.play_items(picks, behavior);
            }
            ExtraMsg::TreeRoot(result) => {
                self.extra.tree.loading.remove("");
                match result {
                    Ok(entries) => self.extra.tree.set_children("", &entries),
                    Err(e) => self.toast_error(format!("Loading the folders failed: {e}")),
                }
            }
            ExtraMsg::TreeToggle(id) => {
                let tree = &mut self.extra.tree;
                if !tree.expanded.remove(&id) {
                    tree.expanded.insert(id.clone());
                    if !tree.children.contains_key(&id)
                        && let Some(source) = self.source.clone()
                        && tree.loading.insert(id.clone())
                    {
                        let key = id.clone();
                        return Task::perform(
                            async move { source.folder(Some(&id)).await.map_err(err) },
                            self.scoped(move |r| Message::Extra(ExtraMsg::TreeChildren(key, r))),
                        );
                    }
                }
            }
            ExtraMsg::TreeChildren(id, result) => {
                self.extra.tree.loading.remove(&id);
                match result {
                    Ok(folder) => self.extra.tree.set_children(&id, &folder.folders),
                    Err(e) => {
                        self.extra.tree.expanded.remove(&id);
                        self.toast_error(format!("Loading the folder failed: {e}"));
                    }
                }
            }
            ExtraMsg::FolderLoaded(requested, result) => {
                // Only the folder that is still open.
                if !matches!(&self.route, Route::Folder(r) if *r == requested) {
                    return Task::none();
                }
                self.extra.folder_loading = false;
                match result {
                    Ok(folder) => return self.folder_arrived(folder),
                    Err(e) => self.toast_error(format!("Loading the folder failed: {e}")),
                }
            }
            ExtraMsg::Ancestors(result) => {
                if let Ok(chain) = result {
                    let tree = &mut self.extra.tree;
                    for f in chain {
                        tree.set_children(&f.id, &f.folders);
                        tree.names.insert(f.id.clone(), f.name.clone());
                        if let Some(p) = &f.parent_id {
                            tree.parents.insert(f.id.clone(), p.clone());
                        }
                    }
                    if let Route::Folder(Some(id)) = self.route.clone() {
                        self.extra.tree.reveal(&id);
                    }
                }
            }
            ExtraMsg::FolderRefresh => {
                let Route::Folder(id) = self.route.clone() else { return Task::none() };
                let Some(source) = self.source.clone() else { return Task::none() };
                self.extra.folder_loading = true;
                return self.load_folder(source, id);
            }
            ExtraMsg::FolderPlay(id, behavior) => return self.play_folder(id, behavior),
            ExtraMsg::FolderShuffle(id) => return self.folder_action(id, MenuAction::Shuffle),
            ExtraMsg::Favorite(kind, server, id, favorite) => {
                return self.set_item_favorite(kind, &server, &id, favorite);
            }
            ExtraMsg::Rate(kind, server, id, stars) => {
                return self.set_item_rating(kind, &server, &id, stars);
            }
            ExtraMsg::Revert { kind, server, id, favorite, rating, error } => {
                if let Some(f) = favorite {
                    self.visit_items(kind, &server, &id, &mut |fav, _| *fav = f);
                    self.sync_favorite_lists(kind, &server, &id, f);
                }
                if let Some(r) = rating {
                    self.visit_items(kind, &server, &id, &mut |_, rt| *rt = (r > 0).then_some(r));
                }
                self.toast_error(error);
            }
            ExtraMsg::ShowFavorites(v) => {
                self.config.settings.lists.show_favorites = v;
                self.save_config();
            }
            ExtraMsg::ShowRatings(v) => {
                self.config.settings.lists.show_ratings = v;
                self.save_config();
            }
        }
        Task::none()
    }

    /// A folder was loaded: show it, fill the tree, open the way to it.
    fn folder_arrived(&mut self, folder: Folder) -> Task<Message> {
        let tree = &mut self.extra.tree;
        let id = folder.id.clone();
        tree.set_children(&id, &folder.folders);
        let mut fetch = None;
        if !id.is_empty() {
            tree.names.insert(id.clone(), folder.name.clone());
            if let Some(parent) = folder.parent_id.clone().filter(|p| !p.is_empty()) {
                tree.parents.insert(id.clone(), parent.clone());
                if !tree.children.contains_key(&parent) {
                    fetch = Some(parent);
                }
            }
            tree.expanded.insert(id.clone());
            tree.reveal(&id);
        }
        self.extra.folder = Some(folder.clone());
        let mut tasks = vec![self.extra_visible(&ListId::Folder)];
        if let (Some(parent), Some(source)) = (fetch, self.source.clone()) {
            tasks.push(Task::perform(
                async move { ancestor_chain(&*source, parent).await },
                self.scoped(|r| Message::Extra(ExtraMsg::Ancestors(r))),
            ));
        }
        Task::batch(tasks)
    }

    fn play_folder(&mut self, id: String, behavior: PlayBehavior) -> Task<Message> {
        let action = match behavior {
            PlayBehavior::Now => MenuAction::Play,
            PlayBehavior::Next => MenuAction::PlayNext,
            PlayBehavior::Last => MenuAction::AddToQueue,
        };
        self.folder_action(id, action)
    }

    fn folder_action(&mut self, id: String, action: MenuAction) -> Task<Message> {
        if self.demo {
            let songs = self.extra.folder.as_ref().map(|f| f.songs.clone()).unwrap_or_default();
            return self.apply_to_songs(action, songs);
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        Task::perform(async move { folder_songs_recursive(&*source, &id).await }, move |r| {
            Message::Menu(MenuMsg::Songs(action.clone(), r))
        })
    }

    // ---- favourites and ratings of albums and artists ------------------------------------

    /// The source of `server` allows favouriting albums and artists.
    pub fn can_favorite_items(&self, server: &ServerId) -> bool {
        self.sources.get(server).is_some_and(|s| s.capabilities().contains(Capabilities::FAVORITE_ITEMS))
    }

    /// Whether hearts are shown (settings: Show favourites).
    pub fn show_favorites(&self) -> bool {
        self.config.settings.lists.show_favorites
    }

    /// Whether star ratings are shown (settings: Show ratings).
    pub fn show_ratings(&self) -> bool {
        self.config.settings.lists.show_ratings
    }

    /// Runs `f` on the favourite flag and rating of every loaded copy of the item.
    fn visit_items(
        &mut self,
        kind: ItemKind,
        server: &ServerId,
        id: &str,
        f: &mut dyn FnMut(&mut bool, &mut Option<u8>),
    ) {
        match kind {
            ItemKind::Album => {
                let mut each = |a: &mut Album| {
                    if &a.server_id == server && a.id == id {
                        f(&mut a.favorite, &mut a.rating);
                    }
                };
                let lib = &mut self.library;
                lib.albums.iter_mut().for_each(&mut each);
                self.lists.albums.values_mut().for_each(&mut each);
                lib.album.iter_mut().for_each(&mut each);
                if let Some(d) = &mut lib.artist {
                    d.albums.iter_mut().for_each(&mut each);
                }
                for shelf in &mut lib.home {
                    shelf.albums.iter_mut().for_each(&mut each);
                }
                self.search.results.albums.iter_mut().for_each(&mut each);
                self.extra.fav_albums.iter_mut().for_each(&mut each);
                if let Some(g) = &mut self.extra.genre {
                    g.albums.iter_mut().for_each(&mut each);
                }
            }
            ItemKind::Artist => {
                let mut each = |a: &mut Artist| {
                    if &a.server_id == server && a.id == id {
                        f(&mut a.favorite, &mut a.rating);
                    }
                };
                let lib = &mut self.library;
                lib.artists.iter_mut().for_each(&mut each);
                if let Some(d) = &mut lib.artist {
                    each(&mut d.artist);
                }
                self.search.results.artists.iter_mut().for_each(&mut each);
                self.extra.fav_artists.iter_mut().for_each(&mut each);
            }
            ItemKind::Song => {}
        }
    }

    /// Updates the favourites lists in place: an un-favourited item leaves, a favourited one goes on top.
    fn sync_favorite_lists(&mut self, kind: ItemKind, server: &ServerId, id: &str, favorite: bool) {
        match kind {
            ItemKind::Album => {
                let same = |a: &Album| &a.server_id == server && a.id == id;
                if favorite {
                    if self.extra.fav_albums.iter().any(same) {
                        return;
                    }
                    let known = self
                        .extra
                        .removed_albums
                        .iter()
                        .position(same)
                        .map(|i| self.extra.removed_albums.remove(i));
                    let known = known.or_else(|| self.known_album(server, id));
                    if let Some(a) = known {
                        self.extra.fav_albums.insert(0, Album { favorite: true, ..a });
                    }
                } else if let Some(i) = self.extra.fav_albums.iter().position(same) {
                    let gone = self.extra.fav_albums.remove(i);
                    self.extra.removed_albums.push(gone);
                    self.extra.removed_albums.truncate(20);
                    self.lists.reset(&ListId::Favorites(FavoritesTab::Albums));
                }
            }
            ItemKind::Artist => {
                let same = |a: &Artist| &a.server_id == server && a.id == id;
                if favorite {
                    if self.extra.fav_artists.iter().any(same) {
                        return;
                    }
                    let known = self
                        .extra
                        .removed_artists
                        .iter()
                        .position(same)
                        .map(|i| self.extra.removed_artists.remove(i));
                    let known = known.or_else(|| self.known_artist(server, id));
                    if let Some(a) = known {
                        self.extra.fav_artists.insert(0, Artist { favorite: true, ..a });
                    }
                } else if let Some(i) = self.extra.fav_artists.iter().position(same) {
                    let gone = self.extra.fav_artists.remove(i);
                    self.extra.removed_artists.push(gone);
                    self.extra.removed_artists.truncate(20);
                    self.lists.reset(&ListId::Favorites(FavoritesTab::Artists));
                }
            }
            ItemKind::Song => {}
        }
    }

    /// A loaded copy of an album.
    fn known_album(&self, server: &ServerId, id: &str) -> Option<Album> {
        let lib = &self.library;
        let same = |a: &&Album| &a.server_id == server && a.id == id;
        lib.albums
            .iter()
            .find(same)
            .or_else(|| lib.album.iter().find(same))
            .or_else(|| lib.artist.as_ref().and_then(|d| d.albums.iter().find(same)))
            .or_else(|| lib.home.iter().find_map(|s| s.albums.iter().find(same)))
            .or_else(|| self.search.results.albums.iter().find(same))
            .or_else(|| self.extra.genre.as_ref().and_then(|g| g.albums.iter().find(same)))
            .cloned()
    }

    /// A loaded copy of an artist.
    fn known_artist(&self, server: &ServerId, id: &str) -> Option<Artist> {
        let lib = &self.library;
        let same = |a: &&Artist| &a.server_id == server && a.id == id;
        lib.artists
            .iter()
            .find(same)
            .or_else(|| lib.artist.as_ref().map(|d| &d.artist).filter(same))
            .or_else(|| self.search.results.artists.iter().find(same))
            .cloned()
    }

    fn item_state(&mut self, kind: ItemKind, server: &ServerId, id: &str) -> Option<(bool, u8)> {
        let mut found = None;
        self.visit_items(kind, server, id, &mut |fav, rating| {
            found.get_or_insert((*fav, rating.unwrap_or(0)));
        });
        found
    }

    /// Favourites or un-favourites an album or artist on its server.
    pub fn set_item_favorite(
        &mut self,
        kind: ItemKind,
        server: &ServerId,
        id: &str,
        favorite: bool,
    ) -> Task<Message> {
        if kind == ItemKind::Song || !self.can_favorite_items(server) {
            return Task::none();
        }
        let Some(source) = self.sources.get(server).cloned() else { return Task::none() };
        let previous = self.item_state(kind, server, id).map(|s| s.0).unwrap_or(!favorite);
        self.visit_items(kind, server, id, &mut |fav, _| *fav = favorite);
        self.sync_favorite_lists(kind, server, id, favorite);
        let notice = match (source.kind(), kind, favorite) {
            (SourceKind::YouTubeMusic, ItemKind::Album, true) => "Saved to your library.",
            (SourceKind::YouTubeMusic, ItemKind::Album, false) => "Removed from your library.",
            (SourceKind::YouTubeMusic, _, true) => "Subscribed.",
            (SourceKind::YouTubeMusic, _, false) => "Unsubscribed.",
            (_, _, true) => "Added to favorites.",
            (_, _, false) => "Removed from favorites.",
        };
        let (server, id) = (server.clone(), id.to_string());
        Task::perform(
            {
                let id = id.clone();
                async move {
                    source
                        .set_favorite_items(kind, &[id], favorite)
                        .await
                        .map(|_| notice.to_string())
                        .map_err(err)
                }
            },
            move |r| match r {
                Ok(text) => Message::Menu(MenuMsg::Done(Ok(text))),
                Err(e) => Message::Extra(ExtraMsg::Revert {
                    kind,
                    server: server.clone(),
                    id: id.clone(),
                    favorite: Some(previous),
                    rating: None,
                    error: format!("Could not change the favorite: {e}"),
                }),
            },
        )
    }

    /// Rates an album or artist (0 clears).
    pub fn set_item_rating(
        &mut self,
        kind: ItemKind,
        server: &ServerId,
        id: &str,
        stars: u8,
    ) -> Task<Message> {
        let Some(source) = self.sources.get(server).cloned() else { return Task::none() };
        if kind == ItemKind::Song || !source.capabilities().contains(Capabilities::RATINGS) {
            return Task::none();
        }
        let stars = stars.min(5);
        let previous = self.item_state(kind, server, id).map(|s| s.1).unwrap_or(0);
        self.visit_items(kind, server, id, &mut |_, rating| *rating = (stars > 0).then_some(stars));
        let (server, id) = (server.clone(), id.to_string());
        Task::perform(
            {
                let id = id.clone();
                async move {
                    source
                        .set_item_rating(kind, &id, stars)
                        .await
                        .map(|_| {
                            if stars == 0 {
                                "Rating cleared.".to_string()
                            } else {
                                format!("Rated {stars} / 5.")
                            }
                        })
                        .map_err(err)
                }
            },
            move |r| match r {
                Ok(text) => Message::Menu(MenuMsg::Done(Ok(text))),
                Err(e) => Message::Extra(ExtraMsg::Revert {
                    kind,
                    server: server.clone(),
                    id: id.clone(),
                    favorite: None,
                    rating: Some(previous),
                    error: format!("Could not rate: {e}"),
                }),
            },
        )
    }

    // ---- demo data -----------------------------------------------------------------------

    fn demo_extra(&mut self, route: &Route) {
        match route {
            Route::Genres => {
                if !self.library.genres_loaded {
                    self.library.genres = ["Synthwave", "Indie Pop", "Ambient", "Rock", "Electronic"]
                        .iter()
                        .enumerate()
                        .map(|(i, n)| Genre {
                            id: (*n).into(),
                            name: (*n).into(),
                            album_count: Some(12 - 2 * i as u32),
                            song_count: Some(140 - 25 * i as u32),
                        })
                        .collect();
                    self.library.genres_loaded = true;
                }
                self.refresh_genres_view();
            }
            Route::Genre(name) => {
                self.extra.genre = Some(GenreView { name: name.clone(), ..Default::default() });
                self.list_queries.open_genre();
                self.demo_genre(name, false);
                self.demo_genre(name, true);
            }
            Route::Favorites => {
                let mut albums = demo::albums();
                albums.truncate(4);
                albums.iter_mut().for_each(|a| a.favorite = true);
                self.extra.fav_artists = albums
                    .iter()
                    .map(|a| Artist {
                        id: format!("demo-artist-{}", a.id),
                        server_id: a.server_id.clone(),
                        source: a.source,
                        name: a.artist_line(),
                        image: None,
                        album_count: Some(1),
                        song_count: a.song_count,
                        favorite: true,
                        rating: None,
                        biography: None,
                        similar: vec![],
                    })
                    .collect();
                self.extra.fav_albums = albums;
                self.extra.fav_albums_load = Load::Done;
                self.extra.fav_artists_load = Load::Done;
                self.library.favorites = (0..3)
                    .flat_map(demo::album_songs)
                    .map(|mut s| {
                        s.favorite = true;
                        s
                    })
                    .collect();
                self.library.favorites_loaded = true;
            }
            Route::Folders | Route::Folder(None) => self.demo_folder(None),
            Route::Folder(Some(id)) => self.demo_folder(Some(id.clone())),
            _ => {}
        }
    }

    fn demo_folder(&mut self, id: Option<String>) {
        let albums = demo::albums();
        let entries: Vec<FolderEntry> =
            albums.iter().map(|a| FolderEntry { id: a.id.clone(), name: a.artist_line() }).collect();
        self.extra.tree.set_children("", &entries);
        let folder = match id.as_deref().and_then(demo::album_index) {
            Some(i) => {
                let tree = &mut self.extra.tree;
                tree.set_children(&albums[i].id, &[]);
                tree.reveal(&albums[i].id);
                Folder {
                    id: albums[i].id.clone(),
                    name: albums[i].artist_line(),
                    parent_id: None,
                    folders: vec![],
                    songs: demo::album_songs(i),
                }
            }
            None => Folder {
                id: String::new(),
                name: "Folders".into(),
                parent_id: None,
                folders: entries,
                songs: vec![],
            },
        };
        self.extra.folder = Some(folder);
    }
}

/// The folders above `parent`, nearest first, as far as the server shows them.
async fn ancestor_chain(source: &dyn MusicSource, parent: String) -> Result<Vec<Folder>, String> {
    let mut chain: Vec<Folder> = Vec::new();
    let mut next = Some(parent);
    while let Some(id) = next.take() {
        if chain.len() >= 12 || chain.iter().any(|f| f.id == id) {
            break;
        }
        match source.folder(Some(&id)).await {
            Ok(f) => {
                next = f.parent_id.clone().filter(|p| !p.is_empty());
                chain.push(f);
            }
            Err(_) => break,
        }
    }
    Ok(chain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(name: &str, albums: u32, songs: u32) -> Genre {
        Genre { id: name.into(), name: name.into(), album_count: Some(albums), song_count: Some(songs) }
    }

    #[test]
    fn genres_sort_and_search() {
        let all = [g("Rock", 5, 50), g("ambient", 9, 20), g("Pop", 1, 99)];
        let names = |v: Vec<Genre>| v.into_iter().map(|g| g.name).collect::<Vec<_>>();
        assert_eq!(names(arrange_genres(&all, "", GenreSort::Name, false)), ["ambient", "Pop", "Rock"]);
        assert_eq!(names(arrange_genres(&all, "", GenreSort::Albums, true)), ["ambient", "Rock", "Pop"]);
        assert_eq!(names(arrange_genres(&all, "", GenreSort::Songs, false)), ["ambient", "Rock", "Pop"]);
        assert_eq!(names(arrange_genres(&all, " RO", GenreSort::Name, false)), ["Rock"]);
    }

    fn entry(id: &str, name: &str) -> FolderEntry {
        FolderEntry { id: id.into(), name: name.into() }
    }

    #[test]
    fn tree_ancestors_and_reveal() {
        let mut t = FolderTree::default();
        t.set_children("", &[entry("a", "A")]);
        t.set_children("a", &[entry("b", "B")]);
        t.set_children("b", &[entry("c", "C")]);
        assert_eq!(t.ancestors("c"), ["a", "b"]);
        assert!(t.ancestors("a").is_empty());
        t.reveal("c");
        assert!(t.expanded.contains("a") && t.expanded.contains("b") && !t.expanded.contains("c"));
    }

    #[test]
    fn long_breadcrumbs_collapse_in_the_middle() {
        let mut t = FolderTree::default();
        t.set_children("", &[entry("a", "A")]);
        t.set_children("a", &[entry("b", "B")]);
        t.set_children("b", &[entry("c", "C")]);
        t.set_children("c", &[entry("d", "D")]);
        t.set_children("d", &[entry("e", "E")]);
        let names = |v: Vec<Option<(String, String)>>| {
            v.into_iter().map(|c| c.map_or("…".to_string(), |c| c.0)).collect::<Vec<_>>()
        };
        assert_eq!(names(breadcrumbs(&t, "e", 10)), ["A", "B", "C", "D", "E"]);
        assert_eq!(names(breadcrumbs(&t, "e", 4)), ["A", "…", "D", "E"]);
        assert!(breadcrumbs(&t, "", 4).is_empty());
    }
}
