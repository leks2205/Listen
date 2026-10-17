//! How a kind file declares its menu entries.
//!
//! ```ignore
//! // in menu/song.rs
//! out.push(
//!     Entry::action(Icon::LuListMusic, "Add to playlist…", MenuAction::ChoosePlaylist)
//!         .requires(Capabilities::PLAYLIST_ADD),
//! );
//! out.push(Entry::submenu(Icon::LuPlay, "Play", children));          // a side panel
//! out.push(Entry::action(..).when(cx.single()));                      // conditional
//! out.push(Entry::action(..).disabled());                             // greyed out
//! ```
//!
//! Kind files only declare entries; [`resolve`] drops those the source can't do or whose
//! `when` is false, and submenus left empty.

use super::{MenuAction, MenuTarget, PlayHow, TargetKind};
use crate::app::Listen;
use crate::app::list::ListId;
use crate::icons::Icon;
use listen_source::{Capabilities, MusicSource};
use std::sync::Arc;

/// One row of a menu or submenu.
#[derive(Debug, Clone)]
pub struct Entry {
    pub icon: Icon,
    pub label: String,
    pub kind: EntryKind,
    /// Shown greyed out and not clickable.
    pub enabled: bool,
    /// All of these must be offered by the source, else the entry is hidden.
    pub requires: Capabilities,
    /// False hides the entry.
    pub visible: bool,
    /// What clicking a submenu row itself does (Feishin: the default play action
    /// of the "Play" row); hovering it still opens the submenu.
    pub primary: Option<MenuAction>,
}

#[derive(Debug, Clone)]
pub enum EntryKind {
    /// Runs the action (and closes the menu, unless it only changes the page).
    Action(MenuAction),
    /// Opens a side panel with these entries.
    Submenu(Vec<Entry>),
    /// A separator line (no icon / label).
    Divider,
}

impl Entry {
    pub fn action(icon: Icon, label: impl Into<String>, action: MenuAction) -> Self {
        Self::new(icon, label.into(), EntryKind::Action(action))
    }

    pub fn submenu(icon: Icon, label: impl Into<String>, children: Vec<Entry>) -> Self {
        Self::new(icon, label.into(), EntryKind::Submenu(children))
    }

    pub fn divider() -> Self {
        Self::new(Icon::LuX, String::new(), EntryKind::Divider)
    }

    fn new(icon: Icon, label: String, kind: EntryKind) -> Self {
        Entry {
            icon,
            label,
            kind,
            enabled: true,
            requires: Capabilities::empty(),
            visible: true,
            primary: None,
        }
    }

    /// Clicking the submenu row runs `action`.
    pub fn primary(mut self, action: MenuAction) -> Self {
        self.primary = Some(action);
        self
    }

    /// Hidden unless the source offers `caps`.
    pub fn requires(mut self, caps: Capabilities) -> Self {
        self.requires |= caps;
        self
    }

    /// Hidden unless `condition`.
    pub fn when(mut self, condition: bool) -> Self {
        self.visible &= condition;
        self
    }

    /// Shown, but greyed out.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Shown always, clickable only when `condition` (Feishin greys such
    /// entries out for several items instead of hiding them).
    pub fn enabled_if(mut self, condition: bool) -> Self {
        self.enabled &= condition;
        self
    }

    pub fn is_submenu(&self) -> bool {
        matches!(self.kind, EntryKind::Submenu(_))
    }

    /// Whether arrow keys can land on it.
    pub fn focusable(&self) -> bool {
        self.enabled && !matches!(self.kind, EntryKind::Divider)
    }

    pub fn children(&self) -> &[Entry] {
        match &self.kind {
            EntryKind::Submenu(c) => c,
            _ => &[],
        }
    }
}

/// What a kind file may look at when it declares its entries.
pub struct Ctx<'a> {
    pub state: &'a Listen,
    pub target: &'a MenuTarget,
    /// The list the menu was opened from (for "Clear selection").
    pub list: Option<&'a ListId>,
    /// The source of the targeted items (genres / folders: the one being browsed).
    pub source: Option<Arc<dyn MusicSource>>,
    pub caps: Capabilities,
    /// The targeted items live on the server whose pages are open (go-to entries).
    pub browsing: bool,
}

impl<'a> Ctx<'a> {
    pub fn new(state: &'a Listen, target: &'a MenuTarget, list: Option<&'a ListId>) -> Self {
        let server = target.server().or(state.config.active_server.as_ref());
        let source = server.and_then(|s| state.sources.get(s)).cloned().or_else(|| state.source.clone());
        let caps = source.as_ref().map_or(Capabilities::empty(), |s| s.capabilities());
        let browsing = server.is_some() && server == state.config.active_server.as_ref();
        Ctx { state, target, list, source, caps, browsing }
    }

