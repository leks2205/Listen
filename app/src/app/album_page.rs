//! State and messages of the album page (`ui::album_detail`): track list search and
//! sort, header actions, the comment and the "More from" carousels. Behaviour follows
//! Feishin's `album-detail-*` and `library-header.tsx`.

use super::list::{ListId, Picked};
use super::menu::{MenuAction, MenuMsg, MenuTarget};
use super::*;
use listen_core::settings::PlayBehavior;
use listen_source::{AlbumQuery, AlbumSort, Capabilities, ItemKind, SortOrder};
use rand::seq::SliceRandom;
use std::hash::{Hash, Hasher};

/// How many songs an album radio queues (Feishin `artistRadioCount`).
const RADIO_COUNT: usize = 20;
/// Albums in a "More from" carousel.
const MORE_COUNT: usize = 24;
/// Genres that get a "More from" carousel.
const MORE_GENRES: usize = 2;

/// What the track list of an album can be sorted by (Feishin `CLIENT_SIDE_SONG_FILTERS`
/// as far as Listen has the data).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackSort {
    /// The album's own order (disc, track).
    #[default]
    Id,
    Album,
    AlbumArtist,
    Artist,
    Bpm,
    Channels,
    Comment,
    Duration,
    Favorited,
    Genre,
    Name,
    PlayCount,
    Random,
    Rating,
    RecentlyAdded,
    Year,
}

impl TrackSort {
    pub const ALL: [TrackSort; 16] = [
        TrackSort::Id,
        TrackSort::Album,
        TrackSort::AlbumArtist,
        TrackSort::Artist,
        TrackSort::Bpm,
        TrackSort::Channels,
        TrackSort::Comment,
        TrackSort::Duration,
        TrackSort::Favorited,
        TrackSort::Genre,
        TrackSort::Name,
        TrackSort::PlayCount,
        TrackSort::Random,
        TrackSort::Rating,
        TrackSort::RecentlyAdded,
        TrackSort::Year,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TrackSort::Id => "Default",
            TrackSort::Album => "Album",
            TrackSort::AlbumArtist => "Album artist",
            TrackSort::Artist => "Artist",
            TrackSort::Bpm => "BPM",
            TrackSort::Channels => "Channels",
            TrackSort::Comment => "Comment",
            TrackSort::Duration => "Duration",
            TrackSort::Favorited => "Favorited",
            TrackSort::Genre => "Genre",
            TrackSort::Name => "Name",
            TrackSort::PlayCount => "Play count",
            TrackSort::Random => "Random",
            TrackSort::Rating => "Rating",
            TrackSort::RecentlyAdded => "Recently added",
            TrackSort::Year => "Year",
        }
    }

    /// The order a sort starts in when picked (Feishin `defaultOrder`).
    pub fn default_desc(self) -> bool {
        matches!(
            self,
            TrackSort::Bpm
                | TrackSort::Duration
                | TrackSort::Favorited
                | TrackSort::PlayCount
                | TrackSort::Rating
                | TrackSort::RecentlyAdded
                | TrackSort::Year
        )
    }
}

impl std::fmt::Display for TrackSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Songs whose title, album, artists or genres contain every word of `term`.
pub fn filter_songs(songs: &[Song], term: &str) -> Vec<Song> {
    let words: Vec<String> = term.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return songs.to_vec();
    }
    songs
        .iter()
        .filter(|s| {
            let haystack = format!(
                "{} {} {} {} {}",
                s.title,
                s.album.as_deref().unwrap_or_default(),
                s.artist_line(),
                s.genres.join(" "),
                s.comment.as_deref().unwrap_or_default(),
            )
            .to_lowercase();
            words.iter().all(|w| haystack.contains(w))
        })
        .cloned()
        .collect()
}

fn shuffle_key(seed: u64, id: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut h);
    id.hash(&mut h);
    h.finish()
}

/// `songs` in `sort` order. Ties keep the album's order (disc, track), like Feishin's
/// secondary keys; `Random` is stable for one `seed`.
pub fn sort_songs(songs: Vec<Song>, sort: TrackSort, desc: bool, seed: u64) -> Vec<Song> {
    sort_items(songs, |s| s, sort, desc, seed)
}

