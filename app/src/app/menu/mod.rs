//! Right-click menus: open-menu state, messages and the actions. Drawn by `ui::context_menu`.
//!
//! * [`target`]: [`MenuTarget`], one or several items of one kind.
//! * [`entry`]: [`Entry`] (label, icon, required capabilities, action or submenu) and shared submenus.
//! * [`song`], [`album`], [`artist`], [`playlist`], [`genre`], [`folder`], [`queue`]: the entries of
//!   each kind, one `out.push(Entry::...)` each; gate with `.requires(..)` / `.when(..)`.
//! * this file: [`ContextMenu`], [`MenuMsg`], and dispatch of a [`MenuAction`] to the targets
//!   (`menu_action` per target, `apply_to_songs` once the songs are known).

pub mod actions;
pub mod album;
pub mod artist;
pub mod entry;
pub mod folder;
pub mod genre;
pub mod playlist;
pub mod queue;
pub mod song;
pub mod target;

pub use entry::{Ctx, Entry, EntryKind};
pub use target::{MenuTarget, TargetKind};

use super::list::{ListId, ListMsg};
use super::{Listen, Message, Route, err};
use iced::{Point, Task};
use listen_core::model::{Playlist, ServerId, Song, SourceKind};
use listen_core::settings::PlayBehavior;
use listen_source::ItemKind;
use rand::seq::SliceRandom;

/// The open menu.
#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub target: MenuTarget,
    /// Where it was opened (window coordinates).
    pub at: Point,
    pub page: MenuPage,
    /// The list the menu was opened from ("Clear selection" acts on it).
    pub list: Option<ListId>,
    /// The highlighted row of the main page (hover or keyboard).
    pub focus: Option<usize>,
    /// The open submenu of the main page.
    pub sub: Option<SubState>,
}

/// An open submenu: the main-page row it belongs to and its highlighted child.
#[derive(Debug, Clone, PartialEq)]
pub struct SubState {
    pub index: usize,
    /// `Some` once the keyboard (or the pointer) is on a child.
    pub focus: Option<usize>,
}

impl ContextMenu {
    pub fn new(target: MenuTarget, at: Point, list: Option<ListId>) -> Self {
        ContextMenu { target, at, page: MenuPage::Main, list, focus: None, sub: None }
    }
}

/// Main-page entries of `menu`, filtered by what the source can do.
pub fn entries(state: &Listen, menu: &ContextMenu) -> Vec<Entry> {
    let cx = Ctx::new(state, &menu.target, menu.list.as_ref());
    let mut out = Vec::new();
    match &menu.target {
        MenuTarget::Songs { .. } => song::entries(&cx, &mut out),
        MenuTarget::Albums(_) => album::entries(&cx, &mut out),
        MenuTarget::Artists(_) => artist::entries(&cx, &mut out),
        MenuTarget::Playlists(_) => playlist::entries(&cx, &mut out),
        MenuTarget::Genres(_) => genre::entries(&cx, &mut out),
        MenuTarget::Folders(_) => folder::entries(&cx, &mut out),
    }
    entry::resolve(out, cx.caps)
}

#[derive(Debug, Clone, Default)]
pub enum MenuPage {
    #[default]
    Main,
    /// "Add to playlist": the playlists (`None` while loading), a new playlist's name, the search text.
    Playlists { list: Option<Result<Vec<Playlist>, String>>, new_name: String, search: String },
    /// "Rename playlist": the name being typed.
    Rename { name: String },
    /// "Delete playlist": asks once more.
    ConfirmDelete,
}

/// A key the open menu reacts to (see [`MenuKey::from_key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    /// Opens the submenu of the highlighted row.
    Right,
    /// Closes the open submenu.
    Left,
    Enter,
}

