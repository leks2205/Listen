//! Row selection of a list (clicks, Ctrl/Shift, select all, keyboard moves) and the floating selection
//! bar, as in Feishin's `item-list-controls.ts` and `use-table-keyboard-navigation.ts`. The last added
//! row is the anchor of Shift ranges and keyboard moves.

use crate::app::list::{ListId, ListMsg};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{icon, label, label_w};
use iced::font::Weight;
use iced::widget::{button, container, row};
use iced::{Alignment, Element, Length, Padding};
use std::collections::BTreeSet;

/// The modifiers of a click that matter for selecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    /// Ctrl (or Cmd/Super): toggle one row.
    pub toggle: bool,
    /// Shift: add the range from the last selected row.
    pub range: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { toggle: false, range: false };
    pub const CTRL: Mods = Mods { toggle: true, range: false };
    pub const SHIFT: Mods = Mods { toggle: false, range: true };

    pub fn from_iced(m: iced::keyboard::Modifiers) -> Self {
        Mods { toggle: m.control() || m.logo(), range: m.shift() }
    }
}

/// A move of the keyboard cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    set: BTreeSet<usize>,
    /// Insertion order; the last entry is the anchor.
    order: Vec<usize>,
}

impl Selection {
    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn contains(&self, index: usize) -> bool {
        self.set.contains(&index)
    }

    /// The selected rows, ascending.
    pub fn indices(&self) -> Vec<usize> {
        self.set.iter().copied().collect()
    }

    /// The row the keyboard moves from and a Shift range starts at.
    pub fn last(&self) -> Option<usize> {
        self.order.last().copied()
    }

    /// The first selected row (lowest index).
    pub fn first(&self) -> Option<usize> {
        self.set.first().copied()
    }

    pub fn clear(&mut self) {
        self.set.clear();
        self.order.clear();
    }

    pub fn set_only(&mut self, index: usize) {
        self.clear();
        self.set.insert(index);
        self.order.push(index);
    }

    /// Selects exactly these rows (the last one is the anchor).
    pub fn set_many(&mut self, rows: impl IntoIterator<Item = usize>) {
        self.clear();
        for r in rows {
            self.add(r);
        }
    }

    fn add(&mut self, index: usize) {
        if self.set.insert(index) {
            self.order.push(index);
        }
    }

    fn remove(&mut self, index: usize) {
        if self.set.remove(&index) {
            self.order.retain(|i| *i != index);
        }
    }

    pub fn select_all(&mut self, total: usize) {
        self.set = (0..total).collect();
        self.order = (0..total).collect();
    }

    pub fn is_all(&self, total: usize) -> bool {
        total > 0 && self.set.len() == total
    }

    /// `mod+a`: select all rows, or none when all are selected (Feishin).
    pub fn toggle_all(&mut self, total: usize) {
        if self.is_all(total) {
            self.clear();
        } else {
            self.select_all(total);
        }
    }

    /// A click on row `index` of `total`: Ctrl toggles, Shift adds the range from the anchor, plain
    /// selects only this row (or clears when it is the only one selected).
    pub fn click(&mut self, index: usize, mods: Mods, total: usize) {
        if index >= total {
            return;
        }
        if mods.toggle {
            if self.contains(index) {
                self.remove(index);
            } else {
                self.add(index);
            }
        } else if mods.range {
            match self.last() {
                Some(anchor) => {
                    let (lo, hi) = (anchor.min(index), anchor.max(index));
                    for i in lo..=hi.min(total - 1) {
                        self.add(i);
                    }
                }
                None => self.set_only(index),
            }
        } else if self.set.len() == 1 && self.contains(index) {
            self.clear();
        } else {
            self.set_only(index);
        }
    }

