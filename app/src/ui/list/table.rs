//! The virtualized table: only the rows in view are built, between two
//! spacers that stand for the rest (the queue uses the same pattern).
//! Rows have a fixed height (compact 40, default 64, large 88).

use super::columns::{Cx, ListItem, content_px, sized};
use super::header;
use super::{Body, RowGroup, divider_h};
use crate::app::list::{HPart, ListMsg, ListState};
use crate::app::{Message, ScrollState};
use crate::style::{self, metrics};
use iced::widget::{Column, Row, column, container, mouse_area, scrollable, space};
use iced::{Element, Length, Padding};
use listen_core::settings::{ColumnConfig, ColumnId, Pin, TableConfig};
use std::ops::Range;
use std::rc::Rc;

/// Padding left and right of the table.
pub const PAD: f32 = 12.0;
const OVERSCAN: usize = 2;

/// `[first, last)` rows intersecting the viewport, with a little overscan.
pub fn visible_rows(scroll: ScrollState, row_height: f32, total: usize) -> (usize, usize) {
    let height = if scroll.height > 0.0 { scroll.height } else { 1200.0 };
    let first = (scroll.offset / row_height).floor().max(0.0) as usize;
    let last = ((scroll.offset + height) / row_height).ceil() as usize + OVERSCAN;
    (first.saturating_sub(OVERSCAN / 2).min(total), last.min(total))
}

/// Width the columns share: the list width minus its padding and the scrollbar.
pub fn inner_width(width: f32) -> f32 {
    (width - 2.0 * PAD - 14.0).max(100.0)
}

/// Drawn widths of `columns`. With `scroll`, pinned columns keep their width and auto-fit
/// scales the rest to fill (min 40 px, then they scroll); without `scroll` all shrink to fit
/// in proportion. Auto-size columns use `content(col)` when known.
pub fn resolve_widths(
    config: &TableConfig,
    columns: &[ColumnConfig],
    available: f32,
    scroll: bool,
    content: impl Fn(ColumnId) -> Option<f32>,
) -> Vec<f32> {
    let base: Vec<f32> = columns
        .iter()
        .map(|c| if c.auto_size { content(c.id).unwrap_or(c.width) } else { c.width }.max(40.0))
        .collect();
    let borders = if config.vertical_borders { columns.len() as f32 } else { 0.0 };
    let total: f32 = base.iter().sum();
    if total <= 0.0 {
        return base;
    }
    let room = (available - borders).max(40.0);
    if !scroll {
        let scale = if config.auto_fit { room / total } else { (room / total).min(1.0) };
        return base.iter().map(|w| (w * scale).floor().max(24.0)).collect();
    }
    let pinned = |c: &ColumnConfig| c.pinned.is_some();
    let pinned_sum: f32 = columns.iter().zip(&base).filter(|(c, _)| pinned(c)).map(|(_, w)| w).sum();
    let free: Vec<usize> = (0..columns.len()).filter(|i| !pinned(&columns[*i])).collect();
    let free_sum: f32 = free.iter().map(|i| base[*i]).sum();
    let mut out: Vec<f32> = base.iter().map(|w| w.floor()).collect();
    if free.is_empty() {
        return out;
    }
    if config.auto_fit {
        let for_free = room - pinned_sum;
        if free_sum > 0.0 && for_free >= 40.0 * free.len() as f32 {
            let scale = for_free / free_sum;
            for i in &free {
                out[*i] = (base[*i] * scale).floor();
            }
        }
    } else {
        let extra = room - pinned_sum - free_sum;
        let autos: Vec<usize> = free.iter().copied().filter(|i| columns[*i].auto_size).collect();
        if extra > 0.0 && !autos.is_empty() {
            for i in &autos {
                out[*i] = (base[*i] + extra / autos.len() as f32).floor();
            }
        }
    }
    out
}

/// Height of the table header plus its divider.
pub const HEADER_BLOCK: f32 = 41.0;
/// Height of a group row.
pub const GROUP_H: f32 = 40.0;
/// The seam between pinned and scrolling columns.
const SEAM: f32 = 1.0;

