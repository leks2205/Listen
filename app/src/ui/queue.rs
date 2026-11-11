//! The play queue: the right-hand panel (queue, lyrics, visualizer in `sidebar.panel_order`), the drawer,
//! the Now Playing page and the toolbar. State and actions are in `app/queue_ui.rs`.
//! Rows are play-order positions ([`QRow`]) whose cells read the queue directly; nothing is copied per frame.

use super::list::columns::{Cx, ListItem};
use super::list::{GroupLook, ListView, RowGroup, header};
use super::{icon, label, label_w};
use crate::app::list::{DropTarget, ListId, ListMsg};
use crate::app::menu::MenuTarget;
use crate::app::play_mode::PlayMode;
use crate::app::queue_ui::{QueueMsg, ResizeAxis};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, mouse_area, responsive, row, space, stack};
use iced::{Alignment, Element, Length, Padding};
use listen_core::model::{ArtistRef, Song, format_duration};
use listen_core::settings::{ColumnId, SideQueueLayout};
use std::sync::LazyLock;

/// Height of the toolbar under the header.
pub const TOOLBAR_H: f32 = 44.0;
/// Height of the drop zone after the last row.
const TAIL_H: f32 = 56.0;
/// Width the "Auto" / "AI" / "Queued" tag takes.
const TAG_W: f32 = 60.0;
/// Id of the scrollable of the lyrics panel.
pub const PANEL_LYRICS_ID: &str = "queue-panel-lyrics";
/// Height of the visualizer strip in the panel.
const VIZ_H: f32 = 120.0;
/// Height of a panel divider you can drag.
const DIVIDER_H: f32 = 5.0;

// ---- rows -------------------------------------------------------------------------

/// A row of the queue lists: a play-order position. The song, its flags and
/// whether it is playing are read from `Listen::queue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QRow(pub usize);

/// Queues longer than this show their first rows only.
const MAX_ROWS: usize = 200_000;

static IDENTITY: LazyLock<Vec<QRow>> = LazyLock::new(|| (0..MAX_ROWS).map(QRow).collect());

/// The rows `0..n` (every position of a queue of `n` songs).
pub fn identity(n: usize) -> &'static [QRow] {
    &IDENTITY[..n.min(MAX_ROWS)]
}

/// The song matches the (lowercased) search text.
pub fn matches(song: &Song, query: &str) -> bool {
    song.title.to_lowercase().contains(query)
        || song.artist_line().to_lowercase().contains(query)
        || song.album.as_deref().is_some_and(|a| a.to_lowercase().contains(query))
        || song.genres.iter().any(|g| g.to_lowercase().contains(query))
}

fn item<'a>(state: &'a Listen, row: &QRow) -> Option<&'a listen_core::queue::QueueItem> {
    state.queue.item_at(row.0)
}

impl ListItem for QRow {
    fn text(&self, _col: ColumnId, _index: usize) -> Option<String> {
        None
    }

    fn cell<'a>(&'a self, cx: &Cx<'a>, col: ColumnId) -> Element<'a, Message> {
        match item(cx.state, self) {
            Some(it) => it.song.cell(cx, col),
            None => space().width(0).into(),
        }
    }

    fn cover<'a>(&self, state: &Listen, size: f32, radius: f32) -> Element<'a, Message> {
        match item(state, self) {
            Some(it) => it.song.cover(state, size, radius),
            None => space().width(size).height(size).into(),
        }
    }

    fn is_current(&self, state: &Listen) -> bool {
        state.queue.current_position() == Some(self.0)
    }

    fn favorite(&self, state: &Listen) -> Option<bool> {
        item(state, self).and_then(|it| it.song.favorite(state))
    }

    fn rating(&self, state: &Listen) -> Option<u8> {
        item(state, self).and_then(|it| it.song.rating(state))
    }

    fn title(&self) -> String {
        String::new()
    }
}

/// The row at `row` of a queue list is the playing one.
pub fn row_is_current(state: &Listen, row: usize) -> bool {
    state.queue_row_pos(row).is_some_and(|p| state.queue.current_position() == Some(p))
}

/// The number column: the position, a play button under the pointer, the
/// playing marker.
pub fn index_cell<'a>(cx: &Cx<'a>, current: bool) -> Element<'a, Message> {
    let t = cx.state.tokens;
    if cx.hovered {
        let play = button(icon(Icon::LuPlay, metrics::font_lg(), t.foreground))
            .padding(4)
            .style(style::icon_button(t, false))
            .on_press(Message::List(ListMsg::PlayRow(
                cx.id.clone(),
                cx.index,
                listen_core::settings::PlayBehavior::Now,
            )));
        return super::tip::tip(cx.state, play, "Play now", Some("listPlayNow"), super::tip::Position::Right);
    }
    if current {
        return icon(Icon::LuPlay, metrics::font_lg(), t.primary);
    }
    let number = cx.state.queue_row_pos(cx.index).unwrap_or(cx.index) + 1;
    label(number.to_string(), metrics::font_md(), t.muted).into()
}

