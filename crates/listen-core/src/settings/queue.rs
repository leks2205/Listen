//! The play queue panel and page (Feishin: application / control / sidebar
//! settings of the queue, and the playback "Player filters").

use super::*;
use crate::model::Song;

/// Feishin `sideQueueType`: a panel next to the page, or a drawer that slides
/// in from the right edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideQueueStyle {
    /// The right panel (Feishin "sideQueue").
    #[default]
    Attached,
    /// The drawer (Feishin "sideDrawerQueue").
    Detached,
}

/// Feishin `sideQueueLayout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideQueueLayout {
    /// A column on the right, resizable in width.
    #[default]
    Horizontal,
    /// A panel under the page, resizable in height.
    Vertical,
}

/// The song field a player filter looks at (Feishin `PlayerFilterField`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterField {
    Name,
    AlbumArtist,
    Artist,
    Duration,
    Genre,
    Year,
    ReleaseYear,
    Note,
    Path,
    PlayCount,
    Favorite,
    Rating,
}

impl FilterField {
    pub const ALL: [FilterField; 12] = [
        FilterField::Name,
        FilterField::AlbumArtist,
        FilterField::Artist,
        FilterField::Duration,
        FilterField::Genre,
        FilterField::Year,
        FilterField::ReleaseYear,
        FilterField::Note,
        FilterField::Path,
        FilterField::PlayCount,
        FilterField::Favorite,
        FilterField::Rating,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FilterField::Name => "Title",
            FilterField::AlbumArtist => "Album artist",
            FilterField::Artist => "Artist",
            FilterField::Duration => "Duration (s)",
            FilterField::Genre => "Genre",
            FilterField::Year => "Year",
            FilterField::ReleaseYear => "Release year",
            FilterField::Note => "Note",
            FilterField::Path => "Path",
            FilterField::PlayCount => "Play count",
            FilterField::Favorite => "Favorite",
            FilterField::Rating => "Rating",
        }
    }

    pub fn kind(self) -> FieldKind {
        match self {
            FilterField::Duration
            | FilterField::Year
            | FilterField::ReleaseYear
            | FilterField::PlayCount
            | FilterField::Rating => FieldKind::Number,
            FilterField::Favorite => FieldKind::Boolean,
            _ => FieldKind::Text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Number,
    Boolean,
}

/// How the field is compared (Feishin `PlayerFilterOperator`, the ones the
/// player filter form offers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Is,
    IsNot,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Gt,
    Lt,
    /// A regular expression, case-insensitive, matched anywhere in the text.
    Regex,
}

impl FilterOp {
    pub fn label(self) -> &'static str {
        match self {
            FilterOp::Is => "is",
            FilterOp::IsNot => "is not",
            FilterOp::Contains => "contains",
            FilterOp::NotContains => "does not contain",
            FilterOp::StartsWith => "starts with",
            FilterOp::EndsWith => "ends with",
            FilterOp::Gt => "is greater than",
            FilterOp::Lt => "is less than",
            FilterOp::Regex => "matches regex",
        }
    }

    /// The operators that make sense for a field of this kind.
    pub fn for_kind(kind: FieldKind) -> &'static [FilterOp] {
        match kind {
            FieldKind::Text => &[
                FilterOp::Is,
                FilterOp::IsNot,
                FilterOp::Contains,
                FilterOp::NotContains,
                FilterOp::StartsWith,
                FilterOp::EndsWith,
                FilterOp::Regex,
            ],
            FieldKind::Number => &[FilterOp::Is, FilterOp::IsNot, FilterOp::Gt, FilterOp::Lt],
            FieldKind::Boolean => &[FilterOp::Is, FilterOp::IsNot],
        }
    }
}

/// One rule: songs that match it are kept out of the queue (Feishin
/// `PlayerFilter`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerFilter {
    pub field: FilterField,
    pub op: FilterOp,
    pub value: String,
    pub enabled: bool,
}

impl Default for PlayerFilter {
    fn default() -> Self {
        Self { field: FilterField::Name, op: FilterOp::Contains, value: String::new(), enabled: true }
    }
}

impl PlayerFilter {
    /// A rule that is switched on and has a value.
    pub fn active(&self) -> bool {
        self.enabled && !self.value.trim().is_empty()
    }

    /// The song matches the rule (so it is kept out of the queue). Text is
    /// compared case-insensitively on the first artist / genre like Feishin;
    /// numbers compare as numbers.
    pub fn matches(&self, song: &Song) -> bool {
        self.matches_with(song, self.regex().as_ref())
    }

    /// The compiled pattern of a regex rule; `None` for other operators and
    /// for an invalid pattern (which never matches, like Feishin).
    pub fn regex(&self) -> Option<regex::Regex> {
        if self.op != FilterOp::Regex {
            return None;
        }
        regex::RegexBuilder::new(self.value.trim()).case_insensitive(true).size_limit(1 << 20).build().ok()
    }

    /// Whether a regex rule's pattern does not compile (for a hint in the form).
    pub fn invalid_regex(&self) -> bool {
        self.op == FilterOp::Regex && !self.value.trim().is_empty() && self.regex().is_none()
    }

