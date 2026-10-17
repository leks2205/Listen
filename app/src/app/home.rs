//! Home page state and loading, as in Feishin's `features/home`: an endless feature carousel of
//! random albums, paged carousels per section (Most played, Explore, ...) and featured genres.
//! The source's own feed (`Library::home`, e.g. YouTube Music's Quick picks) sits above the
//! sections. Rendering is in `ui::home`.

use super::*;
use listen_core::model::Genre;
use listen_core::settings::HomeFeatureStyle;
use listen_source::{SongSort, SortOrder};
use std::collections::HashMap;
use std::time::Instant;

/// Items fetched per request (Feishin: `itemLimit` 20).
pub const LIMIT: usize = 20;
/// The feature carousel keeps at most this many albums.
const FEATURE_MAX: usize = 120;
/// Shift+wheel steps at most this often (Feishin: 250 ms).
const WHEEL_COOLDOWN: Duration = Duration::from_millis(250);
/// Wheel movements below this do nothing (Feishin: 10 px; iced often reports lines).
const WHEEL_MIN: f32 = 0.5;

/// A home section as the settings name it (`home.sections`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Genres,
    MostPlayed,
    Random,
    RecentlyAdded,
    RecentlyPlayed,
    RecentlyReleased,
    Playlists,
}

impl Section {
    /// Every section, in Feishin's default order.
    pub const ALL: [Section; 7] = [
        Section::Genres,
        Section::MostPlayed,
        Section::Random,
        Section::RecentlyAdded,
        Section::RecentlyPlayed,
        Section::RecentlyReleased,
        Section::Playlists,
    ];

    /// The id in `home.sections` (the Feishin setting).
    pub fn key(self) -> &'static str {
        match self {
            Section::Genres => "genres",
            Section::MostPlayed => "mostPlayed",
            Section::Random => "random",
            Section::RecentlyAdded => "recentlyAdded",
            Section::RecentlyPlayed => "recentlyPlayed",
            Section::RecentlyReleased => "recentlyReleased",
            Section::Playlists => "playlists",
        }
    }

    pub fn from_key(key: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|s| s.key() == key)
    }

    /// Feishin's row title (`page.home.*`).
    pub fn title(self) -> &'static str {
        match self {
            Section::Genres => "Genres",
            Section::MostPlayed => "Most Played",
            Section::Random => "Explore from your library",
            Section::RecentlyAdded => "Newly added releases",
            Section::RecentlyPlayed => "Recently played",
            Section::RecentlyReleased => "Recently released",
            Section::Playlists => "Playlists",
        }
    }

    /// Name in the settings list.
    pub fn setting_label(self) -> &'static str {
        match self {
            Section::Random => "Explore",
            Section::RecentlyAdded => "Newly added",
            s => s.title(),
        }
    }
}

/// A row of the home page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowId {
    /// Shelf `i` of the source's own feed (`Library::home`).
    Feed(usize),
    Section(Section),
}

/// What a section fetch returned.
#[derive(Debug, Clone)]
pub enum Items {
    Albums(Vec<Album>),
    Songs(Vec<Song>),
    Playlists(Vec<Playlist>),
}

impl Items {
    fn len(&self) -> usize {
        match self {
            Items::Albums(v) => v.len(),
            Items::Songs(v) => v.len(),
            Items::Playlists(v) => v.len(),
        }
    }
}

/// One section's loaded cards and where its carousel is.
#[derive(Debug, Default)]
pub struct SectionRow {
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
    pub playlists: Vec<Playlist>,
    /// Index of the first card shown.
    pub first: usize,
    pub loading: bool,
    /// The source has no more.
    pub done: bool,
    /// Bumped by a refresh so results of the old fetch are dropped.
    pub generation: u64,
    pub loaded: bool,
}

impl SectionRow {
    pub fn len(&self) -> usize {
        self.albums.len() + self.songs.len() + self.playlists.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The feature carousel: random albums with art, endless.
#[derive(Debug, Default)]
pub struct Feature {
    pub albums: Vec<Album>,
    pub index: usize,
    /// The slide from the album that was shown, while it runs.
    pub anim: Option<FeatureAnim>,
    pub loading: bool,
    pub done: bool,
    pub generation: u64,
}

/// How long the feature carousel slides.
pub const FEATURE_SLIDE: Duration = Duration::from_millis(350);

/// The slide between two feature albums.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureAnim {
    /// The album that is leaving (index into the albums).
    pub from: usize,
    /// +1: the next album comes in from the right; -1 from the left.
    pub dir: i8,
    pub started: Instant,
    /// Holds the slide at this progress (screenshots and tests).
    pub frozen: Option<f32>,
}

impl FeatureAnim {
    /// 0 at the start to 1 when the slide is over (eased out).
    pub fn progress(&self, now: Instant) -> f32 {
        if let Some(p) = self.frozen {
            return p;
        }
        let t = (now.saturating_duration_since(self.started).as_secs_f32() / FEATURE_SLIDE.as_secs_f32())
            .clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
    }