/// Who added a song: "AI" for the AI DJ's picks, "Auto" for the Auto-DJ's.
fn origin_tag(auto: bool, ai: bool) -> Option<&'static str> {
    match (auto, ai) {
        (_, true) => Some(PlayMode::AiDj.tag()),
        (true, false) => Some(PlayMode::AutoDj.tag()),
        _ => None,
    }
}

#[cfg(test)]
mod origin_tests {
    use super::origin_tag;

    #[test]
    fn the_tag_names_who_added_the_song() {
        assert_eq!(origin_tag(true, true), Some("AI"));
        assert_eq!(origin_tag(true, false), Some("Auto"));
        assert_eq!(origin_tag(false, false), None);
    }
}

/// What the tag of a row says: who added the DJ's songs, "Queued" for
/// "Play next" songs that wait (only when the Up Next headers are off).
fn tag_text(state: &Listen, id: &ListId, row: usize) -> Option<&'static str> {
    let pos = state.queue_row_pos(row)?;
    let it = state.queue.item_at(pos)?;
    let hidden = *id == ListId::SideQueue && !state.config.settings.queue.show_mode_tag;
    if it.bridge.is_some() {
        // A DJ bridge: only a short stretch of it plays.
        return Some("Bridge");
    }
    if let Some(tag) = origin_tag(it.auto, it.ai) {
        if hidden { None } else { Some(tag) }
    } else if it.queued
        && !state.config.settings.queue.up_next_queue
        && state.queue.current_position().is_none_or(|p| pos > p)
    {
        Some("Queued")
    } else {
        None
    }
}

/// The tag next to a title (queue lists only).
pub fn tag<'a>(cx: &Cx<'a>) -> Option<Element<'a, Message>> {
    if !cx.id.is_queue() {
        return None;
    }
    let text = tag_text(cx.state, &cx.id, cx.index)?;
    let tag = container(label(text, metrics::font_xs(), cx.state.tokens.primary)).width(TAG_W);
    // "Why this song?" on the tag of a DJ's pick.
    match why_of(cx.state, cx.index) {
        Some(why) => Some(super::tip::tip_raw(cx.state, tag, &why, None, super::tip::Position::Top)),
        None => Some(tag.into()),
    }
}

/// Why the DJ picked the song at queue row `row`, translated; `None` for songs the user added.
pub fn why_of(state: &Listen, row: usize) -> Option<String> {
    let it = state.queue.item_at(state.queue_row_pos(row)?)?;
    it.why.as_ref().map(crate::i18n::why)
}

/// Room the tag takes next to the title text.
pub fn tag_width(cx: &Cx<'_>) -> f32 {
    if cx.id.is_queue() && tag_text(cx.state, &cx.id, cx.index).is_some() {
        TAG_W + metrics::space_md()
    } else {
        0.0
    }
}

/// The artists under a title; in the queue followed by the tempo and key of
/// analysed songs ("Artist · 124 BPM · 8A").
pub fn artists<'a>(cx: &Cx<'a>, artists: &'a [ArtistRef], song: &Song) -> Element<'a, Message> {
    let tempo = if cx.id.is_queue() { super::tempo_key(cx.state, song) } else { None };
    let Some(tempo) = tempo else {
        return super::list::columns::artist_links(cx, artists, true);
    };
    let t = cx.state.tokens;
    let extra = format!(" · {tempo}");
    let width = extra.chars().map(super::char_em).sum::<f32>() * metrics::font_sm();
    let inner = Cx { width: (cx.width - width).max(40.0), ..cx.clone() };
    row![super::list::columns::artist_links(&inner, artists, true), label(extra, metrics::font_sm(), t.muted)]
        .align_y(Alignment::Center)
        .into()
}

/// The context menu target of a queue row (it knows the row's uid).
pub fn target(cx: &Cx<'_>, song: &Song) -> Option<MenuTarget> {
    let pos = cx.state.queue_row_pos(cx.index)?;
    let it = cx.state.queue.item_at(pos)?;
    Some(MenuTarget::queue_item(song, it.uid))
}

// ---- group rows ---------------------------------------------------------------------

