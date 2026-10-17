//! Genres, the folder browser, the Favorites page, favouriting / rating of
//! albums and artists, and the Show favourites / Show ratings settings.

use async_trait::async_trait;
use iced_test::simulator::Simulator;
use listen::app::library_extra::{ExtraMsg, GenreSort};
use listen::app::list::{FavoritesTab, ListId, ListMsg};
use listen::app::{INTER, Listen, Message, Route, SourceHandle};
use listen::{Args, FONTS, demo};
use listen_core::config::ServerConfig;
use listen_core::model::{
    Album, Artist, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
};
use listen_core::settings::{ColumnId, GenreTarget, ListKind};
use listen_source::{
    AlbumQuery, Capabilities, ItemKind, ListQuery, MusicSource, Result, SongQuery, SourceError, StreamSource,
};
use std::sync::Arc;

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

const SIZE: iced::Size = iced::Size::new(1600.0, 950.0);

fn state_on(route: Route) -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let _ = s.update(Message::WindowResized(SIZE));
    let _ = s.update(Message::Navigate(route));
    s
}

fn shows(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), SIZE, s.view());
    ui.find(text).is_ok()
}

fn click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        ui.click(text).unwrap_or_else(|e| panic!("no {text:?} on screen: {e:?}"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

// ---- genres -----------------------------------------------------------------------

#[test]
fn the_genre_list_shows_genres_and_opens_one() {
    let mut s = state_on(Route::Genres);
    assert!(shows(&s, "Genres") && shows(&s, "Synthwave") && shows(&s, "Ambient"));
    let _ = click_text(&mut s, "Ambient");
    // The name is a link to the genre page.
    assert_eq!(s.route, Route::Genre("Ambient".into()));
}

#[test]
fn genres_sort_and_search() {
    let mut s = state_on(Route::Genres);
    let names = |s: &Listen| s.extra.genres_view.iter().map(|g| g.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&s), ["Ambient", "Electronic", "Indie Pop", "Rock", "Synthwave"]);
    let _ = s.update(Message::Extra(ExtraMsg::GenreSort(GenreSort::Albums)));
    assert_eq!(names(&s)[0], "Electronic");
    let _ = s.update(Message::Extra(ExtraMsg::GenreOrder));
    assert_eq!(names(&s)[0], "Synthwave");
    let _ = s.update(Message::Extra(ExtraMsg::GenreSearch("ock".into())));
    assert_eq!(names(&s), ["Rock"]);
    assert!(shows(&s, "Rock") && !shows(&s, "Ambient"));
}

#[test]
fn the_genre_page_toggles_between_tracks_and_albums() {
    let mut s = state_on(Route::Genre("Synthwave".into()));
    // Feishin's default: tracks.
    assert_eq!(s.config.settings.lists.genre_target, GenreTarget::Tracks);
    assert_eq!(s.list_data_len(&ListId::GenreSongs("Synthwave".into())), Some(all_songs()));
    assert!(shows(&s, "Midnight Signal") && shows(&s, "Play"));
    // (The sidebar has "Albums" and "Tracks" too, so the tab's message is sent directly.)
    let _ = s.update(Message::Extra(ExtraMsg::GenreTarget(GenreTarget::Albums)));
    assert_eq!(s.config.settings.lists.genre_target, GenreTarget::Albums);
    assert!(shows(&s, "Night Drive") && !shows(&s, "Midnight Signal"));
    // The keyboard follows the shown list.
    assert_eq!(s.active_list(), Some(ListId::Genre("Synthwave".into())));
}

fn all_songs() -> usize {
    (0..demo::ALBUMS.len()).map(|i| demo::album_songs(i).len()).sum()
}

// ---- favorites --------------------------------------------------------------------

#[test]
fn favorites_has_tracks_albums_and_artists_tabs() {
    let mut s = state_on(Route::Favorites);
    assert!(shows(&s, "Tracks") && shows(&s, "Albums") && shows(&s, "Artists"));
    assert_eq!(s.active_list(), Some(ListId::Favorites(FavoritesTab::Songs)));
    assert!(shows(&s, "Midnight Signal"));

    let _ = s.update(Message::Extra(ExtraMsg::FavTab(FavoritesTab::Albums)));
    assert_eq!(s.active_list(), Some(ListId::Favorites(FavoritesTab::Albums)));
    assert!(shows(&s, "Night Drive") && !shows(&s, "Midnight Signal"));

    let _ = s.update(Message::Extra(ExtraMsg::FavTab(FavoritesTab::Artists)));
    assert_eq!(s.active_list(), Some(ListId::Favorites(FavoritesTab::Artists)));
    assert!(shows(&s, "Neon Coast"));
}

// ---- folders ----------------------------------------------------------------------

#[test]
fn the_folder_browser_has_a_tree_and_the_songs_of_the_open_folder() {
    let mut s = state_on(Route::Folders);
    // "Folders" opens the root.
    assert_eq!(s.route, Route::Folder(None));
    assert!(shows(&s, "All folders") && shows(&s, "Neon Coast"));
    let id = demo::albums()[0].id.clone();
    let _ = s.update(Message::Navigate(Route::Folder(Some(id.clone()))));
    assert!(shows(&s, "Midnight Signal"), "the songs of the folder");
    assert!(shows(&s, "Play next") && shows(&s, "Refresh"));
    assert_eq!(s.active_list(), Some(ListId::Folder));
    // The tree opens the way to the folder and marks it.
    assert_eq!(s.extra.tree.children.get("").map(Vec::len), Some(demo::ALBUMS.len()));
}

#[test]
fn tree_nodes_toggle() {
    let mut s = state_on(Route::Folder(None));
    let id = demo::albums()[1].id.clone();
    let _ = s.update(Message::Extra(ExtraMsg::TreeToggle(id.clone())));
    assert!(s.extra.tree.expanded.contains(&id));
    let _ = s.update(Message::Extra(ExtraMsg::TreeToggle(id.clone())));
    assert!(!s.extra.tree.expanded.contains(&id));
}

// ---- show favourites / ratings ----------------------------------------------------

#[test]
fn the_toggles_hide_hearts_and_stars() {
    let mut s = state_on(Route::Tracks);
    let cols = |s: &Listen| s.config.settings.lists.config(ListKind::Songs).table.visible();
    assert!(cols(&s).iter().any(|c| c.id == ColumnId::UserFavorite));
    let _ = s.update(Message::Extra(ExtraMsg::ShowFavorites(false)));
    assert!(!cols(&s).iter().any(|c| c.id == ColumnId::UserFavorite));
    assert!(!s.show_favorites());
    let _ = s.update(Message::Extra(ExtraMsg::ShowFavorites(true)));
    assert!(cols(&s).iter().any(|c| c.id == ColumnId::UserFavorite));
    let _ = s.update(Message::Extra(ExtraMsg::ShowRatings(false)));
    assert!(!s.show_ratings());
}

// ---- albums and artists with a source ---------------------------------------------

struct Fake;

#[async_trait]
impl MusicSource for Fake {
    fn kind(&self) -> SourceKind {
        SourceKind::Subsonic
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::FAVORITE_ITEMS | Capabilities::RATINGS
    }
    async fn ping(&self) -> Result<()> {
        Ok(())
    }
    async fn albums(&self, _: &AlbumQuery) -> Result<Page<Album>> {
        Err(SourceError::Unsupported)
    }
    async fn album(&self, _: &str) -> Result<Album> {
        Err(SourceError::Unsupported)
    }
    async fn artists(&self, _: &ListQuery) -> Result<Page<Artist>> {
        Err(SourceError::Unsupported)
    }
    async fn artist(&self, _: &str) -> Result<Artist> {
        Err(SourceError::Unsupported)
    }
    async fn artist_albums(&self, _: &str) -> Result<Vec<Album>> {
        Ok(vec![])
    }
    async fn top_songs(&self, _: &Artist, _: usize) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn songs(&self, _: &SongQuery) -> Result<Page<Song>> {
        Err(SourceError::Unsupported)
    }
    async fn random_songs(&self, _: usize, _: Option<&str>) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn search(&self, _: &str, _: usize) -> Result<SearchResults> {
        Ok(SearchResults::default())
    }
    async fn playlists(&self) -> Result<Vec<Playlist>> {
        Ok(vec![])
    }
    async fn playlist_songs(&self, _: &str) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn set_favorite(&self, _: &[String], _: bool) -> Result<()> {
        Ok(())
    }
    async fn stream(&self, _: &Song) -> Result<StreamSource> {
        Err(SourceError::Unsupported)
    }
    fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
        None
    }
}

