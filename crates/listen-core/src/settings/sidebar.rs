//! Sidebar entries and layout.

use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarItem {
    /// Feishin id: `Home`, `Albums`, `Tracks`, `Artists` (album artists), `Artists-all`, …
    pub id: String,
    pub label: String,
    pub visible: bool,
}

/// How the sidebar's playlist rows look (Feishin `sidebarPlaylistMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaylistMode {
    /// One line: the name only.
    Compact,
    /// Cover, name and song count.
    #[default]
    Expanded,
}

/// How playlist folders are shown (Feishin `sidebarPlaylistFolderView`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderView {
    /// Top-level folders only, each named by its whole path.
    Single,
    /// Nested, collapsible folders.
    #[default]
    Tree,
    /// One level at a time with a back button.
    Navigation,
}

impl std::fmt::Display for PlaylistMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Compact => "Compact",
            Self::Expanded => "Expanded",
        })
    }
}

impl std::fmt::Display for FolderView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Single => "Single folder",
            Self::Tree => "Tree view",
            Self::Navigation => "Navigation view",
        })
    }
}

impl PlaylistMode {
    pub const ALL: [Self; 2] = [Self::Compact, Self::Expanded];
}

impl FolderView {
    pub const ALL: [Self; 3] = [Self::Single, Self::Tree, Self::Navigation];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sidebar {
    pub items: Vec<SidebarItem>,
    pub show_playlists: bool,
    pub playlist_folders: bool,
    pub playlist_folder_separator: String,
    pub show_image: bool,
    pub show_queue: bool,
    /// Order of the right-hand panels: `queue`, `lyrics`, `visualizer`.
    pub panel_order: Vec<String>,
    /// Collapsed to a narrow icon rail (Feishin's collapsed navigation).
    pub collapsed: bool,
    /// The playing song's cover is shown large at the bottom of the sidebar
    /// (Feishin's expanded sidebar image; toggled from the player bar).
    pub cover_expanded: bool,
    /// Widths of the left sidebar and the right (queue) panel, dragged by the user.
    pub width: f32,
    pub queue_width: f32,
    /// Playlists whose name matches this regex (case-insensitive) are hidden.
    pub playlist_filter_regex: String,
    /// Playlists in the order dragged by hand (`playlist_order`) instead of the source's.
    pub playlist_sorting: bool,
    pub playlist_mode: PlaylistMode,
    pub playlist_folder_view: FolderView,
    /// Indent per level of the folder tree, in px.
    pub folder_tree_indent: u16,
    /// Colour of the tree's guide lines (`#rrggbb`); empty: the theme's border.
    pub folder_tree_line_color: String,
    /// Back / forward buttons and the page links in the collapsed rail.
    pub collapsed_navigation: bool,
    /// Hand-made playlist order: `<server id>:owned` / `<server id>:shared` → playlist ids.
    pub playlist_order: BTreeMap<String, Vec<String>>,
}

impl Sidebar {
    /// Pages added in later versions show up for configs saved before them.
    pub fn add_missing_items(&mut self) {
        for def in Self::default().items {
            if !self.items.iter().any(|i| i.id == def.id) {
                self.items.push(def);
            }
        }
    }
}

impl Default for Sidebar {
    fn default() -> Self {
        let items = [
            ("Now Playing", "now playing", false),
            ("Search", "search", false),
            ("Home", "home", true),
            ("Favorites", "favorites", true),
            ("Albums", "albums", true),
            ("Tracks", "tracks", true),
            ("Artists", "album artists", true),
            ("Artists-all", "artists", true),
            ("Genres", "genres", true),
            ("Folders", "folders", true),
            ("Playlists", "playlists", false),
            ("Collections", "collections", true),
            ("Recently Played", "recently played", true),
            ("Statistics", "statistics", true),
            ("Mixes", "mixes", true),
            ("Radio", "radio", true),
            ("Settings", "settings", false),
        ];
        Self {
            items: items
                .iter()
                .map(|(id, label, visible)| SidebarItem {
                    id: id.to_string(),
                    label: label.to_string(),
                    visible: *visible,
                })
                .collect(),
            show_playlists: true,
            playlist_folders: false,
            playlist_folder_separator: "/".into(),
            show_image: true,
            show_queue: true,
            panel_order: vec!["queue".into(), "lyrics".into(), "visualizer".into()],
            collapsed: false,
            cover_expanded: false,
            width: 400.0,
            queue_width: 600.0,
            playlist_filter_regex: String::new(),
            playlist_sorting: false,
            playlist_mode: PlaylistMode::Expanded,
            playlist_folder_view: FolderView::Tree,
            folder_tree_indent: 16,
            folder_tree_line_color: String::new(),
            collapsed_navigation: true,
            playlist_order: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_gets_the_new_fields() {
        let s: Sidebar = toml::from_str("show_playlists = false\n").unwrap();
        assert!(!s.show_playlists);
        assert_eq!(s.playlist_mode, PlaylistMode::Expanded);
        assert_eq!(s.folder_tree_indent, 16);
        assert!(s.collapsed_navigation);
    }

    #[test]
    fn new_fields_round_trip() {
        let mut s = Sidebar {
            playlist_mode: PlaylistMode::Compact,
            playlist_folder_view: FolderView::Navigation,
            ..Sidebar::default()
        };
        s.playlist_order.insert("srv:owned".into(), vec!["a".into(), "b".into()]);
        let back: Sidebar = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn missing_pages_are_added_after_the_saved_ones() {
        let mut s = Sidebar::default();
        s.items.retain(|i| i.id != "Collections");
        s.add_missing_items();
        assert_eq!(s.items.last().unwrap().id, "Collections");
    }
}