/// [`sort_songs`] for rows that carry a song (the playlist page sorts rows with
/// their position in the playlist).
pub fn sort_items<T>(
    mut items: Vec<T>,
    song: impl Fn(&T) -> &Song,
    sort: TrackSort,
    desc: bool,
    seed: u64,
) -> Vec<T> {
    let lower = |s: Option<&str>| s.map(str::to_lowercase);
    match sort {
        TrackSort::Id => {
            if desc {
                items.reverse();
            }
            return items;
        }
        TrackSort::Random => {
            items.sort_by_key(|t| shuffle_key(seed, &song(t).id));
            return items;
        }
        TrackSort::Album => items.sort_by_key(|t| lower(song(t).album.as_deref())),
        TrackSort::AlbumArtist => {
            items.sort_by_key(|t| lower(song(t).album_artists.first().map(|a| a.name.as_str())))
        }
        TrackSort::Artist => items.sort_by_key(|t| song(t).artist_line().to_lowercase()),
        TrackSort::Bpm => items.sort_by_key(|t| song(t).bpm),
        TrackSort::Channels => items.sort_by_key(|t| song(t).channels),
        TrackSort::Comment => items.sort_by_key(|t| lower(song(t).comment.as_deref())),
        TrackSort::Duration => items.sort_by_key(|t| song(t).duration),
        TrackSort::Favorited => items.sort_by_key(|t| song(t).favorite),
        TrackSort::Genre => items.sort_by_key(|t| lower(song(t).genres.first().map(String::as_str))),
        TrackSort::Name => items.sort_by_key(|t| song(t).title.to_lowercase()),
        TrackSort::PlayCount => items.sort_by_key(|t| song(t).play_count),
        TrackSort::Rating => items.sort_by_key(|t| song(t).rating),
        TrackSort::RecentlyAdded => items.sort_by_key(|t| song(t).created.clone()),
        TrackSort::Year => items.sort_by_key(|t| song(t).year),
    }
    if desc {
        // Reversing a stable ascending sort would flip the ties too.
        items.reverse();
    }
    items
}

/// The play actions of the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagePlay {
    Now,
    Next,
    Last,
    Shuffle,
    NextShuffled,
    LastShuffled,
}

impl PagePlay {
    /// What a long press on the main Play button does: the shuffled version
    /// (Feishin `LONG_PRESS_PLAY_BEHAVIOR`).
    pub fn long_press_of(b: PlayBehavior) -> Self {
        match b {
            PlayBehavior::Now => PagePlay::Shuffle,
            PlayBehavior::Next => PagePlay::NextShuffled,
            PlayBehavior::Last => PagePlay::LastShuffled,
        }
    }

    pub fn of_behavior(b: PlayBehavior) -> Self {
        match b {
            PlayBehavior::Now => PagePlay::Now,
            PlayBehavior::Next => PagePlay::Next,
            PlayBehavior::Last => PagePlay::Last,
        }
    }
}

/// A "More from" carousel.
#[derive(Debug, Clone, PartialEq)]
pub struct Carousel {
    pub title: String,
    pub albums: Vec<Album>,
    /// Random genre carousels can be reloaded.
    pub genre: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AlbumPageMsg {
    Search(String),
    Sort(TrackSort),
    ToggleOrder,
    /// Select / deselect every track of a disc.
    Disc(u32),
    Play(PagePlay),
    /// The chevron next to the play buttons.
    TogglePlayMenu,
    Radio,
    RadioLoaded(PlayBehavior, Result<Vec<Song>, String>),
    Favorite,
    Rate(u8),
    /// The "..." button was pressed at a point.
    More(iced::Point),
    ToggleComment,
    /// Open an address in the browser.
    OpenUrl(String),
    /// A carousel of the album with this id: (album id, index, result).
    Carousel(String, usize, Result<Carousel, String>),
    /// Load the albums of a genre carousel again (random).
    Refresh(usize),
}

/// State of the album page. The rows of the track list are kept here, because
/// the list component works on a slice and search and sort change it.
#[derive(Debug, Default)]
pub struct AlbumPage {
    /// The album the state belongs to.
    pub album: Option<String>,
    pub search: String,
    pub sort: TrackSort,
    pub desc: bool,
    seed: u64,
    pub comment_open: bool,
    pub play_menu: bool,
    pub radio_busy: bool,
    /// The tracks as listed: searched and sorted.
    pub rows: Vec<Song>,
    pub carousels: Vec<Option<Carousel>>,
}

impl AlbumPage {
    /// Whether disc rows are shown: Feishin drops them when searching or sorting.
    pub fn grouped(&self) -> bool {
        self.search.trim().is_empty() && self.sort == TrackSort::Id && !self.desc
    }
}

impl Listen {
    /// The tracks of the album page as the list shows them.
    pub fn album_rows(&self, id: &str) -> &[Song] {
        if self.album_page.album.as_deref() == Some(id) { &self.album_page.rows } else { &[] }
    }