/// The columns of a table as they are laid out: their widths and, when some
/// are pinned or they are wider than the table, the three blocks (pinned
/// left, scrolling middle, pinned right).
#[derive(Debug, Clone)]
pub struct Shape {
    /// The shown columns in order: pinned left, then the others, then pinned right.
    pub columns: Vec<ColumnConfig>,
    pub widths: Vec<f32>,
    pub left: usize,
    pub right: usize,
    /// Pinned columns or a middle wider than its room: the middle scrolls sideways.
    pub split: bool,
    /// Width the table has for its columns.
    pub available: f32,
}

impl Shape {
    pub fn new(
        table: &TableConfig,
        available: f32,
        scroll: bool,
        content: impl Fn(ColumnId) -> Option<f32>,
    ) -> Shape {
        let columns: Vec<ColumnConfig> =
            table.visible().into_iter().filter(|c| c.id != ColumnId::AlbumGroup).collect();
        let widths = resolve_widths(table, &columns, available, scroll, content);
        let left = columns.iter().filter(|c| c.pinned == Some(Pin::Left)).count();
        let right = columns.iter().filter(|c| c.pinned == Some(Pin::Right)).count();
        let total: f32 = widths.iter().sum();
        let split = scroll && (left + right > 0 || total > available + 0.5);
        Shape { columns, widths, left, right, split, available }
    }

    /// The column index ranges of the (left, middle, right) blocks; without a
    /// split everything is the middle.
    pub fn blocks(&self) -> [Range<usize>; 3] {
        if !self.split {
            return [0..0, 0..self.columns.len(), self.columns.len()..self.columns.len()];
        }
        let n = self.columns.len();
        [0..self.left, self.left..n - self.right, n - self.right..n]
    }

    pub fn block_width(&self, r: &Range<usize>) -> f32 {
        self.widths[r.clone()].iter().sum()
    }

    /// Width of the middle block's room when it scrolls.
    pub fn middle_room(&self) -> f32 {
        let [l, _, r] = self.blocks();
        let seams = (!l.is_empty()) as u8 as f32 + (!r.is_empty()) as u8 as f32;
        (self.available - self.block_width(&l) - self.block_width(&r) - seams * SEAM).max(40.0)
    }

    /// How far the middle block can scroll.
    pub fn max_scroll(&self) -> f32 {
        let [_, m, _] = self.blocks();
        (self.block_width(&m) - self.middle_room()).max(0.0)
    }
}

/// What to build for a table with group rows: rows and groups in view, and
/// the space standing for what is not.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Space(f32),
    Group(usize),
    Row(usize),
}

/// Lays out `count` rows with group rows before the rows named in `before`
/// (row, height; ascending) and keeps what meets `[top - over, top + view_h + over]`,
/// `top` being the scroll offset inside the rows area.
pub fn plan(
    count: usize,
    row_h: f32,
    before: &[(usize, f32)],
    top: f32,
    view_h: f32,
    over: f32,
) -> Vec<Part> {
    let (lo, hi) = (top - over, top + view_h + over);
    let mut parts = Vec::new();
    let mut skipped = 0.0;
    let mut y = 0.0;
    let flush = |parts: &mut Vec<Part>, skipped: &mut f32| {
        if *skipped > 0.0 {
            parts.push(Part::Space(*skipped));
            *skipped = 0.0;
        }
    };
    let mut next_group = 0;
    for local in 0..count {
        while next_group < before.len() && before[next_group].0 <= local {
            let h = before[next_group].1;
            if y + h >= lo && y <= hi {
                flush(&mut parts, &mut skipped);
                parts.push(Part::Group(next_group));
            } else {
                skipped += h;
            }
            y += h;
            next_group += 1;
        }
        if y + row_h >= lo && y <= hi {
            flush(&mut parts, &mut skipped);
            parts.push(Part::Row(local));
        } else {
            skipped += row_h;
        }
        y += row_h;
    }
    flush(&mut parts, &mut skipped);
    parts
}