/// The label rows of the table: "Queue / Up Next / Queue" around the songs
/// added with "Play next" (Feishin `upNextQueue`) and the AI DJ's break marker.
pub fn groups(state: &Listen) -> Vec<RowGroup> {
    let mut out: Vec<RowGroup> = Vec::new();
    let group = |before: usize, title: &str, look: GroupLook| RowGroup {
        before,
        title: title.to_string(),
        checked: false,
        on_toggle: Message::Noop,
        look,
    };
    let searching = !state.queue_ui.search.trim().is_empty();
    let run = state.queue.up_next_range();
    if state.config.settings.queue.up_next_queue && !searching && !run.is_empty() {
        if run.start > 0 {
            out.push(group(0, "Queue", GroupLook::Label));
        }
        out.push(group(run.start, "Up Next", GroupLook::Label));
        if run.end < state.queue.len() {
            out.push(group(run.end, "Queue", GroupLook::Label));
        }
    }
    if let Some(uid) = state.dj_break_before()
        && let Some(pos) = state.queue.iter().position(|i| i.uid == uid)
        && let Some(row) = state.queue_row_of(pos)
    {
        out.push(group(row, "AI DJ break", GroupLook::DjBreak));
    }
    out.sort_by_key(|g| g.before);
    out
}

// ---- the table ------------------------------------------------------------------------

/// The drop zone after the last row: dragged rows land at the end.
fn tail<'a>(state: &'a Listen, id: ListId, count: usize) -> Element<'a, Message> {
    let t = state.tokens;
    let landing = state.queue_landing(&id, count);
    let zone = container(space().width(Length::Fill).height(TAIL_H - 2.0))
        .width(Length::Fill)
        .height(TAIL_H)
        .style(move |_| container::Style {
            border: if landing {
                iced::Border { color: t.primary, width: 2.0, radius: 0.0.into() }
            } else {
                iced::Border::default()
            },
            ..Default::default()
        });
    mouse_area(zone)
        .on_release(Message::List(ListMsg::Drop(DropTarget::QueueAt(count))))
        .on_enter(Message::List(ListMsg::Hover(id.clone(), count)))
        .on_exit(Message::List(ListMsg::Unhover(id, count)))
        .into()
}

/// The queue table of `id`, or the drop zone of an empty queue.
fn table_of<'a>(state: &'a Listen, id: ListId) -> Element<'a, Message> {
    let t = state.tokens;
    if state.queue.is_empty() {
        return super::list::drop_target(
            container(
                column![
                    icon(Icon::LuListMusic, 36.0, t.muted),
                    label("The queue is empty", metrics::font_lg(), t.muted),
                    label("Drop songs, albums or playlists here to play them.", metrics::font_sm(), t.muted),
                ]
                .spacing(metrics::space_sm())
                .align_x(Alignment::Center),
            )
            .center(Length::Fill),
            DropTarget::QueueEmpty,
        );
    }
    let rows = state.queue_rows();
    let count = rows.len();
    let tail_id = id.clone();
    ListView::new(state, id, rows)
        .no_header()
        .groups(groups(state))
        .tail(TAIL_H, move || tail(state, tail_id.clone(), count))
        .empty("No song in the queue matches.")
        .view()
}

/// The queue rows under a toolbar (also used by the full-screen player). `_width` is not used: the
/// toolbar measures the width it gets itself, and its "…" menu takes the fold tier from there.
pub fn list(state: &Listen, _width: f32) -> Element<'_, Message> {
    column![
        toolbar(state, ListId::SideQueue),
        container(table_of(state, ListId::SideQueue)).height(Length::Fill)
    ]
    .into()
}

// ---- the toolbar -------------------------------------------------------------------------

/// Widths under which the buttons fold into the "…" menu, step by step (Feishin: 480 / 380 / 300 px).
/// Tier 1 folds only the server-sync buttons, so it may fold nothing (see [`folded`]).
pub fn overflow_tier(width: f32) -> u8 {
    if width <= 300.0 {
        3
    } else if width <= 380.0 {
        2
    } else if width <= 480.0 {
        1
    } else {
        0
    }
}

/// A toolbar action; at narrow widths it moves into the "…" menu. The toolbar and the menu both
/// list these, so a button and its menu item share one glyph and one name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueTool {
    Save,
    Restore,
    Shuffle,
    Clear,
    Jump,
    Create,
}

impl QueueTool {
    /// In toolbar (and menu) order.
    pub const ALL: [QueueTool; 6] = [
        QueueTool::Save,
        QueueTool::Restore,
        QueueTool::Shuffle,
        QueueTool::Clear,
        QueueTool::Jump,
        QueueTool::Create,
    ];

    /// The first [`overflow_tier`] at which the tool is in the "…" menu instead of the toolbar.
    fn folds_at(self) -> u8 {
        match self {
            QueueTool::Save | QueueTool::Restore => 1,
            QueueTool::Jump | QueueTool::Create => 2,
            QueueTool::Shuffle | QueueTool::Clear => 3,
        }
    }

    /// Only servers that keep a play queue offer save / restore.
    fn needs_sync(self) -> bool {
        matches!(self, QueueTool::Save | QueueTool::Restore)
    }