    /// Builds the rows again from the loaded album (after a load, a search or a sort).
    pub(super) fn album_rows_sync(&mut self) {
        let Some(album) = &self.library.album else {
            self.album_page.rows.clear();
            return;
        };
        let page = &self.album_page;
        let found = filter_songs(&album.songs, &page.search);
        self.album_page.rows = sort_songs(found, page.sort, page.desc, page.seed);
        let total = self.album_page.rows.len();
        if let Some(id) = ListId::of_route(&self.route) {
            self.lists.entry(&id).selection.retain_below(total);
        }
    }

    /// The album was loaded: reset the page when it is another album and load its carousels.
    pub(super) fn album_page_loaded(&mut self) -> Task<Message> {
        let Some(album) = self.library.album.clone() else { return Task::none() };
        let same = self.album_page.album.as_deref() == Some(album.id.as_str());
        if !same {
            self.album_page = AlbumPage { album: Some(album.id.clone()), ..AlbumPage::default() };
            self.lists.reset(&ListId::Album(album.id.clone()));
        }
        self.album_rows_sync();
        if same && !self.album_page.carousels.is_empty() {
            return Task::none();
        }
        self.load_carousels(&album)
    }

    /// What a source can do for this album (the header hides what it cannot).
    /// Demo albums behave like their source kind.
    pub fn album_caps(&self, album: &Album) -> Capabilities {
        if let Some(source) = self.sources.get(&album.server_id) {
            return source.capabilities();
        }
        if self.demo {
            return match album.source {
                SourceKind::YouTubeMusic => {
                    Capabilities::SIMILAR_SONGS | Capabilities::RADIO | Capabilities::PLAYLIST_EDIT
                }
                _ => {
                    Capabilities::RATINGS
                        | Capabilities::FAVORITE_ITEMS
                        | Capabilities::SIMILAR_SONGS
                        | Capabilities::ARTIST_INFO
                        | Capabilities::PLAYLIST_EDIT
                        | Capabilities::GENRES
                }
            };
        }
        Capabilities::empty()
    }

    fn load_carousels(&mut self, album: &Album) -> Task<Message> {
        let mut wanted: Vec<(String, Option<String>, Option<String>)> = Vec::new();
        if let Some(artist) = album.album_artists.first() {
            wanted.push((format!("More from {}", artist.name), artist.id.clone(), None));
        }
        for genre in album.genres.iter().take(MORE_GENRES) {
            wanted.push((format!("More from {genre}"), None, Some(genre.clone())));
        }
        self.album_page.carousels = vec![None; wanted.len()];
        let tasks: Vec<_> =
            wanted.into_iter().enumerate().map(|(i, w)| self.load_carousel(album, i, w)).collect();
        Task::batch(tasks)
    }

    fn load_carousel(
        &mut self,
        album: &Album,
        index: usize,
        (title, artist_id, genre): (String, Option<String>, Option<String>),
    ) -> Task<Message> {
        let current = album.id.clone();
        let wrap = move |r| Message::AlbumPage(AlbumPageMsg::Carousel(current.clone(), index, r));
        if self.demo {
            // Demo albums: the ones by the same artist / in the same genre.
            let (name, own) = (album.album_artists.first().map(|a| a.name.clone()), album.id.clone());
            let found: Vec<Album> = crate::demo::albums()
                .into_iter()
                .filter(|a| a.id != own)
                .filter(|a| match (&genre, &name) {
                    (Some(g), _) => a.genres.contains(g),
                    (None, Some(n)) => a.album_artists.first().map(|x| &x.name) == Some(n),
                    _ => false,
                })
                .collect();
            let carousel = Carousel { title, albums: found, genre };
            return Task::done(wrap(Ok(carousel)));
        }
        let Some(source) = self.sources.get(&album.server_id).cloned() else { return Task::none() };
        let own = album.id.clone();
        Task::perform(
            async move {
                let mut albums = match (&artist_id, &genre) {
                    (Some(id), _) => source.artist_albums(id).await.map_err(err)?,
                    (None, Some(g)) => {
                        let q = AlbumQuery {
                            sort: AlbumSort::Random,
                            order: SortOrder::Ascending,
                            genre: Some(g.clone()),
                            limit: MORE_COUNT + 1,
                            ..AlbumQuery::default()
                        };
                        source.albums(&q).await.map_err(err)?.items
                    }
                    (None, None) => Vec::new(),
                };
                albums.retain(|a| a.id != own);
                if artist_id.is_some() {
                    albums.sort_by_key(|a| std::cmp::Reverse(a.year.unwrap_or(0)));
                }
                albums.truncate(MORE_COUNT);
                Ok(Carousel { title, albums, genre })
            },
            wrap,
        )
    }

