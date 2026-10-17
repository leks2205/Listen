//! The table header (resizable, reorderable columns), the list toolbar and options popover, and the
//! list page header.

use super::Body;
use super::anchor::anchored;
use super::columns::{ListItem, sized};
use super::table::Shape;
use crate::app::list::{ConfigEdit, HPart, ListId, ListMsg};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::tip::{Position, tip};
use crate::ui::{icon, label, label_w};
use iced::font::Weight;
use iced::widget::{Column, Row, button, checkbox, container, mouse_area, row, scrollable, space};
use iced::{Alignment, Element, Length, Padding};
use listen_core::settings::{
    Align, CardSize, ColumnConfig, ColumnId, DisplayType, ListConfig, Pagination, Pin, RowSize,
};
use std::ops::Range;

const HEADER_H: f32 = 40.0;
const RESIZE_W: f32 = 6.0;

/// The label a column has in the header (icons for the narrow ones).
fn header_label<'a>(state: &Listen, col: ColumnId) -> Element<'a, Message> {
    let t = state.tokens;
    match col {
        ColumnId::UserFavorite => icon(Icon::RiHeartLine, metrics::font_md(), t.muted),
        ColumnId::Image | ColumnId::Actions => space().width(0).into(),
        other => label_w(other.label(), metrics::font_sm(), t.muted, Weight::Medium).into(),
    }
}

/// The header row: titles drag to reorder, right edges resize; pinned columns split it like the rows.
pub fn table_header<'a, T: ListItem>(body: &Body<'a, T>, shape: &Shape) -> Element<'a, Message> {
    let t = body.state.tokens;
    let drag = body.state.lists.column_drag.as_ref().filter(|d| d.list == body.id);
    let cells_of = |range: Range<usize>| {
        let mut cells = Row::new().align_y(Alignment::Center);
        for n in range {
            let (col, width) = (&shape.columns[n], shape.widths[n]);
            let target = drag.is_some_and(|d| d.over == Some(col.id) && d.column != col.id);
            let title = sized(header_label(body.state, col.id), (width - RESIZE_W).max(10.0), col.align);
            let handle = mouse_area(container(space()).width(RESIZE_W).height(HEADER_H - 12.0))
                .interaction(iced::mouse::Interaction::ResizingHorizontally)
                .on_press(Message::List(ListMsg::ColumnResizeStart(body.id.clone(), col.id)));
            let cell = container(row![title, handle].align_y(Alignment::Center))
                .width(Length::Fixed(width))
                .height(HEADER_H)
                .center_y(HEADER_H)
                .style(move |_| container::Style {
                    border: iced::Border::default(),
                    background: target.then(|| iced::Background::Color(style::alpha(t.primary, 0.18))),
                    ..Default::default()
                });
            cells = cells.push(
                mouse_area(cell)
                    .on_press(Message::List(ListMsg::ColumnDragStart(body.id.clone(), col.id)))
                    .on_enter(Message::List(ListMsg::ColumnDragOver(col.id))),
            );
        }
        cells
    };
    if !shape.split {
        return container(cells_of(0..shape.columns.len())).height(HEADER_H).center_y(HEADER_H).into();
    }
    let seam = || container(space()).width(1).height(HEADER_H).style(style::divider(t));
    let [l, m, r] = shape.blocks();
    let mut out = Row::new().height(HEADER_H).align_y(Alignment::Center);
    if !l.is_empty() {
        out = out.push(container(cells_of(l.clone())).width(shape.block_width(&l))).push(seam());
    }
    out = out.push(super::table::hscroll(&body.id, HPart::Header, cells_of(m), t));
    if !r.is_empty() {
        out = out.push(seam()).push(container(cells_of(r.clone())).width(shape.block_width(&r)));
    }
    out.into()
}

// ---- toolbar -----------------------------------------------------------------

fn display_icon(d: DisplayType) -> Icon {
    match d {
        DisplayType::Table => Icon::LuTable,
        DisplayType::Grid => Icon::LuLayoutGrid,
        DisplayType::Detail => Icon::LuList,
    }
}

/// Display type toggle and the options button. Put it into a page header.
pub fn toolbar<'a>(state: &'a Listen, id: &ListId, config: &ListConfig) -> Element<'a, Message> {
    let t = state.tokens;
    let mut out = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    let types = id.kind().item_type().display_types();
    if types.len() > 1 {
        for d in types {
            let active = config.display == *d;
            let name = match d {
                DisplayType::Table => "Table view",
                DisplayType::Grid => "Grid view",
                DisplayType::Detail => "Detail view",
            };
            out = out.push(tip(
                state,
                button(icon(display_icon(*d), metrics::font_lg(), if active { t.primary } else { t.muted }))
                    .padding(6)
                    .style(style::icon_button(t, active))
                    .on_press(Message::List(ListMsg::Config(id.clone(), ConfigEdit::Display(*d)))),
                name,
                None,
                Position::Bottom,
            ));
        }
    }
    let idc = id.clone();
    out = out.push(anchored(
        tip(
            state,
            button(icon(Icon::LuSlidersHorizontal, metrics::font_lg(), t.muted))
                .padding(6)
                .style(style::icon_button(t, false))
                .on_press(Message::Noop),
            "List options",
            None,
            Position::Bottom,
        ),
        move |b| Message::List(ListMsg::ConfigMenu(idc.clone(), iced::Point::new(b.x, b.y + b.height + 4.0))),
    ));
    out.into()
}

