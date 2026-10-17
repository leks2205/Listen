//! The reusable item list: virtualized table, card grid and (albums) detail list, with
//! selection, keyboard navigation, play controls, drag sources, configurable columns and
//! cards, pagination and a remembered scroll position (Feishin `components/item-list/*`).
//!
//! # Parts
//!
//! | file | what |
//! | --- | --- |
//! | `app/list.rs` | state per list ([`ListId`]), the [`ListMsg`] messages, the play actions |
//! | `selection.rs` | the selection state machine (click, Ctrl, Shift, keys) and the selection bar |
//! | `columns.rs` | [`ListItem`] (what each column shows for a kind of row) and the shared cells |
//! | `table.rs`, `grid.rs`, `detail.rs` | the three display types, all virtualized |
//! | `header.rs` | the table header (resize / reorder), the display toggle and the options popover |
//! | `hotkeys.rs` | the fixed keys; the configurable list hotkeys are handled in `app/list.rs` |
//! | `anchor.rs` | a widget that reports where it was clicked (popovers next to buttons) |
//! | `crates/listen-core/src/settings/lists.rs` | the saved look: columns, row size, card size, pagination, `PlayBehavior` |
//!
//! # Adopting the list on a page
//!
//! References: the Tracks (`ui/songs.rs`) and Albums (`ui/albums.rs`) pages.
//!
//! 1. Add a [`ListId`] variant (and a [`ListKind`](listen_core::settings::ListKind) in
//!    `ListId::kind` for its own saved look); map the route in `ListId::of_route` to enable
//!    the keys and the scroll restore.
//! 2. Return the rows from `Listen::list_data` as `ListData::Songs(Cow<[Song]>)` / `Albums` /
//!    `Artists` / `Playlists`; every action resolves row indices through it. Call
//!    `Listen::lists.reset(&id)` when the rows are replaced.
//! 3. Show it:
//!
//!    ```ignore
//!    pub fn view(state: &Listen) -> Element<'_, Message> {
//!        ListView::new(state, ListId::Songs, &state.library.songs)
//!            .title("Tracks")
//!            .subtitle(plural(state.library.songs.len(), "track"))
//!            .loading(state.library.songs_loading)
//!            .done(state.library.songs_done)
//!            .view()
//!    }
//!    ```
//!
//!    `.extra(element)` adds header widgets; `.no_header()` drops the header (see
//!    [`header::toolbar`]). Columns come from `ListKind::default_config`; other row types
//!    implement [`ListItem`]. `Listen::play_items(picks, behavior)` plays or queues rows.
//! 4. Add an arm to `Listen::list_visible` that requests the covers of
//!    `Listen::list_range(&id, total)` and loads the next page near the end (`songs_visible`).
//!
//! # Drag and drop
//!
//! A row (or the selection) dragged past 6 px puts a [`DragPayload`](crate::app::list::DragPayload)
//! in `Listen::lists.drag`. Targets wrap their widget in [`drop_target`] with a [`DropTarget`];
//! releasing over one sends `ListMsg::Drop`, handled by `Listen::drop_on`.

pub mod album_group;
pub mod anchor;
pub mod columns;
pub mod dates;
pub mod detail;
pub mod grid;
pub mod header;
pub mod hotkeys;
pub mod selection;
pub mod table;

use crate::app::list::{DropTarget, ListId, ListMenu, ListMsg, ListState};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{icon, label, label_w};
use columns::ListItem;
use iced::font::Weight;
use iced::widget::{Column, Row, button, container, mouse_area, pin, responsive, row, space, stack};
use iced::{Alignment, Element, Length, Padding};
use listen_core::settings::{DisplayType, ListConfig, Pagination, PlayBehavior};
use std::rc::Rc;

/// `[start, end)` of the rows on the shown page (all rows when not paginated).
pub fn page_bounds(config: &ListConfig, page: usize, total: usize) -> (usize, usize) {
    match config.pagination {
        Pagination::Infinite => (0, total),
        Pagination::Paginated => {
            let per = config.items_per_page.max(1) as usize;
            let start = (page * per).min(total);
            (start, (start + per).min(total))
        }
    }
}

