//! The left sidebar's state and playlist list logic (owned / shared, regex filter, hand-made order,
//! folders from names). Rendering is in `ui::sidebar`. Feishin: `features/sidebar/components/*.tsx`.

use super::{Listen, Message, Route};
use iced::{Point, Task};
use listen_core::model::Playlist;
use listen_core::settings::{FolderView, PlaylistMode, SideQueueLayout};
use regex::{Regex, RegexBuilder};
use std::collections::BTreeSet;

/// Where the release notes are.
pub const RELEASES_URL: &str = "https://github.com/leks2205/Listen/releases";

/// The collapsible parts of the expanded sidebar (Feishin's accordion items).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Library,
    Collections,
    Playlists,
    Shared,
}

/// Whose playlists a list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Owned,
    Shared,
}

impl Scope {
    pub fn key(self) -> &'static str {
        match self {
            Scope::Owned => "owned",
            Scope::Shared => "shared",
        }
    }
}

/// What is being dragged to a new place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragKind {
    /// A row of the settings list of sidebar items (the key is the item id).
    Item,
    /// A playlist of the sidebar (the key is the playlist id).
    Playlist(Scope),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbDrag {
    pub kind: DragKind,
    pub from: String,
    /// The row under the pointer.
    pub over: String,
}

/// A floating panel of the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub enum Popup {
    AppMenu(Point),
    /// The collapsed rail's Collections button.
    RailCollections(Point),
    /// The three dots of a collection.
    Collection {
        index: usize,
        at: Point,
    },
    Rename {
        index: usize,
        at: Point,
        name: String,
    },
}

#[derive(Debug, Default)]
pub struct SidebarState {
    /// Sections the user folded (all open by default, like Feishin).
    pub folded: BTreeSet<Section>,
    /// Open folders as `<scope>:<path>`.
    pub open_folders: BTreeSet<String>,
    /// Navigation view: the folder path stack of the owned / shared list.
    pub nav_owned: Vec<String>,
    pub nav_shared: Vec<String>,
    /// The playlists in the collapsed rail are folded away.
    pub rail_playlists_folded: bool,
    pub drag: Option<SbDrag>,
    pub popup: Option<Popup>,
    /// The separator box while it is empty (an empty separator is not applied).
    pub separator_input: Option<String>,
}

impl SidebarState {
    pub fn nav(&self, scope: Scope) -> &Vec<String> {
        match scope {
            Scope::Owned => &self.nav_owned,
            Scope::Shared => &self.nav_shared,
        }
    }

    fn nav_mut(&mut self, scope: Scope) -> &mut Vec<String> {
        match scope {
            Scope::Owned => &mut self.nav_owned,
            Scope::Shared => &mut self.nav_shared,
        }
    }

    pub fn folder_key(scope: Scope, path: &str) -> String {
        format!("{}:{path}", scope.key())
    }

    pub fn folder_open(&self, scope: Scope, path: &str) -> bool {
        self.open_folders.contains(&Self::folder_key(scope, path))
    }

    pub fn section_open(&self, section: Section) -> bool {
        !self.folded.contains(&section)
    }
}

/// One change of the sidebar settings.
#[derive(Debug, Clone)]
pub enum SidebarSetting {
    PlaylistList(bool),
    FilterRegex(String),
    Sorting(bool),
    Mode(PlaylistMode),
    Folders(bool),
    Separator(String),
    FolderView(FolderView),
    TreeIndent(u16),
    LineColor(String),
    CollapsedNavigation(bool),
}

#[derive(Debug, Clone)]
pub enum SidebarMsg {
    ToggleSection(Section),
    ToggleFolder(Scope, String),
    /// Open or fold every folder of a list.
    SetAllFolders(Scope, bool),
    NavEnter(Scope, String),
    NavUp(Scope),
    ToggleRailPlaylists,
    DragStart(DragKind, String),
    DragOver(String),
    DragEnd,
    OpenAppMenu(Point),
    OpenRailCollections(Point),
    OpenCollection(usize, Point),
    RenameCollection(usize),
    RenameInput(String),
    RenameSave,
    DeleteCollection(usize),
    ClosePopup,
    SideQueueLayout(SideQueueLayout),
    Quit,
    OpenReleases,
    Setting(SidebarSetting),
    /// Close the popup, then do this (a menu entry).
    Then(Box<Message>),
}