// ---- options popover -----------------------------------------------------------

pub const PANEL_W: f32 = 380.0;
pub const PANEL_H: f32 = 560.0;

/// Segmented choice: one button per option, the active one filled.
fn segmented<'a, V: Copy + PartialEq + 'a>(
    state: &Listen,
    options: &[(&'static str, V)],
    current: V,
    msg: impl Fn(V) -> Message + 'a,
) -> Element<'a, Message> {
    let t = state.tokens;
    let mut r = Row::new().spacing(metrics::space_xs());
    for (name, value) in options {
        let active = *value == current;
        r = r.push(
            button(label_w(
                *name,
                metrics::font_sm(),
                if active { iced::Color::WHITE } else { t.foreground },
                Weight::Medium,
            ))
            .padding(Padding::from([4.0, 10.0]))
            .style(move |theme, status| {
                if active {
                    style::primary_button(t)(theme, status)
                } else {
                    style::default_button(t)(theme, status)
                }
            })
            .on_press(msg(*value)),
        );
    }
    r.into()
}

fn check<'a>(label_text: &'static str, on: bool, msg: impl Fn(bool) -> Message + 'a) -> Element<'a, Message> {
    checkbox(on).label(label_text).size(16).text_size(metrics::font_sm()).on_toggle(msg).into()
}

fn small<'a>(
    state: &Listen,
    content: Element<'a, Message>,
    active: bool,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let t = state.tokens;
    button(content)
        .padding(Padding::from([3.0, 6.0]))
        .style(style::icon_button(t, active))
        .on_press_maybe(msg)
        .into()
}

fn section_title<'a>(state: &Listen, s: &'static str) -> Element<'a, Message> {
    label_w(s, metrics::font_sm(), state.tokens.muted, Weight::Semibold).into()
}

fn align_icon(a: Align) -> Icon {
    match a {
        Align::Start => Icon::LuAlignLeft,
        Align::Center => Icon::LuAlignCenter,
        Align::End => Icon::LuAlignRight,
    }
}

fn stepper<'a>(state: &Listen, value: String, down: Message, up: Message) -> Element<'a, Message> {
    let t = state.tokens;
    row![
        small(state, label("−", metrics::font_md(), t.foreground).into(), false, Some(down)),
        container(label(value, metrics::font_sm(), t.foreground)).width(36).center_x(36),
        small(state, label("+", metrics::font_md(), t.foreground).into(), false, Some(up)),
    ]
    .align_y(Alignment::Center)
    .into()
}

/// One column of the picker: checkbox, move up / down, pin left / right, alignment, fit.
fn column_line<'a>(
    state: &Listen,
    id: &ListId,
    c: &ColumnConfig,
    first: bool,
    last: bool,
) -> Element<'a, Message> {
    let t = state.tokens;
    let cfg = |e: ConfigEdit| Message::List(ListMsg::Config(id.clone(), e));
    let col = c.id;
    let pin = |p: Pin, name: &'static str| {
        let on = c.pinned == Some(p);
        small(
            state,
            label(name, metrics::font_xs(), if on { t.primary } else { t.muted }).into(),
            on,
            Some(cfg(ConfigEdit::ColumnPin(col, if on { None } else { Some(p) }))),
        )
    };
    row![
        container(check(col.picker_label(), c.enabled, {
            let id = id.clone();
            move |on| Message::List(ListMsg::Config(id.clone(), ConfigEdit::ColumnEnabled(col, on)))
        }))
        .width(Length::Fill),
        small(
            state,
            icon(Icon::LuArrowUp, metrics::font_sm(), t.muted),
            false,
            (!first).then(|| cfg(ConfigEdit::ColumnShift(col, -1)))
        ),
        small(
            state,
            icon(Icon::LuArrowDown, metrics::font_sm(), t.muted),
            false,
            (!last).then(|| cfg(ConfigEdit::ColumnShift(col, 1)))
        ),
        pin(Pin::Left, "L"),
        pin(Pin::Right, "R"),
        small(
            state,
            icon(align_icon(c.align), metrics::font_sm(), t.muted),
            false,
            Some(cfg(ConfigEdit::ColumnAlign(col, c.align.next())))
        ),
        small(
            state,
            label("Fit", metrics::font_xs(), if c.auto_size { t.primary } else { t.muted }).into(),
            c.auto_size,
            Some(cfg(ConfigEdit::ColumnAutoSize(col, !c.auto_size)))
        ),
    ]
    .spacing(metrics::space_xs())
    .align_y(Alignment::Center)
    .into()
}