/// Number of pages (1 when not paginated).
pub fn page_count(config: &ListConfig, total: usize) -> usize {
    match config.pagination {
        Pagination::Infinite => 1,
        Pagination::Paginated => total.div_ceil(config.items_per_page.max(1) as usize).max(1),
    }
}

/// The offset that shows the item at `top` .. `top + height` fully, if it is not.
pub fn scroll_into_view(offset: f32, viewport: f32, top: f32, height: f32) -> Option<f32> {
    if viewport <= 0.0 {
        return None;
    }
    if top < offset {
        Some(top.max(0.0))
    } else if top + height > offset + viewport {
        Some((top + height - viewport).max(0.0))
    } else {
        None
    }
}

/// The message for a scroll of a list's scrollable.
pub fn scrolled(id: &ListId, v: &iced::widget::scrollable::Viewport) -> Message {
    Message::List(ListMsg::Scrolled(
        id.clone(),
        crate::app::ScrollState {
            offset: v.absolute_offset().y,
            height: v.bounds().height,
            width: v.bounds().width,
        },
    ))
}

fn divider_h<'a>(t: crate::style::Tokens) -> Element<'a, Message> {
    crate::ui::divider_h(t)
}

/// A row of the table that heads a run of rows (the "Disc 2" rows of an
/// album). It sits before row `before` and scrolls with the rows.
#[derive(Debug, Clone)]
pub struct RowGroup {
    pub before: usize,
    pub title: String,
    /// The checkbox state (all rows of the group are selected).
    pub checked: bool,
    /// Sent when the checkbox is toggled.
    pub on_toggle: Message,
    pub look: GroupLook,
}

impl RowGroup {
    /// The height of the row: 40 px, or the album header's own.
    pub fn height(&self) -> f32 {
        match &self.look {
            GroupLook::Album(meta) => meta.height,
            _ => table::GROUP_H,
        }
    }
}

/// How a [`RowGroup`] row looks.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum GroupLook {
    /// A checkbox that selects the group's rows (the discs of an album).
    #[default]
    Checkbox,
    /// A plain label ("Up Next" in the queue).
    Label,
    /// A marker in the accent colour with a microphone (where the AI DJ will talk).
    DjBreak,
    /// The header of an album in a song table with the album group column.
    Album(Rc<album_group::GroupMeta>),
}

type Piece<'a> = Rc<dyn Fn() -> Element<'a, Message> + 'a>;

/// What a page adds around the rows of a table: content above the header and
/// below the last row that scrolls with them, and group rows between them.
/// Only the table display type honours it.
#[derive(Clone, Default)]
pub struct Extras<'a> {
    /// (height, builder): the exact height is needed to virtualize the rows.
    pub lead: Option<(f32, Piece<'a>)>,
    pub tail: Option<(f32, Piece<'a>)>,
    pub groups: Rc<Vec<RowGroup>>,
}

/// What the three display types draw from. Built per layout (the size of the
/// body is only known then).
pub struct Body<'a, T> {
    pub state: &'a Listen,
    pub id: ListId,
    pub items: &'a [T],
    pub config: ListConfig,
    pub list: &'a ListState,
    pub size: iced::Size,
    pub loading: bool,
    /// The rows of the shown page: `start..end` of `items`.
    pub start: usize,
    pub end: usize,
    pub extras: Extras<'a>,
}

/// A list on a page: header, body and bars. See the module docs.
pub struct ListView<'a, T> {
    state: &'a Listen,
    id: ListId,
    items: &'a [T],
    title: Option<String>,
    subtitle: Option<String>,
    loading: bool,
    done: bool,
    header: bool,
    extra: Option<Element<'a, Message>>,
    empty: Option<String>,
    extras: Extras<'a>,
}