    /// Exactly one item is targeted.
    pub fn single(&self) -> bool {
        !self.target.is_multi()
    }
}

/// The shared Play ▸ submenu (Feishin: play-action.tsx): Play, Next, Last, then the same
/// shuffled. The shuffled queue variants shuffle the songs once and queue them.
pub fn play_submenu() -> Entry {
    Entry::submenu(
        Icon::LuPlay,
        "Play",
        vec![
            Entry::action(Icon::LuPlay, "Play", MenuAction::Play),
            Entry::action(Icon::LuSkipForward, "Next", MenuAction::PlayNext),
            Entry::action(Icon::RiPlayListAddLine, "Last", MenuAction::AddToQueue),
            Entry::divider(),
            Entry::action(Icon::LuShuffle, "Play (shuffled)", MenuAction::Shuffle),
            Entry::action(Icon::LuSkipForward, "Next (shuffled)", MenuAction::PlayNextShuffled),
            Entry::action(Icon::RiPlayListAddLine, "Last (shuffled)", MenuAction::AddToQueueShuffled),
        ],
    )
    // Clicking "Play" itself does what the Play buttons do (Lists → Play button behavior).
    .primary(MenuAction::PlayDefault)
}

/// "Album radio" / "Artist radio" ▸ Play, Next, Last (Feishin: play-album-radio-action.tsx).
/// Greyed out for several items; hidden where the source has no radio.
pub fn radio_submenu(cx: &Ctx, label: &str) -> Entry {
    Entry::submenu(
        Icon::RiRadioLine,
        label,
        vec![
            Entry::action(Icon::LuPlay, "Play", MenuAction::Radio(PlayHow::Now)),
            Entry::action(Icon::LuSkipForward, "Next", MenuAction::Radio(PlayHow::Next)),
            Entry::action(Icon::RiPlayListAddLine, "Last", MenuAction::Radio(PlayHow::Last)),
        ],
    )
    .primary(MenuAction::Radio(match cx.state.play_behavior() {
        listen_core::settings::PlayBehavior::Now => PlayHow::Now,
        listen_core::settings::PlayBehavior::Next => PlayHow::Next,
        listen_core::settings::PlayBehavior::Last => PlayHow::Last,
    }))
    .when(cx.caps.intersects(Capabilities::SIMILAR_SONGS | Capabilities::RADIO))
    .enabled_if(cx.single())
}

/// "Favorite" ▸ add / remove, worded for the source (YouTube Music likes, saves, subscribes).
/// Albums and artists need [`Capabilities::FAVORITE_ITEMS`]; hidden when favourites are off.
pub fn favorite_submenu(cx: &Ctx, kind: TargetKind, ytm: bool) -> Entry {
    let (add, remove) = match (ytm, kind) {
        (true, TargetKind::Song) => ("Like", "Remove from Liked Music"),
        (true, TargetKind::Album) => ("Save to library", "Remove from library"),
        (true, _) => ("Subscribe", "Unsubscribe"),
        (false, _) => ("Add to favorites", "Remove from favorites"),
    };
    let mut e = Entry::submenu(
        Icon::RiHeartLine,
        "Favorite",
        vec![
            Entry::action(Icon::RiHeartLine, add, MenuAction::Favorite(true)),
            Entry::action(Icon::RiHeartFill, remove, MenuAction::Favorite(false)),
        ],
    )
    .when(cx.state.show_favorites());
    // A local folder cannot like songs.
    if kind == TargetKind::Song {
        e = e.when(cx.source.as_ref().is_none_or(|s| s.kind() != listen_core::model::SourceKind::Local));
    } else {
        e = e.requires(Capabilities::FAVORITE_ITEMS);
    }
    e
}

/// "Go to" ▸ the album, then each artist (Feishin: go-to-action.tsx). Greyed out for several
/// items; left out when empty or the items are not on the browsed server.
pub fn go_to_submenu(cx: &Ctx, album: Option<String>, artists: &[(String, String)]) -> Entry {
    let mut children = Vec::new();
    if let Some(id) = album.filter(|id| !id.is_empty()) {
        children.push(Entry::action(Icon::RiAlbumLine, "Go to album", MenuAction::GoToAlbum(id)));
    }
    for (id, name) in artists {
        children.push(Entry::action(
            Icon::LuUser,
            format!("Go to {name}"),
            MenuAction::GoToArtist(id.clone()),
        ));
    }
    Entry::submenu(Icon::LuLink, "Go to", children).when(cx.browsing).enabled_if(cx.single())
}

/// Play next / Add to queue, shared by every kind that can be queued.
pub fn queue_entries() -> [Entry; 2] {
    [
        Entry::action(Icon::LuSkipForward, "Play next", MenuAction::PlayNext),
        Entry::action(Icon::RiPlayListAddLine, "Add to queue", MenuAction::AddToQueue),
    ]
}

