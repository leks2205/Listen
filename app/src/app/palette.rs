//! The command palette (Feishin `features/search/components/command-palette*`): `mod+k` opens
//! a search box (400 ms debounce) with live results, a page stack of commands and ↑ ↓ ⏎ Esc keys.
//! Rendering is `ui::palette`.

use super::overlay::Modal;
use super::search::{Debounce, SearchMsg};
use super::*;
use std::collections::HashSet;

pub const MODAL_KEY: &str = "palette";
/// The id of the palette's text input.
pub const INPUT_ID: &str = "palette-input";
/// Results per type shown in the palette.
pub const RESULT_LIMIT: usize = 5;

/// Pages of the palette (the breadcrumb at the bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    GoTo,
    Servers,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::GoTo => "Go to page",
            Page::Servers => "Servers",
        }
    }
}

/// A group of rows under a heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Albums,
    Artists,
    Songs,
    Playlists,
    Commands,
    Pages,
    Library,
    Servers,
    Manage,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Albums => "Albums",
            Group::Artists => "Artists",
            Group::Songs => "Tracks",
            Group::Playlists => "Playlists",
            Group::Commands => "Search",
            Group::Pages => "Pages",
            Group::Library => "Library",
            Group::Servers => "Select server",
            Group::Manage => "Manage",
        }
    }

    /// Result groups can be folded away.
    pub fn collapsible(self) -> bool {
        matches!(self, Group::Albums | Group::Artists | Group::Songs | Group::Playlists)
    }
}

/// What an item does when it is chosen.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Cmd {
    /// Open the search page with the typed text.
    SearchFor,
    GoToPage,
    ServerCommands,
    Go(Route),
    Server(ServerId),
    ManageServers,
    /// Opens the create playlist dialog.
    CreatePlaylist,
    Album(Album),
    Artist(Artist),
    Song(Song),
    Playlist(Playlist),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub cmd: Cmd,
    pub label: String,
    pub sub: String,
}

/// A row of the list: a heading (with the number of results) or an item.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Row {
    Heading(Group, Option<usize>),
    Item(Item),
}

#[derive(Debug)]
pub struct PaletteState {
    pub pages: Vec<Page>,
    pub query: String,
    /// Index among the items (headings do not count).
    pub selected: usize,
    pub results: SearchResults,
    pub loading: bool,
    pub collapsed: HashSet<Group>,
    generation: u64,
    debounce: Debounce,
}

impl Default for PaletteState {
    fn default() -> Self {
        Self {
            pages: vec![Page::Home],
            query: String::new(),
            selected: 0,
            results: SearchResults::default(),
            loading: false,
            collapsed: HashSet::new(),
            generation: 0,
            debounce: Debounce::default(),
        }
    }
}

impl PaletteState {
    pub fn page(&self) -> Page {
        self.pages.last().copied().unwrap_or(Page::Home)
    }
}

#[derive(Debug, Clone)]
pub enum PaletteMsg {
    /// The sidebar's search box.
    Open,
    Input(String),
    Debounced(u64),
    Loaded(u64, Result<SearchResults, String>),
    /// ↑ (-1) / ↓ (1).
    Move(i8),
    /// ⏎ on the selected item.
    Activate,
    /// A click on item `n`.
    Choose(usize),
    /// Esc: back one page, or close from the first page.
    Back,
    /// A click on a breadcrumb: back to this depth.
    Crumb(usize),
    Clear,
    Toggle(Group),
    /// Show every result of a group on the search page.
    ViewAll(Group),
}

/// The keys the palette handles whatever has the focus (the text input
/// captures Esc and Enter itself).
pub fn key_message(event: &iced::keyboard::Event) -> Option<PaletteMsg> {
    use iced::keyboard::{Event, Key, key::Named};
    match event {
        Event::KeyPressed { key: Key::Named(Named::ArrowDown), .. } => Some(PaletteMsg::Move(1)),
        Event::KeyPressed { key: Key::Named(Named::ArrowUp), .. } => Some(PaletteMsg::Move(-1)),
        Event::KeyPressed { key: Key::Named(Named::Escape), .. } => Some(PaletteMsg::Back),
        _ => None,
    }
}