    pub(super) fn on_album_page(&mut self, msg: AlbumPageMsg) -> Task<Message> {
        match msg {
            AlbumPageMsg::Search(text) => {
                self.album_page.search = text;
                self.album_rows_changed();
            }
            AlbumPageMsg::Sort(sort) => {
                self.album_page.sort = sort;
                self.album_page.desc = sort.default_desc();
                self.album_page.seed = rand::random();
                self.album_rows_changed();
            }
            AlbumPageMsg::ToggleOrder => {
                self.album_page.desc = !self.album_page.desc;
                self.album_rows_changed();
            }
            AlbumPageMsg::Disc(disc) => {
                let Some(id) = self.album_page.album.clone() else { return Task::none() };
                let list = ListId::Album(id);
                let rows: Vec<usize> = self
                    .album_page
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.disc_number.unwrap_or(1) == disc)
                    .map(|(i, _)| i)
                    .collect();
                let selection = &mut self.lists.entry(&list).selection;
                let all = rows.iter().all(|i| selection.contains(*i));
                let mut keep: Vec<usize> =
                    selection.indices().into_iter().filter(|i| !rows.contains(i)).collect();
                if !all {
                    keep.extend(rows);
                }
                selection.clear();
                for i in keep {
                    selection.click(i, crate::ui::list::selection::Mods::CTRL, usize::MAX);
                }
            }
            AlbumPageMsg::Play(kind) => return self.page_play(kind),
            AlbumPageMsg::TogglePlayMenu => self.album_page.play_menu = !self.album_page.play_menu,
            AlbumPageMsg::Radio => return self.album_radio(),
            AlbumPageMsg::RadioLoaded(behavior, result) => {
                self.album_page.radio_busy = false;
                match result {
                    Ok(songs) if !songs.is_empty() => {
                        return self.play_items(songs.into_iter().map(Picked::Song).collect(), behavior);
                    }
                    Ok(_) => self.toast_info("No radio found for this album.".into()),
                    Err(e) => self.toast_error(format!("Could not start the album radio: {e}")),
                }
            }
            AlbumPageMsg::Favorite => return self.page_favorite(),
            AlbumPageMsg::Rate(stars) => return self.page_rate(stars),
            AlbumPageMsg::More(at) => {
                self.album_page.play_menu = false;
                if let Some(album) = &self.library.album {
                    return self.on_menu(MenuMsg::OpenAt(MenuTarget::album(album), at));
                }
            }
            AlbumPageMsg::ToggleComment => self.album_page.comment_open = !self.album_page.comment_open,
            AlbumPageMsg::OpenUrl(url) => open_url(&url),
            AlbumPageMsg::Carousel(album, index, result) => {
                if self.album_page.album.as_deref() != Some(album.as_str()) {
                    return Task::none();
                }
                match result {
                    Ok(carousel) => {
                        let images: Vec<_> = carousel
                            .albums
                            .iter()
                            .filter_map(|a| a.image.clone().map(|i| (a.server_id.clone(), i)))
                            .collect();
                        if let Some(slot) = self.album_page.carousels.get_mut(index) {
                            *slot = Some(carousel);
                        }
                        let tasks: Vec<_> =
                            images.iter().map(|(s, i)| self.request_image_on(s, Some(i))).collect();
                        return Task::batch(tasks);
                    }
                    Err(e) => tracing::warn!("album carousel: {e}"),
                }
            }
            AlbumPageMsg::Refresh(index) => {
                let Some(album) = self.library.album.clone() else { return Task::none() };
                let Some(Some(carousel)) = self.album_page.carousels.get(index).cloned() else {
                    return Task::none();
                };
                return self.load_carousel(&album, index, (carousel.title, None, carousel.genre));
            }
        }
        Task::none()
    }