/// Play / Shuffle, for kinds that are a collection of songs.
pub fn play_shuffle() -> [Entry; 2] {
    [
        Entry::action(Icon::LuPlay, "Play", MenuAction::Play),
        Entry::action(Icon::LuShuffle, "Shuffle", MenuAction::Shuffle),
    ]
}

/// "Add to playlist…" (hidden where the source cannot add).
pub fn add_to_playlist() -> Entry {
    Entry::action(Icon::LuListMusic, "Add to playlist…", MenuAction::ChoosePlaylist)
        .requires(Capabilities::PLAYLIST_ADD)
}

/// "Clear selection", when the menu was opened from a list with a selection.
pub fn clear_selection(cx: &Ctx) -> Entry {
    Entry::action(Icon::LuX, "Clear selection", MenuAction::ClearSelection).when(cx.list.is_some())
}

/// "Set rating" ▸ no stars to five stars (Feishin: set-rating-action.tsx); the first clears.
/// `current` (see [`shared_rating`]) is marked. Hidden when ratings are off.
pub fn rating_submenu(cx: &Ctx, current: Option<u8>) -> Entry {
    let children: Vec<Entry> = (0..=5u8)
        .map(|n| {
            let stars = format!("{}{}", "★".repeat(n as usize), "☆".repeat(5 - n as usize));
            let label = if current == Some(n) { format!("{stars} ✓") } else { stars };
            Entry::action(Icon::LuStar, label, MenuAction::Rate(n))
        })
        .collect();
    Entry::submenu(Icon::LuStar, "Set rating", children)
        .requires(Capabilities::RATINGS)
        .when(cx.state.show_ratings())
}

/// The rating all items share (0 = unrated), if they do.
pub fn shared_rating(ratings: impl IntoIterator<Item = Option<u8>>) -> Option<u8> {
    let mut all = ratings.into_iter().map(|r| r.unwrap_or(0));
    let first = all.next()?;
    all.all(|r| r == first).then_some(first)
}

/// Drops entries that are hidden or whose capabilities are missing, recursively;
/// a submenu left empty goes too, and so does a divider at an edge or doubled.
pub fn resolve(entries: Vec<Entry>, caps: Capabilities) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for mut e in entries {
        if !e.visible || !caps.contains(e.requires) {
            continue;
        }
        if let EntryKind::Submenu(children) = e.kind {
            let children = resolve(children, caps);
            if children.is_empty() {
                continue;
            }
            e.kind = EntryKind::Submenu(children);
        }
        let divider = matches!(e.kind, EntryKind::Divider);
        if divider && out.last().is_none_or(|l| matches!(l.kind, EntryKind::Divider)) {
            continue;
        }
        out.push(e);
    }
    if out.last().is_some_and(|l| matches!(l.kind, EntryKind::Divider)) {
        out.pop();
    }
    out
}

/// Next focusable index from `from` in direction `step` (+1 / -1), wrapping.
pub fn step_focus(entries: &[Entry], from: Option<usize>, step: isize) -> Option<usize> {
    let n = entries.len() as isize;
    if n == 0 {
        return None;
    }
    let mut i = match from {
        Some(i) => i as isize,
        None if step > 0 => -1,
        None => n,
    };
    for _ in 0..n {
        i = (i + step).rem_euclid(n);
        if entries[i as usize].focusable() {
            return Some(i as usize);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(label: &str) -> Entry {
        Entry::action(Icon::LuPlay, label, MenuAction::Play)
    }

    #[test]
    fn resolve_applies_caps_conditions_and_tidies_dividers() {
        let entries = vec![
            Entry::divider(),
            a("one"),
            a("gated").requires(Capabilities::PLAYLIST_ADD),
            Entry::divider(),
            a("hidden").when(false),
            Entry::divider(),
            Entry::submenu(Icon::LuPlay, "empty", vec![a("x").when(false)]),
            a("two"),
            Entry::divider(),
        ];
        let out = resolve(entries.clone(), Capabilities::empty());
        let labels: Vec<_> = out.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["one", "", "two"]);
        let out = resolve(entries, Capabilities::PLAYLIST_ADD);
        assert_eq!(out.iter().filter(|e| e.label == "gated").count(), 1);
    }

    #[test]
    fn focus_wraps_and_skips_disabled_and_dividers() {
        let e = vec![a("a"), Entry::divider(), a("b").disabled(), a("c")];
        assert_eq!(step_focus(&e, None, 1), Some(0));
        assert_eq!(step_focus(&e, Some(0), 1), Some(3));
        assert_eq!(step_focus(&e, Some(3), 1), Some(0));
        assert_eq!(step_focus(&e, None, -1), Some(3));
        assert_eq!(step_focus(&e, Some(0), -1), Some(3));
        assert_eq!(step_focus(&[], None, 1), None);
    }
}