impl MenuKey {
    pub fn from_key(key: &iced::keyboard::Key) -> Option<Self> {
        use iced::keyboard::{Key, key::Named};
        match key {
            Key::Named(Named::ArrowUp) => Some(MenuKey::Up),
            Key::Named(Named::ArrowDown) => Some(MenuKey::Down),
            Key::Named(Named::ArrowRight) => Some(MenuKey::Right),
            Key::Named(Named::ArrowLeft) => Some(MenuKey::Left),
            Key::Named(Named::Enter) => Some(MenuKey::Enter),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum MenuMsg {
    Open(MenuTarget),
    /// Open at an explicit point (a button, not the cursor).
    OpenAt(MenuTarget, iced::Point),
    /// Open for a list's selection (at the cursor, or at a point).
    OpenIn {
        target: MenuTarget,
        at: Option<iced::Point>,
        list: ListId,
    },
    Close,
    /// Escape: closes the open submenu, else the menu.
    Back,
    /// The pointer is on a main-page row (opens its submenu, closes another).
    Hover(usize),
    /// The pointer is on a child of the open submenu.
    HoverSub(usize),
    /// A click on a row that has a submenu (opens it; touch / no hover).
    ToggleSub(usize),
    Key(MenuKey),
    Do(MenuAction),
    PlaylistsLoaded(Result<Vec<Playlist>, String>),
    NewName(String),
    /// The songs of an album/playlist target, fetched for an action.
    Songs(MenuAction, Result<Vec<Song>, String>),
    /// A server-side change finished: the notice to show.
    Done(Result<String, String>),
    /// A playlist was renamed (id, new name).
    Renamed(Result<(String, String), String>),
    /// A playlist was deleted (id, name).
    Deleted(Result<(String, String), String>),
}

/// When radio songs play (the Play / Next / Last of its submenu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayHow {
    Now,
    Next,
    Last,
}

/// Where "Move items" puts queue rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMove {
    Top,
    Next,
    Bottom,
}

#[derive(Debug, Clone)]
pub enum MenuAction {
    Play,
    /// The row "Play" itself: what the Play buttons do (now, next or last).
    PlayDefault,
    Shuffle,
    PlayNext,
    AddToQueue,
    StartRadio,
    ChoosePlaylist,
    /// (playlist id, name)
    AddToPlaylist(String, String),
    CreatePlaylist,
    ChooseRename,
    /// Opens the edit dialog of the (single) playlist.
    EditPlaylist,
    /// Edit the rules of a smart playlist.
    EditSmart(String),
    /// Remove the entries at these positions of playlist `id` (asks first).
    RemoveFromPlaylist(String, Vec<usize>),
    /// Rename the playlist to the typed name.
    Rename,
    ChooseDelete,
    Delete,
    /// Play the shuffled songs next / last (Feishin: "Next (shuffled)", "Last (shuffled)").
    PlayNextShuffled,
    AddToQueueShuffled,
    /// Favorite / unfavorite every target (YouTube Music: like, save to library, subscribe).
    Favorite(bool),
    /// Album radio / Artist radio of the (single) target.
    Radio(PlayHow),
    /// Move queue rows (uids) within the queue.
    MoveQueue(QueueMove, Vec<u64>),
    /// Shuffle the given queue rows among themselves (`None`: the whole queue).
    ShuffleQueue(Option<Vec<u64>>),
    /// Rate every targeted item 1..=5 stars (0 clears).
    Rate(u8),
    GoToAlbum(String),
    GoToArtist(String),
    /// The queue rows (uids) to remove.
    RemoveFromQueue(Vec<u64>),
    /// Insert the songs before the queue row with this uid (`u64::MAX`: at the end).
    InsertBefore(u64),
    CopyLink(String),
    RemoveFromHistory,
    /// Open the page of the (single) target.
    Open,
    /// Download for offline play.
    Download,
    RemoveDownload,
    /// Clear the selection of the list the menu was opened from.
    ClearSelection,
    // Details, tag editor, share links, file manager, saving files (app/item).
    /// "Get info": the details modal.
    GetInfo,
    /// "Edit metadata": the tag editor (local folder songs).
    EditTags,
    /// "Share": the share modal (Navidrome) or a copied link.
    ShareItem,
    /// "Show in file manager" (local songs).
    ShowInFolder,
    /// "Save file": the original file(s) into the Downloads folder.
    SaveFile,
}

impl Listen {
    /// Opens a menu on `target` (at `at`, else the last right-click point).
    pub fn open_menu(&mut self, target: MenuTarget, at: Option<Point>, list: Option<ListId>) {
        if target.is_empty() {
            return;
        }
        self.lists.menu = None;
        self.menu = Some(ContextMenu::new(target, at.unwrap_or(self.right_click_at), list));
    }