// ---- pure logic -----------------------------------------------------------------

/// Hides playlists whose name matches `src` (case-insensitive). An empty or
/// invalid expression filters nothing, like Feishin.
pub fn compile_filter(src: &str) -> Option<Regex> {
    if src.is_empty() {
        return None;
    }
    RegexBuilder::new(src).case_insensitive(true).build().ok()
}

/// Mine or shared. No owner means mine; with an unknown user name, editability decides.
pub fn scope_of(p: &Playlist, username: &str) -> Scope {
    match p.owner.as_deref() {
        None | Some("") => Scope::Owned,
        Some(owner) if owner == username => Scope::Owned,
        Some(_) if username.is_empty() && p.editable => Scope::Owned,
        Some(_) => Scope::Shared,
    }
}

/// The playlists of one scope minus filtered ones, in the hand-made order when on (others follow).
pub fn visible<'a>(
    all: &'a [Playlist],
    scope: Scope,
    username: &str,
    filter: Option<&Regex>,
    order: Option<&[String]>,
) -> Vec<&'a Playlist> {
    let scoped: Vec<&Playlist> = all
        .iter()
        .filter(|p| scope_of(p, username) == scope)
        .filter(|p| !filter.is_some_and(|r| r.is_match(&p.name)))
        .collect();
    let Some(order) = order else { return scoped };
    let mut out: Vec<&Playlist> =
        order.iter().filter_map(|id| scoped.iter().find(|p| &p.id == id).copied()).collect();
    out.extend(scoped.iter().filter(|p| !order.contains(&p.id)));
    out
}

/// `ids` with `source` moved to `target`'s place (after it when coming from above, else before).
pub fn move_id(ids: &[String], source: &str, target: &str) -> Vec<String> {
    let (Some(from), Some(to)) = (ids.iter().position(|i| i == source), ids.iter().position(|i| i == target))
    else {
        return ids.to_vec();
    };
    let mut out = ids.to_vec();
    let item = out.remove(from);
    out.insert(to, item);
    out
}

/// A folder or a playlist in the folder views.
#[derive(Debug, Clone)]
pub enum Node<'a> {
    Folder { name: String, path: String, leaves: usize, children: Vec<Node<'a>> },
    Leaf { name: String, playlist: &'a Playlist },
}

/// Nested folders from names split at every `sep` ("Rock/Classic/Best"), folders first.
pub fn build_tree<'a>(items: &[&'a Playlist], sep: &str) -> Vec<Node<'a>> {
    fn insert<'a>(nodes: &mut Vec<Node<'a>>, folders: &[&str], depth: usize, sep: &str, leaf: Node<'a>) {
        let Some(name) = folders.get(depth) else {
            nodes.push(leaf);
            return;
        };
        let path = folders[..=depth].join(sep);
        let at = nodes.iter().position(|n| matches!(n, Node::Folder { path: p, .. } if *p == path));
        let at = at.unwrap_or_else(|| {
            nodes.push(Node::Folder { name: (*name).into(), path, leaves: 0, children: Vec::new() });
            nodes.len() - 1
        });
        if let Node::Folder { children, .. } = &mut nodes[at] {
            insert(children, folders, depth + 1, sep, leaf);
        }
    }
    fn count(nodes: &mut [Node<'_>]) -> usize {
        let mut total = 0;
        for n in nodes.iter_mut() {
            match n {
                Node::Leaf { .. } => total += 1,
                Node::Folder { leaves, children, .. } => {
                    *leaves = count(children);
                    total += *leaves;
                }
            }
        }
        total
    }
    fn folders_first<'a>(nodes: Vec<Node<'a>>) -> Vec<Node<'a>> {
        let (mut folders, leaves): (Vec<_>, Vec<_>) =
            nodes.into_iter().partition(|n| matches!(n, Node::Folder { .. }));
        for f in &mut folders {
            if let Node::Folder { children, .. } = f {
                *children = folders_first(std::mem::take(children));
            }
        }
        folders.extend(leaves);
        folders
    }

    let mut root: Vec<Node<'a>> = Vec::new();
    for &p in items {
        let segments: Vec<&str> = if sep.is_empty() {
            vec![p.name.as_str()]
        } else {
            p.name.split(sep).filter(|s| !s.is_empty()).collect()
        };
        if segments.len() <= 1 {
            root.push(Node::Leaf { name: p.name.clone(), playlist: p });
            continue;
        }
        let (leaf, folders) = segments.split_last().expect("two or more segments");
        insert(&mut root, folders, 0, sep, Node::Leaf { name: (*leaf).into(), playlist: p });
    }
    let mut root = folders_first(root);
    count(&mut root);
    root
}

