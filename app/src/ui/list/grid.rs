//! The virtualized card grid: poster cards with hover controls (play now /
//! next / last, like, rating, more), selection and the configurable lines
//! under each card.

use super::Body;
use super::anchor::anchored;
use super::columns::{Cx, ListItem};
use super::table::interactive;
use crate::app::list::ListMsg;
use crate::app::{Message, ScrollState};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::album_detail::expanded::PANEL_H;
use crate::ui::{icon, label_w};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, scrollable, space, stack};
use iced::{Alignment, Element, Length, Padding};
use listen_core::settings::{Align, CardSize, GridConfig};

/// Padding left and right of the grid.
pub const PAD: f32 = 12.0;

/// Where the cards sit: columns, card size and row height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayout {
    pub columns: usize,
    pub card: f32,
    pub row_height: f32,
    pub gap: f32,
}

/// Feishin's item-grid breakpoints (by container width).
fn breakpoint_columns(width: f32) -> usize {
    match width {
        w if w >= 2560.0 => 10,
        w if w >= 1920.0 => 8,
        w if w >= 1440.0 => 7,
        w if w >= 1200.0 => 6,
        w if w >= 960.0 => 5,
        w if w >= 768.0 => 4,
        w if w >= 600.0 => 3,
        _ => 2,
    }
}

/// Height of the text under a card: its enabled lines (the compact card has none).
fn text_block(config: &GridConfig) -> f32 {
    if config.size == CardSize::Compact {
        return 0.0;
    }
    let lines = config.enabled_rows().count();
    if lines == 0 { 0.0 } else { 8.0 + 22.0 + (lines - 1) as f32 * 20.0 }
}

impl GridLayout {
    pub fn new(width: f32, config: &GridConfig) -> Self {
        let columns = if config.items_per_row_enabled {
            config.items_per_row.max(1) as usize
        } else {
            let base = breakpoint_columns(width);
            if config.size == CardSize::Large { ((base as f32 * 0.75).round() as usize).max(1) } else { base }
        };
        let gap = config.gap.px();
        let inner = (width - PAD * 2.0 - 14.0).max(80.0);
        let card = ((inner - gap * (columns as f32 - 1.0)) / columns as f32).floor().max(40.0);
        GridLayout { columns, card, row_height: card + text_block(config) + gap, gap }
    }

    /// `[first, last)` item indices of rows intersecting the viewport (plus one row of overscan).
    pub fn visible_items(&self, scroll: ScrollState, total: usize) -> (usize, usize) {
        self.visible_items_with(scroll, total, None)
    }

    /// [`Self::visible_items`] when an expanded panel of `height` sits under row `row`.
    pub fn visible_items_with(
        &self,
        scroll: ScrollState,
        total: usize,
        panel: Option<(usize, f32)>,
    ) -> (usize, usize) {
        let height = if scroll.height > 0.0 { scroll.height } else { 1200.0 };
        let top = self.without_panel(scroll.offset - PAD, panel);
        let bottom = self.without_panel(scroll.offset + height, panel);
        let first_row = (top / self.row_height).floor().max(0.0) as usize;
        let last_row = (bottom / self.row_height).ceil() as usize + 1;
        ((first_row * self.columns).min(total), (last_row * self.columns).min(total))
    }

    /// `y` (px from the top of the cards) minus the expanded panel (`height` tall, after
    /// row `row`), so dividing by the row height gives the row.
    fn without_panel(&self, y: f32, panel: Option<(usize, f32)>) -> f32 {
        match panel {
            Some((row, h)) => {
                let start = (row + 1) as f32 * self.row_height;
                if y <= start {
                    y
                } else if y <= start + h {
                    start
                } else {
                    y - h
                }
            }
            None => y,
        }
    }
}

fn halign(a: Align) -> iced::alignment::Horizontal {
    super::columns::horizontal(a)
}