/// Options of the album group header, as in Feishin's `table-config.tsx` "album group".
pub fn album_group_options<'a>(state: &Listen, id: &ListId) -> Element<'a, Message> {
    let t = state.tokens;
    let group = &state.config.settings.lists.album_group;
    let cfg = |e: ConfigEdit| Message::List(ListMsg::Config(id.clone(), e));
    let mut col = Column::new().spacing(metrics::space_sm()).push(section_title(state, "Album group"));
    let sizes: Vec<(&'static str, u32)> =
        vec![("No cover", 0), ("64", 64), ("96", 96), ("128", 128), ("160", 160)];
    col = col.push(segmented(state, &sizes, group.image_size, {
        let id = id.clone();
        move |px| Message::List(ListMsg::Config(id.clone(), ConfigEdit::GroupImageSize(px)))
    }));
    let n = group.items.len();
    for (i, e) in group.items.iter().enumerate() {
        let item = e.item;
        col = col.push(
            row![
                container(check(item.label(), e.enabled, {
                    let id = id.clone();
                    move |on| {
                        Message::List(ListMsg::Config(id.clone(), ConfigEdit::GroupItemEnabled(item, on)))
                    }
                }))
                .width(Length::Fill),
                small(
                    state,
                    icon(Icon::LuArrowUp, metrics::font_sm(), t.muted),
                    false,
                    (i > 0).then(|| cfg(ConfigEdit::GroupItemShift(item, -1)))
                ),
                small(
                    state,
                    icon(Icon::LuArrowDown, metrics::font_sm(), t.muted),
                    false,
                    (i + 1 < n).then(|| cfg(ConfigEdit::GroupItemShift(item, 1)))
                ),
            ]
            .spacing(metrics::space_xs())
            .align_y(Alignment::Center),
        );
    }
    col = col
        .push(check("Favorite and rating", group.show_favorite_rating, {
            let id = id.clone();
            move |on| Message::List(ListMsg::Config(id.clone(), ConfigEdit::GroupFavoriteRating(on)))
        }))
        .push(check("Cover above the facts", group.vertical_layout, {
            let id = id.clone();
            move |on| Message::List(ListMsg::Config(id.clone(), ConfigEdit::GroupVertical(on)))
        }));
    col.into()
}

pub fn config_panel<'a>(state: &'a Listen, id: &ListId) -> Element<'a, Message> {
    let t = state.tokens;
    let config = state.config.settings.lists.config(id.kind());
    let cfg = |e: ConfigEdit| Message::List(ListMsg::Config(id.clone(), e));
    let mut col = Column::new().spacing(metrics::space_sm()).push(label_w(
        "List options",
        metrics::font_lg(),
        t.foreground,
        Weight::Bold,
    ));

    let types = id.kind().item_type().display_types();
    if types.len() > 1 {
        let options: Vec<(&'static str, DisplayType)> = types.iter().map(|d| (d.label(), *d)).collect();
        col = col.push(section_title(state, "Display")).push(segmented(state, &options, config.display, {
            let id = id.clone();
            move |d| Message::List(ListMsg::Config(id.clone(), ConfigEdit::Display(d)))
        }));
    }

    if config.display == DisplayType::Grid {
        let rows: Vec<(&'static str, CardSize)> = CardSize::ALL.iter().map(|s| (s.label(), *s)).collect();
        col = col.push(section_title(state, "Card size")).push(segmented(state, &rows, config.grid.size, {
            let id = id.clone();
            move |s| Message::List(ListMsg::Config(id.clone(), ConfigEdit::CardSize(s)))
        }));
        col = col.push(
            row![
                label("Gap", metrics::font_sm(), t.foreground),
                space::horizontal(),
                stepper(
                    state,
                    format!("{:?}", config.grid.gap).to_lowercase(),
                    cfg(ConfigEdit::Gap(config.grid.gap.smaller())),
                    cfg(ConfigEdit::Gap(config.grid.gap.larger())),
                ),
            ]
            .align_y(Alignment::Center),
        );
        col = col.push(
            row![
                check("Cards per row", config.grid.items_per_row_enabled, {
                    let id = id.clone();
                    move |v| Message::List(ListMsg::Config(id.clone(), ConfigEdit::ItemsPerRowEnabled(v)))
                }),
                space::horizontal(),
                stepper(
                    state,
                    config.grid.items_per_row.to_string(),
                    cfg(ConfigEdit::ItemsPerRow(config.grid.items_per_row.saturating_sub(1))),
                    cfg(ConfigEdit::ItemsPerRow(config.grid.items_per_row + 1)),
                ),
            ]
            .align_y(Alignment::Center),
        );
        col = col.push(section_title(state, "Lines under a card"));
        let n = config.grid.rows.len();
        for (i, r) in config.grid.rows.iter().enumerate() {
            let rid = r.id;
            col = col.push(
                row![
                    container(check(r.id.picker_label(), r.enabled, {
                        let id = id.clone();
                        move |on| {
                            Message::List(ListMsg::Config(id.clone(), ConfigEdit::CardRowEnabled(rid, on)))
                        }
                    }))
                    .width(Length::Fill),
                    small(
                        state,
                        icon(Icon::LuArrowUp, metrics::font_sm(), t.muted),
                        false,
                        (i > 0).then(|| cfg(ConfigEdit::CardRowShift(rid, -1)))
                    ),
                    small(
                        state,
                        icon(Icon::LuArrowDown, metrics::font_sm(), t.muted),
                        false,
                        (i + 1 < n).then(|| cfg(ConfigEdit::CardRowShift(rid, 1)))
                    ),
                    small(
                        state,
                        icon(align_icon(r.align), metrics::font_sm(), t.muted),
                        false,
                        Some(cfg(ConfigEdit::CardRowAlign(rid, r.align.next())))
                    ),
                ]
                .spacing(metrics::space_xs())
                .align_y(Alignment::Center),
            );
        }
    } else {
        let sizes: Vec<(&'static str, RowSize)> = RowSize::ALL.iter().map(|s| (s.label(), *s)).collect();
        col = col.push(section_title(state, "Row size")).push(segmented(state, &sizes, config.table.size, {
            let id = id.clone();
            move |s| Message::List(ListMsg::Config(id.clone(), ConfigEdit::RowSize(s)))
        }));
        let toggle = |name: &'static str, on: bool, f: fn(bool) -> ConfigEdit| {
            let id = id.clone();
            check(name, on, move |v| Message::List(ListMsg::Config(id.clone(), f(v))))
        };
        col = col
            .push(toggle("Show header", config.table.header, ConfigEdit::Header))
            .push(toggle(
                "Highlight the row under the pointer",
                config.table.row_hover_highlight,
                ConfigEdit::RowHover,
            ))
            .push(toggle("Alternate row colors", config.table.alternate_rows, ConfigEdit::AlternateRows))
            .push(toggle(
                "Horizontal borders",
                config.table.horizontal_borders,
                ConfigEdit::HorizontalBorders,
            ))
            .push(toggle("Vertical borders", config.table.vertical_borders, ConfigEdit::VerticalBorders))
            .push(toggle("Fit columns to the width", config.table.auto_fit, ConfigEdit::AutoFit));
        col = col.push(section_title(state, "Columns"));
        let n = config.table.columns.len();
        for (i, c) in config.table.columns.iter().enumerate() {
            col = col.push(column_line(state, id, c, i == 0, i + 1 == n));
        }
        if config.table.column(ColumnId::AlbumGroup).is_some_and(|c| c.enabled) {
            col = col.push(album_group_options(state, id));
        }
    }

    let kinds = [("Infinite scroll", Pagination::Infinite), ("Pages", Pagination::Paginated)];
    col = col.push(section_title(state, "Loading")).push(segmented(state, &kinds, config.pagination, {
        let id = id.clone();
        move |p| Message::List(ListMsg::Config(id.clone(), ConfigEdit::Pagination(p)))
    }));
    if config.pagination == Pagination::Paginated {
        col = col.push(
            row![
                label("Items per page", metrics::font_sm(), t.foreground),
                space::horizontal(),
                stepper(
                    state,
                    config.items_per_page.to_string(),
                    cfg(ConfigEdit::ItemsPerPage(config.items_per_page.saturating_sub(25))),
                    cfg(ConfigEdit::ItemsPerPage(config.items_per_page + 25)),
                ),
            ]
            .align_y(Alignment::Center),
        );
    }
    col = col.push(
        button(label_w("Reset to defaults", metrics::font_sm(), t.foreground, Weight::Medium))
            .padding(Padding::from([6.0, 12.0]))
            .style(style::default_button(t))
            .on_press(cfg(ConfigEdit::Reset)),
    );

    let boxed = container(
        scrollable(container(col).padding(14))
            .style(style::scrollbar(t))
            .height(Length::Fixed(PANEL_H - 8.0)),
    )
    .width(PANEL_W)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(t.background_alt)),
        border: iced::border::rounded(metrics::radius_md()).color(t.border).width(1),
        shadow: iced::Shadow {
            color: style::alpha(iced::Color::BLACK, 0.45),
            offset: iced::Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..Default::default()
    });
    iced::widget::opaque(boxed)
}