    fn glyph(self) -> Icon {
        match self {
            QueueTool::Save => Icon::LuUpload,
            QueueTool::Restore => Icon::LuDownload,
            QueueTool::Shuffle => Icon::LuShuffle,
            QueueTool::Clear => Icon::LuTrash2,
            QueueTool::Jump => Icon::LuLocateFixed,
            QueueTool::Create => Icon::RiPlayListAddLine,
        }
    }

    fn name(self) -> &'static str {
        match self {
            QueueTool::Save => "Save queue to server",
            QueueTool::Restore => "Restore queue from server",
            QueueTool::Shuffle => "Shuffle queue",
            QueueTool::Clear => "Clear queue",
            QueueTool::Jump => "Jump to playing song",
            QueueTool::Create => "Create playlist from queue",
        }
    }

    /// The message, `None` while it can't run (a save / restore is running).
    fn message(self, state: &Listen) -> Option<Message> {
        let m = match self {
            QueueTool::Save => QueueMsg::SaveToServer,
            QueueTool::Restore => QueueMsg::RestoreFromServer,
            QueueTool::Shuffle => QueueMsg::Shuffle,
            QueueTool::Clear => QueueMsg::Clear,
            QueueTool::Jump => QueueMsg::JumpToCurrent,
            QueueTool::Create => QueueMsg::CreatePlaylist,
        };
        (!(self.needs_sync() && state.queue_ui.busy)).then_some(Message::Queue(m))
    }
}

/// The tools a toolbar at fold `tier` shows (`sync`: the server keeps a play queue).
pub fn unfolded(tier: u8, sync: bool) -> Vec<QueueTool> {
    QueueTool::ALL.into_iter().filter(|x| (sync || !x.needs_sync()) && tier < x.folds_at()).collect()
}

/// The tools a toolbar at fold `tier` moves into its "…" menu; the "…" button shows only when this
/// is not empty.
pub fn folded(tier: u8, sync: bool) -> Vec<QueueTool> {
    QueueTool::ALL.into_iter().filter(|x| (sync || !x.needs_sync()) && tier >= x.folds_at()).collect()
}

fn tool<'a>(
    state: &Listen,
    glyph: Icon,
    msg: Option<Message>,
    active: bool,
    name: &str,
) -> Element<'a, Message> {
    let t = state.tokens;
    super::tip::tip(
        state,
        button(icon(glyph, metrics::font_lg(), if msg.is_some() { t.foreground } else { t.muted }))
            .padding(6)
            .style(style::icon_button(t, active))
            .on_press_maybe(msg),
        name,
        None,
        super::tip::Position::Bottom,
    )
}

/// Save / restore (servers with a play queue), shuffle, clear, jump to the
/// playing song, create a playlist, search and the list options.
pub fn toolbar(state: &Listen, id: ListId) -> Element<'_, Message> {
    let t = state.tokens;
    responsive(move |size| {
        // The tier from the width this toolbar really has; the "…" menu receives it in its message.
        let tier = overflow_tier(size.width);
        let sync = state.queue_server_sync();
        let mut tools = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
        for x in unfolded(tier, sync) {
            tools = tools.push(tool(state, x.glyph(), x.message(state), false, x.name()));
        }
        if !folded(tier, sync).is_empty() {
            tools = tools.push(super::list::anchor::anchored(
                super::tip::tip(
                    state,
                    button(icon(Icon::LuEllipsis, metrics::font_lg(), t.foreground))
                        .padding(6)
                        .style(style::icon_button(t, state.queue_ui.more_menu.is_some()))
                        .on_press(Message::Noop),
                    "More",
                    None,
                    super::tip::Position::Bottom,
                ),
                move |b| {
                    Message::Queue(QueueMsg::MoreMenu(Some((
                        iced::Point::new(b.x, b.y + b.height + 4.0),
                        tier,
                    ))))
                },
            ));
        }
        let search = iced::widget::text_input(crate::i18n::tr("Search"), &state.queue_ui.search)
            .on_input(|v| Message::Queue(QueueMsg::Search(v)))
            .size(metrics::font_sm())
            .padding(Padding::from([4.0, 8.0]))
            .style(style::input(t))
            .width(Length::Fill);
        let config = state.config.settings.lists.config(id.kind());
        container(
            row![
                tools,
                container(space().width(1).height(22)).style(style::divider(t)),
                search,
                header::toolbar(state, &id, &config)
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([0.0, metrics::space_md()]))
        .height(TOOLBAR_H)
        .center_y(TOOLBAR_H)
        .into()
    })
    .height(TOOLBAR_H)
    .into()
}