    /// The rows changed (search, sort): the selection and page start over.
    fn album_rows_changed(&mut self) {
        self.album_rows_sync();
        if let Some(id) = self.album_page.album.clone() {
            self.lists.reset(&ListId::Album(id));
        }
    }

    /// Plays or queues the whole album per the header buttons.
    fn page_play(&mut self, kind: PagePlay) -> Task<Message> {
        self.album_page.play_menu = false;
        let Some(album) = self.library.album.clone() else { return Task::none() };
        if album.songs.is_empty() {
            return Task::none();
        }
        let (action, shuffled) = match kind {
            PagePlay::Now => return self.update(Message::PlayAlbumTrack(0)),
            PagePlay::Shuffle => (MenuAction::Shuffle, false),
            PagePlay::Next => (MenuAction::PlayNext, false),
            PagePlay::Last => (MenuAction::AddToQueue, false),
            PagePlay::NextShuffled => (MenuAction::PlayNext, true),
            PagePlay::LastShuffled => (MenuAction::AddToQueue, true),
        };
        let mut songs = album.songs;
        if shuffled {
            songs.shuffle(&mut rand::rng());
        }
        self.apply_to_songs(action, songs)
    }

    /// Queues songs similar to the album (the radio of a source) per the Play button setting.
    fn album_radio(&mut self) -> Task<Message> {
        let Some(album) = self.library.album.clone() else { return Task::none() };
        let Some(seed) = album.songs.first().cloned() else { return Task::none() };
        if self.album_page.radio_busy {
            return Task::none();
        }
        let behavior = self.play_behavior();
        if self.demo {
            // No server: the other demo albums stand in for the radio.
            let songs: Vec<Song> = crate::demo::albums()
                .into_iter()
                .filter(|a| a.id != album.id)
                .flat_map(|a| a.songs)
                .collect();
            return Task::done(Message::AlbumPage(AlbumPageMsg::RadioLoaded(behavior, Ok(songs))));
        }
        let Some(source) = self.sources.get(&album.server_id).cloned() else { return Task::none() };
        self.album_page.radio_busy = true;
        Task::perform(
            async move {
                let mut songs = vec![];
                // YouTube Music's radio starts from the song itself.
                if source.kind() == SourceKind::YouTubeMusic {
                    songs.push(seed.clone());
                }
                songs.extend(source.similar_songs(&seed, RADIO_COUNT).await.map_err(err)?);
                songs.truncate(RADIO_COUNT);
                Ok(songs)
            },
            move |r| Message::AlbumPage(AlbumPageMsg::RadioLoaded(behavior, r)),
        )
    }

    fn page_favorite(&mut self) -> Task<Message> {
        let Some(album) = self.library.album.clone() else { return Task::none() };
        if !self.album_caps(&album).contains(Capabilities::FAVORITE_ITEMS) {
            return Task::none();
        }
        let favorite = !album.favorite;
        self.set_album_state(&album.id, |a| a.favorite = favorite);
        let Some(source) = self.sources.get(&album.server_id).cloned() else { return Task::none() };
        let id = album.id;
        let ytm = album.source == SourceKind::YouTubeMusic;
        Task::perform(
            async move {
                source
                    .set_favorite_items(ItemKind::Album, &[id], favorite)
                    .await
                    .map(|_| match (favorite, ytm) {
                        (true, true) => "Saved to your library.",
                        (false, true) => "Removed from your library.",
                        (true, false) => "Added to favorites.",
                        (false, false) => "Removed from favorites.",
                    })
                    .map(str::to_string)
                    .map_err(|e| format!("Could not change the favorite: {e}"))
            },
            |r| Message::Menu(MenuMsg::Done(r)),
        )
    }