    pub(super) fn on_menu(&mut self, msg: MenuMsg) -> Task<Message> {
        match msg {
            MenuMsg::Open(target) => self.open_menu(target, None, None),
            MenuMsg::OpenAt(target, at) => self.open_menu(target, Some(at), None),
            MenuMsg::OpenIn { target, at, list } => self.open_menu(target, at, Some(list)),
            MenuMsg::Close => self.menu = None,
            MenuMsg::Back => {
                match &mut self.menu {
                    Some(m) if m.sub.is_some() => m.sub = None,
                    _ => self.menu = None,
                };
            }
            MenuMsg::Hover(i) => self.menu_hover(i),
            MenuMsg::HoverSub(i) => {
                if let Some(ContextMenu { sub: Some(sub), .. }) = &mut self.menu {
                    sub.focus = Some(i);
                }
            }
            MenuMsg::ToggleSub(i) => {
                if let Some(m) = &mut self.menu {
                    m.focus = Some(i);
                    m.sub = if m.sub.as_ref().is_some_and(|s| s.index == i) {
                        None
                    } else {
                        Some(SubState { index: i, focus: None })
                    };
                }
            }
            MenuMsg::Key(key) => return self.menu_key(key),
            MenuMsg::NewName(name) => match &mut self.menu {
                Some(ContextMenu { page: MenuPage::Playlists { new_name, .. }, .. }) => *new_name = name,
                Some(ContextMenu { page: MenuPage::Rename { name: current }, .. }) => *current = name,
                _ => {}
            },
            MenuMsg::PlaylistsLoaded(result) => {
                if let Some(ContextMenu { page: MenuPage::Playlists { list, .. }, .. }) = &mut self.menu {
                    *list = Some(result);
                }
            }
            MenuMsg::Do(action) => return self.menu_action(action),
            MenuMsg::Songs(action, Ok(songs)) => return self.apply_to_songs(action, songs),
            MenuMsg::Songs(_, Err(e)) => self.toast_error(format!("Could not load the songs: {e}")),
            MenuMsg::Done(Ok(text)) => self.toast_success(text),
            MenuMsg::Done(Err(e)) => self.toast_error(e),
            MenuMsg::Renamed(Err(e)) | MenuMsg::Deleted(Err(e)) => self.toast_error(e),
            MenuMsg::Renamed(Ok((id, name))) => {
                if let Some(p) = self.library.playlists.iter_mut().find(|p| p.id == id) {
                    p.name = name.clone();
                }
                if let Some(d) = self.library.playlist.as_mut().filter(|d| d.playlist.id == id) {
                    d.playlist.name = name.clone();
                }
                self.toast_success(format!("Renamed to “{name}”."));
                return self.load_playlists();
            }
            MenuMsg::Deleted(Ok((id, name))) => {
                self.library.playlists.retain(|p| p.id != id);
                self.toast_success(format!("Deleted “{name}”."));
                let open = self.route == Route::Playlist(id.clone());
                self.library.playlist = self.library.playlist.take().filter(|d| d.playlist.id != id);
                let reload = self.load_playlists();
                return if open { Task::batch([self.navigate(Route::Home), reload]) } else { reload };
            }
        }
        Task::none()
    }

    /// Highlights main-page row `i`; opens its submenu, or closes an open one.
    fn menu_hover(&mut self, i: usize) {
        let Some(menu) = self.menu.clone() else { return };
        if !matches!(menu.page, MenuPage::Main) {
            return;
        }
        let entries = entries(self, &menu);
        let Some(entry) = entries.get(i).filter(|e| e.focusable()) else { return };
        let Some(m) = &mut self.menu else { return };
        m.focus = Some(i);
        if entry.is_submenu() {
            if m.sub.as_ref().map(|s| s.index) != Some(i) {
                m.sub = Some(SubState { index: i, focus: None });
            }
        } else {
            m.sub = None;
        }
    }