/// The buttons the toolbar at fold `tier` folded, as a popover under its "…" button.
fn more_menu<'a>(state: &'a Listen, tier: u8) -> Element<'a, Message> {
    let t = state.tokens;
    let mut col = Column::new();
    for x in folded(tier, state.queue_server_sync()) {
        col = col.push(
            button(
                row![
                    icon(x.glyph(), metrics::font_lg(), t.foreground),
                    label_w(x.name(), metrics::font_md(), t.foreground, Weight::Medium)
                ]
                .spacing(metrics::space_md())
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(Padding::from([8.0, metrics::space_md()]))
            .style(style::nav_item(t, false))
            .on_press_maybe(x.message(state)),
        );
    }
    iced::widget::opaque(container(col).width(260).padding(4).style(move |_| container::Style {
        background: Some(iced::Background::Color(t.background_alt)),
        border: iced::border::rounded(metrics::radius_md()).color(t.border).width(1),
        shadow: iced::Shadow {
            color: style::alpha(iced::Color::BLACK, 0.45),
            offset: iced::Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..Default::default()
    }))
}

// ---- header ------------------------------------------------------------------------------

/// "1415 songs · 98 h" under the title, translated.
pub fn summary(state: &Listen) -> String {
    let n = state.queue.len();
    let total: std::time::Duration = state.queue.iter().map(|i| i.song.duration).sum();
    let songs = super::plural(n, "song");
    if total.is_zero() { songs } else { format!("{songs} · {}", format_duration(total)) }
}

/// The play-mode button (names the current mode; a click moves to the next, like the player bar's)
/// and, while a DJ is on, the Focus chip.
pub fn mode_switcher(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let mode = state.play_mode();
    let on = mode != PlayMode::Off;
    let color = if on { t.primary } else { t.muted };
    let b = button(
        row![
            super::mode_glyph(mode, metrics::font_md(), color),
            label_w(mode.tag(), metrics::font_sm(), color, Weight::Semibold),
        ]
        .spacing(metrics::space_xs())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([metrics::space_xs(), metrics::space_sm()]))
    .style(style::icon_button(t, on))
    .on_press(Message::ToggleShuffle);
    let hint = super::tip::play_mode_tip(state);
    let mode_button =
        super::tip::tip_raw(state, b, &hint, Some("toggleShuffle"), super::tip::Position::Bottom);
    // Focus applies to the DJs' picks: offered in Auto-DJ and AI DJ (and while on, so it can be switched off).
    let focus = state.focus_on();
    if !state.focus_chip_shown() {
        return mode_button;
    }
    let color = if focus { t.primary } else { t.muted };
    let chip = button(label_w("Focus", metrics::font_sm(), color, Weight::Semibold))
        .padding(Padding::from([metrics::space_xs(), metrics::space_sm()]))
        .style(style::icon_button(t, focus))
        .on_press(Message::Focus(crate::app::focus::FocusMsg::Toggle));
    let chip = super::tip::tip(
        state,
        chip,
        if focus {
            "Focus is on: little singing, instrumental versions, no DJ talk"
        } else {
            "Focus: the DJs pick songs with little singing and keep quiet"
        },
        None,
        super::tip::Position::Bottom,
    );
    row![mode_button, chip].spacing(metrics::space_xs()).align_y(Alignment::Center).into()
}

fn header_row(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    container(
        row![
            label_w("Queue", metrics::font_xl(), t.foreground, Weight::Bold),
            space::horizontal(),
            super::data_label(summary(state), metrics::font_sm(), t.muted),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    )
    .height(metrics::ACTION_BAR_HEIGHT)
    .padding(Padding::from([0.0, metrics::space_lg()]))
    .center_y(metrics::ACTION_BAR_HEIGHT)
    .into()
}

// ---- panels -----------------------------------------------------------------------------

/// `order` with exactly the three panels, each once (a saved or imported
/// order may have lost or repeated one).
pub fn normalize_order(order: &mut Vec<String>) {
    let mut seen: Vec<String> = Vec::new();
    for p in order.iter() {
        if ["queue", "lyrics", "visualizer"].contains(&p.as_str()) && !seen.contains(p) {
            seen.push(p.clone());
        }
    }
    for p in ["queue", "lyrics", "visualizer"] {
        if !seen.iter().any(|s| s == p) {
            seen.push(p.to_string());
        }
    }
    *order = seen;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Queue,
    Lyrics,
    Visualizer,
    /// Lyrics with the visualizer under them.
    Combined,
}

/// The right sidebar's panels, top to bottom, in the saved order; switched-off ones left out, lyrics and
/// visualizer merged at the first one's place when they share a panel.
pub fn panels(state: &Listen) -> Vec<Panel> {
    let s = &state.config.settings;
    let mut order = s.sidebar.panel_order.clone();
    normalize_order(&mut order);
    let lyrics = s.queue.lyrics_in_sidebar;
    let viz = s.visualizer.in_queue_panel && state.visualizer_error.is_none();
    let combined = lyrics && viz && s.queue.combined_panel;
    let mut out = Vec::new();
    let mut merged = false;
    for p in &order {
        match p.as_str() {
            "queue" => out.push(Panel::Queue),
            "lyrics" | "visualizer" if combined => {
                if !merged {
                    merged = true;
                    out.push(Panel::Combined);
                }
            }
            "lyrics" if lyrics => out.push(Panel::Lyrics),
            "visualizer" if viz => out.push(Panel::Visualizer),
            _ => {}
        }
    }
    // The queue panel can be switched off, but never every panel.
    if !s.sidebar.show_queue && out.len() > 1 {
        out.retain(|p| *p != Panel::Queue);
    }
    out
}

fn queue_panel(state: &Listen, width: f32) -> Element<'_, Message> {
    let mut col = column![header_row(state)];
    if state.config.settings.queue.show_mode_tag {
        col = col.push(
            container(mode_switcher(state)).padding(Padding::from([metrics::SPACE_XS, metrics::SPACE_MD])),
        );
    }
    if let Some(steer) = tell_the_dj(state) {
        col = col.push(container(steer).padding(Padding::from([metrics::SPACE_XS, metrics::SPACE_MD])));
    }
    col.push(list(state, width)).into()
}

/// "Tell the DJ" while the AI DJ is on: a request box, the quick chips (an active one is filled; a
/// click removes it again) and the typed requests with an ×.
pub fn tell_the_dj(state: &Listen) -> Option<Element<'_, Message>> {
    use crate::app::dj_steer::SteerMsg;
    use iced::widget::text_input;
    if !state.ai_dj_on() {
        return None;
    }
    let t = state.tokens;
    let steer = |m: SteerMsg| Message::Steer(m);
    let input = text_input(crate::i18n::tr("Tell the DJ…"), &state.dj.request_input)
        .on_input(move |s| steer(SteerMsg::Input(s)))
        .on_submit(steer(SteerMsg::Submit))
        .size(metrics::font_sm())
        .padding(Padding::from([4.0, 8.0]))
        .style(style::input(t));
    let pill = |content: Element<'static, Message>, on: bool, m: Message| -> Element<'static, Message> {
        button(content)
            .padding(Padding::from([2.0, 8.0]))
            .style(move |theme, status| {
                if on {
                    style::primary_button(t)(theme, status)
                } else {
                    style::default_button(t)(theme, status)
                }
            })
            .on_press(m)
            .into()
    };
    let on_color = |on: bool| if on { iced::Color::WHITE } else { t.foreground };
    let chips = Row::with_children(listen_dj::Chip::ALL.into_iter().map(|c| {
        let on = state.dj.requests.has(c);
        pill(
            label_w(c.label(), metrics::font_xs(), on_color(on), Weight::Medium).into(),
            on,
            steer(SteerMsg::Chip(c)),
        )
    }))
    .spacing(metrics::space_xs())
    .wrap();
    let typed: Vec<Element<'_, Message>> = state
        .dj
        .requests
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, r)| match r {
            listen_dj::Steer::Text(text) => Some(pill(
                row![
                    label(super::ellipsize(text, 40), metrics::font_xs(), iced::Color::WHITE),
                    icon(Icon::LuX, metrics::font_xs(), iced::Color::WHITE),
                ]
                .spacing(4)
                .align_y(Alignment::Center)
                .into(),
                true,
                steer(SteerMsg::Remove(i)),
            )),
            listen_dj::Steer::Chip(_) => None,
        })
        .collect();
    let mut col = column![input, chips].spacing(metrics::space_xs());
    if !typed.is_empty() {
        col = col.push(Row::with_children(typed).spacing(metrics::space_xs()).wrap());
    }
    Some(col.into())
}