/// A group row: a checkbox with the title.
fn group_row<'a, T: ListItem>(body: &Body<'a, T>, group: &super::RowGroup) -> Element<'a, Message> {
    let t = body.state.tokens;
    match &group.look {
        super::GroupLook::Album(meta) => {
            if let Some(item) = body.items.get(meta.first)
                && let Some(header) = item.album_header(body.state, &body.id, meta)
            {
                return header;
            }
            return container(space()).height(meta.height).into();
        }
        super::GroupLook::Checkbox => {}
        super::GroupLook::Label => {
            return container(crate::ui::label_w(
                group.title.clone(),
                metrics::font_sm(),
                t.muted,
                iced::font::Weight::Semibold,
            ))
            .width(Length::Fill)
            .height(GROUP_H)
            .padding(Padding::from([0.0, metrics::space_md()]))
            .center_y(GROUP_H)
            .into();
        }
        super::GroupLook::DjBreak => {
            return container(
                iced::widget::row![
                    crate::ui::icon(crate::icons::Icon::LuMicVocal, metrics::font_sm(), t.primary),
                    crate::ui::label(group.title.clone(), metrics::font_xs(), t.primary),
                ]
                .spacing(metrics::space_xs())
                .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(GROUP_H)
            .padding(Padding::from([0.0, metrics::space_md()]))
            .center_y(GROUP_H)
            .into();
        }
    }
    container(
        iced::widget::checkbox(group.checked)
            .label(group.title.clone())
            .size(16)
            .text_size(metrics::font_sm())
            .on_toggle({
                let msg = group.on_toggle.clone();
                move |_| msg.clone()
            }),
    )
    .width(Length::Fill)
    .height(GROUP_H)
    .padding(Padding::from([0.0, metrics::space_md()]))
    .center_y(GROUP_H)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(style::alpha(t.foreground, 0.05))),
        ..Default::default()
    })
    .into()
}

/// The rows to put under the table header: the extras of the page, else the
/// album headers when the table has the album group column.
fn groups_of<T: ListItem>(body: &Body<'_, T>) -> (Rc<Vec<RowGroup>>, bool) {
    if !body.extras.groups.is_empty() {
        return (body.extras.groups.clone(), false);
    }
    let on = body.config.table.column(ColumnId::AlbumGroup).is_some_and(|c| c.enabled);
    if !on {
        return (body.extras.groups.clone(), false);
    }
    let generated = T::album_groups(body.state, body.items, body.start, body.end);
    let any = !generated.is_empty();
    (Rc::new(generated), any)
}

fn divider_v<'a>(t: crate::style::Tokens) -> Element<'a, Message> {
    container(space()).width(SEAM).height(Length::Fill).style(style::divider(t)).into()
}

/// The sideways scrollable of the middle block; `part` says which of the three
/// (header, rows, bar) it is so that they move together.
pub(super) fn hscroll<'a>(
    id: &crate::app::list::ListId,
    part: HPart,
    content: impl Into<Element<'a, Message>>,
    t: crate::style::Tokens,
) -> Element<'a, Message> {
    let list = id.clone();
    let bar = match part {
        HPart::Bar => scrollable::Scrollbar::new().width(8).scroller_width(8).margin(1),
        _ => scrollable::Scrollbar::hidden(),
    };
    scrollable(content)
        .id(id.hscroll_id(part))
        .direction(scrollable::Direction::Horizontal(bar))
        .on_scroll(move |v| Message::List(ListMsg::HScroll(list.clone(), part, v.absolute_offset().x)))
        .style(style::scrollbar(t))
        .width(Length::Fill)
        .into()
}