    fn page_rate(&mut self, stars: u8) -> Task<Message> {
        let Some(album) = self.library.album.clone() else { return Task::none() };
        if album.source == SourceKind::YouTubeMusic
            || !self.album_caps(&album).contains(Capabilities::RATINGS)
        {
            return Task::none();
        }
        // The same stars again clear the rating.
        let stars = if album.rating == Some(stars) { 0 } else { stars.min(5) };
        self.set_album_state(&album.id, |a| a.rating = (stars > 0).then_some(stars));
        let Some(source) = self.sources.get(&album.server_id).cloned() else { return Task::none() };
        let id = album.id;
        Task::perform(
            async move {
                source
                    .set_item_rating(ItemKind::Album, &id, stars)
                    .await
                    .map(|_| {
                        if stars == 0 { "Rating cleared.".to_string() } else { format!("Rated {stars} / 5.") }
                    })
                    .map_err(|e| format!("Could not rate the album: {e}"))
            },
            |r| Message::Menu(MenuMsg::Done(r)),
        )
    }

    /// Changes an album everywhere it is held (the page, the albums list, the carousels).
    fn set_album_state(&mut self, id: &str, change: impl Fn(&mut Album)) {
        if let Some(a) = self.library.album.as_mut().filter(|a| a.id == id) {
            change(a);
        }
        for a in self.library.albums.iter_mut().filter(|a| a.id == id) {
            change(a);
        }
        for a in self.album_page.carousels.iter_mut().flatten().flat_map(|c| c.albums.iter_mut()) {
            if a.id == id {
                change(a);
            }
        }
    }
}

/// Opens an address in the default browser.
pub fn open_url(url: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("spotify:")) {
        return;
    }
    let result = if cfg!(target_os = "windows") {
        {
            let mut cmd = std::process::Command::new("rundll32");
            listen_core::platform::no_window(&mut cmd);
            cmd.args(["url.dll,FileProtocolHandler", url]).spawn()
        }
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = result {
        tracing::warn!("opening {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, title: &str, disc: u32, track: u32, secs: u64) -> Song {
        let mut s = crate::demo::album_songs(0).remove(0);
        s.id = id.into();
        s.title = title.into();
        s.disc_number = Some(disc);
        s.track_number = Some(track);
        s.duration = std::time::Duration::from_secs(secs);
        s
    }

    fn ids(songs: &[Song]) -> Vec<&str> {
        songs.iter().map(|s| s.id.as_str()).collect()
    }

    fn sample() -> Vec<Song> {
        vec![
            song("a", "Zebra", 1, 1, 100),
            song("b", "apple", 1, 2, 300),
            song("c", "Mango", 2, 1, 100),
            song("d", "Berry", 2, 2, 200),
        ]
    }

    #[test]
    fn default_sort_keeps_the_album_order_and_reverses_on_request() {
        assert_eq!(ids(&sort_songs(sample(), TrackSort::Id, false, 0)), ["a", "b", "c", "d"]);
        assert_eq!(ids(&sort_songs(sample(), TrackSort::Id, true, 0)), ["d", "c", "b", "a"]);
    }

    #[test]
    fn sorts_by_name_ignoring_case_and_by_duration_with_stable_ties() {
        assert_eq!(ids(&sort_songs(sample(), TrackSort::Name, false, 0)), ["b", "d", "c", "a"]);
        // Descending duration: 300, 200, then the two 100 s songs.
        let by_duration = sort_songs(sample(), TrackSort::Duration, true, 0);
        assert_eq!(ids(&by_duration)[..2], ["b", "d"]);
    }

    #[test]
    fn random_is_stable_for_a_seed_and_keeps_every_song() {
        let a = sort_songs(sample(), TrackSort::Random, false, 7);
        let b = sort_songs(sample(), TrackSort::Random, false, 7);
        assert_eq!(a, b);
        let mut all = ids(&a);
        all.sort();
        assert_eq!(all, ["a", "b", "c", "d"]);
    }

    #[test]
    fn search_needs_every_word_in_any_field() {
        let songs = sample();
        assert_eq!(ids(&filter_songs(&songs, "")), ["a", "b", "c", "d"]);
        assert_eq!(ids(&filter_songs(&songs, "APPLE")), ["b"]);
        assert!(filter_songs(&songs, "apple zebra").is_empty());
        assert!(filter_songs(&songs, "no such song").is_empty());
    }

    #[test]
    fn descending_orders_start_where_feishin_starts() {
        assert!(TrackSort::Duration.default_desc());
        assert!(!TrackSort::Name.default_desc());
        assert_eq!(TrackSort::ALL.len(), 16);
    }
}
