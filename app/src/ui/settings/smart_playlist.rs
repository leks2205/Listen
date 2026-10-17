//! General tab: custom fields of the smart-playlist builder (Feishin
//! `QueryBuilderSettings`), shown when the server supports smart playlists.

use super::{line, section};
use crate::app::smart_playlist::QueryTagSetting;
use crate::app::{Listen, Message, Setting};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{icon, label};
use iced::widget::{Column, button, pick_list, row, text_input};
use iced::{Alignment, Element, Length, Padding};
use listen_source::nsp::fields::Kind;

fn change(c: QueryTagSetting) -> Message {
    Message::Setting(Setting::QueryBuilder(c))
}

/// The "Query builder" section, or `None` where smart playlists do not exist.
pub fn query_builder(state: &Listen) -> Option<Element<'_, Message>> {
    if !state.smart_supported() {
        return None;
    }
    let t = state.tokens;
    let input =
        |placeholder: &'static str, value: &str, on: fn(usize, String) -> QueryTagSetting, i: usize| {
            text_input(crate::i18n::tr(placeholder), value)
                .on_input(move |v| change(on(i, v)))
                .size(metrics::font_sm())
                .padding(Padding::from([5.0, 8.0]))
                .style(style::input(t))
                .width(Length::FillPortion(2))
        };
    let kinds: Vec<&'static str> = ["string", "number", "boolean", "date", "playlist"].to_vec();
    let mut fields = Column::new().spacing(metrics::space_sm());
    for (i, f) in state.config.settings.query_builder.tags.iter().enumerate() {
        let kind = Kind::parse(&f.kind).name();
        fields = fields.push(
            row![
                input("Label", &f.label, QueryTagSetting::Label, i),
                pick_list(kinds.clone(), Some(kind), move |k: &str| change(QueryTagSetting::Kind(
                    i,
                    k.into()
                )))
                .text_size(metrics::font_sm())
                .width(Length::FillPortion(1)),
                input("Tag", &f.tag, QueryTagSetting::Tag, i),
                crate::ui::tip::tip(
                    state,
                    button(icon(Icon::LuX, metrics::font_md(), t.muted))
                        .padding(5)
                        .style(style::icon_button(t, false))
                        .on_press(change(QueryTagSetting::Remove(i))),
                    "Remove",
                    None,
                    crate::ui::tip::Position::Left,
                ),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
        );
    }
    fields = fields.push(
        button(label("Add", metrics::font_sm(), t.foreground))
            .padding(Padding::from([4.0, 12.0]))
            .style(style::default_button(t))
            .on_press(change(QueryTagSetting::Add)),
    );
    Some(section(
        state,
        "Query builder",
        vec![line(state, "Custom fields", "Add custom fields to use in query builders", fields.into())],
    ))
}