/// The thin scrollbar under a table whose middle block scrolls sideways.
fn hbar<'a, T: ListItem>(body: &Body<'a, T>, shape: &Shape) -> Element<'a, Message> {
    let t = body.state.tokens;
    let [l, m, r] = shape.blocks();
    let thumb = space().width(shape.block_width(&m)).height(1);
    let seams = |r: &Range<usize>| if r.is_empty() { 0.0 } else { SEAM };
    iced::widget::row![
        space().width(shape.block_width(&l) + seams(&l)),
        hscroll(&body.id, HPart::Bar, thumb, t),
        space().width(shape.block_width(&r) + seams(&r)),
    ]
    .height(10)
    .into()
}

/// The scrollable table of `body` (header on top, then the rows). With
/// [`super::Extras`] the lead, the header, the rows (with group rows) and the
/// tail share one scrollable.
pub fn view<'a, T: ListItem>(body: &Body<'a, T>) -> Element<'a, Message> {
    let config = &body.config;
    let table = &config.table;
    let (groups, generated) = groups_of(body);
    // Group rows span the whole width: the columns cannot scroll sideways then.
    let scroll = groups.is_empty();
    // Only auto-sized columns need the rows' text.
    let data = table.visible().iter().any(|c| c.auto_size).then(|| body.state.list_data(&body.id)).flatten();
    let shape = Shape::new(table, inner_width(body.size.width), scroll, |col| {
        data.as_ref().and_then(|d| super::columns::content_width(d, col))
    });
    let row_h = table.size.height();
    let count = body.end - body.start;
    let extras = &body.extras;
    let inline = extras.lead.is_some() || extras.tail.is_some() || (!extras.groups.is_empty());
    let t = body.state.tokens;

    let header = || -> Element<'a, Message> {
        container(header::table_header(body, &shape)).padding(Padding::from([0.0, PAD])).into()
    };
    let bar = (shape.split && shape.max_scroll() > 0.0).then(|| hbar(body, &shape));

    if inline {
        let lead_h = extras.lead.as_ref().map_or(0.0, |l| l.0);
        let header_h = if table.header { HEADER_BLOCK } else { 0.0 };
        // Where the first row starts, relative to the scroll offset.
        let top = body.list.scroll.offset - lead_h - header_h;
        let mut rows = Column::new();
        if let Some((_, build)) = &extras.lead {
            rows = rows.push(build());
        }
        if table.header {
            rows = rows.push(header()).push(divider_h(t));
        }
        let before: Vec<(usize, f32)> = groups.iter().map(|g| (g.before, g.height())).collect();
        let parts = plan(count, row_h, &before, top, body.size.height, row_h * OVERSCAN as f32);
        rows = rows.push(rows_block(body, &shape, parts, &groups, true));
        if let Some((_, build)) = &extras.tail {
            rows = rows.push(build());
        }
        let id = body.id.clone();
        let list = scrollable(rows)
            .id(body.id.scroll_id())
            .on_scroll(move |v| super::scrolled(&id, &v))
            .style(style::scrollbar(t))
            .height(Length::Fill);
        let mut out = Column::new().push(list);
        if let Some(bar) = bar {
            out = out.push(container(bar).padding(Padding::from([0.0, PAD])));
        }
        return out.into();
    }

    let parts = if generated {
        let before: Vec<(usize, f32)> = groups.iter().map(|g| (g.before, g.height())).collect();
        plan(count, row_h, &before, body.list.scroll.offset, body.size.height, row_h * OVERSCAN as f32)
    } else {
        let scroll_state =
            ScrollState { offset: body.list.scroll.offset, height: body.size.height, width: body.size.width };
        let (first, last) = visible_rows(scroll_state, row_h, count);
        let mut parts = vec![Part::Space(first as f32 * row_h)];
        parts.extend((first..last).map(Part::Row));
        parts.push(Part::Space(count.saturating_sub(last) as f32 * row_h));
        parts
    };
    let rows = rows_block(body, &shape, parts, &groups, false);
    let id = body.id.clone();
    let list = scrollable(rows)
        .id(body.id.scroll_id())
        .on_scroll(move |v| super::scrolled(&id, &v))
        .style(style::scrollbar(t))
        .height(Length::Fill);

    let mut out = Column::new();
    if table.header {
        out = out.push(header()).push(divider_h(t));
    }
    out = out.push(list);
    if let Some(bar) = bar {
        out = out.push(container(bar).padding(Padding::from([0.0, PAD])));
    }
    out.into()
}