fn visualizer_panel(state: &Listen) -> Element<'_, Message> {
    if state.milkdrop_active() {
        return super::milkdrop::surface(state, true);
    }
    match &state.spectrum {
        Some(frame) => super::visualizer::strip(frame, &state.config.settings.visualizer),
        None => space::horizontal().into(),
    }
}

/// Lyrics in the sidebar: the full lyrics view (karaoke, translation, romaji
/// and the controls), scrolled by its own id.
pub fn lyrics_panel(state: &Listen) -> Element<'_, Message> {
    super::lyrics::view_in(state, PANEL_LYRICS_ID)
}

fn combined_panel(state: &Listen) -> Element<'_, Message> {
    column![
        container(lyrics_panel(state)).height(Length::Fill),
        container(visualizer_panel(state)).width(Length::Fill).height(VIZ_H),
    ]
    .into()
}

fn panel_body(state: &Listen, panel: Panel, width: f32) -> Element<'_, Message> {
    match panel {
        Panel::Queue => queue_panel(state, width),
        Panel::Lyrics => lyrics_panel(state),
        Panel::Visualizer => visualizer_panel(state),
        Panel::Combined => combined_panel(state),
    }
}

/// A divider between two stacked panels that resizes the fixed-height one.
fn divider<'a>(state: &Listen, axis: ResizeAxis) -> Element<'a, Message> {
    let t = state.tokens;
    mouse_area(
        container(container(space()).width(Length::Fill).height(1).style(style::divider(t)))
            .height(DIVIDER_H)
            .center_y(DIVIDER_H),
    )
    .on_press(Message::Queue(QueueMsg::ResizeStart(axis)))
    .interaction(iced::mouse::Interaction::ResizingVertically)
    .into()
}

