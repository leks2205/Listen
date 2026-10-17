//! The smart-playlist editor (Feishin: the "Is smart playlist?" step of the create
//! form and `PlaylistQueryBuilder`): name, description, the nested rule groups and the
//! sort / limit, in a wide modal. The rule groups are in `builder`.

mod builder;

use crate::app::smart_playlist::{Editor, SmartMsg};
use crate::app::{Listen, Message, overlay::OverlayMsg};
use crate::style::{self, Tokens, metrics};
use crate::ui::{label, label_w};
use iced::font::Weight;
use iced::widget::{Column, button, checkbox, column, container, row, scrollable, space, text_input};
use iced::{Alignment, Color, Element, Length, Padding};

const WIDTH: f32 = 860.0;

/// A small "smart" pill for playlists whose songs come from rules.
pub fn badge<'a>(t: Tokens) -> Element<'a, Message> {
    container(label_w("SMART", metrics::font_xs(), t.primary, Weight::Semibold))
        .padding(Padding::from([1.0, 6.0]))
        .style(move |_| container::Style {
            background: Some(style::alpha(t.primary, 0.15).into()),
            border: iced::Border {
                color: style::alpha(t.primary, 0.6),
                width: 1.0,
                radius: metrics::radius_sm().into(),
            },
            ..Default::default()
        })
        .into()
}

fn field<'a>(state: &Listen, name: &'static str, input: Element<'a, Message>) -> Element<'a, Message> {
    column![label_w(name, metrics::font_sm(), state.tokens.muted, Weight::Medium), input]
        .spacing(metrics::space_xs())
        .into()
}

fn text_field<'a>(
    state: &Listen,
    placeholder: &'static str,
    value: &'a str,
    on_input: fn(String) -> SmartMsg,
) -> Element<'a, Message> {
    text_input(crate::i18n::tr(placeholder), value)
        .on_input(move |v| Message::Smart(on_input(v)))
        .size(metrics::font_md())
        .padding(Padding::from([6.0, 10.0]))
        .style(style::input(state.tokens))
        .width(Length::Fill)
        .into()
}

fn body<'a>(state: &'a Listen, e: &'a Editor) -> Element<'a, Message> {
    let t = state.tokens;
    if e.loading {
        return label("Loading the rules...", metrics::font_md(), t.muted).into();
    }
    let flag = |name: &'static str, on: bool, f: fn(bool) -> SmartMsg| {
        checkbox(on)
            .label(name)
            .size(16)
            .text_size(metrics::font_sm())
            .on_toggle(move |v| Message::Smart(f(v)))
    };
    let mut col = Column::new().spacing(metrics::space_md());
    col = col.push(field(state, "Name", text_field(state, "Playlist name", &e.name, SmartMsg::Name)));
    col = col.push(field(
        state,
        "Description",
        text_field(state, "Optional description", &e.comment, SmartMsg::Comment),
    ));
    col = col.push(
        row![
            flag("Public", e.public, SmartMsg::Public),
            flag("Refresh the songs when the playlist is opened", e.sync, SmartMsg::Sync)
        ]
        .spacing(metrics::space_lg()),
    );
    col = col.push(label_w("Query editor", metrics::font_lg(), t.foreground, Weight::Semibold));
    col = col.push(builder::group(state, e, &e.query.root, vec![]));
    col = col.push(builder::sort_and_limit(state, e));
    if let Some(error) = &e.error {
        col = col.push(crate::ui::t_text(error.as_str()).size(metrics::font_sm()).color(t.error));
    }
    col.into()
}

/// The editor modal's panel (`ui::overlay` draws it for `smart_playlist::MODAL_KEY`).
pub fn panel(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let Some(e) = state.smart.editor.as_ref() else { return space().into() };
    let title = if e.id.is_some() { "Edit smart playlist" } else { "Create smart playlist" };
    let height = (state.window_size.height - 220.0).clamp(240.0, 720.0);
    let width = (state.window_size.width - 48.0).clamp(300.0, WIDTH);
    let cancel = button(label("Cancel", metrics::font_md(), t.foreground))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::default_button(t))
        .on_press(Message::Overlay(OverlayMsg::CloseModal));
    let save_label = if e.saving {
        "Saving..."
    } else if e.id.is_some() {
        "Save"
    } else {
        "Create"
    };
    let save = button(label(save_label, metrics::font_md(), Color::WHITE))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::primary_button(t))
        .on_press_maybe((!e.saving && !e.loading).then_some(Message::Smart(SmartMsg::Save)));
    container(
        column![
            label_w(title, metrics::font_lg(), t.foreground, Weight::Semibold),
            scrollable(container(body(state, e)).padding(Padding { right: 12.0, ..Padding::ZERO }))
                .height(Length::Fixed(height))
                .style(style::scrollbar(t)),
            row![space::horizontal(), cancel, save].spacing(metrics::space_sm()).align_y(Alignment::Center),
        ]
        .spacing(metrics::space_lg()),
    )
    .width(width)
    .padding(metrics::space_lg())
    .style(move |_| container::Style {
        background: Some(t.background_alt.into()),
        border: iced::Border { color: t.border, width: 1.0, radius: metrics::radius_md().into() },
        shadow: iced::Shadow {
            color: Color { a: 0.5, ..Color::BLACK },
            offset: iced::Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        ..Default::default()
    })
    .into()
}