/// Subscription while the palette is open.
pub fn subscription(state: &Listen) -> iced::Subscription<Message> {
    if !state.palette_open() {
        return iced::Subscription::none();
    }
    iced::event::listen_with(|event, status, _| match event {
        iced::Event::Keyboard(e) => match (&e, status) {
            (
                iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
                    ..
                },
                iced::event::Status::Ignored,
            ) => Some(Message::Palette(PaletteMsg::Activate)),
            _ => key_message(&e).map(Message::Palette),
        },
        _ => None,
    })
}

fn matches(label: &str, query: &str) -> bool {
    query.is_empty() || label.to_lowercase().contains(&query.to_lowercase())
}

impl Listen {
    pub fn palette_open(&self) -> bool {
        matches!(self.overlay.modal, Some(Modal::Custom(MODAL_KEY)))
    }

    /// `mod+k`: opens the palette, or closes it when it is open.
    pub fn open_palette(&mut self) -> Task<Message> {
        if self.palette_open() {
            self.close_modal();
            return Task::none();
        }
        self.palette = PaletteState::default();
        self.open_modal(MODAL_KEY);
        iced::widget::operation::focus(iced::widget::Id::new(INPUT_ID))
    }

    /// The rows of the current page for the typed text.
    pub fn palette_rows(&self) -> Vec<Row> {
        let p = &self.palette;
        let q = p.query.trim();
        let mut rows = Vec::new();
        let item =
            |cmd: Cmd, label: &str| Row::Item(Item { cmd, label: label.to_string(), sub: String::new() });
        match p.page() {
            Page::Home => {
                let shown = !q.is_empty();
                let r = &p.results;
                let mut group = |g: Group, items: Vec<Item>| {
                    if shown && !items.is_empty() {
                        rows.push(Row::Heading(g, Some(items.len())));
                        if !p.collapsed.contains(&g) {
                            rows.extend(items.into_iter().map(Row::Item));
                        }
                    }
                };
                group(
                    Group::Albums,
                    r.albums
                        .iter()
                        .take(RESULT_LIMIT)
                        .map(|a| Item {
                            cmd: Cmd::Album(a.clone()),
                            label: a.name.clone(),
                            sub: a.artist_line(),
                        })
                        .collect(),
                );
                group(
                    Group::Artists,
                    r.artists
                        .iter()
                        .take(RESULT_LIMIT)
                        .map(|a| Item {
                            cmd: Cmd::Artist(a.clone()),
                            label: a.name.clone(),
                            sub: String::new(),
                        })
                        .collect(),
                );
                group(
                    Group::Songs,
                    r.songs
                        .iter()
                        .take(RESULT_LIMIT)
                        .map(|s| Item {
                            cmd: Cmd::Song(s.clone()),
                            label: s.title.clone(),
                            sub: s.artist_line(),
                        })
                        .collect(),
                );
                group(
                    Group::Playlists,
                    r.playlists
                        .iter()
                        .take(RESULT_LIMIT)
                        .map(|x| Item {
                            cmd: Cmd::Playlist(x.clone()),
                            label: x.name.clone(),
                            sub: x.owner.clone().unwrap_or_default(),
                        })
                        .collect(),
                );
                rows.push(Row::Heading(Group::Commands, None));
                rows.push(Row::Item(Item {
                    cmd: Cmd::SearchFor,
                    label: if q.is_empty() { "Search...".into() } else { format!("Search for “{q}”") },
                    sub: String::new(),
                }));
                // Feishin's order: search, create playlist, go to page, server commands.
                if self.can_create_playlist() && matches("Create playlist", q) {
                    rows.push(item(Cmd::CreatePlaylist, "Create playlist..."));
                }
                for (cmd, label) in
                    [(Cmd::GoToPage, "Go to page..."), (Cmd::ServerCommands, "Server commands...")]
                {
                    if matches(label, q) {
                        rows.push(item(cmd, label));
                    }
                }
            }
            Page::GoTo => {
                let go = |route: Route, label: &'static str| {
                    (self.supports_route(&route) && matches(label, q)).then(|| item(Cmd::Go(route), label))
                };
                let pages: Vec<Row> = [
                    go(Route::Home, "Home"),
                    go(Route::Search, "Search"),
                    go(Route::Settings, "Settings"),
                    // Pages Listen has beyond Feishin's.
                    go(Route::Favorites, "Favorites"),
                    go(Route::Recent, "Recently played"),
                    go(Route::Stats, "Statistics"),
                    go(Route::Mixes, "Mixes"),
                    go(Route::Radio, "Radio"),
                    go(Route::Downloads, "Downloads"),
                ]
                .into_iter()
                .flatten()
                .collect();
                if !pages.is_empty() {
                    rows.push(Row::Heading(Group::Pages, None));
                    rows.extend(pages);
                }
                let library: Vec<Row> = [
                    go(Route::Albums, "Albums"),
                    go(Route::Tracks, "Tracks"),
                    go(Route::AlbumArtists, "Album artists"),
                    go(Route::Artists, "Artists"),
                    go(Route::Genres, "Genres"),
                    go(Route::Folders, "Folders"),
                    go(Route::Playlists, "Playlists"),
                ]
                .into_iter()
                .flatten()
                .collect();
                if !library.is_empty() {
                    rows.push(Row::Heading(Group::Library, None));
                    rows.extend(library);
                }
            }
            Page::Servers => {
                let servers: Vec<Row> = self
                    .config
                    .servers
                    .iter()
                    .filter(|s| matches(&s.name, q))
                    .map(|s| item(Cmd::Server(s.id.clone()), &format!("{}...", s.name)))
                    .collect();
                if !servers.is_empty() {
                    rows.push(Row::Heading(Group::Servers, None));
                    rows.extend(servers);
                }
                if matches("Manage servers", q) {
                    rows.push(Row::Heading(Group::Manage, None));
                    rows.push(item(Cmd::ManageServers, "Manage servers..."));
                }
            }
        }
        rows
    }

    /// The selectable items of the current page, in order.
    pub fn palette_items(&self) -> Vec<Item> {
        self.palette_rows()
            .into_iter()
            .filter_map(|r| match r {
                Row::Item(i) => Some(i),
                Row::Heading(..) => None,
            })
            .collect()
    }

    fn palette_reset_selection(&mut self) {
        self.palette.selected = 0;
    }

    /// Covers of the result items.
    fn palette_images(&mut self) -> Task<Message> {
        let r = &self.palette.results;
        let refs: Vec<ImageRef> = r
            .albums
            .iter()
            .take(RESULT_LIMIT)
            .filter_map(|a| a.image.clone())
            .chain(r.artists.iter().take(RESULT_LIMIT).filter_map(|a| a.image.clone()))
            .chain(r.songs.iter().take(RESULT_LIMIT).filter_map(|s| s.image.clone()))
            .chain(r.playlists.iter().take(RESULT_LIMIT).filter_map(|p| p.image.clone()))
            .collect();
        self.request_images(refs.iter().map(Some))
    }

    fn palette_close(&mut self) {
        self.close_modal();
        self.palette = PaletteState::default();
    }

    pub(super) fn on_palette(&mut self, msg: PaletteMsg) -> Task<Message> {
        if matches!(msg, PaletteMsg::Open) {
            return if self.palette_open() { Task::none() } else { self.open_palette() };
        }
        if !self.palette_open() {
            return Task::none();
        }
        let focus = || iced::widget::operation::focus(iced::widget::Id::new(INPUT_ID));
        match msg {
            PaletteMsg::Open => {}
            PaletteMsg::Input(text) => {
                self.palette.query = text;
                self.palette_reset_selection();
                let token = self.palette.debounce.bump();
                if self.palette.query.trim().is_empty() {
                    self.palette.generation += 1;
                    self.palette.loading = false;
                    self.palette.results = SearchResults::default();
                } else {
                    return Debounce::after(token, |t| Message::Palette(PaletteMsg::Debounced(t)));
                }
            }
            PaletteMsg::Debounced(token) => {
                let query = self.palette.query.trim().to_string();
                let Some(source) = self.source.clone().filter(|_| !query.is_empty()) else {
                    return Task::none();
                };
                if !self.palette.debounce.is_current(token) {
                    return Task::none();
                }
                self.palette.generation += 1;
                self.palette.loading = true;
                let generation = self.palette.generation;
                return Task::perform(
                    async move { source.search(&query, RESULT_LIMIT).await.map_err(err) },
                    move |r| Message::Palette(PaletteMsg::Loaded(generation, r)),
                );
            }
            PaletteMsg::Loaded(generation, result) => {
                if generation != self.palette.generation {
                    return Task::none();
                }
                self.palette.loading = false;
                match result {
                    Ok(results) => {
                        self.palette.results = results;
                        self.palette_reset_selection();
                        return self.palette_images();
                    }
                    Err(e) => tracing::warn!("palette search: {e}"),
                }
            }
            PaletteMsg::Move(dir) => {
                let n = self.palette_items().len();
                if n > 0 {
                    let s = self.palette.selected.min(n - 1);
                    self.palette.selected = if dir >= 0 { (s + 1) % n } else { (s + n - 1) % n };
                }
            }
            PaletteMsg::Activate => {
                let at = self.palette.selected;
                return self.palette_choose(at);
            }
            PaletteMsg::Choose(at) => return self.palette_choose(at),
            PaletteMsg::Back => {
                if self.palette.pages.len() > 1 {
                    self.palette.pages.pop();
                    self.palette_reset_selection();
                    return focus();
                }
                self.palette_close();
            }
            PaletteMsg::Crumb(depth) => {
                self.palette.pages.truncate(depth + 1);
                self.palette_reset_selection();
                return focus();
            }
            PaletteMsg::Clear => {
                self.palette.query.clear();
                self.palette.results = SearchResults::default();
                self.palette.generation += 1;
                self.palette.loading = false;
                self.palette_reset_selection();
                return focus();
            }
            PaletteMsg::Toggle(group) => {
                if !self.palette.collapsed.remove(&group) {
                    self.palette.collapsed.insert(group);
                }
                self.palette_reset_selection();
            }
            PaletteMsg::ViewAll(group) => {
                let tab = match group {
                    Group::Albums => list::SearchTab::Albums,
                    Group::Artists => list::SearchTab::Artists,
                    Group::Playlists => list::SearchTab::Playlists,
                    _ => list::SearchTab::Songs,
                };
                self.search.query = self.palette.query.trim().to_string();
                self.search.tab = tab;
                self.palette_close();
                return Task::batch([self.run_search(), self.on_search(SearchMsg::Tab(tab))]);
            }
        }
        Task::none()
    }

    /// Runs item `at` of the page, then closes the palette (unless it opens another page).
    fn palette_choose(&mut self, at: usize) -> Task<Message> {
        let Some(item) = self.palette_items().into_iter().nth(at) else { return Task::none() };
        let focus = iced::widget::operation::focus(iced::widget::Id::new(INPUT_ID));
        match item.cmd {
            Cmd::GoToPage => {
                self.palette.pages.push(Page::GoTo);
                self.palette_reset_selection();
                return focus;
            }
            Cmd::ServerCommands => {
                self.palette.pages.push(Page::Servers);
                self.palette_reset_selection();
                return focus;
            }
            _ => {}
        }
        let query = self.palette.query.trim().to_string();
        self.palette_close();
        match item.cmd {
            Cmd::SearchFor => {
                self.search.query = query;
                if self.search.query.is_empty() {
                    return self.navigate(Route::Search);
                }
                self.run_search()
            }
            Cmd::Go(route) => self.navigate(route),
            Cmd::Server(id) => self.update(Message::SwitchServer(id)),
            Cmd::ManageServers => {
                self.config.settings.settings_tab = listen_core::settings::SettingsTab::General;
                self.navigate(Route::Settings)
            }
            Cmd::CreatePlaylist => {
                self.on_playlist_page(super::playlists::PlaylistPageMsg::Create(Vec::new()))
            }
            Cmd::Album(a) => self.navigate(Route::Album(a.id)),
            Cmd::Artist(a) => self.navigate(Route::Artist(a.id)),
            Cmd::Playlist(p) => self.navigate(Route::Playlist(p.id)),
            Cmd::Song(s) => self.play_picked(vec![s], 0),
            Cmd::GoToPage | Cmd::ServerCommands => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Args;

    fn rig() -> Listen {
        Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() }).0
    }

    fn results() -> SearchResults {
        SearchResults {
            albums: crate::demo::albums().into_iter().take(2).collect(),
            songs: crate::demo::album_songs(0).into_iter().take(3).collect(),
            ..Default::default()
        }
    }

    fn labels(s: &Listen) -> Vec<String> {
        s.palette_items().into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn opens_with_the_commands_and_toggles() {
        let mut s = rig();
        assert!(!s.palette_open());
        let _ = s.open_palette();
        assert!(s.palette_open());
        assert_eq!(labels(&s), ["Search...", "Create playlist...", "Go to page...", "Server commands..."]);
        let _ = s.open_palette();
        assert!(!s.palette_open(), "the hotkey closes it again");
    }

    #[test]
    fn keyboard_flow_through_pages_and_back() {
        let mut s = rig();
        let _ = s.open_palette();
        // Down three times: "Server commands...", Enter pushes the page.
        for _ in 0..3 {
            let _ = s.on_palette(PaletteMsg::Move(1));
        }
        assert_eq!(s.palette.selected, 3);
        let _ = s.on_palette(PaletteMsg::Activate);
        assert_eq!(s.palette.page(), Page::Servers);
        assert_eq!(s.palette.selected, 0);
        // Esc goes one page back, the second Esc closes.
        let _ = s.on_palette(PaletteMsg::Back);
        assert_eq!(s.palette.page(), Page::Home);
        assert!(s.palette_open());
        let _ = s.on_palette(PaletteMsg::Back);
        assert!(!s.palette_open());
    }

    #[test]
    fn up_wraps_and_go_to_page_navigates() {
        let mut s = rig();
        let _ = s.open_palette();
        let _ = s.on_palette(PaletteMsg::Move(-1));
        assert_eq!(s.palette.selected, 3, "up from the first item wraps to the last");
        let _ = s.on_palette(PaletteMsg::Move(1));
        let _ = s.on_palette(PaletteMsg::Move(1));
        let _ = s.on_palette(PaletteMsg::Move(1));
        assert_eq!(s.palette.selected, 2, "Go to page...");
        let _ = s.on_palette(PaletteMsg::Activate);
        assert_eq!(s.palette.page(), Page::GoTo);
        let items = labels(&s);
        assert!(items.contains(&"Albums".to_string()) && items.contains(&"Settings".to_string()));
        // Typing filters the commands.
        let _ = s.on_palette(PaletteMsg::Input("tra".into()));
        assert_eq!(labels(&s), ["Tracks"]);
        let _ = s.on_palette(PaletteMsg::Activate);
        assert!(!s.palette_open());
        assert_eq!(s.route, Route::Tracks);
    }

    #[test]
    fn results_come_first_and_stale_ones_are_dropped() {
        let mut s = rig();
        let _ = s.open_palette();
        let _ = s.on_palette(PaletteMsg::Input("a".into()));
        let _ = s.on_palette(PaletteMsg::Input("ab".into()));
        let gen_now = s.palette.generation;
        // A late result of an older search does nothing.
        let _ = s.on_palette(PaletteMsg::Loaded(gen_now + 5, Ok(results())));
        assert!(s.palette.results.albums.is_empty());
        let _ = s.on_palette(PaletteMsg::Loaded(gen_now, Ok(results())));
        let rows = s.palette_rows();
        assert!(matches!(rows[0], Row::Heading(Group::Albums, Some(2))));
        let n = labels(&s).len();
        assert_eq!(n, 2 + 3 + 1, "2 albums, 3 songs, then the search command");
        // Down to a song and play it.
        for _ in 0..3 {
            let _ = s.on_palette(PaletteMsg::Move(1));
        }
        let _ = s.on_palette(PaletteMsg::Activate);
        assert!(!s.palette_open());
        assert!(s.queue.current().is_some());
    }

    #[test]
    fn a_folded_group_leaves_the_selection() {
        let mut s = rig();
        let _ = s.open_palette();
        let _ = s.on_palette(PaletteMsg::Input("ab".into()));
        let gen_now = s.palette.generation;
        let _ = s.on_palette(PaletteMsg::Loaded(gen_now, Ok(results())));
        let _ = s.on_palette(PaletteMsg::Toggle(Group::Albums));
        assert_eq!(labels(&s).len(), 3 + 1);
        assert!(matches!(s.palette_rows()[0], Row::Heading(Group::Albums, Some(2))), "the heading stays");
    }

    #[test]
    fn search_for_opens_the_search_page_with_the_text() {
        let mut s = rig();
        let _ = s.open_palette();
        let _ = s.on_palette(PaletteMsg::Input("hello".into()));
        // The only item before any result is "Search for ..." (first).
        assert_eq!(labels(&s)[0], "Search for “hello”");
        let _ = s.on_palette(PaletteMsg::Activate);
        assert_eq!(s.search.query, "hello");
        assert!(!s.palette_open());
    }

    #[test]
    fn create_playlist_is_a_home_command_that_opens_the_dialog() {
        let mut s = rig();
        let _ = s.open_palette();
        let at = labels(&s).iter().position(|l| l == "Create playlist...").expect("on the first page");
        // Feishin's order: search, create playlist, go to page, server commands.
        let all = labels(&s);
        assert!(all.iter().position(|l| l.starts_with("Search")).unwrap() < at);
        assert!(at < all.iter().position(|l| l == "Go to page...").unwrap());
        let _ = s.on_palette(PaletteMsg::Choose(at));
        assert!(!s.palette_open(), "the palette closes");
        assert!(s.playlist_page.form.is_some(), "the create dialog is open");
        // It is found by typing, too; a source that cannot create playlists does not offer it.
        let _ = s.open_palette();
        let _ = s.on_palette(PaletteMsg::Input("create".into()));
        assert!(labels(&s).iter().any(|l| l == "Create playlist..."));
    }

    #[test]
    fn go_to_page_has_feishins_pages_and_listens_own() {
        let mut s = rig();
        let _ = s.open_palette();
        let at = labels(&s).iter().position(|l| l == "Go to page...").unwrap();
        let _ = s.on_palette(PaletteMsg::Choose(at));
        let pages = labels(&s);
        for want in [
            "Home",
            "Search",
            "Settings",
            "Albums",
            "Tracks",
            "Album artists",
            "Genres",
            "Folders",
            "Playlists",
        ] {
            assert!(pages.iter().any(|l| l == want), "{want} missing in {pages:?}");
        }
        for own in ["Favorites", "Recently played", "Statistics", "Radio"] {
            assert!(pages.iter().any(|l| l == own), "{own}");
        }
    }

    #[test]
    fn keys_map_to_messages() {
        use iced::keyboard::{Event, Key, Location, Modifiers, key::Named};
        let press = |n: Named| Event::KeyPressed {
            key: Key::Named(n),
            modified_key: Key::Named(n),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        };
        assert!(matches!(key_message(&press(Named::ArrowDown)), Some(PaletteMsg::Move(1))));
        assert!(matches!(key_message(&press(Named::ArrowUp)), Some(PaletteMsg::Move(-1))));
        assert!(matches!(key_message(&press(Named::Escape)), Some(PaletteMsg::Back)));
        assert!(key_message(&press(Named::Tab)).is_none());
    }
}