    pub fn finished(&self, now: Instant) -> bool {
        self.frozen.is_none() && now.saturating_duration_since(self.started) >= FEATURE_SLIDE
    }
}

/// Where the leaving and the entering slide are, in px from their resting place,
/// at `progress` of a slide over `width`.
pub fn slide_offsets(progress: f32, width: f32, dir: i8) -> (f32, f32) {
    let d = f32::from(dir.signum());
    (-d * progress * width, d * (1.0 - progress) * width)
}

#[derive(Debug, Default)]
pub struct HomeState {
    pub rows: HashMap<Section, SectionRow>,
    /// First card shown per feed shelf.
    pub feed_first: Vec<usize>,
    pub genres: Vec<Genre>,
    pub genres_loaded: bool,
    pub feature: Feature,
    wheel_at: Option<Instant>,
}

impl HomeState {
    pub fn row(&self, section: Section) -> Option<&SectionRow> {
        self.rows.get(&section)
    }

    pub fn feed_first(&self, shelf: usize) -> usize {
        self.feed_first.get(shelf).copied().unwrap_or(0)
    }

    /// Demo mode: rows without a server.
    pub fn demo(albums: &[Album]) -> Self {
        let mut s = HomeState::default();
        let mut reversed = albums.to_vec();
        reversed.reverse();
        let row = |albums: Vec<Album>| SectionRow { albums, done: true, loaded: true, ..Default::default() };
        s.rows.insert(Section::RecentlyAdded, row(albums.to_vec()));
        s.rows.insert(Section::MostPlayed, row(reversed));
        s.rows.insert(Section::Random, row(albums.iter().rev().cloned().collect()));
        s.genres = ["Rock", "Jazz", "Electronic", "Hip-Hop", "Classical", "Folk", "Metal", "Pop"]
            .iter()
            .map(|n| Genre { id: (*n).into(), name: (*n).into(), album_count: None, song_count: None })
            .collect();
        s.genres_loaded = true;
        s.feature = Feature { albums: albums.to_vec(), done: true, ..Default::default() };
        s
    }
}

/// Where the first card goes after a click on the previous (`dir < 0`) or next
/// button of a carousel showing `n` of `len` cards from `first`.
pub fn page_target(first: usize, n: usize, len: usize, dir: i8) -> usize {
    if dir > 0 { if first + n < len { first + n } else { first } } else { first.saturating_sub(n) }
}

/// Where the first card goes when a drag moves a carousel of `n` cards `k`
/// cards on (`dir > 0`) or back: never past the last full row.
pub fn shift_target(first: usize, k: usize, n: usize, len: usize, dir: i8) -> usize {
    if dir > 0 { (first + k).min(len.saturating_sub(n)).max(first) } else { first.saturating_sub(k) }
}

/// How many cards a drag of `dx` px moves a row whose cards are `card_px` apart
/// (at least one once it counts as a drag).
pub fn cards_dragged(dx: f32, card_px: f32) -> usize {
    ((dx.abs() / card_px.max(1.0)).round() as usize).max(1)
}

/// The next button works while there are more cards or more can be loaded.
pub fn can_next(first: usize, n: usize, len: usize, done: bool) -> bool {
    first + n < len || !done
}

/// Position `step` cards on in an endless carousel of `len` cards.
pub fn wrap_index(index: usize, step: usize, len: usize, dir: i8) -> usize {
    if len == 0 {
        return 0;
    }
    let step = step % len;
    if dir >= 0 { (index + step) % len } else { (index + len - step) % len }
}

/// Moves `key` in the visible list `sections` by `dir` places.
pub fn move_section(sections: &mut [String], i: usize, dir: i8) {
    let Some(j) = i.checked_add_signed(isize::from(dir)).filter(|j| *j < sections.len()) else { return };
    if i < sections.len() {
        sections.swap(i, j);
    }
}

/// Shows or hides `section` (new ones go to the end).
pub fn set_section_visible(sections: &mut Vec<String>, section: Section, visible: bool) {
    let key = section.key();
    let has = sections.iter().any(|s| s == key);
    if visible && !has {
        sections.push(key.to_string());
    } else if !visible {
        sections.retain(|s| s != key);
    }
}

/// The sections in settings order, visible ones first, then the hidden ones.
pub fn section_list(sections: &[String]) -> Vec<(Section, bool)> {
    let visible: Vec<Section> = sections.iter().filter_map(|k| Section::from_key(k)).collect();
    let mut out: Vec<(Section, bool)> = Vec::new();
    for s in visible {
        if !out.iter().any(|(o, _)| *o == s) {
            out.push((s, true));
        }
    }
    for s in Section::ALL {
        if !out.iter().any(|(o, _)| *o == s) {
            out.push((s, false));
        }
    }
    out
}

#[derive(Debug, Clone)]
pub enum HomeMsg {
    /// A section fetch finished (`start` = how many cards were loaded before).
    Section {
        section: Section,
        generation: u64,
        start: usize,
        result: Result<Items, String>,
    },
    /// Previous (-1) / next (1) page of a carousel showing `n` cards.
    Page(RowId, i8, usize),
    /// A drag over a row ended: how far (px, negative = left), the cards shown, the card spacing.
    Drag(RowId, f32, usize, f32),
    /// Shift + mouse wheel over a carousel.
    Wheel(RowId, f32, usize),
    Refresh(Section),
    FeatureLoaded {
        generation: u64,
        start: usize,
        result: Result<Vec<Album>, String>,
    },
    /// Previous / next feature album(s); the step is 1 (single) or the cards shown.
    Feature(i8, usize),
    FeatureWheel(f32, usize),
    /// A frame of the slide: the view reads the clock, this redraws it and ends it.
    FeatureFrame,
    Genres(Result<Vec<Genre>, String>),
    PlayGenre(String),
    PlaySectionSong(Section, usize),
    GenreSongs(Result<Vec<Song>, String>),
    /// Settings: show / hide a section.
    SectionVisible(Section, bool),
    /// Settings: move a visible section.
    SectionMove(usize, i8),
}

impl Listen {
    /// The source's feed and every visible section, the feature carousel and the genres.
    pub(super) fn load_home(&mut self) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        let feed = Task::perform(
            async move {
                match source.home().await {
                    Ok(shelves) => shelves,
                    Err(listen_source::SourceError::Unsupported) => vec![],
                    Err(e) => {
                        tracing::warn!("home feed: {e}");
                        vec![]
                    }
                }
            },
            self.scoped(Message::HomeLoaded),
        );
        let mut tasks = vec![feed];
        let sections: Vec<Section> =
            self.config.settings.home.sections.iter().filter_map(|k| Section::from_key(k)).collect();
        for section in sections {
            tasks.push(self.refresh_section(section));
        }
        if self.config.settings.home.feature {
            tasks.push(self.load_feature(true));
        }
        Task::batch(tasks)
    }