impl<'a, T: ListItem + 'a> ListView<'a, T> {
    pub fn new(state: &'a Listen, id: ListId, items: &'a [T]) -> Self {
        Self {
            state,
            id,
            items,
            title: None,
            subtitle: None,
            loading: false,
            done: true,
            header: true,
            extra: None,
            empty: None,
            extras: Extras::default(),
        }
    }

    /// Content above the table header that scrolls with the rows (`height` is exact).
    pub fn lead(mut self, height: f32, build: impl Fn() -> Element<'a, Message> + 'a) -> Self {
        self.extras.lead = Some((height, Rc::new(build)));
        self
    }

    /// Content below the last row that scrolls with the rows (`height` is exact).
    pub fn tail(mut self, height: f32, build: impl Fn() -> Element<'a, Message> + 'a) -> Self {
        self.extras.tail = Some((height, Rc::new(build)));
        self
    }

    /// Group rows in the table (see [`RowGroup`]).
    pub fn groups(mut self, groups: Vec<RowGroup>) -> Self {
        self.extras.groups = Rc::new(groups);
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The count line next to the title ("1415 tracks").
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        self.subtitle = Some(text.into());
        self
    }

    /// More rows are being loaded: shows "Loading…" below the rows.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Every row is loaded (the page count of a paginated list is final).
    pub fn done(mut self, done: bool) -> Self {
        self.done = done;
        self
    }

    /// Without the title row; the page draws its own header.
    pub fn no_header(mut self) -> Self {
        self.header = false;
        self
    }

    /// A widget in the header row, left of the display toggle.
    pub fn extra(mut self, element: impl Into<Element<'a, Message>>) -> Self {
        self.extra = Some(element.into());
        self
    }

    /// Text shown instead of the body when there are no rows (and nothing is loading).
    pub fn empty(mut self, text: impl Into<String>) -> Self {
        self.empty = Some(text.into());
        self
    }

    pub fn view(self) -> Element<'a, Message> {
        let state = self.state;
        let t = state.tokens;
        let id = self.id.clone();
        let config = state.config.settings.lists.config(id.kind());
        let list = state.lists.get(&id);
        let total = self.items.len();
        let (start, end) = page_bounds(&config, list.page, total);

        let mut main = Column::new();
        if self.header {
            let mut bar = Row::new().align_y(Alignment::Center).spacing(metrics::space_md());
            if let Some(title) = &self.title {
                bar = bar.push(label_w(title.clone(), 36.0, t.foreground, Weight::Bold));
            }
            bar = bar.push(space::horizontal());
            if let Some(sub) = &self.subtitle {
                bar = bar.push(label(sub.clone(), metrics::font_md(), t.muted));
            }
            if let Some(extra) = self.extra {
                bar = bar.push(extra);
            }
            bar = bar.push(header::toolbar(state, &id, &config));
            main = main.push(container(bar).padding(Padding {
                top: 24.0,
                right: metrics::PAGE_PADDING_X,
                bottom: 12.0,
                left: metrics::PAGE_PADDING_X,
            }));
        }

        let paginated = config.pagination == Pagination::Paginated;
        let page_bar = paginated.then(|| pagination_bar(state, &id, &config, list.page, total, self.done));
        let nothing = total == 0 && !self.loading;
        if let (true, Some(text)) = (nothing, &self.empty) {
            main = main.push(container(label(text.clone(), metrics::font_md(), t.muted)).padding(48));
        } else {
            let (items, loading, extras) = (self.items, self.loading, self.extras);
            let body = responsive(move |size| {
                let body = Body {
                    state,
                    id: id.clone(),
                    items,
                    config: config.clone(),
                    list: state.lists.get(&id),
                    size,
                    loading,
                    start,
                    end,
                    extras: extras.clone(),
                };
                match body.config.display {
                    DisplayType::Grid => grid::view(&body),
                    DisplayType::Detail => T::detail(&body).unwrap_or_else(|| table::view(&body)),
                    DisplayType::Table => table::view(&body),
                }
            });
            main = main.push(container(body).width(Length::Fill).height(Length::Fill));
        }
        if let Some(bar) = page_bar {
            main = main.push(bar);
        }

        // Always a stack, so the selection bar appearing keeps the scroll position.
        let mut out = stack![main.width(Length::Fill).height(Length::Fill)];
        if let Some(bar) = selection::bar(state, &self.id, list.selection.len()) {
            out = out.push(bar);
        }
        out.into()
    }
}