    /// Arrow keys and Enter, as in Feishin: Up / Down move, Right / Left enter / leave a submenu,
    /// Enter runs the highlighted row.
    fn menu_key(&mut self, key: MenuKey) -> Task<Message> {
        let Some(menu) = self.menu.clone() else { return Task::none() };
        if !matches!(menu.page, MenuPage::Main) {
            return Task::none();
        }
        let entries = entries(self, &menu);
        let in_sub = menu.sub.as_ref().is_some_and(|s| s.focus.is_some());
        let children =
            menu.sub.as_ref().and_then(|s| entries.get(s.index)).map(|e| e.children()).unwrap_or(&[]);
        match key {
            MenuKey::Up | MenuKey::Down => {
                let step = if key == MenuKey::Down { 1 } else { -1 };
                if let (true, Some(sub)) = (in_sub, &menu.sub) {
                    let next = entry::step_focus(children, sub.focus, step);
                    if let Some(ContextMenu { sub: Some(s), .. }) = &mut self.menu {
                        s.focus = next;
                    }
                } else if let Some(m) = &mut self.menu {
                    m.focus = entry::step_focus(&entries, menu.focus, step);
                    m.sub = None;
                }
            }
            MenuKey::Right => {
                if let Some(i) = menu.focus.filter(|i| entries.get(*i).is_some_and(|e| e.is_submenu())) {
                    let first = entry::step_focus(entries[i].children(), None, 1);
                    if let Some(m) = &mut self.menu {
                        m.sub = Some(SubState { index: i, focus: first });
                    }
                }
            }
            MenuKey::Left => {
                if let Some(m) = &mut self.menu {
                    m.sub = None;
                }
            }
            MenuKey::Enter => {
                let chosen = match (&menu.sub, in_sub) {
                    (Some(sub), true) => sub.focus.and_then(|c| children.get(c)),
                    _ => menu.focus.and_then(|i| entries.get(i)),
                };
                match chosen.map(|e| (e.enabled, &e.kind)) {
                    Some((true, EntryKind::Action(action))) => return self.menu_action(action.clone()),
                    Some((true, EntryKind::Submenu(sub_children))) => {
                        let first = entry::step_focus(sub_children, None, 1);
                        if let (Some(m), Some(i)) = (&mut self.menu, menu.focus) {
                            m.sub = Some(SubState { index: i, focus: first });
                        }
                    }
                    _ => {}
                }
            }
        }
        Task::none()
    }

    /// Ids of the target's first song, to ask which playlists already hold it (YouTube Music).
    async fn first_song_ids(source: &dyn listen_source::MusicSource, target: MenuTarget) -> Vec<String> {
        let ytm = source.kind() == SourceKind::YouTubeMusic;
        let songs = match &target {
            MenuTarget::Songs { songs, .. } => songs.clone(),
            MenuTarget::Albums(a) if ytm => match a.first() {
                Some(a) => source.album(&a.id).await.map(|a| a.songs).unwrap_or_default(),
                None => vec![],
            },
            MenuTarget::Playlists(p) if ytm => match p.first() {
                Some(p) => source.playlist_songs(&p.id).await.unwrap_or_default(),
                None => vec![],
            },
            _ => vec![],
        };
        songs.into_iter().take(1).map(|s| s.id).collect()
    }