// ---- page header: title, play, sort, filters, search -------------------------------
// Feishin `album-list-header.tsx` / `song-list-header.tsx`; state in `app/list_query.rs`.

use crate::app::list_query::{
    Exclusive, FilterEdit, ListQueryMsg, Popup, has_collections, has_controls, is_songs,
};
use crate::ui::overlay::ModalParts;
use iced::widget::{column, text_input};
use listen_core::settings::{CollectionList, PlayBehavior};
use listen_source::{QueryFilters, SortOrder};

const SORT_W: f32 = 220.0;
const SORT_ITEM: f32 = 32.0;
const COLLECTIONS_W: f32 = 300.0;

fn query(msg: ListQueryMsg) -> Message {
    Message::ListQuery(msg)
}

/// Id of the search box of a list (so a hotkey could focus it).
pub fn search_id(id: &ListId) -> iced::widget::Id {
    iced::widget::Id::from(format!("list-search-{id:?}"))
}

fn tool<'a>(
    state: &Listen,
    glyph: Icon,
    active: bool,
    msg: Message,
    name: &str,
    action: Option<&str>,
) -> Element<'a, Message> {
    let t = state.tokens;
    tip(
        state,
        button(icon(glyph, metrics::font_lg(), if active { t.primary } else { t.muted }))
            .padding(6)
            .style(style::icon_button(t, active))
            .on_press(msg),
        name,
        action,
        Position::Bottom,
    )
}

/// A list page header: title, count, Play buttons, sort / filter / search controls, display toggle.
/// Pages using it draw the list with `ListView::no_header`.
pub fn page_header<'a>(
    state: &'a Listen,
    id: &ListId,
    title: &'static str,
    count: String,
) -> Element<'a, Message> {
    page_header_with(state, id, title.to_string(), count, None)
}