/// All panels of the right sidebar in one column: the queue takes what is
/// left, the others have a height you can drag.
fn stacked(state: &Listen, width: f32) -> Element<'_, Message> {
    let panel_h = state.config.settings.queue.panel_height;
    let list = panels(state);
    let queue_at = list.iter().position(|p| *p == Panel::Queue);
    let mut col = Column::new();
    for (i, panel) in list.iter().enumerate() {
        if i > 0 {
            // A panel under the divider grows when dragged up; one above the queue grows when dragged down.
            let axis = if queue_at.is_some_and(|q| i <= q) {
                ResizeAxis::PanelHeightFromTop
            } else {
                ResizeAxis::PanelHeight
            };
            col = col.push(divider(state, axis));
        }
        let body = panel_body(state, *panel, width);
        col = col.push(if *panel == Panel::Queue {
            container(body).width(Length::Fill).height(Length::Fill)
        } else {
            container(body).width(Length::Fill).height(panel_h)
        });
    }
    // Without a queue panel the rest takes all the height.
    if queue_at.is_none() {
        col = col.push(space().height(Length::Fill));
    }
    col.into()
}

/// The panels side by side (the vertical layout puts them under the page).
fn side_by_side(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let mut r = Row::new();
    for (i, panel) in panels(state).iter().enumerate() {
        if i > 0 {
            r = r.push(super::divider_v(t));
        }
        r = r.push(
            container(panel_body(state, *panel, 400.0)).width(Length::FillPortion(1)).height(Length::Fill),
        );
    }
    r.into()
}

/// The right-hand panel (horizontal layout).
pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let width = state.config.settings.sidebar.queue_width;
    container(stacked(state, width)).width(width).height(Length::Fill).style(style::sidebar(t)).into()
}

/// `main` (the sidebar and the page) with the queue panel attached to the
/// right or under it, as set (Feishin `sideQueueType` / `sideQueueLayout`).
pub fn attach<'a>(state: &'a Listen, main: Element<'a, Message>) -> Element<'a, Message> {
    let t = state.tokens;
    if !state.queue_attached() {
        return main;
    }
    match state.config.settings.queue.side_layout {
        SideQueueLayout::Horizontal => {
            row![main, super::resize_handle(t, crate::app::PanelSide::Right), view(state)].into()
        }
        SideQueueLayout::Vertical => {
            let height = state.config.settings.queue.height;
            column![
                container(main).height(Length::Fill),
                divider(state, ResizeAxis::QueueHeight),
                container(side_by_side(state)).width(Length::Fill).height(height).style(style::sidebar(t)),
            ]
            .into()
        }
    }
}

// ---- the Now Playing page -----------------------------------------------------------------

/// Feishin's Now Playing route: the queue toolbar and the queue table.
pub fn page(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let head = container(
        row![
            label_w("Now Playing", 36.0, t.foreground, Weight::Bold),
            space::horizontal(),
            super::data_label(summary(state), metrics::font_md(), t.muted),
            mode_switcher(state),
        ]
        .spacing(metrics::space_md())
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 24.0,
        right: metrics::PAGE_PADDING_X,
        bottom: 8.0,
        left: metrics::PAGE_PADDING_X,
    });
    let steer = tell_the_dj(state)
        .map(|e| container(e).padding(Padding::from([0.0, metrics::PAGE_PADDING_X])).width(Length::Fill));
    column![head,]
        .push(steer)
        .push(column![
            toolbar(state, ListId::Queue),
            super::divider_h(t),
            container(table_of(state, ListId::Queue)).width(Length::Fill).height(Length::Fill),
        ])
        .into()
}

// ---- the drawer ----------------------------------------------------------------------------