/// The rows of `parts`: one column (with group rows), or for a split table the three
/// blocks side by side with the middle one scrolling sideways.
fn rows_block<'a, T: ListItem>(
    body: &Body<'a, T>,
    shape: &Shape,
    parts: Vec<Part>,
    groups: &Rc<Vec<RowGroup>>,
    inline: bool,
) -> Element<'a, Message> {
    let t = body.state.tokens;
    let _ = inline;
    let loading: Element<'a, Message> =
        container(crate::ui::label("Loading…", metrics::font_md(), t.muted)).padding(12).into();
    if !shape.split {
        let mut inner = Column::new();
        for part in parts {
            match part {
                Part::Space(h) => inner = inner.push(space().height(h)),
                Part::Group(g) => inner = inner.push(group_row(body, &groups[g])),
                Part::Row(local) => {
                    let index = body.start + local;
                    if let Some(item) = body.items.get(index) {
                        inner =
                            inner.push(row_element(body, index, item, shape, 0..shape.columns.len(), None));
                    }
                }
            }
        }
        if body.loading {
            inner = inner.push(loading);
        }
        return container(inner).padding(Padding::from([0.0, PAD])).into();
    }
    let [l, m, r] = shape.blocks();
    let mut cols: [Vec<Element<'a, Message>>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for part in parts {
        match part {
            Part::Space(h) => {
                for c in &mut cols {
                    c.push(space().height(h).into());
                }
            }
            Part::Group(_) => {}
            Part::Row(local) => {
                let index = body.start + local;
                if let Some(item) = body.items.get(index) {
                    for (n, range) in [l.clone(), m.clone(), r.clone()].into_iter().enumerate() {
                        if range.is_empty() {
                            continue;
                        }
                        let el = row_element(body, index, item, shape, range, Some(n as u8));
                        cols[n].push(el);
                    }
                }
            }
        }
    }
    if body.loading {
        cols[1].push(loading);
    }
    let [cl, cm, cr] = cols.map(Column::with_children);
    let mut out = iced::widget::Row::new();
    if !l.is_empty() {
        out = out.push(cl.width(shape.block_width(&l))).push(divider_v(t));
    }
    out = out.push(hscroll(&body.id, HPart::Body, cm, t));
    if !r.is_empty() {
        out = out.push(divider_v(t)).push(cr.width(shape.block_width(&r)));
    }
    container(out).padding(Padding::from([0.0, PAD])).into()
}