/// "‹ 2 / 14 ›" under a paginated list.
fn pagination_bar<'a>(
    state: &Listen,
    id: &ListId,
    config: &ListConfig,
    page: usize,
    total: usize,
    done: bool,
) -> Element<'a, Message> {
    let t = state.tokens;
    let pages = page_count(config, total);
    let page = page.min(pages - 1);
    let go = |to: usize| Message::List(ListMsg::SetPage(id.clone(), to));
    let nav = |glyph: Icon, to: usize, enabled: bool, name: &str| {
        crate::ui::tip::tip(
            state,
            button(icon(glyph, metrics::font_lg(), if enabled { t.foreground } else { t.muted }))
                .padding(6)
                .style(style::icon_button(t, false))
                .on_press_maybe(enabled.then(|| go(to))),
            name,
            None,
            crate::ui::tip::Position::Top,
        )
    };
    let (start, end) = page_bounds(config, page, total);
    let more = if done { "" } else { "+" };
    container(
        row![
            label(
                if total == 0 {
                    "No items".to_string()
                } else {
                    format!("{}–{} of {total}{more}", start + 1, end)
                },
                metrics::font_sm(),
                t.muted
            ),
            space::horizontal(),
            nav(Icon::LuChevronLeft, page.saturating_sub(1), page > 0, "Previous page"),
            label(format!("Page {} of {pages}{more}", page + 1), metrics::font_sm(), t.foreground),
            nav(Icon::LuChevronRight, page + 1, page + 1 < pages || !done, "Next page"),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([8.0, metrics::PAGE_PADDING_X]))
    .into()
}

// ---- drop targets ---------------------------------------------------------------

/// `content` as a place rows dragged from a list can be dropped on.
pub fn drop_target<'a>(content: impl Into<Element<'a, Message>>, target: DropTarget) -> Element<'a, Message> {
    mouse_area(content).on_release(Message::List(ListMsg::Drop(target))).into()
}

// ---- overlay: popovers and the drag label --------------------------------------

const MENU_W: f32 = 232.0;
/// The play popover over a row's play button.
const POP_W: f32 = 116.0;
const POP_H: f32 = 44.0;
const ITEM: f32 = 34.0;

fn menu_item<'a>(state: &Listen, glyph: Icon, title: &'static str, msg: Message) -> Element<'a, Message> {
    let t = state.tokens;
    button(
        container(
            row![
                icon(glyph, metrics::font_lg(), t.foreground),
                label_w(title, metrics::font_md(), t.foreground, Weight::Medium)
            ]
            .spacing(metrics::space_md())
            .align_y(Alignment::Center),
        )
        .center_y(Length::Fill),
    )
    .width(Length::Fill)
    .height(ITEM)
    .padding(Padding::from([0.0, metrics::space_md()]))
    .style(style::nav_item(t, false))
    .on_press(msg)
    .into()
}