    /// The Home page is shown: load it the first time, or the feature carousel when it was switched on.
    pub(super) fn home_enter(&mut self) -> Task<Message> {
        if !self.library.home_loaded {
            return self.load_home();
        }
        let f = &self.library.home_state.feature;
        if self.config.settings.home.feature && f.albums.is_empty() && !f.loading && !f.done {
            return self.load_feature(false);
        }
        Task::none()
    }

    /// Starts a section over (first fetch, or the refresh button).
    fn refresh_section(&mut self, section: Section) -> Task<Message> {
        if section == Section::Genres {
            return self.load_genres();
        }
        let row = self.library.home_state.rows.entry(section).or_default();
        let generation = row.generation + 1;
        *row = SectionRow { generation, ..Default::default() };
        self.load_section(section, 0)
    }

    fn load_genres(&mut self) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        Task::perform(
            async move { source.genres().await.map_err(err) },
            self.scoped(|r| Message::Home(HomeMsg::Genres(r))),
        )
    }

    /// Fetches the cards of `section` after the first `start`.
    fn load_section(&mut self, section: Section, start: usize) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        let row = self.library.home_state.rows.entry(section).or_default();
        if row.loading || row.done {
            return Task::none();
        }
        row.loading = true;
        let generation = row.generation;
        let jellyfin = source.kind() == SourceKind::Jellyfin;
        Task::perform(
            async move { fetch_section(source, section, start, jellyfin).await },
            self.scoped(move |result| Message::Home(HomeMsg::Section { section, generation, start, result })),
        )
    }

    /// More random albums for the feature carousel (`restart`: a new set).
    fn load_feature(&mut self, restart: bool) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        let f = &mut self.library.home_state.feature;
        if restart {
            *f = Feature { generation: f.generation + 1, ..Default::default() };
        }
        if f.loading || f.done || f.albums.len() >= FEATURE_MAX {
            return Task::none();
        }
        f.loading = true;
        let (generation, start) = (f.generation, f.albums.len());
        let q = AlbumQuery { sort: AlbumSort::Random, limit: LIMIT, start, ..Default::default() };
        Task::perform(
            async move { source.albums(&q).await.map(|p| p.items).map_err(err) },
            self.scoped(move |result| Message::Home(HomeMsg::FeatureLoaded { generation, start, result })),
        )
    }

    /// Images (and the blurred background) of what the feature carousel shows.
    fn feature_images(&mut self, step: usize) -> Task<Message> {
        let f = &self.library.home_state.feature;
        let style = self.config.settings.home.feature_style;
        let shown = if style == HomeFeatureStyle::Single { 1 } else { step.max(1) };
        let albums: Vec<Album> = (0..shown.min(f.albums.len()))
            .map(|i| f.albums[(f.index + i) % f.albums.len()].clone())
            .collect();
        let refs: Vec<ImageRef> = albums.iter().filter_map(|a| a.image.clone()).collect();
        let blur = albums.first().and_then(|a| a.image.clone());
        let mut tasks = vec![self.request_images(refs.iter().map(Some))];
        if style == HomeFeatureStyle::Single {
            tasks.push(self.request_blur(blur.as_ref()));
        }
        Task::batch(tasks)
    }

    /// Starts the album of `--autoplay` (and the testing hook for a named one).
    pub(super) fn start_autoplay(&mut self, id: String, images: Task<Message>) -> Task<Message> {
        self.autoplay_dj = self.ai_dj_on();
        if let (Ok(name), Some(source)) = (std::env::var("LISTEN_AUTOPLAY_ALBUM"), self.source.clone()) {
            let find = Task::perform(
                async move {
                    let found = source.search(&name, 20).await.map_err(err)?;
                    let album = found
                        .albums
                        .into_iter()
                        .find(|a| a.name.eq_ignore_ascii_case(&name))
                        .ok_or(format!("no album “{name}”"))?;
                    source.album(&album.id).await.map_err(err)
                },
                Message::AlbumForPlay,
            );
            return Task::batch([images, find]);
        }
        Task::batch([images, self.update(Message::PlayAlbum(id))])
    }

    /// Cards of a row: (cards loaded, all loaded) for the paging buttons.
    fn row_extent(&self, row: RowId) -> (usize, bool) {
        match row {
            RowId::Feed(i) => {
                let n = self
                    .library
                    .home
                    .get(i)
                    .map_or(0, |s| s.albums.len() + s.playlists.len() + s.songs.len());
                (n, true)
            }
            RowId::Section(s) => self.library.home_state.row(s).map_or((0, true), |r| (r.len(), r.done)),
        }
    }

    fn home_images(&mut self, row: RowId, first: usize, n: usize) -> Task<Message> {
        let refs: Vec<ImageRef> = match row {
            RowId::Feed(i) => self.library.home.get(i).map_or(vec![], |s| {
                s.albums
                    .iter()
                    .map(|a| a.image.clone())
                    .chain(s.playlists.iter().map(|p| p.image.clone()))
                    .chain(s.songs.iter().map(|x| x.image.clone()))
                    .skip(first)
                    .take(n * 2)
                    .flatten()
                    .collect()
            }),
            RowId::Section(s) => self.library.home_state.row(s).map_or(vec![], |r| {
                r.albums
                    .iter()
                    .map(|a| a.image.clone())
                    .chain(r.playlists.iter().map(|p| p.image.clone()))
                    .chain(r.songs.iter().map(|x| x.image.clone()))
                    .skip(first)
                    .take(n * 2)
                    .flatten()
                    .collect()
            }),
        };
        self.request_images(refs.iter().map(Some))
    }

    /// Moves a row `step` cards (a page: `step == n`) while `n` are shown.
    fn home_page(&mut self, row: RowId, dir: i8, step: usize, n: usize) -> Task<Message> {
        let n = n.max(1);
        let (len, done) = self.row_extent(row);
        let state = &mut self.library.home_state;
        let first = match row {
            RowId::Feed(i) => state.feed_first(i),
            RowId::Section(s) => state.row(s).map_or(0, |r| r.first),
        };
        let target =
            if step == n { page_target(first, n, len, dir) } else { shift_target(first, step, n, len, dir) };
        match row {
            RowId::Feed(i) => {
                if state.feed_first.len() <= i {
                    state.feed_first.resize(i + 1, 0);
                }
                state.feed_first[i] = target;
            }
            RowId::Section(s) => {
                if let Some(r) = state.rows.get_mut(&s) {
                    r.first = target;
                }
            }
        }
        let mut tasks = vec![self.home_images(row, target, n)];
        if let RowId::Section(s) = row
            && !done
            && target + 2 * n > len
        {
            tasks.push(self.load_section(s, len));
        }
        Task::batch(tasks)
    }

    /// True when a Shift+wheel step may happen now (and starts the cooldown).
    fn wheel_step(&mut self, dy: f32) -> Option<i8> {
        let moved = dy.abs() >= WHEEL_MIN;
        let now = Instant::now();
        let cooled = self.library.home_state.wheel_at.is_none_or(|t| now.duration_since(t) >= WHEEL_COOLDOWN);
        if !self.modifiers.shift() || !moved || !cooled {
            return None;
        }
        self.library.home_state.wheel_at = Some(now);
        // iced: negative y scrolls down; Feishin steps forward on a downward wheel.
        Some(if dy < 0.0 { 1 } else { -1 })
    }

    fn feature_step(&mut self, dir: i8, step: usize) -> Task<Message> {
        let style = self.config.settings.home.feature_style;
        let step = if style == HomeFeatureStyle::Single { 1 } else { step.max(1) };
        let f = &mut self.library.home_state.feature;
        let from = f.index;
        f.index = wrap_index(f.index, step, f.albums.len(), dir);
        if style == HomeFeatureStyle::Single && f.albums.len() > 1 && f.index != from {
            f.anim = Some(FeatureAnim { from, dir, started: Instant::now(), frozen: None });
        }
        let near_end = f.albums.len().saturating_sub(f.index) < if step == 1 { 3 } else { 2 * step };
        let mut tasks = vec![self.feature_images(step)];
        if near_end {
            tasks.push(self.load_feature(false));
        }
        Task::batch(tasks)
    }

    pub(super) fn on_home(&mut self, msg: HomeMsg) -> Task<Message> {
        match msg {
            HomeMsg::Section { section, generation, start, result } => {
                let row = self.library.home_state.rows.entry(section).or_default();
                if row.generation != generation {
                    return Task::none();
                }
                row.loading = false;
                row.loaded = true;
                match result {
                    Ok(items) => {
                        let got = items.len();
                        if start != row.len() {
                            return Task::none();
                        }
                        let mut autoplay = None;
                        match items {
                            Items::Albums(v) => {
                                for a in v {
                                    if !row.albums.iter().any(|x| x.id == a.id) {
                                        row.albums.push(a);
                                    }
                                }
                                autoplay = row.albums.first().map(|a| a.id.clone());
                            }
                            Items::Songs(v) => {
                                for s in v {
                                    if !row.songs.iter().any(|x| x.id == s.id) {
                                        row.songs.push(s);
                                    }
                                }
                            }
                            Items::Playlists(v) => {
                                row.playlists = v;
                                row.done = true;
                            }
                        }
                        row.done |= got < LIMIT;
                        // A page that was all duplicates (random) ends the row.
                        if got > 0 && row.len() == start {
                            row.done = true;
                        }
                        let first = row.first;
                        let images = self.home_images(RowId::Section(section), first, 10);
                        let autoplay = autoplay.filter(|_| self.autoplay);
                        if let Some(id) = autoplay {
                            self.autoplay = false;
                            return self.start_autoplay(id, images);
                        }
                        return images;
                    }
                    Err(e) => {
                        tracing::warn!("home row {}: {e}", section.title());
                        row.done = true;
                    }
                }
            }
            HomeMsg::Page(row, dir, n) => return self.home_page(row, dir, n, n),
            HomeMsg::Drag(row, dx, n, card_px) => {
                // Dragging left brings the cards on the right into view.
                let dir = if dx < 0.0 { 1 } else { -1 };
                return self.home_page(row, dir, cards_dragged(dx, card_px), n);
            }
            HomeMsg::Wheel(row, dy, n) => {
                if let Some(dir) = self.wheel_step(dy) {
                    return self.home_page(row, dir, n, n);
                }
            }
            HomeMsg::Refresh(section) => return self.refresh_section(section),
            HomeMsg::FeatureLoaded { generation, start, result } => {
                let f = &mut self.library.home_state.feature;
                if f.generation != generation {
                    return Task::none();
                }
                f.loading = false;
                match result {
                    Ok(albums) => {
                        let got = albums.len();
                        if start == f.albums.len() {
                            for a in albums.into_iter().filter(|a| a.image.is_some()) {
                                if !f.albums.iter().any(|x| x.id == a.id) {
                                    f.albums.push(a);
                                }
                            }
                        }
                        f.done = got < LIMIT;
                        // Too few with art after filtering: ask for the next page (Feishin: < 2 pages).
                        let more = !f.done && f.albums.len() < LIMIT * 2 && f.albums.len() < FEATURE_MAX;
                        let images = self.feature_images(1);
                        return if more { Task::batch([images, self.load_feature(false)]) } else { images };
                    }
                    Err(e) => {
                        tracing::warn!("home feature: {e}");
                        f.done = true;
                    }
                }
            }
            HomeMsg::Feature(dir, step) => return self.feature_step(dir, step),
            HomeMsg::FeatureFrame => {
                let f = &mut self.library.home_state.feature;
                if f.anim.is_some_and(|a| a.finished(Instant::now())) {
                    f.anim = None;
                }
            }
            HomeMsg::FeatureWheel(dy, step) => {
                if let Some(dir) = self.wheel_step(dy) {
                    return self.feature_step(dir, step);
                }
            }
            HomeMsg::Genres(result) => {
                let state = &mut self.library.home_state;
                state.genres_loaded = true;
                match result {
                    Ok(mut genres) => {
                        use rand::seq::SliceRandom;
                        genres.shuffle(&mut rand::rng());
                        state.genres = genres;
                    }
                    Err(e) => tracing::warn!("home genres: {e}"),
                }
            }
            HomeMsg::PlayGenre(name) => {
                let Some(source) = self.source.clone() else { return Task::none() };
                let q =
                    SongQuery { sort: SongSort::Random, genre: Some(name), limit: 100, ..Default::default() };
                return Task::perform(
                    async move { source.songs(&q).await.map(|p| p.items).map_err(err) },
                    |r| Message::Home(HomeMsg::GenreSongs(r)),
                );
            }
            HomeMsg::PlaySectionSong(section, i) => {
                let songs = self.library.home_state.row(section).map(|r| r.songs.clone()).unwrap_or_default();
                return self.play_picked(songs, i);
            }
            HomeMsg::GenreSongs(Ok(songs)) => return self.play_picked(songs, 0),
            HomeMsg::GenreSongs(Err(e)) => self.toast_error(format!("Could not load the genre: {e}")),
            HomeMsg::SectionVisible(section, visible) => {
                set_section_visible(&mut self.config.settings.home.sections, section, visible);
                self.save_config();
                let loaded = self.library.home_state.row(section).is_some_and(|r| r.loaded)
                    || (section == Section::Genres && self.library.home_state.genres_loaded);
                if visible && !loaded {
                    return self.refresh_section(section);
                }
            }
            HomeMsg::SectionMove(i, dir) => {
                move_section(&mut self.config.settings.home.sections, i, dir);
                self.save_config();
            }
        }
        Task::none()
    }
}

