//! Compact layout (window <= 768 px), Feishin's MobileLayout: the page above a 90 px player bar,
//! no side queue, and a floating "Menu" button that opens the sidebar as a 320 px drawer.

use super::{icon, label_w, sidebar};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{button, column, container, mouse_area, opaque, pin, row, space, stack};
use iced::{Alignment, Element, Length, Padding, Point};

/// The Menu button is hidden when the window is shorter than this (Feishin).
pub const MENU_MIN_HEIGHT: f32 = 192.0;
/// Gap between the Menu button and the window edge / player bar (0.75 rem).
const MENU_GAP: f32 = 12.0;

/// Everything below the window bar: page (or full-screen player), divider and the 90 px player bar.
pub fn body(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    if state.fullscreen {
        return super::fullscreen::mobile(state);
    }
    let mut middle = stack![container(super::page::view(state)).height(Length::Fill)];
    if state.window_size.height >= MENU_MIN_HEIGHT {
        let menu = button(
            row![
                icon(Icon::LuMenu, metrics::font_lg(), t.foreground),
                label_w("Menu", metrics::font_md(), t.foreground, Weight::Medium)
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([metrics::space_sm(), metrics::space_md()]))
        .style(style::menu_float_button(t))
        .on_press(Message::ToggleDrawer);
        middle = middle.push(
            container(menu)
                .padding(Padding { left: MENU_GAP, bottom: MENU_GAP, ..Padding::ZERO })
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(iced::alignment::Vertical::Bottom),
        );
    }
    column![
        middle,
        super::divider_h(t),
        container(super::player_bar::view(state))
            .height(metrics::PLAYER_BAR_HEIGHT)
            .style(style::player_bar(t)),
    ]
    .into()
}

/// `root` with the sidebar drawer and volume popover over it. `root` stays the first stack child
/// so its scrollables keep their state.
pub fn overlays<'a>(state: &'a Listen, root: Element<'a, Message>) -> Element<'a, Message> {
    if state.layout_mode() != super::LayoutMode::Compact || state.needs_login() {
        return stack![root].into();
    }
    if state.drawer_open && !state.fullscreen {
        let top =
            if state.config.settings.window.custom_title_bar { super::title_bar::HEIGHT + 1.0 } else { 0.0 };
        let dim = mouse_area(container(space()).width(Length::Fill).height(Length::Fill).style(|_| {
            container::Style {
                background: Some(iced::Background::Color(style::alpha(iced::Color::BLACK, 0.5))),
                ..Default::default()
            }
        }))
        .on_press(Message::CloseDrawer)
        .on_right_press(Message::CloseDrawer);
        let panel = opaque(
            container(sidebar::drawer(state))
                .width(sidebar::DRAWER_WIDTH)
                .height((state.window_size.height - top).max(0.0)),
        );
        return stack![root, dim, pin(panel).position(Point::new(0.0, top))].into();
    }
    if state.volume_popup && !state.fullscreen {
        let at = super::player_bar::compact::popover_position(state.window_size);
        return super::popover::over(
            root,
            super::player_bar::compact::popover_panel(state),
            at,
            Message::CloseVolumePopup,
        );
    }
    stack![root].into()
}