pub fn view<'a, T: ListItem>(body: &Body<'a, T>) -> Element<'a, Message> {
    let layout = GridLayout::new(body.size.width, &body.config.grid);
    let count = body.end - body.start;
    let rows_total = count.div_ceil(layout.columns);
    let scroll =
        ScrollState { offset: body.list.scroll.offset, height: body.size.height, width: body.size.width };
    // An expanded card opens a panel under its row.
    let expanded = body
        .state
        .lists
        .expanded
        .as_ref()
        .filter(|(l, i)| *l == body.id && (body.start..body.end).contains(i))
        .map(|(_, i)| *i);
    let panel_row = expanded.map(|i| (i - body.start) / layout.columns);
    let panel = panel_row.map(|r| (r, PANEL_H + layout.gap));
    let (first, last) = layout.visible_items_with(scroll, count, panel);
    let (first_row, last_row) = (first / layout.columns, last.div_ceil(layout.columns));

    // Space for what is above (with the panel, if it is above) and below.
    let panel_space = |rows_before: usize| match panel_row {
        Some(r) if r < rows_before => PANEL_H + layout.gap,
        _ => 0.0,
    };
    let mut out =
        Column::new().push(space().height(first_row as f32 * layout.row_height + panel_space(first_row)));
    for r in first_row..last_row {
        let mut cards = Row::new().spacing(layout.gap);
        for local in (r * layout.columns)..((r + 1) * layout.columns).min(count) {
            let index = body.start + local;
            if let Some(item) = body.items.get(index) {
                cards = cards.push(card(body, &layout, index, item));
            }
        }
        out = out.push(container(cards).height(layout.row_height));
        if panel_row == Some(r)
            && let Some(item) = expanded.and_then(|i| body.items.get(i))
            && let Some(p) = item.expanded_panel(body.state, &body.id, expanded.unwrap_or_default())
        {
            out = out.push(
                container(p)
                    .height(PANEL_H + layout.gap)
                    .padding(Padding { bottom: layout.gap, ..Padding::ZERO }),
            );
        }
    }
    let rows_after = rows_total.saturating_sub(last_row);
    let panel_after = match panel_row {
        Some(r) if r >= last_row => PANEL_H + layout.gap,
        _ => 0.0,
    };
    out = out.push(space().height(rows_after as f32 * layout.row_height + panel_after));
    if body.loading {
        out = out.push(
            container(crate::ui::label("Loading…", metrics::font_md(), body.state.tokens.muted)).padding(PAD),
        );
    }
    let t = body.state.tokens;
    let id = body.id.clone();
    scrollable(container(out).padding(Padding { top: PAD, ..Padding::from([0.0, PAD]) }))
        .id(body.id.scroll_id())
        .on_scroll(move |v| super::scrolled(&id, &v))
        .style(style::scrollbar(t))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// One card: the cover (with controls while the pointer is over it) and the lines below.
