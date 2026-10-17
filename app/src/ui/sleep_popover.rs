//! The sleep timer popover (Feishin's `sleep-timer-button.tsx`): status with
//! countdown and Cancel, End of song / End of album, a grid of minute
//! presets and a custom hours : minutes : seconds form.

use super::{label, label_w};
use crate::app::fullscreen_state::FullscreenMsg;
use crate::app::sleep_timer::{CustomField, PRESETS, SleepMode, SleepMsg, custom_seconds, format_remaining};
use crate::app::{Listen, Message};
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{
    Column, Row, button, column, container, mouse_area, opaque, row, space, stack, text_input,
};
use iced::{Alignment, Background, Element, Length, Padding, Shadow, Vector, border};

pub const WIDTH: f32 = 320.0;

fn act(m: SleepMsg) -> Message {
    Message::Fullscreen(FullscreenMsg::Sleep(m))
}

/// `root` with the popover above the right end of the player bar while it is open.
pub fn overlay<'a>(state: &'a Listen, root: Element<'a, Message>) -> Element<'a, Message> {
    if !state.fs.sleep_ui.open || state.needs_login() {
        return stack![root].into();
    }
    let backdrop = mouse_area(space().width(Length::Fill).height(Length::Fill))
        .on_press(act(SleepMsg::Close))
        .on_right_press(act(SleepMsg::Close));
    let placed = container(opaque(panel(state)))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(Padding {
            right: metrics::space_lg(),
            bottom: metrics::PLAYER_BAR_HEIGHT + metrics::space_sm(),
            ..Padding::ZERO
        });
    stack![root, backdrop, placed].into()
}

/// A full-width outlined button.
fn outline<'a>(state: &Listen, text: String, msg: Option<Message>) -> Element<'a, Message> {
    let t = state.tokens;
    let color = if msg.is_some() { t.foreground } else { t.muted };
    button(container(label_w(text, metrics::font_md(), color, Weight::Medium)).center_x(Length::Fill))
        .width(Length::Fill)
        .padding(Padding::from([8.0, 12.0]))
        .style(style::default_button(t))
        .on_press_maybe(msg)
        .into()
}

fn card<'a>(state: &Listen, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let t = state.tokens;
    container(content)
        .padding(metrics::space_md())
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.surface)),
            border: border::rounded(metrics::radius_md()),
            ..Default::default()
        })
        .into()
}

pub fn panel(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let sleep = &state.sleep_timer;

    // Status: what is running, with Cancel.
    let mut status =
        Column::new().spacing(metrics::space_xs()).align_x(Alignment::Center).width(Length::Fill);
    status = status.push(label_w("Sleep timer", metrics::font_md(), t.foreground, Weight::Semibold));
    if let Some(timer) = sleep {
        let text = match timer.mode {
            SleepMode::EndOfSong => "End of song".to_string(),
            SleepMode::EndOfAlbum => "End of album".to_string(),
            SleepMode::Timed => format_remaining(timer.remaining),
        };
        status = status.push(label_w(text, metrics::font_xl(), t.primary, Weight::Bold)).push(
            button(label_w("Cancel", metrics::font_sm(), t.muted, Weight::Medium))
                .padding(Padding::from([2.0, 8.0]))
                .style(style::icon_button(t, false))
                .on_press(act(SleepMsg::Cancel)),
        );
    }

    // End of song / album. Track shuffle scatters the albums: no end of album then.
    let ends = column![
        outline(state, "End of song".into(), Some(act(SleepMsg::EndOfSong))),
        outline(state, "End of album".into(), (!state.queue.shuffle()).then(|| act(SleepMsg::EndOfAlbum))),
    ]
    .spacing(metrics::space_sm());

    // 5 .. 240 minutes, three to a row.
    let mut grid = Column::new().spacing(metrics::space_sm());
    for chunk in PRESETS.chunks(3) {
        let mut r = Row::new().spacing(metrics::space_sm());
        for m in chunk {
            r = r.push(outline(state, m.to_string(), Some(act(SleepMsg::Minutes(*m)))));
        }
        grid = grid.push(r);
    }

    let custom: Element<'_, Message> = match &state.fs.sleep_ui.custom {
        None => outline(state, "Custom".into(), Some(act(SleepMsg::CustomOpen))),
        Some(fields) => {
            let input = |field: CustomField, value: &str, unit: &'static str| {
                row![
                    text_input(crate::i18n::tr("0"), value)
                        .on_input(move |v| act(SleepMsg::CustomEdit(field, v)))
                        .size(metrics::font_md())
                        .padding(Padding::from([5.0, 8.0]))
                        .style(style::input(t))
                        .width(Length::Fill),
                    label(unit, metrics::font_sm(), t.muted),
                ]
                .spacing(metrics::space_xs())
                .align_y(Alignment::Center)
                .width(Length::Fill)
            };
            let start = custom_seconds(fields) > 0;
            column![
                row![
                    input(CustomField::Hours, &fields[0], "hr"),
                    label(":", metrics::font_md(), t.muted),
                    input(CustomField::Minutes, &fields[1], "min"),
                    label(":", metrics::font_md(), t.muted),
                    input(CustomField::Seconds, &fields[2], "sec"),
                ]
                .spacing(metrics::space_xs())
                .align_y(Alignment::Center),
                row![
                    button(
                        container(label_w("Set", metrics::font_md(), iced::Color::WHITE, Weight::Medium))
                            .center_x(Length::Fill)
                    )
                    .width(Length::Fill)
                    .padding(Padding::from([6.0, 12.0]))
                    .style(style::primary_button(t))
                    .on_press_maybe(start.then(|| act(SleepMsg::CustomStart))),
                    outline(state, "Cancel".into(), Some(act(SleepMsg::CustomClose))),
                ]
                .spacing(metrics::space_sm()),
            ]
            .spacing(metrics::space_sm())
            .into()
        }
    };

    container(
        column![card(state, status), card(state, ends), card(state, grid), card(state, custom)]
            .spacing(metrics::space_sm()),
    )
    .width(WIDTH)
    .padding(metrics::space_sm())
    .style(move |_| container::Style {
        background: Some(Background::Color(t.background_alt)),
        border: border::rounded(metrics::radius_md()).color(t.border).width(1),
        shadow: Shadow {
            color: style::alpha(iced::Color::BLACK, 0.45),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..Default::default()
    })
    .into()
}
