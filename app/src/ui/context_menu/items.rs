//! One row of a menu or submenu.

use super::ITEM;
use crate::app::menu::{Entry, EntryKind, MenuAction, MenuMsg};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{divider_h, icon, label_w};
use iced::font::Weight;
use iced::widget::{button, container, mouse_area, row};
use iced::{Alignment, Background, Element, Length, Padding, border};

/// Height of a divider row.
pub const DIVIDER: f32 = 9.0;

pub fn height(entry: &Entry) -> f32 {
    if matches!(entry.kind, EntryKind::Divider) { DIVIDER } else { ITEM }
}

/// What a row sends: where the pointer is on it and what a click does.
pub struct Wiring {
    pub hover: Message,
    pub press: Message,
}

pub fn act(action: MenuAction) -> Message {
    Message::Menu(MenuMsg::Do(action))
}

/// A row for `entry`; `focused` is the keyboard / pointer highlight.
pub fn entry_row<'a>(state: &Listen, entry: &Entry, focused: bool, wiring: Wiring) -> Element<'a, Message> {
    let t = state.tokens;
    if matches!(entry.kind, EntryKind::Divider) {
        return container(divider_h(t)).padding(Padding::from([4.0, 0.0])).height(DIVIDER).into();
    }
    let color = if entry.enabled { t.foreground } else { t.muted };
    let mut content = row![
        icon(entry.icon, metrics::font_lg(), color),
        container(label_w(entry.label.clone(), metrics::font_sm(), color, Weight::Medium))
            .width(Length::Fill)
            .clip(true)
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    if entry.is_submenu() {
        content = content.push(icon(Icon::LuChevronRight, metrics::font_md(), t.muted));
    }
    // iced puts a button's content at the top; center it so the hover box
    // sits evenly around the text.
    let button = button(container(content).center_y(Length::Fill))
        .width(Length::Fill)
        .height(ITEM)
        .padding(Padding::from([0.0, metrics::space_md()]))
        .style(move |_, status| {
            let hot = focused || matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: hot.then_some(Background::Color(t.surface)),
                text_color: t.foreground,
                border: border::rounded(t.radius_sm),
                shadow: Default::default(),
                snap: true,
            }
        })
        .on_press_maybe(entry.enabled.then_some(wiring.press));
    mouse_area(button).on_enter(wiring.hover).into()
}

/// A plain row (not an `Entry`): for the pages that list playlists.
pub fn plain<'a>(state: &Listen, glyph: Icon, title: String, msg: Option<Message>) -> Element<'a, Message> {
    let t = state.tokens;
    let color = if msg.is_some() { t.foreground } else { t.muted };
    button(
        container(
            row![
                icon(glyph, metrics::font_lg(), color),
                container(label_w(title, metrics::font_md(), color, Weight::Medium))
                    .width(Length::Fill)
                    .clip(true)
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
    .on_press_maybe(msg)
    .into()
}