    fn menu_action(&mut self, action: MenuAction) -> Task<Message> {
        let Some(menu) = self.menu.clone() else { return Task::none() };
        let action = match action {
            MenuAction::PlayDefault => match self.play_behavior() {
                PlayBehavior::Now => MenuAction::Play,
                PlayBehavior::Next => MenuAction::PlayNext,
                PlayBehavior::Last => MenuAction::AddToQueue,
            },
            other => other,
        };
        match &action {
            // Actions that keep the menu open.
            MenuAction::ChoosePlaylist => {
                let source = Ctx::new(self, &menu.target, None).source;
                let target = menu.target.clone();
                if let Some(m) = &mut self.menu {
                    let new_name = new_name(&m.page);
                    m.page = MenuPage::Playlists { list: None, new_name, search: String::new() };
                }
                let Some(source) = source else {
                    return self.on_menu(MenuMsg::PlaylistsLoaded(Err("server not connected".into())));
                };
                let load = Task::perform(
                    async move {
                        let ids = Self::first_song_ids(&*source, target).await;
                        source.playlists_for_adding(&ids).await.map_err(err)
                    },
                    |r| Message::Menu(MenuMsg::PlaylistsLoaded(r)),
                );
                let focus = iced::widget::operation::focus(iced::widget::Id::new(
                    crate::ui::context_menu::PICKER_SEARCH_ID,
                ));
                return Task::batch([load, focus]);
            }
            MenuAction::ChooseRename => {
                if let (Some(m), MenuTarget::Playlists(p)) = (&mut self.menu, &menu.target) {
                    m.page = MenuPage::Rename { name: p.first().map(|p| p.name.clone()).unwrap_or_default() };
                }
                return Task::none();
            }
            MenuAction::ChooseDelete => {
                if let Some(m) = &mut self.menu {
                    m.page = MenuPage::ConfirmDelete;
                }
                return Task::none();
            }
            MenuAction::Rename if matches!(&menu.page, MenuPage::Rename { name } if name.trim().is_empty()) =>
            {
                return Task::none();
            }
            MenuAction::CreatePlaylist if matches!(&menu.page, MenuPage::Playlists { new_name, .. } if new_name.trim().is_empty()) =>
            {
                return Task::none();
            }
            _ => {}
        }
        self.menu = None;
        let single_playlist = match &menu.target {
            MenuTarget::Playlists(p) if p.len() == 1 => Some(p[0].clone()),
            _ => None,
        };
        match (action, menu.target) {
            (MenuAction::EditPlaylist, _) => match single_playlist {
                Some(p) => self.on_playlist_page(super::playlists::PlaylistPageMsg::EditPlaylist(p)),
                None => Task::none(),
            },
            (MenuAction::EditSmart(id), _) => {
                self.update(Message::Smart(super::smart_playlist::SmartMsg::Edit(id)))
            }
            (MenuAction::RemoveFromPlaylist(id, positions), _) => {
                self.on_playlist_page(super::playlists::PlaylistPageMsg::Remove(id, positions))
            }
            (MenuAction::GoToAlbum(id), _) => self.navigate(Route::Album(id)),
            (MenuAction::GoToArtist(id), _) => self.navigate(Route::Artist(id)),
            (MenuAction::Open, MenuTarget::Albums(a)) => match a.first() {
                Some(a) => self.navigate(Route::Album(a.id.clone())),
                None => Task::none(),
            },
            (MenuAction::Open, MenuTarget::Artists(a)) => match a.first() {
                Some(a) => self.navigate(Route::Artist(a.id.clone())),
                None => Task::none(),
            },
            (MenuAction::Open, MenuTarget::Playlists(p)) => match p.first() {
                Some(p) => self.navigate(Route::Playlist(p.id.clone())),
                None => Task::none(),
            },
            (MenuAction::Open, MenuTarget::Genres(g)) => match g.first() {
                Some(g) => self.navigate(Route::Genre(g.name.clone())),
                None => Task::none(),
            },
            (MenuAction::Open, _) => Task::none(),
            (MenuAction::ClearSelection, _) => match menu.list {
                Some(list) => self.on_list(ListMsg::Clear(list)),
                None => Task::none(),
            },
            (MenuAction::Rename, _) => {
                let (Some(p), MenuPage::Rename { name }) = (single_playlist, menu.page) else {
                    return Task::none();
                };
                let name = name.trim().to_string();
                let Some(source) = self.sources.get(&p.server_id).cloned() else { return Task::none() };
                let id = p.id.clone();
                Task::perform(
                    async move {
                        match source.rename_playlist(&id, &name).await {
                            Ok(()) => Ok((id, name)),
                            Err(e) => Err(format!("Could not rename the playlist: {e}")),
                        }
                    },
                    |r| Message::Menu(MenuMsg::Renamed(r)),
                )
            }
            (MenuAction::Delete, _) => {
                let Some(p) = single_playlist else { return Task::none() };
                let Some(source) = self.sources.get(&p.server_id).cloned() else { return Task::none() };
                let (id, name) = (p.id.clone(), p.name.clone());
                Task::perform(
                    async move {
                        match source.delete_playlist(&id).await {
                            Ok(()) => Ok((id, name)),
                            Err(e) => Err(format!("Could not delete the playlist: {e}")),
                        }
                    },
                    |r| Message::Menu(MenuMsg::Deleted(r)),
                )
            }
            (
                a @ (MenuAction::GetInfo
                | MenuAction::EditTags
                | MenuAction::ShareItem
                | MenuAction::ShowInFolder),
                target,
            ) => self.item_action(a, target),
            (MenuAction::CopyLink(url), _) => {
                self.toast_success("Link copied.".into());
                iced::clipboard::write(url)
            }
            (MenuAction::RemoveFromQueue(uids), _) => {
                let playing = self.queue.current().map(|i| i.uid);
                let mut removed = false;
                for uid in uids.into_iter().filter(|u| Some(*u) != playing) {
                    self.queue.remove(uid);
                    removed = true;
                }
                if removed { self.refresh_next() } else { Task::none() }
            }
            (MenuAction::RemoveFromHistory, MenuTarget::Songs { mut history_entries, .. }) => {
                // Highest index first: each removal shifts the ones after it.
                history_entries.sort_unstable_by(|a, b| b.cmp(a));
                for i in history_entries {
                    self.history_remove_entry(i);
                }
                Task::none()
            }
            (
                a @ (MenuAction::Favorite(_)
                | MenuAction::Radio(_)
                | MenuAction::MoveQueue(..)
                | MenuAction::ShuffleQueue(_)),
                target,
            ) => self.menu_extra(a, target),
            (MenuAction::Rate(stars), target) => self.rate_target(target, stars),
            (action, target) => {
                let action = match action {
                    MenuAction::CreatePlaylist => {
                        MenuAction::AddToPlaylist(String::new(), new_name(&menu.page))
                    }
                    a => a,
                };
                self.apply_to_target(action, target)
            }
        }
    }

