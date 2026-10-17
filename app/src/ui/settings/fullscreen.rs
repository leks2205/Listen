//! Settings of the full-screen player and the player bar's click, cover and right-hand buttons.

use super::{line, section, switch_with};
use crate::app::fullscreen_state::{FsSetting, FullscreenMsg};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{icon, label, label_w};
use iced::font::Weight;
use iced::widget::{button, checkbox, row, slider};
use iced::{Alignment, Element, Padding};
use listen_core::settings::RIGHT_CONTROLS;

fn set(s: FsSetting) -> Message {
    Message::Fullscreen(FullscreenMsg::Setting(s))
}

/// Rows appended to the "Player bar" section.
pub(super) fn player_bar_rows(state: &Listen) -> Vec<Element<'_, Message>> {
    let t = state.tokens;
    let b = &state.config.settings.player_bar;
    let mut rows = vec![
        line(
            state,
            "Player bar opens full-screen player",
            "A click on the empty area of the player bar opens the full-screen player",
            switch_with(b.open_drawer, |v| set(FsSetting::OpenDrawer(v))),
        ),
        line(
            state,
            "Expand album art button",
            "Show the expand button on the player bar's cover; turning it off also collapses the sidebar image",
            switch_with(state.config.settings.sidebar.show_image, |v| set(FsSetting::ExpandArt(v))),
        ),
        line(
            state,
            "Show remaining time",
            "Show the time left instead of the duration (a click on the duration switches too)",
            switch_with(b.show_time_remaining, |v| set(FsSetting::ShowRemaining(v))),
        ),
        line(
            state,
            "Right-hand buttons",
            "Select the buttons and the order in which they appear on the right of the player bar",
            row![].into(),
        ),
    ];
    // Every known button, in the saved order (new ones last).
    let mut items = b.right_controls.clone();
    for (id, _) in RIGHT_CONTROLS {
        if !items.iter().any(|c| c.id == *id) {
            items.push(listen_core::settings::RightControl { id: (*id).into(), visible: true });
        }
    }
    let last = items.len().saturating_sub(1);
    let arrow = |glyph: Icon, to: Option<Message>| {
        crate::ui::tip::tip_icon(
            state,
            button(icon(glyph, metrics::font_md(), t.muted))
                .padding(4)
                .style(style::icon_button(t, false))
                .on_press_maybe(to),
            glyph,
            crate::ui::tip::Position::Left,
        )
    };
    for (i, item) in items.iter().enumerate() {
        let name = RIGHT_CONTROLS.iter().find(|(id, _)| *id == item.id).map_or("", |(_, n)| *n);
        rows.push(
            row![
                checkbox(item.visible)
                    .label(name)
                    .on_toggle(move |v| set(FsSetting::RightVisible(i, v)))
                    .size(18)
                    .text_size(metrics::font_md()),
                iced::widget::space().width(iced::Length::Fill),
                arrow(Icon::LuChevronUp, (i > 0).then(|| set(FsSetting::RightMove(i, -1)))),
                arrow(Icon::LuChevronDown, (i < last).then(|| set(FsSetting::RightMove(i, 1)))),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .padding(Padding::from([0.0, 8.0]))
            .into(),
        );
    }
    rows
}

/// The "Full-screen player" section.
pub(super) fn full_screen(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let minutes = state.config.settings.fullscreen.auto_open_minutes;
    let unit = if minutes == 0 { "off".to_string() } else { format!("{minutes} min") };
    section(
        state,
        "Full-screen player",
        vec![
            line(
                state,
                "Full-screen auto-open timeout",
                "Open the full-screen player after this many minutes without input while a song plays (0 = off)",
                row![
                    slider(0.0..=120.0, minutes as f32, |v| set(
                        FsSetting::AutoOpenMinutes(v.round() as u32)
                    ))
                    .width(160),
                    label(unit, metrics::font_md(), t.muted),
                ]
                .spacing(metrics::space_sm())
                .align_y(Alignment::Center)
                .into(),
            ),
            line(
                state,
                "Configure",
                "Background, blur, opacity, cover size, alignment, title and metadata badges",
                button(label_w("Configure…", metrics::font_md(), t.foreground, Weight::Medium))
                    .padding(Padding::from([6.0, 14.0]))
                    .style(style::default_button(t))
                    .on_press(Message::Fullscreen(FullscreenMsg::OpenSettings))
                    .into(),
            ),
        ],
    )
}