/// One table row, or one block of it for a split table: its cells, its look
/// (selected, hovered, alternate) and the mouse handling.
fn row_element<'a, T: ListItem>(
    body: &Body<'a, T>,
    index: usize,
    item: &'a T,
    shape: &Shape,
    range: Range<usize>,
    part: Option<u8>,
) -> Element<'a, Message> {
    let t = body.state.tokens;
    let table = &body.config.table;
    let ListState { selection, hover, .. } = body.list;
    let selected = selection.contains(index);
    let hovered = *hover == Some(index);
    let row_h = table.size.height();
    let border = if table.horizontal_borders { 1.0 } else { 0.0 };

    let mut cells = Row::new().align_y(iced::Alignment::Center);
    for n in range.clone() {
        let (col, width) = (&shape.columns[n], shape.widths[n]);
        let cx = Cx {
            state: body.state,
            id: body.id.clone(),
            index,
            size: table.size,
            hovered,
            selected,
            width: content_px(width),
            card: false,
        };
        cells = cells.push(sized(item.cell(&cx, col.id), width, col.align));
        if table.vertical_borders && n + 1 < range.end {
            cells = cells.push(container(space()).width(1).height(row_h - border).style(style::divider(t)));
        }
    }

    let alternate = table.alternate_rows && index % 2 == 1;
    let highlight = hovered && table.row_hover_highlight;
    // Edit mode of a playlist: the row a dragged row would land on.
    let landing = body.state.playlist_landing(&body.id, index) || body.state.queue_landing(&body.id, index);
    // The corners that are the row's own: all of a whole row, the outer ones of a block.
    let r = t.radius_md;
    let radius = match part {
        Some(0) if shape.right == 0 && shape.blocks()[1].is_empty() => iced::border::Radius::from(r),
        Some(0) => iced::border::Radius { top_left: r, top_right: 0.0, bottom_right: 0.0, bottom_left: r },
        Some(2) => iced::border::Radius { top_left: 0.0, top_right: r, bottom_right: r, bottom_left: 0.0 },
        Some(_) if shape.left == 0 && shape.right == 0 => iced::border::Radius::from(r),
        Some(_) if shape.left == 0 => {
            iced::border::Radius { top_left: r, top_right: 0.0, bottom_right: 0.0, bottom_left: r }
        }
        Some(_) if shape.right == 0 => {
            iced::border::Radius { top_left: 0.0, top_right: r, bottom_right: r, bottom_left: 0.0 }
        }
        Some(_) => iced::border::Radius::from(0.0),
        None => iced::border::Radius::from(r),
    };
    let content = container(cells)
        .width(if part.is_some() { Length::Fixed(shape.block_width(&range)) } else { Length::Fill })
        .height(row_h - border)
        .center_y(row_h - border)
        .style(move |_| container::Style {
            background: if selected {
                Some(iced::Background::Color(style::alpha(t.primary, if hovered { 0.34 } else { 0.26 })))
            } else if highlight {
                Some(iced::Background::Color(t.surface))
            } else if alternate {
                Some(iced::Background::Color(style::alpha(t.foreground, 0.04)))
            } else {
                None
            },
            border: if landing {
                iced::Border { color: t.primary, width: 2.0, radius }
            } else {
                iced::Border { radius, ..Default::default() }
            },
            ..Default::default()
        });
    let row: Element<'a, Message> =
        if border > 0.0 { column![content, divider_h(t)].into() } else { content.into() };
    interactive_part(&body.id, index, row, part)
}

/// `row` with the list's mouse handling: press selects, double-click activates,
/// right-click opens the menu, hover is tracked.
pub fn interactive<'a>(
    id: &crate::app::list::ListId,
    index: usize,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    interactive_part(id, index, content, None)
}

