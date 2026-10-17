//! The right-click menu, drawn over everything at the cursor.
//!
//! * `mod.rs` (this file): [`overlay`], the box around a page and its placing.
//! * `items.rs`: one row. `submenu.rs`: the side panel of a row with children.
//! * `footer.rs`: the preview footer. `pages.rs`: playlist picker, rename, delete.
//!
//! The rows themselves are declared in `app::menu::{song, album, ...}`.

mod footer;
mod items;
mod pages;
mod submenu;

pub use footer::{Preview, preview};
pub use pages::SEARCH_ID as PICKER_SEARCH_ID;

use crate::app::menu::{ContextMenu, EntryKind, MenuMsg, MenuPage};
use crate::app::{Listen, Message};
use crate::style::{self, metrics};
use iced::widget::{Column, container};
use iced::{Background, Element, Shadow, Vector, border};
use items::Wiring;
use listen_source::Capabilities;

pub(super) const WIDTH: f32 = 256.0;
pub(super) const ITEM: f32 = 34.0;
pub(super) const LIST_MAX: f32 = 280.0;

pub(super) fn boxed_width() -> f32 {
    WIDTH
}

/// The open menu over `root` (with a backdrop that closes it). `root` is
/// always the first child of a stack, open or closed: a different widget tree
/// would drop the state of every scrollable below and jump them to the top.
pub fn overlay<'a>(state: &'a Listen, root: Element<'a, Message>) -> Element<'a, Message> {
    let Some(menu) = &state.menu else { return iced::widget::stack![root].into() };
    let entries = crate::app::menu::entries(state, menu);
    let (panel, height) = panel(state, menu, &entries);
    // Open at the cursor, flipped/shifted to stay inside the window.
    let at = super::popover::place(menu.at, iced::Size::new(WIDTH, height), state.window_size);
    let mut panels = vec![(panel, at)];
    if matches!(menu.page, MenuPage::Main)
        && let Some((sub, size, offset)) = submenu::panel(state, menu, &entries)
    {
        panels.push((sub, submenu::position(at, offset, size, state.window_size)));
    }
    super::popover::over_all(root, panels, Message::Menu(MenuMsg::Close))
}

/// The box around a page.
fn boxed<'a>(state: &Listen, body: Element<'a, Message>, width: f32) -> Element<'a, Message> {
    let t = state.tokens;
    let boxed = container(body).width(width).padding(4).style(move |_| container::Style {
        background: Some(Background::Color(t.background)),
        border: border::rounded(metrics::radius_md()).color(t.border).width(1),
        shadow: Shadow {
            color: style::alpha(iced::Color::BLACK, 0.45),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..Default::default()
    });
    // Clicks inside the menu must not reach the backdrop.
    iced::widget::opaque(boxed)
}

/// The menu box and its height (for placing it).
fn panel<'a>(
    state: &'a Listen,
    menu: &'a ContextMenu,
    entries: &[crate::app::menu::Entry],
) -> (Element<'a, Message>, f32) {
    let (body, height) = match &menu.page {
        MenuPage::Main => {
            let mut col = Column::new();
            for (i, entry) in entries.iter().enumerate() {
                let press = match &entry.kind {
                    EntryKind::Action(action) => items::act(action.clone()),
                    _ => match &entry.primary {
                        Some(action) => items::act(action.clone()),
                        None => Message::Menu(MenuMsg::ToggleSub(i)),
                    },
                };
                col = col.push(items::entry_row(
                    state,
                    entry,
                    menu.focus == Some(i),
                    Wiring { hover: Message::Menu(MenuMsg::Hover(i)), press },
                ));
            }
            col = col.push(footer::view(state, &menu.target));
            let rows: f32 = entries.iter().map(items::height).sum();
            (col.into(), rows + footer::HEIGHT)
        }
        MenuPage::Playlists { list, new_name, search } => {
            let cx = crate::app::menu::Ctx::new(state, &menu.target, None);
            let server = menu.target.server().or(state.config.active_server.as_ref());
            let ytm =
                cx.source.as_ref().is_some_and(|s| s.kind() == listen_core::model::SourceKind::YouTubeMusic);
            pages::playlists(
                state,
                pages::Picker {
                    list: list.as_ref(),
                    new_name,
                    search,
                    can_create: cx.caps.contains(Capabilities::PLAYLIST_CREATE),
                    recent: server.and_then(|s| state.playlist_page.recent.get(s)).map(String::as_str),
                    skip_duplicates: (!ytm).then_some(state.config.settings.lists.skip_duplicates),
                },
            )
        }
        MenuPage::Rename { name } => pages::rename(state, name),
        MenuPage::ConfirmDelete => pages::confirm_delete(state, &menu.target),
    };
    (boxed(state, body, WIDTH), height + 10.0)
}