/// Feishin's "single folder" view: only the first segment makes a folder.
pub fn group_single<'a>(items: &[&'a Playlist], sep: &str) -> Vec<Node<'a>> {
    let mut folders: Vec<Node<'a>> = Vec::new();
    let mut roots: Vec<Node<'a>> = Vec::new();
    for &p in items {
        let split = (!sep.is_empty())
            .then(|| p.name.find(sep))
            .flatten()
            .filter(|&i| i > 0 && i < p.name.len() - sep.len());
        let Some(i) = split else {
            roots.push(Node::Leaf { name: p.name.clone(), playlist: p });
            continue;
        };
        let (folder, rest) = (&p.name[..i], &p.name[i + sep.len()..]);
        let leaf = Node::Leaf { name: rest.into(), playlist: p };
        match folders.iter_mut().find(|n| matches!(n, Node::Folder { name, .. } if name == folder)) {
            Some(Node::Folder { children, leaves, .. }) => {
                children.push(leaf);
                *leaves += 1;
            }
            _ => folders.push(Node::Folder {
                name: folder.into(),
                path: folder.into(),
                leaves: 1,
                children: vec![leaf],
            }),
        }
    }
    folders.extend(roots);
    folders
}

/// The folders of the current view, for "expand all".
pub fn folder_paths(nodes: &[Node<'_>]) -> Vec<String> {
    let mut out = Vec::new();
    for n in nodes {
        if let Node::Folder { path, children, .. } = n {
            out.push(path.clone());
            out.extend(folder_paths(children));
        }
    }
    out
}

/// What the navigation view shows at `stack` (a path of folder names).
pub fn at_path<'a, 'b>(nodes: &'b [Node<'a>], stack: &[String]) -> &'b [Node<'a>] {
    let mut list = nodes;
    for name in stack {
        match list.iter().find_map(|n| match n {
            Node::Folder { name: f, children, .. } if f == name => Some(children),
            _ => None,
        }) {
            Some(children) => list = children,
            None => return &[],
        }
    }
    list
}

// ---- the app side ----------------------------------------------------------------

impl Listen {
    /// The user name of the active server (empty in demo mode).
    pub fn server_username(&self) -> &str {
        self.config
            .active_server
            .as_ref()
            .and_then(|id| self.config.servers.iter().find(|s| &s.id == id))
            .map_or("", |s| s.username.as_str())
    }

    fn order_key(&self, scope: Scope) -> String {
        let server = self.config.active_server.as_ref().map_or("", |s| s.0.as_str());
        format!("{server}:{}", scope.key())
    }

    /// The playlists of one list as the sidebar shows them.
    pub fn sidebar_playlists(&self, scope: Scope) -> Vec<&Playlist> {
        let sb = &self.config.settings.sidebar;
        let filter = compile_filter(&sb.playlist_filter_regex);
        let order = sb
            .playlist_sorting
            .then(|| sb.playlist_order.get(&self.order_key(scope)).map(Vec::as_slice).unwrap_or(&[]));
        visible(&self.library.playlists, scope, self.server_username(), filter.as_ref(), order)
    }