/// [`interactive`] for one block of a row with pinned columns: each block
/// reports the pointer entering and leaving on its own.
fn interactive_part<'a>(
    id: &crate::app::list::ListId,
    index: usize,
    content: Element<'a, Message>,
    part: Option<u8>,
) -> Element<'a, Message> {
    let area = mouse_area(content);
    // Rows of the queue are drop targets: dragged rows land before them.
    let area = if id.is_queue() {
        area.on_release(Message::List(ListMsg::Drop(crate::app::list::DropTarget::QueueAt(index))))
    } else {
        area
    };
    let (enter, exit) = match part {
        Some(p) => (
            Message::List(ListMsg::HoverPart(id.clone(), index, p)),
            Message::List(ListMsg::UnhoverPart(id.clone(), index, p)),
        ),
        None => (
            Message::List(ListMsg::Hover(id.clone(), index)),
            Message::List(ListMsg::Unhover(id.clone(), index)),
        ),
    };
    area.on_press(Message::List(ListMsg::Press(id.clone(), index)))
        .on_double_click(Message::List(ListMsg::DoubleClick(id.clone(), index)))
        .on_right_press(Message::List(ListMsg::Context(id.clone(), index)))
        .on_enter(enter)
        .on_exit(exit)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::settings::{ColumnId, ItemType, Pin};

    fn at(offset: f32) -> ScrollState {
        ScrollState { offset, height: 640.0, width: 1000.0 }
    }

    #[test]
    fn only_the_rows_in_view_are_built() {
        let (first, last) = visible_rows(at(0.0), 64.0, 1000);
        assert_eq!(first, 0);
        assert!((10..=14).contains(&last), "{last}");
        let (first, last) = visible_rows(at(64.0 * 500.0), 64.0, 1000);
        assert!((499..=500).contains(&first));
        assert!((510..=514).contains(&last));
        // Short lists are not over-run.
        assert_eq!(visible_rows(at(0.0), 64.0, 3), (0, 3));
        assert_eq!(visible_rows(at(1e6), 64.0, 3).1, 3);
        // Rows are smaller in compact mode: more of them are built.
        assert!(visible_rows(at(0.0), 40.0, 1000).1 > last_of(at(0.0)));
    }

    fn last_of(s: ScrollState) -> usize {
        visible_rows(s, 64.0, 1000).1
    }

    #[test]
    fn plan_keeps_groups_in_order_and_counts_all_space() {
        // 10 rows of 40 px, a group before row 0 and before row 5.
        let all = plan(10, 40.0, &[(0, GROUP_H), (5, GROUP_H)], 0.0, 10_000.0, 0.0);
        let groups: Vec<_> = all.iter().filter(|p| matches!(p, Part::Group(_))).collect();
        let rows: Vec<_> = all.iter().filter(|p| matches!(p, Part::Row(_))).collect();
        assert_eq!((groups.len(), rows.len()), (2, 10));
        assert_eq!(all[0], Part::Group(0));
        // Out of view: one spacer of the full height (10 rows + 2 groups).
        let none = plan(10, 40.0, &[(0, GROUP_H), (5, GROUP_H)], 9000.0, 100.0, 0.0);
        assert_eq!(none, vec![Part::Space(10.0 * 40.0 + 2.0 * GROUP_H)]);
        // A window in the middle keeps the spacers on both sides.
        let mid = plan(100, 40.0, &[], 2000.0, 200.0, 0.0);
        let total: f32 = mid
            .iter()
            .map(|p| match p {
                Part::Space(h) => *h,
                Part::Row(_) => 40.0,
                Part::Group(_) => GROUP_H,
            })
            .sum();
        assert_eq!(total, 4000.0);
        assert!(mid.len() < 20);
    }

    #[test]
    fn groups_can_have_their_own_height() {
        // A tall album header before row 0 and a normal one before row 3.
        let parts = plan(5, 40.0, &[(0, 200.0), (3, GROUP_H)], 0.0, 10_000.0, 0.0);
        assert_eq!(parts[0], Part::Group(0));
        // Everything out of view collapses into spacers of the exact total height.
        let none = plan(5, 40.0, &[(0, 200.0), (3, GROUP_H)], 9000.0, 100.0, 0.0);
        assert_eq!(none, vec![Part::Space(200.0 + GROUP_H + 5.0 * 40.0)]);
        // Scrolled past the tall header: it is skipped but its height still counts.
        let past = plan(5, 40.0, &[(0, 200.0)], 210.0, 40.0, 0.0);
        assert!(!past.contains(&Part::Group(0)));
        assert_eq!(past, vec![Part::Space(200.0), Part::Row(0), Part::Row(1), Part::Space(120.0)]);
    }

    fn shape_of(t: &TableConfig, avail: f32, scroll: bool) -> Shape {
        Shape::new(t, avail, scroll, |_| None)
    }

    #[test]
    fn widths_fill_or_shrink() {
        let mut t = TableConfig::for_item(ItemType::Song);
        let cols = t.visible();
        let sum: f32 = cols.iter().map(|c| c.width).sum();
        let none = |_| None;
        // Auto-fit stretches to the width.
        let w = resolve_widths(&t, &cols, 2000.0, true, none);
        assert!((w.iter().sum::<f32>() - 2000.0).abs() < cols.len() as f32);
        // Without it the columns keep their width exactly and scroll when there is not room.
        t.auto_fit = false;
        let w = resolve_widths(&t, &cols, 5000.0, true, none);
        assert!((w.iter().sum::<f32>() - sum).abs() < cols.len() as f32);
        let w = resolve_widths(&t, &cols, 700.0, true, none);
        assert!(w.iter().sum::<f32>() > 700.0, "wider than the table: it scrolls");
        // Rows with group rows cannot scroll sideways: the columns shrink.
        let w = resolve_widths(&t, &cols, 700.0, false, none);
        assert!(w.iter().sum::<f32>() <= 700.0);
        // Auto-size follows the content.
        t.column_mut(ColumnId::Album).unwrap().auto_size = true;
        let cols = t.visible();
        let w = resolve_widths(&t, &cols, 50.0, true, |c| (c == ColumnId::Album).then_some(123.0));
        let i = cols.iter().position(|c| c.id == ColumnId::Album).unwrap();
        assert_eq!(w[i], 123.0);
    }

    #[test]
    fn auto_size_columns_share_the_room_that_is_left() {
        let mut t = TableConfig::for_item(ItemType::Song);
        t.auto_fit = false;
        t.column_mut(ColumnId::Album).unwrap().auto_size = true;
        let cols = t.visible();
        let base: f32 = cols.iter().map(|c| c.width).sum();
        let w = resolve_widths(&t, &cols, base + 300.0, true, |_| None);
        let i = cols.iter().position(|c| c.id == ColumnId::Album).unwrap();
        assert_eq!(w[i], cols[i].width + 300.0);
    }

    #[test]
    fn pinned_columns_keep_their_width_and_the_rest_fills_the_table() {
        let mut t = TableConfig::for_item(ItemType::Song);
        t.column_mut(ColumnId::RowIndex).unwrap().pinned = Some(Pin::Left);
        t.column_mut(ColumnId::Duration).unwrap().pinned = Some(Pin::Right);
        let shape = shape_of(&t, 1500.0, true);
        assert!(shape.split);
        assert_eq!((shape.left, shape.right), (1, 1));
        assert_eq!(shape.columns[0].id, ColumnId::RowIndex);
        assert_eq!(shape.columns.last().unwrap().id, ColumnId::Duration);
        assert_eq!(shape.widths[0], 60.0);
        assert_eq!(*shape.widths.last().unwrap(), 100.0);
        // Auto-fit scales the middle to the room: nothing scrolls.
        assert!((shape.widths.iter().sum::<f32>() - 1500.0).abs() < 8.0);
        assert_eq!(shape.max_scroll(), 0.0);
        let [l, m, r] = shape.blocks();
        assert_eq!((l.len(), r.len()), (1, 1));
        assert_eq!(m.len(), shape.columns.len() - 2);
    }

    #[test]
    fn a_middle_wider_than_the_table_scrolls_sideways() {
        let mut t = TableConfig::for_item(ItemType::Song);
        t.auto_fit = false;
        for id in [ColumnId::Bpm, ColumnId::Path, ColumnId::Comment, ColumnId::Composer] {
            t.column_mut(id).unwrap().enabled = true;
        }
        let shape = shape_of(&t, 900.0, true);
        assert!(shape.split, "wider than the table");
        assert!(shape.max_scroll() > 0.0);
        // The scrolling room is the table minus the pinned blocks and their seams.
        assert_eq!(shape.middle_room(), 900.0);
        let flat = shape_of(&t, 900.0, false);
        assert!(!flat.split && flat.widths.iter().sum::<f32>() <= 900.0);
        // A table that fits does not split without pins.
        t.auto_fit = true;
        assert!(!shape_of(&t, 1500.0, true).split);
    }

    #[test]
    fn the_album_group_column_is_not_a_cell() {
        let mut t = TableConfig::for_item(ItemType::Song);
        t.column_mut(ColumnId::AlbumGroup).unwrap().enabled = true;
        let shape = shape_of(&t, 1500.0, true);
        assert!(shape.columns.iter().all(|c| c.id != ColumnId::AlbumGroup));
        assert!(!shape.split, "its default pin does not split the table");
    }
}