/// One page of a section from the source.
async fn fetch_section(
    source: Arc<dyn MusicSource>,
    section: Section,
    start: usize,
    jellyfin: bool,
) -> Result<Items, String> {
    let album = |sort: AlbumSort, order: SortOrder| AlbumQuery {
        sort,
        order,
        limit: LIMIT,
        start,
        ..Default::default()
    };
    let albums = |q: AlbumQuery| {
        let source = source.clone();
        async move { source.albums(&q).await.map(|p| Items::Albums(p.items)).map_err(err) }
    };
    match section {
        Section::MostPlayed if jellyfin => {
            let q = SongQuery {
                sort: SongSort::MostPlayed,
                order: SortOrder::Descending,
                limit: LIMIT,
                start,
                ..Default::default()
            };
            source.songs(&q).await.map(|p| Items::Songs(p.items)).map_err(err)
        }
        Section::MostPlayed => albums(album(AlbumSort::MostPlayed, SortOrder::Descending)).await,
        Section::Random => albums(album(AlbumSort::Random, SortOrder::Ascending)).await,
        Section::RecentlyAdded => albums(album(AlbumSort::RecentlyAdded, SortOrder::Descending)).await,
        Section::RecentlyPlayed => albums(album(AlbumSort::RecentlyPlayed, SortOrder::Descending)).await,
        Section::RecentlyReleased => albums(album(AlbumSort::Year, SortOrder::Descending)).await,
        Section::Playlists => {
            if start > 0 {
                return Ok(Items::Playlists(vec![]));
            }
            source.playlists().await.map(Items::Playlists).map_err(err)
        }
        Section::Genres => Ok(Items::Albums(vec![])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Args;

    fn rig() -> Listen {
        Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() }).0
    }

    #[test]
    fn paging_steps_by_the_cards_shown_and_stops_at_the_ends() {
        assert_eq!(page_target(0, 4, 10, 1), 4);
        assert_eq!(page_target(4, 4, 10, 1), 8);
        assert_eq!(page_target(8, 4, 10, 1), 8, "the last page stays");
        assert_eq!(page_target(8, 4, 10, -1), 4);
        assert_eq!(page_target(2, 4, 10, -1), 0);
        assert!(can_next(0, 4, 10, true));
        assert!(!can_next(8, 4, 10, true));
        assert!(can_next(8, 4, 10, false), "more can be loaded");
    }

    #[test]
    fn the_feature_carousel_wraps_both_ways() {
        assert_eq!(wrap_index(0, 1, 5, 1), 1);
        assert_eq!(wrap_index(4, 1, 5, 1), 0);
        assert_eq!(wrap_index(0, 1, 5, -1), 4);
        assert_eq!(wrap_index(1, 3, 5, -1), 3);
        assert_eq!(wrap_index(0, 1, 0, 1), 0);
    }

    #[test]
    fn section_settings_round_trip() {
        let mut s: Vec<String> = ["mostPlayed", "random"].map(String::from).to_vec();
        set_section_visible(&mut s, Section::Playlists, true);
        assert_eq!(s, ["mostPlayed", "random", "playlists"]);
        move_section(&mut s, 2, -1);
        assert_eq!(s, ["mostPlayed", "playlists", "random"]);
        move_section(&mut s, 0, -1);
        assert_eq!(s[0], "mostPlayed", "moving off the top does nothing");
        set_section_visible(&mut s, Section::MostPlayed, false);
        let list = section_list(&s);
        assert_eq!(list[0], (Section::Playlists, true));
        assert!(list.contains(&(Section::MostPlayed, false)));
        assert_eq!(list.len(), Section::ALL.len());
    }

    fn many(n: usize) -> Vec<Album> {
        let base = crate::demo::albums();
        (0..n)
            .map(|i| {
                let mut a = base[i % base.len()].clone();
                a.id = format!("a{i}");
                a
            })
            .collect()
    }

    #[test]
    fn a_row_pages_and_loads_more_when_the_end_is_near() {
        let mut s = rig();
        let section = Section::MostPlayed;
        s.library
            .home_state
            .rows
            .insert(section, SectionRow { albums: many(20), loaded: true, ..Default::default() });
        let row = RowId::Section(section);
        let _ = s.on_home(HomeMsg::Page(row, 1, 5));
        assert_eq!(s.library.home_state.row(section).unwrap().first, 5);
        // 5 + 10 > 20 would not be near, 10 + 10 > 20 is not either, 15 + 10 > 20 is.
        assert!(!s.library.home_state.row(section).unwrap().loading);
        let _ = s.on_home(HomeMsg::Page(row, 1, 5));
        let _ = s.on_home(HomeMsg::Page(row, 1, 5));
        let r = s.library.home_state.row(section).unwrap();
        assert_eq!(r.first, 15);
        // Demo has no source, so nothing is requested, but a real one would be:
        // feed the next page by hand.
        let generation = r.generation;
        let _ = s.on_home(HomeMsg::Section {
            section,
            generation,
            start: 20,
            result: Ok(Items::Albums(many(40)[20..].to_vec())),
        });
        let r = s.library.home_state.row(section).unwrap();
        assert_eq!(r.len(), 40);
        assert!(!r.done);
        // A stale result (after a refresh) is dropped.
        let _ = s.on_home(HomeMsg::Section {
            section,
            generation: generation + 7,
            start: 40,
            result: Ok(Items::Albums(many(60)[40..].to_vec())),
        });
        assert_eq!(s.library.home_state.row(section).unwrap().len(), 40);
        // The last, short page ends the row.
        let _ = s.on_home(HomeMsg::Section {
            section,
            generation,
            start: 40,
            result: Ok(Items::Albums(many(45)[40..].to_vec())),
        });
        assert!(s.library.home_state.row(section).unwrap().done);
    }

    #[test]
    fn feed_shelves_page_too() {
        let mut s = rig();
        s.library.home = vec![HomeShelf {
            title: "Quick picks".into(),
            albums: many(12),
            playlists: vec![],
            songs: vec![],
        }];
        let _ = s.on_home(HomeMsg::Page(RowId::Feed(0), 1, 5));
        assert_eq!(s.library.home_state.feed_first(0), 5);
        let _ = s.on_home(HomeMsg::Page(RowId::Feed(0), 1, 5));
        let _ = s.on_home(HomeMsg::Page(RowId::Feed(0), 1, 5));
        assert_eq!(s.library.home_state.feed_first(0), 10, "no further than the last card page");
        let _ = s.on_home(HomeMsg::Page(RowId::Feed(0), -1, 5));
        assert_eq!(s.library.home_state.feed_first(0), 5);
    }

    #[test]
    fn the_feature_carousel_steps_and_the_wheel_needs_shift() {
        let mut s = rig();
        s.library.home_state.feature = Feature { albums: many(6), done: true, ..Default::default() };
        let _ = s.on_home(HomeMsg::Feature(1, 1));
        assert_eq!(s.library.home_state.feature.index, 1);
        let _ = s.on_home(HomeMsg::Feature(-1, 1));
        let _ = s.on_home(HomeMsg::Feature(-1, 1));
        assert_eq!(s.library.home_state.feature.index, 5, "previous wraps to the end");
        // Wheel without Shift does nothing.
        let _ = s.on_home(HomeMsg::FeatureWheel(-1.0, 1));
        assert_eq!(s.library.home_state.feature.index, 5);
        s.modifiers = iced::keyboard::Modifiers::SHIFT;
        let _ = s.on_home(HomeMsg::FeatureWheel(-1.0, 1));
        assert_eq!(s.library.home_state.feature.index, 0);
        // The cooldown swallows a second step right away.
        let _ = s.on_home(HomeMsg::FeatureWheel(-1.0, 1));
        assert_eq!(s.library.home_state.feature.index, 0);
    }

    #[test]
    fn the_single_feature_slides_between_albums_and_the_slide_ends() {
        let mut s = rig();
        s.library.home_state.feature = Feature { albums: many(6), done: true, ..Default::default() };
        assert!(s.library.home_state.feature.anim.is_none());
        let _ = s.on_home(HomeMsg::Feature(1, 1));
        let anim = s.library.home_state.feature.anim.expect("sliding");
        assert_eq!((anim.from, anim.dir), (0, 1));
        // A frame in the middle keeps it; one after the slide ends it.
        let _ = s.on_home(HomeMsg::FeatureFrame);
        assert!(s.library.home_state.feature.anim.is_some());
        s.library.home_state.feature.anim =
            Some(FeatureAnim { started: Instant::now() - FEATURE_SLIDE * 2, ..anim });
        let _ = s.on_home(HomeMsg::FeatureFrame);
        assert!(s.library.home_state.feature.anim.is_none());
        // A single album has nothing to slide to.
        s.library.home_state.feature = Feature { albums: many(1), done: true, ..Default::default() };
        let _ = s.on_home(HomeMsg::Feature(1, 1));
        assert!(s.library.home_state.feature.anim.is_none());
    }

    #[test]
    fn slide_offsets_cross_over_and_ease_out() {
        let now = Instant::now();
        let a = FeatureAnim { from: 0, dir: 1, started: now, frozen: None };
        assert_eq!(a.progress(now), 0.0);
        assert!(a.progress(now + FEATURE_SLIDE / 2) > 0.5, "eased out: more than half way at half time");
        assert_eq!(a.progress(now + FEATURE_SLIDE * 2), 1.0);
        // Next: the old one leaves to the left, the new one comes from the right.
        assert_eq!(slide_offsets(0.0, 800.0, 1), (0.0, 800.0));
        assert_eq!(slide_offsets(1.0, 800.0, 1), (-800.0, 0.0));
        // Previous: the other way.
        assert_eq!(slide_offsets(0.25, 800.0, -1), (200.0, -600.0));
    }

    #[test]
    fn dragging_moves_a_row_by_the_cards_dragged_and_stops_at_the_end() {
        assert_eq!(cards_dragged(-30.0, 200.0), 1, "any drag moves at least one card");
        assert_eq!(cards_dragged(-410.0, 200.0), 2);
        assert_eq!(cards_dragged(1000.0, 200.0), 5);
        // Forward: by k, never past the last full row, never backwards.
        assert_eq!(shift_target(0, 3, 5, 20, 1), 3);
        assert_eq!(shift_target(14, 3, 5, 20, 1), 15);
        assert_eq!(shift_target(15, 3, 5, 20, 1), 15);
        assert_eq!(shift_target(0, 3, 5, 4, 1), 0, "a short row does not move");
        // Back: to the start at most.
        assert_eq!(shift_target(5, 3, 5, 20, -1), 2);
        assert_eq!(shift_target(1, 3, 5, 20, -1), 0);
    }

    #[test]
    fn a_drag_over_a_row_pages_it() {
        let mut s = rig();
        let (row, section) = (RowId::Section(Section::MostPlayed), Section::MostPlayed);
        s.library.home_state.rows.entry(section).or_default().albums = many(20);
        // Left by two cards (card spacing 100 px): the cards on the right come in.
        let _ = s.on_home(HomeMsg::Drag(row, -210.0, 5, 100.0));
        assert_eq!(s.library.home_state.row(section).unwrap().first, 2);
        // Right: back.
        let _ = s.on_home(HomeMsg::Drag(row, 100.0, 5, 100.0));
        assert_eq!(s.library.home_state.row(section).unwrap().first, 1);
    }

    #[test]
    fn section_visibility_is_saved_in_the_settings() {
        let mut s = rig();
        let _ = s.on_home(HomeMsg::SectionVisible(Section::Playlists, false));
        assert!(!s.config.settings.home.sections.iter().any(|k| k == "playlists"));
        let _ = s.on_home(HomeMsg::SectionVisible(Section::Playlists, true));
        assert_eq!(s.config.settings.home.sections.last().map(String::as_str), Some("playlists"));
    }
}