    fn matches_with(&self, song: &Song, re: Option<&regex::Regex>) -> bool {
        let first = |v: Vec<&str>| v.first().map(|s| s.to_string()).unwrap_or_default();
        let text = match self.field {
            FilterField::Name => Some(song.title.clone()),
            FilterField::AlbumArtist => {
                Some(first(song.album_artists.iter().map(|a| a.name.as_str()).collect()))
            }
            FilterField::Artist => Some(first(song.artists.iter().map(|a| a.name.as_str()).collect())),
            FilterField::Genre => Some(first(song.genres.iter().map(String::as_str).collect())),
            FilterField::Note => Some(song.comment.clone().unwrap_or_default()),
            FilterField::Path => Some(song.path.clone().unwrap_or_default()),
            FilterField::Favorite => Some(song.favorite.to_string()),
            FilterField::Duration => Some(song.duration.as_secs().to_string()),
            FilterField::Year => Some(song.year.unwrap_or(0).to_string()),
            FilterField::ReleaseYear => Some(song.year.unwrap_or(0).to_string()),
            FilterField::PlayCount => Some(song.play_count.unwrap_or(0).to_string()),
            FilterField::Rating => Some(song.rating.unwrap_or(0).to_string()),
        };
        let Some(have) = text else { return false };
        let want = self.value.trim();
        let (hl, wl) = (have.to_lowercase(), want.to_lowercase());
        match self.op {
            FilterOp::Contains => hl.contains(&wl),
            FilterOp::NotContains => !hl.contains(&wl),
            FilterOp::StartsWith => hl.starts_with(&wl),
            FilterOp::EndsWith => hl.ends_with(&wl),
            FilterOp::Is => hl == wl,
            FilterOp::IsNot => hl != wl,
            FilterOp::Regex => re.is_some_and(|r| r.is_match(&have)),
            FilterOp::Gt | FilterOp::Lt => match (have.parse::<f64>(), want.parse::<f64>()) {
                (Ok(a), Ok(b)) if self.op == FilterOp::Gt => a > b,
                (Ok(a), Ok(b)) => a < b,
                _ => false,
            },
        }
    }
}

/// `settings.queue`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueSettings {
    /// Scroll the queue to the playing song on a track change (Feishin `followCurrentSong`).
    pub follow_current_song: bool,
    /// Label the songs added with "Play next" / "Add to queue" as "Up Next"
    /// (Feishin `upNextQueue`).
    pub up_next_queue: bool,
    /// Show the play mode row and the Auto / AI tags in the sidebar queue.
    pub show_mode_tag: bool,
    /// Ask before "Play now" replaces a queue that has songs (Feishin `confirmQueueChanges`).
    pub confirm_queue_changes: bool,
    /// Keep the queue between runs (Feishin "Save play queue").
    pub save_play_queue: bool,
    /// Save the queue to the server every `autosave_every` track changes
    /// (Subsonic `savePlayQueue`).
    pub autosave_server: bool,
    pub autosave_every: u32,
    pub side_style: SideQueueStyle,
    pub side_layout: SideQueueLayout,
    /// Height of the queue panel in the vertical layout.
    pub height: f32,
    /// Lyrics panel under / instead of the queue in the right sidebar.
    pub lyrics_in_sidebar: bool,
    /// Lyrics and visualizer share one panel.
    pub combined_panel: bool,
    /// Height of the panels below the queue (lyrics, visualizer).
    pub panel_height: f32,
    /// Rules that keep songs out of the queue.
    pub filters: Vec<PlayerFilter>,
}

impl Default for QueueSettings {
    fn default() -> Self {
        Self {
            follow_current_song: true,
            up_next_queue: true,
            show_mode_tag: true,
            confirm_queue_changes: false,
            save_play_queue: true,
            autosave_server: false,
            autosave_every: 1,
            side_style: SideQueueStyle::Attached,
            side_layout: SideQueueLayout::Horizontal,
            height: 320.0,
            lyrics_in_sidebar: false,
            combined_panel: false,
            panel_height: 240.0,
            filters: Vec::new(),
        }
    }
}

impl QueueSettings {
    /// `songs` without those a switched-on player filter matches.
    pub fn filtered(&self, songs: Vec<Song>) -> Vec<Song> {
        let rules: Vec<(&PlayerFilter, Option<regex::Regex>)> =
            self.filters.iter().filter(|f| f.active()).map(|f| (f, f.regex())).collect();
        if rules.is_empty() {
            return songs;
        }
        songs.into_iter().filter(|s| !rules.iter().any(|(f, re)| f.matches_with(s, re.as_ref()))).collect()
    }
}

impl std::fmt::Display for SideQueueStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SideQueueStyle::Attached => "Attached",
            SideQueueStyle::Detached => "Detached",
        })
    }
}

impl std::fmt::Display for SideQueueLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SideQueueLayout::Horizontal => "Horizontal",
            SideQueueLayout::Vertical => "Vertical",
        })
    }
}

impl std::fmt::Display for FilterField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl std::fmt::Display for FilterOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}