    /// Rates every targeted item (0 clears); sources without ratings ignore it.
    fn rate_target(&mut self, target: MenuTarget, stars: u8) -> Task<Message> {
        let tasks: Vec<_> = match target {
            MenuTarget::Songs { songs, .. } => songs.iter().map(|s| self.rate_song(s, stars)).collect(),
            MenuTarget::Albums(albums) => albums
                .iter()
                .map(|a| self.set_item_rating(ItemKind::Album, &a.server_id, &a.id, stars))
                .collect(),
            MenuTarget::Artists(artists) => artists
                .iter()
                .map(|a| self.set_item_rating(ItemKind::Artist, &a.server_id, &a.id, stars))
                .collect(),
            _ => Vec::new(),
        };
        Task::batch(tasks)
    }

    /// Runs `action` on every song of the target, fetching the songs of albums etc. first.
    pub(super) fn apply_to_target(&mut self, action: MenuAction, target: MenuTarget) -> Task<Message> {
        if let MenuTarget::Songs { songs, .. } = target {
            return self.apply_to_songs(action, songs);
        }
        if let MenuTarget::Albums(albums) = &target
            && (self.demo || albums.iter().all(|a| !a.songs.is_empty()))
        {
            let songs = albums.iter().flat_map(|a| a.songs.clone()).collect();
            return self.apply_to_songs(action, songs);
        }
        let browsed = self.config.active_server.as_ref().and_then(|s| self.sources.get(s)).cloned();
        let browsed = self.source.clone().or(browsed);
        let source_of = |server: &ServerId| self.sources.get(server).cloned();
        // (source, what to fetch from it)
        enum Fetch {
            Album(String),
            Playlist(String),
            Artist(Box<listen_core::model::Artist>),
            Genre(String),
            Folder(String),
        }
        let mut work: Vec<(std::sync::Arc<dyn listen_source::MusicSource>, Fetch)> = Vec::new();
        match target {
            MenuTarget::Songs { .. } => {}
            MenuTarget::Albums(v) => work.extend(
                v.iter().filter_map(|a| Some((source_of(&a.server_id)?, Fetch::Album(a.id.clone())))),
            ),
            MenuTarget::Playlists(v) => work.extend(
                v.iter().filter_map(|p| Some((source_of(&p.server_id)?, Fetch::Playlist(p.id.clone())))),
            ),
            MenuTarget::Artists(v) => work.extend(
                v.iter().filter_map(|a| Some((source_of(&a.server_id)?, Fetch::Artist(Box::new(a.clone()))))),
            ),
            MenuTarget::Genres(v) => {
                work.extend(v.iter().filter_map(|g| Some((browsed.clone()?, Fetch::Genre(g.name.clone())))))
            }
            MenuTarget::Folders(v) => {
                work.extend(v.iter().filter_map(|f| Some((browsed.clone()?, Fetch::Folder(f.id.clone())))))
            }
        }
        if work.is_empty() {
            return Task::none();
        }
        Task::perform(
            async move {
                let mut songs = Vec::new();
                for (source, fetch) in work {
                    match fetch {
                        Fetch::Album(id) => songs.extend(source.album(&id).await.map_err(err)?.songs),
                        Fetch::Playlist(id) => songs.extend(source.playlist_songs(&id).await.map_err(err)?),
                        Fetch::Artist(a) => songs.extend(source.artist_songs(&a).await.map_err(err)?),
                        Fetch::Genre(g) => {
                            songs.extend(super::library_extra::genre_songs(&*source, &g).await?)
                        }
                        Fetch::Folder(id) => {
                            songs.extend(super::library_extra::folder_songs_recursive(&*source, &id).await?)
                        }
                    }
                }
                Ok(songs)
            },
            move |r| Message::Menu(MenuMsg::Songs(action.clone(), r)),
        )
    }

