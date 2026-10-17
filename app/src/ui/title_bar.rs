//! Feishin's "windows" style window bar (30 px) for windows without system decorations: icon + track
//! info on the left (drag to move, double-click to maximize), minimize / maximize / close on the right;
//! right-click opens the desktop's window menu.

use super::{icon, label};
use crate::app::{Listen, Message, WindowAction};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::widget::{button, container, mouse_area, row, space, svg};
use iced::{Alignment, Element, Length, Padding};

pub const HEIGHT: f32 = 30.0;
const ICON_SIZE: f32 = 20.0;

/// The app icon cropped to its tile (the desktop icon has a 16 px margin around the tile).
static APP_ICON: std::sync::LazyLock<svg::Handle> = std::sync::LazyLock::new(|| {
    let svg = include_str!("../../../packaging/io.github.leks2205.Listen.svg").replacen(
        r#"viewBox="0 0 256 256""#,
        r#"viewBox="16 16 224 224""#,
        1,
    );
    svg::Handle::from_memory(svg.into_bytes())
});

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let app_icon = svg(APP_ICON.clone()).width(ICON_SIZE).height(ICON_SIZE);
    let info = state.title();
    if state.config.settings.window.bar_style() == listen_core::settings::WindowBarStyle::Macos {
        return macos(state, info);
    }
    let drag_area = mouse_area(
        container(
            row![app_icon, label(info, metrics::font_sm(), t.foreground)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding(Padding { left: 16.0, ..Padding::ZERO })
        .width(Length::Fill)
        .height(HEIGHT)
        .center_y(HEIGHT)
        .clip(true),
    )
    .on_press(Message::Window(WindowAction::Drag))
    .on_double_click(Message::Window(WindowAction::ToggleMaximize))
    .on_right_press(Message::Window(WindowAction::SystemMenu));

    let win_button = |glyph: Icon, size: f32, close: bool, action: WindowAction, name: &str| {
        super::tip::tip(
            state,
            button(container(icon(glyph, size, t.foreground)).center(Length::Fill))
                .width(50)
                .height(HEIGHT)
                .padding(0)
                .style(style::window_button(t, close))
                .on_press(Message::Window(action)),
            name,
            None,
            super::tip::Position::Bottom,
        )
    };

    container(
        row![
            drag_area,
            row![
                win_button(Icon::RiSubtractLine, 19.0, false, WindowAction::Minimize, "Minimize"),
                win_button(Icon::RiCheckboxBlankLine, 13.0, false, WindowAction::ToggleMaximize, "Maximize"),
                win_button(Icon::RiCloseLine, 19.0, true, WindowAction::Close, "Close"),
            ],
            space().width(0),
        ]
        .align_y(Alignment::Center),
    )
    .height(HEIGHT)
    .width(Length::Fill)
    .style(style::title_bar(t))
    .into()
}

/// Feishin's "macOS" bar: round red / yellow / green buttons on the left,
/// the title centered.
fn macos(state: &Listen, info: String) -> Element<'_, Message> {
    let t = state.tokens;
    let dot = |color: iced::Color, action: WindowAction, name: &str| -> Element<'_, Message> {
        let b = button(space().width(12).height(12))
            .padding(0)
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(iced::Background::Color(if hovered {
                        color
                    } else {
                        iced::Color { a: 0.85, ..color }
                    })),
                    border: iced::Border { radius: 6.0.into(), ..Default::default() },
                    ..Default::default()
                }
            })
            .on_press(Message::Window(action));
        super::tip::tip(state, b, name, None, super::tip::Position::Bottom)
    };
    let buttons = row![
        dot(iced::Color::from_rgb8(0xff, 0x5f, 0x57), WindowAction::Close, "Close"),
        dot(iced::Color::from_rgb8(0xfe, 0xbc, 0x2e), WindowAction::Minimize, "Minimize"),
        dot(iced::Color::from_rgb8(0x28, 0xc8, 0x40), WindowAction::ToggleMaximize, "Maximize"),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let drag_area = mouse_area(
        container(label(info, metrics::font_sm(), t.foreground))
            .width(Length::Fill)
            .height(HEIGHT)
            .center_x(Length::Fill)
            .center_y(HEIGHT)
            .clip(true),
    )
    .on_press(Message::Window(WindowAction::Drag))
    .on_double_click(Message::Window(WindowAction::ToggleMaximize))
    .on_right_press(Message::Window(WindowAction::SystemMenu));
    container(
        row![container(buttons).padding(Padding { left: 12.0, right: 12.0, ..Padding::ZERO }), drag_area]
            .align_y(Alignment::Center),
    )
    .height(HEIGHT)
    .width(Length::Fill)
    .style(style::title_bar(t))
    .into()
}

/// Invisible strips around the window edge that start a resize: 5 px edges
/// and 10 px square corners (diagonal resize).
pub fn resize_edges<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    use iced::mouse::Interaction;
    use iced::window::Direction;
    let edge = |dir: Direction, w: Length, h: Length, cursor: Interaction| -> Element<'a, Message> {
        mouse_area(space().width(w).height(h))
            .on_press(Message::Window(WindowAction::Resize(dir)))
            .interaction(cursor)
            .into()
    };
    const E: f32 = 5.0;
    const C: f32 = 10.0;
    let corner = |dir: Direction, cursor: Interaction| edge(dir, Length::Fixed(C), Length::Fixed(C), cursor);
    let top_bottom =
        |dir: Direction, align: Alignment, nw: (Direction, Interaction), ne: (Direction, Interaction)| {
            row![
                corner(nw.0, nw.1),
                edge(dir, Length::Fill, Length::Fixed(E), Interaction::ResizingVertically),
                corner(ne.0, ne.1),
            ]
            .align_y(align)
        };
    iced::widget::stack![
        content,
        iced::widget::column![
            top_bottom(
                Direction::North,
                Alignment::Start,
                (Direction::NorthWest, Interaction::ResizingDiagonallyDown),
                (Direction::NorthEast, Interaction::ResizingDiagonallyUp),
            ),
            row![
                edge(Direction::West, Length::Fixed(E), Length::Fill, Interaction::ResizingHorizontally),
                space::horizontal(),
                edge(Direction::East, Length::Fixed(E), Length::Fill, Interaction::ResizingHorizontally),
            ]
            .height(Length::Fill),
            top_bottom(
                Direction::South,
                Alignment::End,
                (Direction::SouthWest, Interaction::ResizingDiagonallyUp),
                (Direction::SouthEast, Interaction::ResizingDiagonallyDown),
            ),
        ],
    ]
    .into()
}