    /// The folder nodes of a list in the chosen view.
    pub fn sidebar_nodes(&self, scope: Scope) -> Option<Vec<Node<'_>>> {
        let sb = &self.config.settings.sidebar;
        if !sb.playlist_folders {
            return None;
        }
        let items = self.sidebar_playlists(scope);
        Some(match sb.playlist_folder_view {
            FolderView::Single => group_single(&items, &sb.playlist_folder_separator),
            _ => build_tree(&items, &sb.playlist_folder_separator),
        })
    }

    pub(super) fn on_sidebar(&mut self, msg: SidebarMsg) -> Task<Message> {
        match msg {
            SidebarMsg::ToggleSection(section) => {
                if !self.sb.folded.remove(&section) {
                    self.sb.folded.insert(section);
                }
            }
            SidebarMsg::ToggleFolder(scope, path) => {
                let key = SidebarState::folder_key(scope, &path);
                if !self.sb.open_folders.remove(&key) {
                    self.sb.open_folders.insert(key);
                }
            }
            SidebarMsg::SetAllFolders(scope, open) => {
                let paths = self.sidebar_nodes(scope).map(|n| folder_paths(&n)).unwrap_or_default();
                for path in paths {
                    let key = SidebarState::folder_key(scope, &path);
                    if open {
                        self.sb.open_folders.insert(key);
                    } else {
                        self.sb.open_folders.remove(&key);
                    }
                }
            }
            SidebarMsg::NavEnter(scope, name) => self.sb.nav_mut(scope).push(name),
            SidebarMsg::NavUp(scope) => {
                self.sb.nav_mut(scope).pop();
            }
            SidebarMsg::ToggleRailPlaylists => self.sb.rail_playlists_folded = !self.sb.rail_playlists_folded,
            SidebarMsg::DragStart(kind, key) => {
                self.sb.drag = Some(SbDrag { kind, over: key.clone(), from: key });
            }
            SidebarMsg::DragOver(key) => {
                if let Some(drag) = &mut self.sb.drag {
                    drag.over = key;
                }
            }
            SidebarMsg::DragEnd => return self.sidebar_drop(),
            SidebarMsg::OpenAppMenu(at) => self.sb.popup = Some(Popup::AppMenu(at)),
            SidebarMsg::OpenRailCollections(at) => self.sb.popup = Some(Popup::RailCollections(at)),
            SidebarMsg::OpenCollection(index, at) => self.sb.popup = Some(Popup::Collection { index, at }),
            SidebarMsg::RenameCollection(index) => {
                let at = match &self.sb.popup {
                    Some(Popup::Collection { at, .. }) => *at,
                    _ => Point::ORIGIN,
                };
                let name = self.config.settings.lists.collections.get(index).map(|c| c.name.clone());
                self.sb.popup = name.map(|name| Popup::Rename { index, at, name });
            }
            SidebarMsg::RenameInput(text) => {
                if let Some(Popup::Rename { name, .. }) = &mut self.sb.popup {
                    *name = text;
                }
            }
            SidebarMsg::RenameSave => {
                if let Some(Popup::Rename { index, name, .. }) = self.sb.popup.take() {
                    let name = name.trim().to_string();
                    if let Some(c) =
                        self.config.settings.lists.collections.get_mut(index).filter(|_| !name.is_empty())
                    {
                        c.name = name;
                        self.save_config();
                    }
                }
            }
            SidebarMsg::DeleteCollection(index) => {
                self.sb.popup = None;
                let list = &mut self.config.settings.lists.collections;
                if index < list.len() {
                    list.remove(index);
                    self.save_config();
                }
            }
            SidebarMsg::ClosePopup => self.sb.popup = None,
            SidebarMsg::SideQueueLayout(layout) => {
                self.sb.popup = None;
                self.config.settings.queue.side_layout = layout;
                self.save_config();
            }
            SidebarMsg::Quit => {
                self.save_queue_now();
                self.save_window_state_now();
                return iced::exit();
            }
            SidebarMsg::OpenReleases => {
                self.sb.popup = None;
                super::album_page::open_url(RELEASES_URL);
            }
            SidebarMsg::Setting(s) => self.change_sidebar_setting(s),
            SidebarMsg::Then(msg) => {
                self.sb.popup = None;
                return self.handle(*msg);
            }
        }
        Task::none()
    }

    fn change_sidebar_setting(&mut self, setting: SidebarSetting) {
        let sb = &mut self.config.settings.sidebar;
        match setting {
            SidebarSetting::PlaylistList(v) => sb.show_playlists = v,
            SidebarSetting::FilterRegex(v) => sb.playlist_filter_regex = v,
            SidebarSetting::Sorting(v) => sb.playlist_sorting = v,
            SidebarSetting::Mode(v) => sb.playlist_mode = v,
            SidebarSetting::Folders(v) => sb.playlist_folders = v,
            SidebarSetting::Separator(v) => {
                if v.is_empty() {
                    self.sb.separator_input = Some(v);
                    return;
                }
                self.sb.separator_input = None;
                sb.playlist_folder_separator = v;
                self.sb.open_folders.clear();
                self.sb.nav_owned.clear();
                self.sb.nav_shared.clear();
            }
            SidebarSetting::FolderView(v) => {
                sb.playlist_folder_view = v;
                self.sb.nav_owned.clear();
                self.sb.nav_shared.clear();
            }
            SidebarSetting::TreeIndent(v) => sb.folder_tree_indent = v.min(64),
            SidebarSetting::LineColor(v) => sb.folder_tree_line_color = v,
            SidebarSetting::CollapsedNavigation(v) => sb.collapsed_navigation = v,
        }
        self.save_config();
    }

    /// Pointer released: drops the dragged row, or opens the playlist if it was a click.
    fn sidebar_drop(&mut self) -> Task<Message> {
        let Some(drag) = self.sb.drag.take() else { return Task::none() };
        match drag.kind {
            DragKind::Item => {
                let items = &mut self.config.settings.sidebar.items;
                let (Some(from), Some(to)) = (
                    items.iter().position(|i| i.id == drag.from),
                    items.iter().position(|i| i.id == drag.over),
                ) else {
                    return Task::none();
                };
                if from != to {
                    let item = items.remove(from);
                    items.insert(to, item);
                    self.save_config();
                }
            }
            DragKind::Playlist(scope) => {
                if drag.from == drag.over {
                    return self.navigate(Route::Playlist(drag.from));
                }
                // The whole list's order is saved, not only the visible part.
                let ids: Vec<String> = self.sidebar_playlists(scope).iter().map(|p| p.id.clone()).collect();
                let order = move_id(&ids, &drag.from, &drag.over);
                let key = self.order_key(scope);
                self.config.settings.sidebar.playlist_order.insert(key, order);
                self.save_config();
            }
        }
        Task::none()
    }

    /// Mouse release / leaving the window ends a sidebar drag.
    pub(super) fn sidebar_subscription(&self) -> Option<iced::Subscription<Message>> {
        self.sb.drag.as_ref()?;
        Some(iced::event::listen_with(|event, _, _| match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
            | iced::Event::Mouse(iced::mouse::Event::CursorLeft) => {
                Some(Message::Sidebar(SidebarMsg::DragEnd))
            }
            _ => None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::{ServerId, SourceKind};
    use std::time::Duration;

    fn pl(id: &str, name: &str, owner: Option<&str>) -> Playlist {
        Playlist {
            id: id.into(),
            server_id: ServerId("s".into()),
            source: SourceKind::Subsonic,
            name: name.into(),
            description: None,
            owner: owner.map(Into::into),
            image: None,
            song_count: Some(1),
            duration: Duration::ZERO,
            public: false,
            editable: owner.is_none() || owner == Some("me"),
        }
    }

    fn names(nodes: &[Node<'_>]) -> Vec<String> {
        nodes
            .iter()
            .map(|n| match n {
                Node::Folder { name, .. } => format!("[{name}]"),
                Node::Leaf { name, .. } => name.clone(),
            })
            .collect()
    }

    #[test]
    fn owned_and_shared_split_by_owner() {
        let all = [pl("1", "A", None), pl("2", "B", Some("me")), pl("3", "C", Some("amy"))];
        let owned = visible(&all, Scope::Owned, "me", None, None);
        let shared = visible(&all, Scope::Shared, "me", None, None);
        assert_eq!(owned.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["1", "2"]);
        assert_eq!(shared.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["3"]);
    }

    #[test]
    fn unknown_user_name_falls_back_to_editable() {
        let all = [pl("2", "B", Some("me")), pl("3", "C", Some("amy"))];
        assert_eq!(visible(&all, Scope::Owned, "", None, None).len(), 1);
    }

    #[test]
    fn filter_hides_matches_case_insensitively_and_ignores_bad_regex() {
        let all = [pl("1", "Daily Mix 1", None), pl("2", "Gym", None)];
        let re = compile_filter("^daily mix.*");
        let shown = visible(&all, Scope::Owned, "", re.as_ref(), None);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].name, "Gym");
        assert!(compile_filter("(").is_none());
        assert!(compile_filter("").is_none());
        assert_eq!(visible(&all, Scope::Owned, "", compile_filter("(").as_ref(), None).len(), 2);
    }

    #[test]
    fn saved_order_comes_first_and_new_playlists_follow() {
        let all = [pl("a", "A", None), pl("b", "B", None), pl("c", "C", None)];
        let order = vec!["c".to_string(), "gone".to_string(), "a".to_string()];
        let ids: Vec<_> =
            visible(&all, Scope::Owned, "", None, Some(&order)).iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids, ["c", "a", "b"]);
    }

    #[test]
    fn moving_a_row_lands_on_the_target() {
        let ids: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        assert_eq!(move_id(&ids, "a", "c"), ["b", "c", "a", "d"]);
        assert_eq!(move_id(&ids, "d", "b"), ["a", "d", "b", "c"]);
        assert_eq!(move_id(&ids, "a", "zzz"), ids);
    }

    #[test]
    fn tree_nests_folders_and_puts_them_first() {
        let all = [
            pl("1", "Plain", None),
            pl("2", "Rock/Classic/Best", None),
            pl("3", "Rock/New", None),
            pl("4", "Jazz/Cool", None),
            pl("5", "/odd", None),
            pl("6", "trailing/", None),
        ];
        let items: Vec<&Playlist> = all.iter().collect();
        let tree = build_tree(&items, "/");
        assert_eq!(names(&tree), ["[Rock]", "[Jazz]", "Plain", "/odd", "trailing/"]);
        let Node::Folder { children, leaves, path, .. } = &tree[0] else { panic!() };
        assert_eq!((*leaves, path.as_str()), (2, "Rock"));
        assert_eq!(names(children), ["[Classic]", "New"]);
        let Node::Folder { path, .. } = &children[0] else { panic!() };
        assert_eq!(path, "Rock/Classic");
        assert_eq!(folder_paths(&tree), ["Rock", "Rock/Classic", "Jazz"]);
        assert_eq!(names(at_path(&tree, &["Rock".into(), "Classic".into()])), ["Best"]);
        assert!(at_path(&tree, &["Nope".into()]).is_empty());
    }

    #[test]
    fn single_view_folders_only_the_first_segment() {
        let all = [pl("1", "Rock/Classic/Best", None), pl("2", "Rock/New", None), pl("3", "Plain", None)];
        let items: Vec<&Playlist> = all.iter().collect();
        let groups = group_single(&items, "/");
        assert_eq!(names(&groups), ["[Rock]", "Plain"]);
        let Node::Folder { children, leaves, .. } = &groups[0] else { panic!() };
        assert_eq!(*leaves, 2);
        assert_eq!(names(children), ["Classic/Best", "New"]);
    }

    #[test]
    fn longer_separators_work() {
        let all = [pl("1", "Rock :: Best", None)];
        let items: Vec<&Playlist> = all.iter().collect();
        assert_eq!(names(&build_tree(&items, " :: ")), ["[Rock]"]);
        assert_eq!(names(&build_tree(&items, "")), ["Rock :: Best"]);
    }
}