/// [`page_header`] with an owned title and an extra widget next to the Play buttons.
pub fn page_header_with<'a>(
    state: &'a Listen,
    id: &ListId,
    title: String,
    count: String,
    extra: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let config = state.config.settings.lists.config(id.kind());
    let queries = &state.list_queries;
    let filters_on = queries.filters(id).masked(state.query_filters(id)).active();

    let play = |shuffle: bool, glyph: Icon, text: Option<&'static str>| {
        let content: Element<'a, Message> = match text {
            Some(text) => row![
                icon(glyph, metrics::font_lg(), iced::Color::WHITE),
                label_w(text, metrics::font_md(), iced::Color::WHITE, Weight::Semibold)
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
            None => icon(glyph, metrics::font_lg(), t.foreground),
        };
        button(content)
            .padding(Padding::from([6.0, if shuffle { 8.0 } else { 14.0 }]))
            .style(move |theme, status| {
                if shuffle {
                    style::default_button(t)(theme, status)
                } else {
                    style::primary_button(t)(theme, status)
                }
            })
            .on_press(query(ListQueryMsg::PlayAll(id.clone(), state.play_behavior(), shuffle)))
    };
    // A long press on Play plays the shuffled version of what it does.
    let play_long = |btn: iced::widget::Button<'a, Message>| -> Element<'a, Message> {
        crate::ui::long_press::long_press(
            btn,
            query(ListQueryMsg::PlayAll(id.clone(), state.play_behavior(), true)),
        )
    };
    let play_label = match state.play_behavior() {
        PlayBehavior::Now => "Play",
        PlayBehavior::Next => "Play next",
        PlayBehavior::Last => "Play last",
    };
    let mut top = Row::new()
        .push(label_w(title, 36.0, t.foreground, Weight::Bold))
        .push(container(label(count, metrics::font_md(), t.muted)).padding(Padding::from([0.0, 4.0])))
        .push(space::horizontal());
    if let Some(extra) = extra {
        top = top.push(extra);
    }
    let top = top
        .push(tip(
            state,
            play_long(play(false, Icon::LuPlay, Some(play_label))),
            play_label,
            Some("listPlayDefault"),
            Position::Bottom,
        ))
        .push(tip(state, play(true, Icon::LuShuffle, None), "Shuffle play", None, Position::Bottom))
        .spacing(metrics::space_md())
        .align_y(Alignment::Center);

    // Sort dropdown (opens a list under the button) and the order toggle.
    let sort_button = {
        let idc = id.clone();
        anchored(
            tip(
                state,
                button(
                    row![
                        label(state.sort_label(id), metrics::font_md(), t.foreground),
                        icon(Icon::LuChevronDown, metrics::font_md(), t.muted)
                    ]
                    .spacing(metrics::space_xs())
                    .align_y(Alignment::Center),
                )
                .padding(Padding::from([6.0, 10.0]))
                .style(style::default_button(t))
                .on_press(Message::Noop),
                "Sort by",
                None,
                Position::Bottom,
            ),
            move |b| query(ListQueryMsg::OpenSort(idc.clone(), iced::Point::new(b.x, b.y + b.height + 4.0))),
        )
    };
    let order = match queries.order(id) {
        SortOrder::Ascending => Icon::LuArrowUp,
        SortOrder::Descending => Icon::LuArrowDown,
    };
    let collections: Element<'a, Message> = if !has_collections(id) {
        space().width(0).into()
    } else {
        let idc = id.clone();
        anchored(
            tip(
                state,
                button(icon(Icon::LuBookmark, metrics::font_lg(), t.muted))
                    .padding(6)
                    .style(style::icon_button(t, false))
                    .on_press(Message::Noop),
                "Collections",
                None,
                Position::Bottom,
            ),
            move |b| {
                query(ListQueryMsg::OpenCollections(idc.clone(), iced::Point::new(b.x, b.y + b.height + 4.0)))
            },
        )
    };
    let search_text = queries.search(id);
    let search = {
        let idc = id.clone();
        let mut input = text_input(crate::i18n::tr("Search"), search_text)
            .font(crate::ui::font_for(search_text, crate::appearance::current_font()))
            .id(search_id(id))
            .font(crate::ui::font_for(search_text, crate::appearance::current_font()))
            .size(metrics::font_md())
            .padding(Padding { top: 6.0, bottom: 6.0, left: 10.0, right: 10.0 })
            .style(style::input(t))
            .on_input(move |s| query(ListQueryMsg::SearchInput(idc.clone(), s)))
            .width(Length::Fixed(220.0));
        if search_text.is_empty() {
            input = input.icon(text_input::Icon {
                font: iced::Font::DEFAULT,
                code_point: ' ',
                size: None,
                spacing: 0.0,
                side: text_input::Side::Right,
            });
        }
        let mut r = Row::new().align_y(Alignment::Center).spacing(2.0).push(input);
        if !search_text.is_empty() {
            r = r.push(tool(
                state,
                Icon::LuX,
                false,
                query(ListQueryMsg::ClearSearch(id.clone())),
                "Clear search",
                None,
            ));
        }
        r
    };
    let controls = row![
        sort_button,
        tool(
            state,
            order,
            false,
            query(ListQueryMsg::ToggleOrder(id.clone())),
            if queries.order(id) == SortOrder::Ascending {
                "Sort order: ascending"
            } else {
                "Sort order: descending"
            },
            None
        ),
        tool(
            state,
            Icon::LuFilter,
            filters_on,
            query(ListQueryMsg::OpenFilters(id.clone())),
            "Filters",
            None
        ),
        tool(state, Icon::LuRefreshCw, false, query(ListQueryMsg::Refresh(id.clone())), "Refresh", None),
        collections,
        space::horizontal(),
        search,
        toolbar(state, id, &config),
    ]
    .spacing(metrics::space_xs())
    .align_y(Alignment::Center);

    container(Column::new().spacing(metrics::space_md()).push(top).push(controls))
        .padding(Padding {
            top: 24.0,
            right: metrics::PAGE_PADDING_X,
            bottom: 12.0,
            left: metrics::PAGE_PADDING_X,
        })
        .into()
}