    /// The row a keyboard move leads to from the anchor (nothing selected: the first row, End the last).
    /// `columns`: cards per grid row (1 in a table); `page`: rows PageUp / PageDown jump.
    pub fn target(&self, mv: Move, total: usize, columns: usize, page: usize) -> Option<usize> {
        if total == 0 {
            return None;
        }
        let Some(cur) = self.last().filter(|c| *c < total) else {
            return Some(if mv == Move::End { total - 1 } else { 0 });
        };
        let columns = columns.max(1);
        let page = page.max(1) * columns;
        Some(match mv {
            Move::Down => (cur + columns).min(total - 1),
            Move::Up => cur.saturating_sub(columns),
            Move::Right if columns > 1 => (cur + 1).min(total - 1),
            Move::Left if columns > 1 => cur.saturating_sub(1),
            Move::Right | Move::Left => cur,
            Move::Home => 0,
            Move::End => total - 1,
            Move::PageDown => (cur + page).min(total - 1),
            Move::PageUp => cur.saturating_sub(page),
        })
    }

    /// Moves the selection with the keyboard (it becomes just the target row).
    pub fn step(&mut self, mv: Move, total: usize, columns: usize, page: usize) -> Option<usize> {
        let to = self.target(mv, total, columns, page)?;
        self.set_only(to);
        Some(to)
    }

    /// Keeps the selection valid after the list got shorter.
    pub fn retain_below(&mut self, total: usize) {
        self.set.retain(|i| *i < total);
        self.order.retain(|i| *i < total);
    }
}