    pub(super) fn apply_to_songs(&mut self, action: MenuAction, mut songs: Vec<Song>) -> Task<Message> {
        if songs.is_empty() {
            return Task::none();
        }
        // Songs the user puts into the queue pass the player filters (the DJs' do not).
        if matches!(
            action,
            MenuAction::Play
                | MenuAction::Shuffle
                | MenuAction::PlayNext
                | MenuAction::AddToQueue
                | MenuAction::PlayNextShuffled
                | MenuAction::AddToQueueShuffled
                | MenuAction::InsertBefore(_)
        ) {
            songs = self.queue_filter(songs);
            if songs.is_empty() {
                return Task::none();
            }
            if matches!(action, MenuAction::Play | MenuAction::Shuffle) && self.queue_confirm_needed() {
                return self.queue_confirm_apply(action, songs);
            }
        }
        let images: Vec<_> =
            songs.iter().filter_map(|s| s.image.clone().map(|i| (s.server_id.clone(), i))).collect();
        let images = Task::batch(
            images.iter().map(|(server, i)| self.request_image_on(server, Some(i))).collect::<Vec<_>>(),
        );
        // The shuffled queue variants shuffle once, then queue like Next / Last.
        let action = match action {
            MenuAction::PlayNextShuffled => {
                songs.shuffle(&mut rand::rng());
                MenuAction::PlayNext
            }
            MenuAction::AddToQueueShuffled => {
                songs.shuffle(&mut rand::rng());
                MenuAction::AddToQueue
            }
            a => a,
        };
        let task = match action {
            MenuAction::Play | MenuAction::Shuffle => {
                if matches!(action, MenuAction::Shuffle) {
                    songs.shuffle(&mut rand::rng());
                }
                let task = self.play_songs(songs, 0);
                if self.ai_dj_on() {
                    self.dj_reseed();
                }
                task
            }
            MenuAction::PlayNext | MenuAction::AddToQueue => {
                let was_idle = self.queue.current().is_none();
                self.note_listener_picks(&songs);
                let n = songs.len();
                if matches!(action, MenuAction::PlayNext) {
                    self.queue.enqueue_next(songs);
                } else {
                    self.queue.enqueue(songs);
                }
                self.toast_info(match (n, matches!(action, MenuAction::PlayNext)) {
                    (1, true) => "Plays next.".into(),
                    (1, false) => "Added to the queue.".into(),
                    (n, true) => format!("{n} songs play next."),
                    (n, false) => format!("{n} songs added to the queue."),
                });
                if was_idle { self.start_current() } else { self.refresh_next() }
            }
            MenuAction::InsertBefore(uid) => self.queue_insert_before(uid, songs),
            MenuAction::StartRadio => {
                songs.truncate(1);
                if self.ai_dj_on() {
                    self.play_picked(songs, 0)
                } else {
                    // The song, then similar ones right away (whatever the Auto-DJ setting).
                    let start = self.play_songs(songs, 0);
                    Task::batch([start, self.fetch_autodj()])
                }
            }
            MenuAction::RemoveFromHistory => Task::none(),
            MenuAction::SaveFile => self.save_files(songs),
            MenuAction::Download => self.on_offline(super::offline::OfflineMsg::Download(songs)),
            MenuAction::RemoveDownload => {
                let tasks: Vec<_> = songs
                    .into_iter()
                    .map(|s| self.on_offline(super::offline::OfflineMsg::Remove(s.server_id, s.id)))
                    .collect();
                Task::batch(tasks)
            }
            MenuAction::AddToPlaylist(id, name) => self.add_songs_to_playlist(id, name, songs),
            _ => Task::none(),
        };
        Task::batch([images, task])
    }