fn popup_box<'a>(state: &Listen, width: f32, body: Element<'a, Message>) -> Element<'a, Message> {
    let t = state.tokens;
    iced::widget::opaque(container(body).width(width).padding(4).style(move |_| container::Style {
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

/// The sort dropdown and the collections list over `root`.
pub fn popups<'a>(state: &'a Listen, root: Element<'a, Message>) -> Element<'a, Message> {
    let t = state.tokens;
    let Some(popup) = &state.list_queries.popup else { return root };
    let close = query(ListQueryMsg::ClosePopup);
    match popup {
        Popup::Sort { list, at } => {
            let labels = state.sort_labels(list);
            let height = labels.len() as f32 * SORT_ITEM + 8.0;
            let mut col = Column::new();
            for (i, (name, selected)) in labels.into_iter().enumerate() {
                col = col.push(
                    button(
                        row![
                            container(label(name, metrics::font_md(), t.foreground)).width(Length::Fill),
                            if selected {
                                icon(Icon::LuCheck, metrics::font_md(), t.primary)
                            } else {
                                space().width(metrics::font_md()).into()
                            }
                        ]
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(SORT_ITEM)
                    .padding(Padding::from([0.0, metrics::space_md()]))
                    .style(style::nav_item(t, selected))
                    .on_press(query(ListQueryMsg::Sort(list.clone(), i))),
                );
            }
            let body = scrollable(col).height(Length::Fixed(height.min(state.window_size.height - 120.0)));
            let at = crate::ui::popover::place(*at, iced::Size::new(SORT_W, height), state.window_size);
            crate::ui::popover::over(root, popup_box(state, SORT_W, body.into()), at, close)
        }
        Popup::Collections { list, at } => {
            let kind = if *list == ListId::Songs { CollectionList::Songs } else { CollectionList::Albums };
            let saved = &state.config.settings.lists.collections;
            let mut col = Column::new().spacing(2.0);
            let mut shown = 0;
            for (i, c) in saved.iter().enumerate().filter(|(_, c)| c.list == kind) {
                shown += 1;
                col = col.push(
                    row![
                        button(label(c.name.clone(), metrics::font_md(), t.foreground))
                            .width(Length::Fill)
                            .height(SORT_ITEM)
                            .padding(Padding::from([0.0, metrics::space_md()]))
                            .style(style::nav_item(t, false))
                            .on_press(query(ListQueryMsg::ApplyCollection(i))),
                        tool(
                            state,
                            Icon::LuTrash2,
                            false,
                            query(ListQueryMsg::DeleteCollection(i)),
                            "Delete collection",
                            None
                        ),
                    ]
                    .align_y(Alignment::Center),
                );
            }
            if shown == 0 {
                col = col.push(
                    container(label("No saved collections", metrics::font_sm(), t.muted))
                        .padding(metrics::space_md()),
                );
            }
            let name = &state.list_queries.collection_name;
            let idc = list.clone();
            let save = row![
                text_input(crate::i18n::tr("Save these filters as…"), name)
                    .font(crate::ui::font_for(name, crate::appearance::current_font()))
                    .size(metrics::font_md())
                    .padding(Padding::from([6.0, 10.0]))
                    .style(style::input(t))
                    .on_input(|s| query(ListQueryMsg::CollectionName(s)))
                    .on_submit(query(ListQueryMsg::SaveCollection(list.clone()))),
                button(label("Save", metrics::font_md(), iced::Color::WHITE))
                    .padding(Padding::from([6.0, 12.0]))
                    .style(style::primary_button(t))
                    .on_press(query(ListQueryMsg::SaveCollection(idc))),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center);
            let height = 8.0 + shown.max(1) as f32 * (SORT_ITEM + 2.0) + 52.0;
            let body = Column::new().spacing(metrics::space_sm()).push(col).push(save);
            let at =
                crate::ui::popover::place(*at, iced::Size::new(COLLECTIONS_W, height), state.window_size);
            crate::ui::popover::over(root, popup_box(state, COLLECTIONS_W, body.into()), at, close)
        }
    }
}

// ---- the filter modal ------------------------------------------------------------

fn tri<'a>(
    state: &Listen,
    title: &'static str,
    current: Option<bool>,
    make: impl Fn(Option<bool>) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        section_title(state, title),
        segmented(state, &[("Any", None), ("Yes", Some(true)), ("No", Some(false))], current, make)
    ]
    .spacing(metrics::space_xs())
    .into()
}

/// A pick list with a search box: genres or artists.
fn option_list<'a>(
    state: &'a Listen,
    title: &'static str,
    multi: bool,
    on_mode: impl Fn(bool) -> Message + 'a,
    rows: Vec<(String, bool, Message)>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let needle = state.list_queries.option_search.to_lowercase();
    let id = state.list_queries.filter_list.clone().unwrap_or(ListId::Albums);
    let mut list = Column::new().spacing(2.0);
    let mut shown = 0;
    for (name, on, msg) in
        rows.into_iter().filter(|(n, _, _)| needle.is_empty() || n.to_lowercase().contains(&needle))
    {
        if shown >= 300 {
            break;
        }
        shown += 1;
        list = list.push(
            checkbox(on).label(name).size(16).text_size(metrics::font_sm()).on_toggle(move |_| msg.clone()),
        );
    }
    if shown == 0 {
        list = list.push(label("Nothing to pick", metrics::font_sm(), t.muted));
    }
    column![
        row![
            section_title(state, title),
            space::horizontal(),
            segmented(state, &[("Single", false), ("Multiple", true)], multi, on_mode)
        ]
        .align_y(Alignment::Center),
        text_input(crate::i18n::tr("Search"), &state.list_queries.option_search)
            .font(crate::ui::font_for(&state.list_queries.option_search, crate::appearance::current_font()))
            .size(metrics::font_sm())
            .padding(Padding::from([4.0, 8.0]))
            .style(style::input(t))
            .on_input(move |s| query(ListQueryMsg::Filter(id.clone(), FilterEdit::OptionSearch(s)))),
        container(scrollable(list).height(Length::Fixed(130.0))).padding(Padding::from([0.0, 4.0])),
    ]
    .spacing(metrics::space_xs())
    .into()
}

/// Custom tag filters (Navidrome tags): pick a tag, then its values.
fn tag_filters<'a>(
    state: &'a Listen,
    id: &ListId,
    filters: &'a crate::app::list_query::Filters,
    edit: impl Fn(FilterEdit) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    let t = state.tokens;
    let q = &state.list_queries;
    let tags = if is_songs(id) { &q.song_tags } else { &q.album_tags };
    if tags.is_empty() {
        return label(
            if q.tags_loaded { "No tags to filter by" } else { "Loading tags…" },
            metrics::font_sm(),
            t.muted,
        )
        .into();
    }
    let picked = |name: &str| filters.tags.iter().find(|(n, _)| n == name).map_or(0, |(_, v)| v.len());
    // The tag being edited: the one picked, else the first with values, else the first.
    let current = q
        .tag_pick
        .clone()
        .filter(|p| tags.iter().any(|t| &t.name == p))
        .or_else(|| filters.tags.first().map(|(n, _)| n.clone()))
        .unwrap_or_else(|| tags[0].name.clone());
    let names: Vec<TagChoice> =
        tags.iter().map(|tag| TagChoice { name: tag.name.clone(), count: picked(&tag.name) }).collect();
    let selected = names.iter().find(|c| c.name == current).cloned();
    let e = edit.clone();
    let chooser =
        iced::widget::pick_list(names, selected, move |c: TagChoice| e(FilterEdit::TagPick(c.name)));
    let tag = tags.iter().find(|tag| tag.name == current).unwrap_or(&tags[0]);
    let chosen: &[String] =
        filters.tags.iter().find(|(n, _)| *n == tag.name).map_or(&[], |(_, v)| v.as_slice());
    let rows = tag
        .options
        .iter()
        .map(|(vid, value)| {
            (value.clone(), chosen.contains(vid), edit(FilterEdit::Tag(tag.name.clone(), vid.clone())))
        })
        .collect();
    column![section_title(state, "Tags"), chooser, tag_values(state, id, rows)]
        .spacing(metrics::space_xs())
        .into()
}

