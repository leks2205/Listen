//! Albums page, on the item list component (`ui::list`). `GridLayout` and `grid_layout` are the fixed
//! poster grid still used by pages that are not on the component.

use super::list::ListView;
use super::plural;
use crate::app::list::ListId;
use crate::app::{Listen, Message, ScrollState};
use iced::Element;

/// Grid padding (Feishin: 12px list padding) and per-card text block height.
const PAD: f32 = 12.0;
const GAP: f32 = 16.0;
const TEXT_BLOCK: f32 = 8.0 + 18.0 + 4.0 + 16.0 + 4.0 + 16.0;

#[derive(Debug, Clone, Copy)]
pub struct GridLayout {
    pub columns: usize,
    pub card: f32,
    pub row_height: f32,
}

impl GridLayout {
    /// `[first, last)` item indices of rows intersecting the viewport (plus one row of overscan).
    pub fn visible_items(&self, scroll: ScrollState, total: usize) -> (usize, usize) {
        let height = if scroll.height > 0.0 { scroll.height } else { 1200.0 };
        let first_row = ((scroll.offset - PAD) / self.row_height).floor().max(0.0) as usize;
        let last_row = ((scroll.offset + height) / self.row_height).ceil() as usize + 1;
        let first = (first_row * self.columns).min(total);
        let last = (last_row * self.columns).min(total);
        (first, last)
    }
}

/// Feishin's item-grid breakpoints (by container width).
pub fn grid_layout(width: f32) -> GridLayout {
    let columns = match width {
        w if w >= 2560.0 => 10,
        w if w >= 1920.0 => 8,
        w if w >= 1440.0 => 7,
        w if w >= 1200.0 => 6,
        w if w >= 960.0 => 5,
        w if w >= 768.0 => 4,
        w if w >= 600.0 => 3,
        _ => 2,
    };
    let inner = width - PAD * 2.0;
    let card = ((inner - GAP * (columns as f32 - 1.0)) / columns as f32).floor();
    GridLayout { columns, card, row_height: card + TEXT_BLOCK + GAP }
}

/// The Albums page: the reference for a grid on the item list component
/// (`ui::list`), with selection, the table / grid / detail toggle and the
/// sort / filter / search header (`ui::list::header::page_header`).
pub fn view(state: &Listen) -> Element<'_, Message> {
    let albums = &state.library.albums;
    let count = if state.library.albums_done {
        plural(albums.len(), "album")
    } else {
        format!("{}+ albums", albums.len())
    };
    let searching = !state.list_queries.albums.search.trim().is_empty();
    iced::widget::column![
        super::list::header::page_header(state, &ListId::Albums, "Albums", count),
        ListView::new(state, ListId::Albums, albums)
            .no_header()
            .loading(state.library.albums_loading)
            .done(state.library.albums_done)
            .empty(if searching || state.list_queries.albums.filters.active() {
                "No albums match."
            } else {
                "No albums."
            })
            .view()
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_and_visible_range() {
        assert_eq!(grid_layout(1300.0).columns, 6);
        assert_eq!(grid_layout(500.0).columns, 2);
        let l = grid_layout(1300.0);
        let (first, last) = l.visible_items(ScrollState { offset: 0.0, height: 800.0, width: 1300.0 }, 1000);
        assert_eq!(first, 0);
        assert!(last >= l.columns * 3 && last <= l.columns * 6);
        let far = ScrollState { offset: l.row_height * 50.0, height: 800.0, width: 1300.0 };
        let (first, _) = l.visible_items(far, 1000);
        assert_eq!(first, 49 * l.columns);
        assert_eq!(l.visible_items(far, 10), (10, 10));
    }
}
