//! The gear's dialog (Feishin's full-screen player settings form): the
//! dynamic background, blur, opacity, cover size, alignment, title display
//! and the metadata badges. Changes apply live behind the dialog.

use crate::app::fullscreen_state::{FsSetting, FullscreenMsg};
use crate::app::{Listen, Message};
use crate::style::{self, metrics};
use crate::ui::label_w;
use crate::ui::overlay::ModalParts;
use iced::font::Weight;
use iced::widget::{Column, button, checkbox, column, container, row, scrollable, slider, toggler};
use iced::{Alignment, Element, Length, Padding};
use listen_core::settings::{BADGES, ItemAlignment, TitleDisplay};

fn set(s: FsSetting) -> Message {
    Message::Fullscreen(FullscreenMsg::Setting(s))
}

fn field<'a>(state: &Listen, name: &'static str, control: Element<'a, Message>) -> Element<'a, Message> {
    let t = state.tokens;
    row![
        container(label_w(name, metrics::font_md(), t.foreground, Weight::Medium))
            .width(Length::FillPortion(2)),
        container(control).width(Length::FillPortion(3)).align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(metrics::space_md())
    .align_y(Alignment::Center)
    .into()
}

fn switch<'a>(on: bool, f: impl Fn(bool) -> FsSetting + 'a) -> Element<'a, Message> {
    toggler(on).on_toggle(move |v| set(f(v))).size(20).into()
}

/// A slider with its value in words.
fn amount<'a>(
    state: &Listen,
    range: std::ops::RangeInclusive<u32>,
    value: u32,
    unit: &'static str,
    f: impl Fn(u32) -> FsSetting + 'a,
) -> Element<'a, Message> {
    let t = state.tokens;
    let (lo, hi) = (*range.start() as f32, *range.end() as f32);
    row![
        slider(lo..=hi, value as f32, move |v| set(f(v.round() as u32))).width(150),
        container(crate::ui::label(format!("{value}{unit}"), metrics::font_md(), t.muted)).width(52),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center)
    .into()
}

/// Two or three exclusive choices as buttons.
fn choice<'a, T: PartialEq + Copy + 'a>(
    state: &Listen,
    options: &[(T, &'static str)],
    current: T,
    f: impl Fn(T) -> FsSetting + 'a,
) -> Element<'a, Message> {
    let t = state.tokens;
    let mut r = row![].spacing(metrics::space_xs());
    for (value, name) in options {
        let active = *value == current;
        r = r.push(
            button(label_w(
                *name,
                metrics::font_sm(),
                if active { t.foreground } else { t.muted },
                Weight::Semibold,
            ))
            .padding(Padding::from([4.0, 10.0]))
            .style(style::nav_item(t, active))
            .on_press(set(f(*value))),
        );
    }
    r.into()
}

fn form(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let s = &state.config.settings.fullscreen;
    let mut col = Column::new().spacing(metrics::space_md()).width(Length::Fill);
    col = col
        .push(field(state, "Dynamic background", switch(s.dynamic_background, FsSetting::DynamicBackground)))
        .push(field(
            state,
            "Use the cover as background",
            switch(s.dynamic_is_image, FsSetting::DynamicIsImage),
        ))
        .push(field(
            state,
            "Image blur",
            amount(state, 0..=6, s.image_blur.round() as u32, " rem", |v| FsSetting::ImageBlur(v as f32)),
        ))
        .push(field(
            state,
            "Opacity",
            amount(state, 0..=100, u32::from(s.opacity), " %", |v| FsSetting::Opacity(v as u8)),
        ))
        .push(field(
            state,
            "Use the cover's aspect ratio",
            switch(s.use_image_aspect_ratio, FsSetting::AspectRatio),
        ))
        .push(field(
            state,
            "Cover size",
            amount(state, 55..=100, u32::from(s.cover_size), " %", |v| FsSetting::CoverSize(v as u8)),
        ))
        .push(field(
            state,
            "Alignment",
            choice(
                state,
                &[
                    (ItemAlignment::Left, "Left"),
                    (ItemAlignment::Center, "Center"),
                    (ItemAlignment::Right, "Right"),
                ],
                s.alignment,
                FsSetting::Alignment,
            ),
        ))
        .push(field(
            state,
            "Title",
            choice(
                state,
                &[(TitleDisplay::Scroll, "Scrolling"), (TitleDisplay::MultiLine, "Wrapped")],
                s.title_display,
                FsSetting::TitleDisplay,
            ),
        ));
    if s.title_display == TitleDisplay::MultiLine {
        col = col.push(field(
            state,
            "Title lines",
            amount(state, 1..=10, u32::from(s.title_lines), "", |v| FsSetting::TitleLines(v as u8)),
        ));
    }
    // The badges under the title.
    let items = &state.config.settings.player_bar.info_items;
    let mut badges = Column::new().spacing(metrics::space_xs());
    for pair in BADGES.chunks(2) {
        let mut r = row![].spacing(metrics::space_md());
        for (id, name) in pair {
            let id = (*id).to_string();
            r = r.push(
                container(
                    checkbox(items.contains(&id))
                        .label(*name)
                        .on_toggle(move |v| set(FsSetting::Badge(id.clone(), v)))
                        .size(18)
                        .text_size(metrics::font_md()),
                )
                .width(Length::FillPortion(1)),
            );
        }
        badges = badges.push(r);
    }
    col = col.push(label_w("Metadata badges", metrics::font_md(), t.foreground, Weight::Medium)).push(badges);
    scrollable(container(col).padding(Padding { right: 12.0, ..Padding::ZERO }))
        .height(Length::Fixed(440.0))
        .style(style::scrollbar(t))
        .into()
}

/// The dialog opened by the gear.
pub fn modal(state: &Listen) -> Option<ModalParts<'_>> {
    let t = state.tokens;
    let done = button(label_w("Done", metrics::font_md(), iced::Color::WHITE, Weight::Medium))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::primary_button(t))
        .on_press(Message::Overlay(crate::app::overlay::OverlayMsg::CloseModal));
    Some(("Full-screen player", column![form(state)].into(), vec![done.into()]))
}