/// An entry of the tag chooser: the tag and how many of its values are picked.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TagChoice {
    name: String,
    count: usize,
}

impl std::fmt::Display for TagChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut chars = self.name.chars();
        let title: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
        if self.count > 0 { write!(f, "{title} ({})", self.count) } else { write!(f, "{title}") }
    }
}

/// The values of one tag: a search box and checkboxes.
fn tag_values<'a>(
    state: &'a Listen,
    id: &ListId,
    rows: Vec<(String, bool, Message)>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let needle = state.list_queries.option_search.to_lowercase();
    let idc = id.clone();
    let mut list = Column::new().spacing(2.0);
    for (name, on, msg) in
        rows.into_iter().filter(|(n, _, _)| needle.is_empty() || n.to_lowercase().contains(&needle)).take(300)
    {
        list = list.push(
            checkbox(on).label(name).size(16).text_size(metrics::font_sm()).on_toggle(move |_| msg.clone()),
        );
    }
    column![
        text_input(crate::i18n::tr("Search"), &state.list_queries.option_search)
            .font(crate::ui::font_for(&state.list_queries.option_search, crate::appearance::current_font()))
            .size(metrics::font_sm())
            .padding(Padding::from([4.0, 8.0]))
            .style(style::input(t))
            .on_input(move |s| query(ListQueryMsg::Filter(idc.clone(), FilterEdit::OptionSearch(s)))),
        container(scrollable(list).height(Length::Fixed(110.0))).padding(Padding::from([0.0, 4.0])),
    ]
    .spacing(metrics::space_xs())
    .into()
}