fn card<'a, T: ListItem>(
    body: &Body<'a, T>,
    layout: &GridLayout,
    index: usize,
    item: &'a T,
) -> Element<'a, Message> {
    let t = body.state.tokens;
    let grid = &body.config.grid;
    let selected = body.list.selection.contains(index);
    let hovered = body.list.hover == Some(index);
    let cx = Cx {
        state: body.state,
        id: body.id.clone(),
        index,
        size: listen_core::settings::RowSize::Default,
        hovered,
        selected,
        width: layout.card,
        card: true,
    };

    let mut cover: Element<'a, Message> = item.cover(body.state, layout.card, metrics::radius_md());
    if grid.size == CardSize::Compact {
        // Only the cover: the name sits on a strip at its bottom.
        let strip = container(label_w(
            crate::ui::ellipsize_px(&item.title(), layout.card - 16.0, metrics::font_sm()),
            metrics::font_sm(),
            iced::Color::WHITE,
            Weight::Medium,
        ))
        .padding(Padding::from([4.0, 8.0]))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(style::alpha(iced::Color::BLACK, 0.55))),
            ..Default::default()
        });
        cover =
            stack![cover, container(strip).height(Length::Fill).align_y(iced::alignment::Vertical::Bottom)]
                .width(layout.card)
                .height(Length::Shrink)
                .into();
    }
    if !body.state.config.settings.lists.grid_multi_select {
        // Grid multi-select off: a click on the cover opens the item.
        cover = button(cover)
            .padding(0)
            .style(style::bare_button(t))
            .on_press(Message::List(ListMsg::Open(body.id.clone(), index)))
            .into();
    }
    if hovered {
        cover =
            stack![cover, controls(&cx, item, layout.card)].width(layout.card).height(Length::Shrink).into();
    }

    let mut lines = Column::new().spacing(metrics::space_xs()).width(layout.card);
    if grid.size != CardSize::Compact {
        for line in grid.enabled_rows() {
            lines = lines.push(
                container(item.cell(&cx, line.id)).width(Length::Fill).align_x(halign(line.align)).clip(true),
            );
        }
    }
    let card = column![cover, lines].spacing(metrics::space_sm()).width(layout.card);
    let framed = container(card).style(move |_| container::Style {
        background: selected.then(|| iced::Background::Color(style::alpha(t.primary, 0.16))),
        border: iced::border::rounded(metrics::radius_md()).color(t.primary).width(if selected {
            2.0
        } else {
            0.0
        }),
        ..Default::default()
    });
    interactive(&body.id, index, framed.into())
}