fn with_source() -> (Listen, Album) {
    let (mut s, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    let id = ServerId("srv".into());
    s.config.servers = vec![ServerConfig {
        id: id.clone(),
        name: "srv".into(),
        kind: SourceKind::Subsonic,
        url: "https://x.example".into(),
        username: "me".into(),
    }];
    let _ = s.update(Message::Connected(id.clone(), Ok(SourceHandle(Arc::new(Fake)))));
    let mut album = demo::albums().remove(0);
    album.server_id = id;
    album.source = SourceKind::Subsonic;
    s.library.albums = vec![album.clone()];
    s.extra.fav_albums = vec![album.clone()];
    (s, album)
}

#[test]
fn favouriting_marks_every_loaded_copy_and_a_refusal_puts_it_back() {
    let (mut s, album) = with_source();
    let _ = s.set_item_favorite(ItemKind::Album, &album.server_id, &album.id, true);
    assert!(s.library.albums[0].favorite && s.extra.fav_albums[0].favorite);
    let _ = s.update(Message::Extra(ExtraMsg::Revert {
        kind: ItemKind::Album,
        server: album.server_id.clone(),
        id: album.id.clone(),
        favorite: Some(false),
        rating: None,
        error: "no".into(),
    }));
    // Back to not favourited: it leaves the Favorites list as well.
    assert!(!s.library.albums[0].favorite && s.extra.fav_albums.is_empty());
}

#[test]
fn rating_an_album_marks_it_and_clears_with_zero() {
    let (mut s, album) = with_source();
    let _ = s.set_item_rating(ItemKind::Album, &album.server_id, &album.id, 4);
    assert_eq!(s.library.albums[0].rating, Some(4));
    let _ = s.set_item_rating(ItemKind::Album, &album.server_id, &album.id, 0);
    assert_eq!(s.library.albums[0].rating, None);
    // Ratings over five stars are clamped.
    let _ = s.set_item_rating(ItemKind::Album, &album.server_id, &album.id, 9);
    assert_eq!(s.library.albums[0].rating, Some(5));
}

#[test]
fn artists_are_favourited_and_rated_too() {
    let (mut s, album) = with_source();
    let artist = Artist {
        id: "ar1".into(),
        server_id: album.server_id.clone(),
        source: SourceKind::Subsonic,
        name: "A".into(),
        image: None,
        album_count: None,
        song_count: None,
        favorite: false,
        rating: None,
        biography: None,
        similar: vec![],
    };
    s.library.artists = vec![artist.clone()];
    let _ = s.set_item_favorite(ItemKind::Artist, &artist.server_id, "ar1", true);
    let _ = s.set_item_rating(ItemKind::Artist, &artist.server_id, "ar1", 3);
    assert!(s.library.artists[0].favorite);
    assert_eq!(s.library.artists[0].rating, Some(3));
}

#[test]
fn a_source_without_the_capability_changes_nothing() {
    let (mut s, mut album) = with_source();
    album.server_id = ServerId("elsewhere".into());
    let _ = s.set_item_favorite(ItemKind::Album, &album.server_id, &album.id, true);
    assert!(!s.library.albums[0].favorite);
}

#[test]
fn favourite_lists_follow_a_change_at_once() {
    let (mut s, album) = with_source();
    assert_eq!(s.extra.fav_albums.len(), 1);
    // Un-favouriting removes the album from the list that is open.
    let _ = s.set_item_favorite(ItemKind::Album, &album.server_id, &album.id, false);
    assert!(s.extra.fav_albums.is_empty());
    // A refusal puts it back.
    let _ = s.update(Message::Extra(ExtraMsg::Revert {
        kind: ItemKind::Album,
        server: album.server_id.clone(),
        id: album.id.clone(),
        favorite: Some(true),
        rating: None,
        error: "no".into(),
    }));
    assert_eq!(s.extra.fav_albums.len(), 1, "restored");
    assert!(s.extra.fav_albums[0].favorite);
    // Favouriting an album that is loaded elsewhere adds it at the top.
    let _ = s.set_item_favorite(ItemKind::Album, &album.server_id, &album.id, false);
    let _ = s.set_item_favorite(ItemKind::Album, &album.server_id, &album.id, true);
    assert_eq!(s.extra.fav_albums.len(), 1);
}

#[test]
fn an_unliked_song_leaves_the_favorites_list() {
    let (mut s, album) = with_source();
    let mut songs = album.songs.clone();
    songs.iter_mut().for_each(|x| {
        x.favorite = true;
        x.server_id = album.server_id.clone();
    });
    s.library.favorites = songs.clone();
    s.library.favorites_loaded = true;
    let n = songs.len();
    let _ = s.update(Message::List(ListMsg::Favorite(ListId::Favorites(FavoritesTab::Songs), 0)));
    assert_eq!(s.library.favorites.len(), n - 1, "gone at once");
    assert!(songs[0].id != s.library.favorites[0].id);
}