/// The arrow strip at the right edge; resting the pointer on it opens the drawer.
fn edge_strip(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let tab = container(icon(Icon::LuChevronLeft, metrics::font_lg(), t.muted))
        .width(18)
        .height(72)
        .center(Length::Shrink)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(t.surface)),
            border: iced::Border {
                color: t.border,
                width: 1.0,
                radius: iced::border::Radius { top_left: 8.0, bottom_left: 8.0, ..Default::default() },
            },
            ..Default::default()
        });
    container(
        mouse_area(tab)
            .on_enter(Message::Queue(QueueMsg::EdgeEnter))
            .on_exit(Message::Queue(QueueMsg::EdgeExit)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(iced::alignment::Horizontal::Right)
    .align_y(iced::alignment::Vertical::Center)
    .padding(Padding { bottom: metrics::PLAYER_BAR_HEIGHT, ..Padding::ZERO })
    .into()
}

/// The drawer that slides in over the page: the same panels as the side panel.
fn drawer(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let width = state.config.settings.sidebar.queue_width;
    let top =
        if state.config.settings.window.custom_title_bar { super::title_bar::HEIGHT + 1.0 } else { 0.0 };
    let panel =
        container(stacked(state, width)).width(width).height(Length::Fill).style(move |_| container::Style {
            background: Some(iced::Background::Color(t.background_alt)),
            border: iced::Border { color: t.border, width: 1.0, radius: 0.0.into() },
            shadow: iced::Shadow {
                color: style::alpha(iced::Color::BLACK, 0.4),
                offset: iced::Vector::new(-4.0, 0.0),
                blur_radius: 18.0,
            },
            ..Default::default()
        });
    let area = mouse_area(iced::widget::opaque(panel))
        .on_enter(Message::Queue(QueueMsg::DrawerEnter))
        .on_exit(Message::Queue(QueueMsg::DrawerExit));
    container(area)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .padding(Padding { top, bottom: metrics::PLAYER_BAR_HEIGHT + 1.0, ..Padding::ZERO })
        .into()
}

/// The drawer, its edge strip and the toolbar's "…" menu over `root`.
pub fn overlay<'a>(state: &'a Listen, root: Element<'a, Message>) -> Element<'a, Message> {
    let mut out = stack![root];
    if state.queue_edge_visible() {
        out = out.push(edge_strip(state));
    }
    if state.queue_drawer_visible() {
        out = out.push(drawer(state));
    }
    if let Some((at, tier)) = state.queue_ui.more_menu {
        let close = Message::Queue(QueueMsg::MoreMenu(None));
        let at = super::popover::place(at, iced::Size::new(260.0, 200.0), state.window_size);
        let backdrop = mouse_area(space().width(Length::Fill).height(Length::Fill))
            .on_press(close.clone())
            .on_right_press(close);
        out = out.push(backdrop).push(iced::widget::pin(more_menu(state, tier)).position(at));
    }
    out.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toolbar_folds_step_by_step() {
        assert_eq!(overflow_tier(600.0), 0);
        assert_eq!(overflow_tier(480.0), 1);
        assert_eq!(overflow_tier(380.0), 2);
        assert_eq!(overflow_tier(300.0), 3);
        assert_eq!(overflow_tier(120.0), 3);
    }

    #[test]
    fn the_more_menu_holds_exactly_what_the_toolbar_folded() {
        for sync in [false, true] {
            for tier in 0..=3 {
                let mut both = unfolded(tier, sync);
                both.extend(folded(tier, sync));
                let all: Vec<QueueTool> =
                    QueueTool::ALL.into_iter().filter(|x| sync || !x.needs_sync()).collect();
                assert_eq!(both.len(), all.len(), "tier {tier}, sync {sync}: a tool is lost or doubled");
                assert!(all.iter().all(|x| both.contains(x)), "tier {tier}, sync {sync}");
            }
        }
        assert_eq!(folded(1, true), [QueueTool::Save, QueueTool::Restore]);
        assert_eq!(folded(2, false), [QueueTool::Jump, QueueTool::Create]);
        assert_eq!(unfolded(3, true), []);
    }

    #[test]
    fn the_more_button_shows_only_when_something_is_folded() {
        assert!(folded(0, true).is_empty());
        assert!(folded(1, false).is_empty(), "tier 1 without sync must not offer an empty menu");
        assert!(!folded(1, true).is_empty());
        assert!(!folded(2, false).is_empty());
    }

    #[test]
    fn a_panel_order_always_has_each_panel_once() {
        let mut o: Vec<String> = vec!["visualizer".into(), "visualizer".into(), "nonsense".into()];
        normalize_order(&mut o);
        assert_eq!(o, ["visualizer", "queue", "lyrics"]);
        let mut empty = Vec::new();
        normalize_order(&mut empty);
        assert_eq!(empty, ["queue", "lyrics", "visualizer"]);
    }

    #[test]
    fn identity_rows_are_their_positions() {
        assert_eq!(identity(3), [QRow(0), QRow(1), QRow(2)]);
        assert!(identity(0).is_empty());
    }
}