/// The round buttons over a hovered cover.
pub(super) fn controls<'a, T: ListItem>(cx: &Cx<'a>, item: &'a T, card: f32) -> Element<'a, Message> {
    let t = cx.state.tokens;
    let id = cx.id.clone();
    let behavior = cx.state.play_behavior();
    let round = |glyph: Icon, msg: Message, name: &str, action: Option<&str>| -> Element<'a, Message> {
        crate::ui::tip::tip(
            cx.state,
            button(container(icon(glyph, metrics::font_lg(), iced::Color::WHITE)).center(Length::Fill))
                .width(30)
                .height(30)
                .padding(0)
                .style(style::art_overlay_button(t))
                .on_press(msg),
            name,
            action,
            crate::ui::tip::Position::Top,
        )
    };
    let mut bottom = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    if item.playable() {
        bottom = bottom
            .push(round(
                Icon::LuSkipForward,
                Message::List(ListMsg::PlayRow(
                    id.clone(),
                    cx.index,
                    listen_core::settings::PlayBehavior::Next,
                )),
                "Play next",
                Some("listPlayNext"),
            ))
            .push(round(
                Icon::RiPlayListAddLine,
                Message::List(ListMsg::PlayRow(
                    id.clone(),
                    cx.index,
                    listen_core::settings::PlayBehavior::Last,
                )),
                "Play last",
                Some("listPlayLast"),
            ));
    }
    bottom = bottom.push(space::horizontal());
    if let Some(on) = item.favorite(cx.state).filter(|_| cx.state.show_favorites()) {
        bottom = bottom.push(round(
            if on { Icon::RiHeartFill } else { Icon::RiHeartLine },
            Message::List(ListMsg::Favorite(id.clone(), cx.index)),
            if on { "Remove from favorites" } else { "Add to favorites" },
            None,
        ));
    }
    if let Some(target) = item.menu_target() {
        bottom = bottom.push(anchored(
            crate::ui::tip::tip(
                cx.state,
                container(icon(Icon::LuEllipsis, metrics::font_lg(), iced::Color::WHITE))
                    .center_x(30)
                    .center_y(30)
                    .style(move |_| container::Style {
                        background: Some(iced::Background::Color(style::alpha(iced::Color::BLACK, 0.6))),
                        border: iced::border::rounded(999),
                        ..Default::default()
                    }),
                "More options",
                None,
                crate::ui::tip::Position::Top,
            ),
            move |b| Message::Menu(crate::app::menu::MenuMsg::OpenAt(target.clone(), b.position())),
        ));
    }
    let grid = cx.state.config.settings.lists.config(cx.id.kind()).display
        == listen_core::settings::DisplayType::Grid;
    if item.expandable() && grid {
        let open = cx.state.lists.expanded == Some((id.clone(), cx.index));
        bottom = bottom.push(round(
            if open { Icon::LuChevronUp } else { Icon::LuChevronDown },
            Message::List(ListMsg::Expand(id.clone(), cx.index)),
            if open { "Collapse" } else { "Expand" },
            None,
        ));
    }
    let mut stack_col = Column::new().width(Length::Fill).height(Length::Fill).padding(8);
    if let Some(r) = item.rating(cx.state).filter(|_| cx.state.show_ratings()) {
        // Stars on the top edge (the same cells as in a table).
        stack_col =
            stack_col.push(container(super::columns::rating_cell(cx, r)).style(|_| container::Style {
                background: Some(iced::Background::Color(style::alpha(iced::Color::BLACK, 0.5))),
                border: iced::border::rounded(999),
                ..Default::default()
            }));
    }
    let play: Element<'a, Message> = if item.playable() {
        container(crate::ui::tip::tip(
            cx.state,
            button(container(icon(Icon::LuPlay, 22.0, iced::Color::WHITE)).center(Length::Fill))
                .width(card.min(150.0) * 0.36)
                .height(card.min(150.0) * 0.36)
                .padding(0)
                .style(style::art_overlay_button(t))
                .on_press(Message::List(ListMsg::PlayRow(id, cx.index, behavior))),
            match behavior {
                listen_core::settings::PlayBehavior::Now => "Play now",
                listen_core::settings::PlayBehavior::Next => "Play next",
                listen_core::settings::PlayBehavior::Last => "Play last",
            },
            Some("listPlayDefault"),
            crate::ui::tip::Position::Bottom,
        ))
        .center(Length::Fill)
        .into()
    } else {
        space().into()
    };
    stack_col.push(play).push(bottom).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::settings::{Gap, ItemType};

    fn cfg() -> GridConfig {
        GridConfig::for_item(ItemType::Album)
    }

    #[test]
    fn breakpoints_and_visible_range() {
        let c = cfg();
        assert_eq!(GridLayout::new(1300.0, &c).columns, 6);
        assert_eq!(GridLayout::new(500.0, &c).columns, 2);
        let l = GridLayout::new(1300.0, &c);
        let (first, last) = l.visible_items(ScrollState { offset: 0.0, height: 800.0, width: 1300.0 }, 1000);
        assert_eq!(first, 0);
        assert!(last >= l.columns * 3 && last <= l.columns * 6);
        let far = ScrollState { offset: l.row_height * 50.0, height: 800.0, width: 1300.0 };
        let (first, _) = l.visible_items(far, 1000);
        assert_eq!(first, 49 * l.columns);
        assert_eq!(l.visible_items(far, 10), (10, 10));
    }

    #[test]
    fn card_size_gap_and_fixed_columns() {
        let mut c = cfg();
        let default = GridLayout::new(1300.0, &c);
        c.size = CardSize::Large;
        let large = GridLayout::new(1300.0, &c);
        assert_eq!(large.columns, 5); // round(6 * 0.75)
        assert!(large.card > default.card);
        c.size = CardSize::Compact;
        let compact = GridLayout::new(1300.0, &c);
        // No text under a compact card.
        assert!(compact.row_height < default.row_height);
        c.size = CardSize::Default;
        c.items_per_row_enabled = true;
        c.items_per_row = 3;
        assert_eq!(GridLayout::new(1300.0, &c).columns, 3);
        c.gap = Gap::Xl;
        let wide = GridLayout::new(1300.0, &c);
        c.gap = Gap::Xs;
        assert!(wide.card < GridLayout::new(1300.0, &c).card);
        // More lines under the card: taller rows.
        let mut more = cfg();
        for r in &mut more.rows {
            r.enabled = true;
        }
        assert!(GridLayout::new(1300.0, &more).row_height > default.row_height);
    }
}