    /// Songs the listener queued steer the AI DJ (the latest ten).
    pub(in crate::app) fn note_listener_picks(&mut self, songs: &[Song]) {
        let picks = &mut self.dj.listener_picks;
        picks.extend(songs.iter().take(5).cloned());
        let excess = picks.len().saturating_sub(10);
        picks.drain(..excess);
    }

    /// Like / unlike a song on its server, and everywhere it is shown.
    pub(super) fn set_song_favorite(&mut self, song: &Song, favorite: bool) -> Task<Message> {
        let Some(source) = self.sources.get(&song.server_id).cloned() else { return Task::none() };
        self.mark_favorite(&song.server_id, &song.id, favorite);
        // Update the Favorites page in place; an unloaded one is fetched on the next visit.
        let loaded = self.library.favorites_loaded;
        let fav = &mut self.library.favorites;
        let same = |s: &Song| s.server_id == song.server_id && s.id == song.id;
        if favorite {
            if loaded && !fav.iter().any(same) {
                fav.insert(0, Song { favorite: true, ..song.clone() });
            }
        } else if let Some(i) = fav.iter().position(same) {
            fav.remove(i);
            self.lists.reset(&ListId::Favorites(super::list::FavoritesTab::Songs));
        }
        let id = song.id.clone();
        let liked = if song.source == listen_core::model::SourceKind::YouTubeMusic {
            if favorite { "Added to Liked Music." } else { "Removed from Liked Music." }
        } else if favorite {
            "Added to favorites."
        } else {
            "Removed from favorites."
        };
        Task::perform(
            async move {
                source
                    .set_favorite(&[id], favorite)
                    .await
                    .map(|_| liked.to_string())
                    .map_err(|e| format!("Could not change the like: {e}"))
            },
            |r| Message::Menu(MenuMsg::Done(r)),
        )
    }

    /// Updates the heart of a song in the queue and the loaded lists.
    pub(super) fn mark_favorite(&mut self, server: &ServerId, id: &str, favorite: bool) {
        let same = |s: &Song| &s.server_id == server && s.id == id;
        self.queue.for_each_song_mut(|s| {
            if same(s) {
                s.favorite = favorite;
            }
        });
        let lib = &mut self.library;
        let lists = [
            Some(&mut lib.songs),
            Some(&mut lib.favorites),
            lib.album.as_mut().map(|a| &mut a.songs),
            lib.playlist.as_mut().map(|p| &mut p.songs),
            lib.artist.as_mut().map(|a| &mut a.top_songs),
        ];
        for list in lists.into_iter().flatten() {
            for s in list.iter_mut().filter(|s| same(s)) {
                s.favorite = favorite;
            }
        }
        self.album_rows_sync();
        self.playlist_rows_sync();
    }
}

fn new_name(page: &MenuPage) -> String {
    match page {
        MenuPage::Playlists { new_name, .. } => new_name.trim().to_string(),
        MenuPage::Main | MenuPage::Rename { .. } | MenuPage::ConfirmDelete => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map() {
        use iced::keyboard::{Key, key::Named};
        assert_eq!(MenuKey::from_key(&Key::Named(Named::ArrowDown)), Some(MenuKey::Down));
        assert_eq!(MenuKey::from_key(&Key::Named(Named::Enter)), Some(MenuKey::Enter));
        assert_eq!(MenuKey::from_key(&Key::Named(Named::Escape)), None);
    }
}