/// The body of the "list-filters" modal (an arm of `ui::overlay::custom_modal`).
pub fn filters_modal<'a>(state: &'a Listen) -> Option<ModalParts<'a>> {
    let t = state.tokens;
    let id = state.list_queries.filter_list.clone()?;
    if !has_controls(&id) {
        return None;
    }
    let supported = state.query_filters(&id);
    let filters = state.list_queries.filters(&id);
    let q = &state.list_queries;
    let edit = {
        let id = id.clone();
        move |e: FilterEdit| query(ListQueryMsg::Filter(id.clone(), e))
    };
    let mut col = Column::new().spacing(metrics::space_md());
    // The plain Subsonic lists take one of favourite, artists, genres and year.
    let locked = |which| filters.disabled(supported, which);
    let off = |title: &'static str| -> Element<'a, Message> {
        column![
            section_title(state, title),
            label("Off while another of these filters is on", metrics::font_sm(), t.muted)
        ]
        .spacing(metrics::space_xs())
        .into()
    };
    if supported.contains(QueryFilters::FAVORITE) {
        let e = edit.clone();
        col = col.push(if locked(Exclusive::Favorite) {
            off("Favorited")
        } else {
            tri(state, "Favorited", filters.favorite, move |v| e(FilterEdit::Favorite(v)))
        });
    }
    if supported.contains(QueryFilters::RATED) {
        let e = edit.clone();
        col = col.push(tri(state, "Rated", filters.rated, move |v| e(FilterEdit::Rated(v))));
    }
    if supported.contains(QueryFilters::COMPILATION) && !is_songs(&id) {
        let e = edit.clone();
        col =
            col.push(tri(state, "Compilation", filters.compilation, move |v| e(FilterEdit::Compilation(v))));
    }
    if supported.contains(QueryFilters::RECENTLY_PLAYED) && !is_songs(&id) {
        let e = edit.clone();
        col = col.push(check("Recently played", filters.recently_played, move |on| {
            e(FilterEdit::RecentlyPlayed(on))
        }));
    }
    if supported.contains(QueryFilters::ARTISTS) {
        let multi = q.multi_artist && supported.contains(QueryFilters::MULTI);
        let rows = q
            .artist_options
            .iter()
            .map(|(aid, name)| {
                (
                    name.clone(),
                    filters.artists.iter().any(|(i, _)| i == aid),
                    edit(FilterEdit::Artist(aid.clone(), name.clone())),
                )
            })
            .collect();
        let e = edit.clone();
        col = col.push(if locked(Exclusive::Artists) {
            off("Artists")
        } else {
            option_list(state, "Artists", multi, move |m| e(FilterEdit::MultiArtist(m)), rows)
        });
    }
    if supported.contains(QueryFilters::GENRES) {
        let multi = q.multi_genre && supported.contains(QueryFilters::MULTI);
        let rows = q
            .genre_options
            .iter()
            .map(|g| (g.clone(), filters.genres.contains(g), edit(FilterEdit::Genre(g.clone()))))
            .collect();
        let e = edit.clone();
        col = col.push(if locked(Exclusive::Genres) {
            off("Genres")
        } else {
            option_list(state, "Genres", multi, move |m| e(FilterEdit::MultiGenre(m)), rows)
        });
    }
    let year_field = |placeholder: &'static str, value: &str, make: Box<dyn Fn(String) -> Message + 'a>| {
        text_input(placeholder, value)
            .font(crate::ui::font_for(value, crate::appearance::current_font()))
            .size(metrics::font_sm())
            .padding(Padding::from([4.0, 8.0]))
            .style(style::input(t))
            .on_input(make)
            .width(Length::Fill)
    };
    if (supported.contains(QueryFilters::YEAR_RANGE) || supported.contains(QueryFilters::YEAR))
        && locked(Exclusive::Year)
    {
        col = col.push(off("Year"));
    } else if supported.contains(QueryFilters::YEAR_RANGE) {
        let (e1, e2) = (edit.clone(), edit.clone());
        col = col.push(
            column![
                section_title(state, "Year"),
                row![
                    year_field("From", &q.year_from, Box::new(move |s| e1(FilterEdit::YearFrom(s)))),
                    year_field("To", &q.year_to, Box::new(move |s| e2(FilterEdit::YearTo(s)))),
                ]
                .spacing(metrics::space_sm())
            ]
            .spacing(metrics::space_xs()),
        );
    } else if supported.contains(QueryFilters::YEAR) {
        let e = edit.clone();
        col = col.push(
            column![
                section_title(state, "Year"),
                year_field("Year", &q.year_from, Box::new(move |s| e(FilterEdit::Year(s))))
            ]
            .spacing(metrics::space_xs()),
        );
    }
    if supported.contains(QueryFilters::TAGS) {
        col = col.push(tag_filters(state, &id, filters, edit.clone()));
    }
    let body: Element<'a, Message> =
        scrollable(container(col).padding(Padding { right: 12.0, ..Padding::ZERO }))
            .height(Length::Fixed((state.window_size.height - 260.0).clamp(200.0, 560.0)))
            .into();
    let reset = button(label("Reset", metrics::font_md(), t.foreground))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::default_button(t))
        .on_press(edit(FilterEdit::Reset));
    let done = button(label("Done", metrics::font_md(), iced::Color::WHITE))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::primary_button(t))
        .on_press(Message::Overlay(crate::app::overlay::OverlayMsg::CloseModal));
    let title = if is_songs(&id) { "Track filters" } else { "Album filters" };
    Some((title, body, vec![reset.into(), done.into()]))
}