fn boxed<'a>(state: &Listen, body: Element<'a, Message>) -> Element<'a, Message> {
    let t = state.tokens;
    iced::widget::opaque(container(body).width(MENU_W).padding(4).style(move |_| container::Style {
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

/// Play now / next / last.
fn play_items<'a>(state: &Listen, make: impl Fn(PlayBehavior) -> Message) -> Vec<Element<'a, Message>> {
    vec![
        menu_item(state, Icon::LuPlay, "Play now", make(PlayBehavior::Now)),
        menu_item(state, Icon::LuSkipForward, "Play next", make(PlayBehavior::Next)),
        menu_item(state, Icon::RiPlayListAddLine, "Play last", make(PlayBehavior::Last)),
    ]
}

/// The popovers of the lists (selection menu, row play options, options panel)
/// and the label that follows a drag. `root` stays the first child.
pub fn overlay<'a>(state: &'a Listen, root: Element<'a, Message>) -> Element<'a, Message> {
    let lists = &state.lists;
    let mut out = stack![root];
    if let Some(menu) = &lists.menu {
        let close = Message::List(ListMsg::CloseMenu);
        let (panel, at, size): (Element<'a, Message>, iced::Point, iced::Size) = match menu {
            ListMenu::Play { list, index, at } => {
                let (id, index) = (list.clone(), *index);
                let items = play_items(state, |b| Message::List(ListMsg::PlayRow(id.clone(), index, b)));
                let height = items.len() as f32 * ITEM + 8.0;
                let col = items.into_iter().fold(Column::new(), |c, i| c.push(i));
                (boxed(state, col.into()), *at, iced::Size::new(MENU_W, height))
            }
            ListMenu::Config { list, at } => {
                (header::config_panel(state, list), *at, iced::Size::new(header::PANEL_W, header::PANEL_H))
            }
        };
        let at = crate::ui::popover::place(at, size, state.window_size);
        let backdrop = mouse_area(space().width(Length::Fill).height(Length::Fill))
            .on_press(close.clone())
            .on_right_press(close);
        out = out.push(backdrop).push(pin(panel).position(at));
    }
    if let Some(pop) = lists.hover_pop.as_ref().filter(|p| p.open) {
        let t = state.tokens;
        let (id, index) = (pop.list.clone(), pop.index);
        let behavior = |b: PlayBehavior| Message::List(ListMsg::PlayRow(id.clone(), index, b));
        let btn = |glyph: Icon, msg: Message, name: &str, action: &str| {
            crate::ui::tip::tip(
                state,
                button(icon(glyph, metrics::font_lg(), t.foreground))
                    .padding(6)
                    .style(style::icon_button(t, false))
                    .on_press(msg),
                name,
                Some(action),
                crate::ui::tip::Position::Bottom,
            )
        };
        let controls = row![
            btn(Icon::LuPlay, behavior(PlayBehavior::Now), "Play now", "listPlayNow"),
            btn(Icon::LuSkipForward, behavior(PlayBehavior::Next), "Play next", "listPlayNext"),
            btn(Icon::RiPlayListAddLine, behavior(PlayBehavior::Last), "Play last", "listPlayLast"),
        ]
        .spacing(2);
        let panel = mouse_area(container(controls).padding(4).style(move |_| container::Style {
            background: Some(iced::Background::Color(t.background)),
            border: iced::border::rounded(metrics::radius_md()).color(t.border).width(2),
            shadow: iced::Shadow {
                color: style::alpha(iced::Color::BLACK, 0.4),
                offset: iced::Vector::new(2.0, 2.0),
                blur_radius: 10.0,
            },
            ..Default::default()
        }))
        .on_enter(Message::List(ListMsg::PlayHoverKeep))
        .on_exit(Message::List(ListMsg::PlayHoverLeave));
        // Centred above the button.
        let size = iced::Size::new(POP_W, POP_H);
        let at = iced::Point::new(
            (pop.rect.center_x() - size.width / 2.0)
                .clamp(4.0, (state.window_size.width - size.width - 4.0).max(4.0)),
            (pop.rect.y - size.height - 6.0).max(4.0),
        );
        out = out.push(pin(iced::widget::opaque(panel)).position(at));
    }
    if let Some(drag) = &lists.drag {
        let t = state.tokens;
        let tag =
            container(label_w(drag.payload.label(), metrics::font_sm(), iced::Color::WHITE, Weight::Medium))
                .padding(Padding::from([4.0, 10.0]))
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(t.primary)),
                    border: iced::border::rounded(999),
                    ..Default::default()
                });
        out = out.push(pin(tag).position(iced::Point::new(drag.at.x + 14.0, drag.at.y + 14.0)));
    }
    out.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::settings::ListKind;

    #[test]
    fn pages_and_bounds() {
        let mut c = ListKind::Songs.default_config();
        assert_eq!(page_bounds(&c, 3, 250), (0, 250));
        assert_eq!(page_count(&c, 250), 1);
        c.pagination = Pagination::Paginated;
        c.items_per_page = 100;
        assert_eq!(page_count(&c, 250), 3);
        assert_eq!(page_bounds(&c, 0, 250), (0, 100));
        assert_eq!(page_bounds(&c, 2, 250), (200, 250));
        // A page past the end is empty, not a panic.
        assert_eq!(page_bounds(&c, 9, 250), (250, 250));
        assert_eq!(page_count(&c, 0), 1);
    }

    #[test]
    fn scrolls_only_when_the_row_is_out_of_view() {
        // Fully visible: stays.
        assert_eq!(scroll_into_view(100.0, 400.0, 200.0, 64.0), None);
        // Below the bottom edge: its bottom meets the bottom of the view.
        assert_eq!(scroll_into_view(0.0, 400.0, 384.0, 64.0), Some(48.0));
        // Above the top edge: its top meets the top.
        assert_eq!(scroll_into_view(500.0, 400.0, 436.0, 64.0), Some(436.0));
        assert_eq!(scroll_into_view(0.0, 0.0, 10.0, 64.0), None);
    }
}
