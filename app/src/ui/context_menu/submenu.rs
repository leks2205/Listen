//! The side panel of a row that has children (Play ▸, Set rating ▸).

use super::items::{self, Wiring};
use super::{WIDTH, boxed};
use crate::app::menu::{ContextMenu, Entry, EntryKind, MenuMsg};
use crate::app::{Listen, Message};
use iced::widget::Column;
use iced::{Element, Point, Rectangle, Size};

/// Width of a submenu.
pub const SUB_WIDTH: f32 = 200.0;

/// Height of the main page's rows above row `index` (the submenu aligns with its row).
pub fn offset_of(entries: &[Entry], index: usize) -> f32 {
    entries.iter().take(index).map(items::height).sum()
}

/// The open submenu of `menu`, its size and the rectangle of its parent row
/// relative to the top of the main panel's rows.
pub fn panel<'a>(
    state: &Listen,
    menu: &ContextMenu,
    entries: &[Entry],
) -> Option<(Element<'a, Message>, Size, f32)> {
    let sub = menu.sub.as_ref()?;
    let parent = entries.get(sub.index)?;
    let EntryKind::Submenu(children) = &parent.kind else { return None };
    let mut col = Column::new();
    for (i, child) in children.iter().enumerate() {
        let press = match &child.kind {
            EntryKind::Action(action) => super::items::act(action.clone()),
            _ => Message::Menu(MenuMsg::HoverSub(i)),
        };
        col = col.push(items::entry_row(
            state,
            child,
            sub.focus == Some(i),
            Wiring { hover: Message::Menu(MenuMsg::HoverSub(i)), press },
        ));
    }
    let height: f32 = children.iter().map(items::height).sum::<f32>() + 8.0;
    Some((
        boxed(state, col.into(), SUB_WIDTH),
        Size::new(SUB_WIDTH, height + 10.0),
        offset_of(entries, sub.index),
    ))
}

/// Where the submenu goes: beside its parent row, flipped at the window edges.
pub fn position(main: Point, row_offset: f32, size: Size, window: Size) -> Point {
    // 4 px of padding around the rows of the main panel.
    let row = Rectangle::new(Point::new(main.x, main.y + 4.0 + row_offset), Size::new(WIDTH, super::ITEM));
    crate::ui::popover::place_beside(row, size, window)
}