/// The "N selected · Clear · More" bar at the bottom of a list; `None` while nothing is selected.
pub fn bar<'a>(state: &'a Listen, id: &ListId, selected: usize) -> Option<Element<'a, Message>> {
    if selected == 0 {
        return None;
    }
    let t = state.tokens;
    let text = if selected == 1 { "1 selected".to_string() } else { format!("{selected} selected") };
    let clear = button(
        row![
            icon(Icon::LuX, metrics::font_md(), t.foreground),
            label_w("Clear", metrics::font_sm(), t.foreground, Weight::Medium)
        ]
        .spacing(metrics::space_xs())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([4.0, 10.0]))
    .style(style::default_button(t))
    .on_press(Message::List(ListMsg::Clear(id.clone())));
    let more = super::anchor::anchored(
        button(
            row![
                icon(Icon::LuEllipsis, metrics::font_md(), t.foreground),
                label_w("More", metrics::font_sm(), t.foreground, Weight::Medium)
            ]
            .spacing(metrics::space_xs())
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([4.0, 10.0]))
        .style(style::default_button(t))
        .on_press(Message::Noop),
        {
            let id = id.clone();
            move |bounds| Message::List(ListMsg::SelectionMenu(id.clone(), bounds.position()))
        },
    );
    let bar = container(
        row![label_w(text, metrics::font_md(), t.foreground, Weight::Medium), clear, more]
            .spacing(metrics::space_lg())
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([8.0, 16.0]))
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(t.background_alt)),
        border: iced::border::rounded(metrics::radius_md()).color(t.border).width(1),
        shadow: iced::Shadow {
            color: style::alpha(iced::Color::BLACK, 0.4),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 14.0,
        },
        ..Default::default()
    });
    let hint = label("Ctrl+click toggles · Shift+click selects a range", metrics::font_xs(), t.muted);
    let _ = hint;
    Some(
        container(bar)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(Padding { bottom: 16.0, ..Padding::ZERO })
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(items: &[usize]) -> Selection {
        let mut s = Selection::default();
        for i in items {
            s.click(*i, Mods::CTRL, 100);
        }
        s
    }

    #[test]
    fn plain_click_selects_one_and_a_second_click_clears() {
        let mut s = Selection::default();
        s.click(3, Mods::NONE, 10);
        assert_eq!(s.indices(), [3]);
        s.click(5, Mods::NONE, 10);
        assert_eq!(s.indices(), [5]);
        s.click(5, Mods::NONE, 10);
        assert!(s.is_empty());
        // Clicking another row of a multi-selection keeps just that row.
        let mut s = sel(&[1, 2, 3]);
        s.click(2, Mods::NONE, 10);
        assert_eq!(s.indices(), [2]);
        // Out of range clicks are ignored.
        s.click(50, Mods::NONE, 10);
        assert_eq!(s.indices(), [2]);
    }

    #[test]
    fn ctrl_click_toggles() {
        let mut s = Selection::default();
        s.click(1, Mods::CTRL, 10);
        s.click(4, Mods::CTRL, 10);
        assert_eq!(s.indices(), [1, 4]);
        s.click(1, Mods::CTRL, 10);
        assert_eq!(s.indices(), [4]);
        assert_eq!(s.last(), Some(4));
        s.click(4, Mods::CTRL, 10);
        assert!(s.is_empty());
        assert_eq!(s.last(), None);
    }

    #[test]
    fn shift_click_adds_the_range_from_the_last_selected_row() {
        let mut s = Selection::default();
        // Nothing selected: just the row.
        s.click(2, Mods::SHIFT, 20);
        assert_eq!(s.indices(), [2]);
        s.click(6, Mods::SHIFT, 20);
        assert_eq!(s.indices(), [2, 3, 4, 5, 6]);
        // The range is added to the selection, not replacing it.
        s.click(10, Mods::CTRL, 20);
        s.click(12, Mods::SHIFT, 20);
        assert_eq!(s.indices(), [2, 3, 4, 5, 6, 10, 11, 12]);
        // Backwards from the anchor.
        let mut s = sel(&[8]);
        s.click(5, Mods::SHIFT, 20);
        assert_eq!(s.indices(), [5, 6, 7, 8]);
        // A plain click collapses it again.
        s.click(7, Mods::NONE, 20);
        assert_eq!(s.indices(), [7]);
    }

    #[test]
    fn select_all_and_clear() {
        let mut s = Selection::default();
        s.toggle_all(5);
        assert_eq!(s.indices(), [0, 1, 2, 3, 4]);
        assert!(s.is_all(5));
        s.toggle_all(5);
        assert!(s.is_empty());
        s.select_all(3);
        s.clear();
        assert!(s.is_empty() && s.last().is_none());
        // Nothing to select in an empty list.
        s.toggle_all(0);
        assert!(s.is_empty());
    }

    #[test]
    fn keyboard_moves_in_a_table() {
        let mut s = Selection::default();
        // Nothing selected: any move selects the first row.
        assert_eq!(s.step(Move::Down, 10, 1, 5), Some(0));
        assert_eq!(s.step(Move::Down, 10, 1, 5), Some(1));
        assert_eq!(s.step(Move::Up, 10, 1, 5), Some(0));
        assert_eq!(s.step(Move::Up, 10, 1, 5), Some(0));
        assert_eq!(s.step(Move::End, 10, 1, 5), Some(9));
        assert_eq!(s.step(Move::Down, 10, 1, 5), Some(9));
        assert_eq!(s.step(Move::PageUp, 10, 1, 5), Some(4));
        assert_eq!(s.step(Move::Home, 10, 1, 5), Some(0));
        assert_eq!(s.step(Move::PageDown, 10, 1, 5), Some(5));
        assert_eq!(s.indices(), [5]);
        // Left / right do nothing in a table.
        assert_eq!(s.step(Move::Right, 10, 1, 5), Some(5));
        // Empty list.
        assert_eq!(Selection::default().target(Move::Down, 0, 1, 5), None);
        // Home / End from nowhere go to the ends.
        let mut s = Selection::default();
        assert_eq!(s.step(Move::End, 10, 1, 5), Some(9));
    }

    #[test]
    fn keyboard_moves_in_a_grid() {
        let mut s = Selection::default();
        s.set_only(7);
        assert_eq!(s.step(Move::Right, 30, 6, 3), Some(8));
        assert_eq!(s.step(Move::Down, 30, 6, 3), Some(14));
        assert_eq!(s.step(Move::Left, 30, 6, 3), Some(13));
        assert_eq!(s.step(Move::Up, 30, 6, 3), Some(7));
        assert_eq!(s.step(Move::Up, 30, 6, 3), Some(1));
        assert_eq!(s.step(Move::Up, 30, 6, 3), Some(0));
        // The last, partial row is clamped.
        s.set_only(25);
        assert_eq!(s.step(Move::Down, 28, 6, 3), Some(27));
        assert_eq!(s.step(Move::PageDown, 28, 6, 3), Some(27));
    }

    #[test]
    fn a_shorter_list_drops_stale_rows() {
        let mut s = sel(&[1, 5, 9]);
        s.retain_below(6);
        assert_eq!(s.indices(), [1, 5]);
        assert_eq!(s.last(), Some(5));
    }
}
