//! Search page (Feishin `features/search`): search-as-you-type with Tracks / Albums / Artists tabs,
//! plus Playlists when the source returns some.

use super::list::{ListView, header};
use super::{icon, label, label_w};
use crate::app::list::{ListId, SearchTab};
use crate::app::search::SearchMsg;
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Row, button, column, container, row, space, text_input};
use iced::{Alignment, Element, Length, Padding};

/// The tabs shown: the three of Feishin, and Playlists when the source found some.
pub fn tabs(state: &Listen) -> Vec<(SearchTab, &'static str, usize)> {
    let r = &state.search.results;
    let mut v = vec![
        (SearchTab::Songs, "Tracks", r.songs.len()),
        (SearchTab::Albums, "Albums", r.albums.len()),
        (SearchTab::Artists, "Artists", r.artists.len()),
    ];
    if !r.playlists.is_empty() || state.search.tab == SearchTab::Playlists {
        v.push((SearchTab::Playlists, "Playlists", r.playlists.len()));
    }
    v
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let s = &state.search;
    let tab = s.tab;
    let id = ListId::Search(tab);
    let config = state.config.settings.lists.config(id.kind());

    let input = row![
        icon(Icon::LuSearch, metrics::font_lg(), t.muted),
        text_input(crate::i18n::tr("Search"), &s.query)
            .font(crate::ui::font_for(&s.query, crate::appearance::current_font()))
            .id(iced::widget::Id::new(super::sidebar::SEARCH_ID))
            .font(super::font_for(&s.query, crate::appearance::current_font()))
            .size(metrics::font_lg())
            .padding(Padding::from([10.0, 14.0]))
            .style(style::input(t))
            .on_input(Message::SearchInput)
            .on_submit(Message::SearchSubmit)
            .width(Length::Fixed(480.0)),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);

    let mut tab_row = Row::new().spacing(metrics::space_xs());
    for (this, name, count) in tabs(state) {
        let active = this == tab;
        let text = if s.query.trim().is_empty() { name.to_string() } else { format!("{name} ({count})") };
        tab_row = tab_row.push(
            button(label_w(
                text,
                metrics::font_md(),
                if active { t.primary } else { t.foreground },
                Weight::Semibold,
            ))
            .padding(Padding::from([6.0, 14.0]))
            .style(style::icon_button(t, active))
            .on_press(Message::Search(SearchMsg::Tab(this))),
        );
    }

    let bar = container(
        row![
            label_w("Search", 36.0, t.foreground, Weight::Bold),
            input,
            tab_row,
            space::horizontal(),
            header::toolbar(state, &id, &config),
        ]
        .spacing(metrics::space_lg())
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 24.0,
        right: metrics::PAGE_PADDING_X,
        bottom: 12.0,
        left: metrics::PAGE_PADDING_X,
    });

    let mut body = column![bar];
    if let Some(e) = &s.error {
        body = body.push(
            container(label(e.clone(), metrics::font_md(), t.error)).padding(Padding::from([0.0, 32.0])),
        );
    }
    let empty = if s.query.trim().is_empty() { "Type to search your library." } else { "No results." };
    let r = &s.results;
    let list: Element<'_, Message> = match tab {
        SearchTab::Songs => {
            ListView::new(state, id, &r.songs).no_header().loading(s.loading).empty(empty).view()
        }
        SearchTab::Albums => {
            ListView::new(state, id, &r.albums).no_header().loading(s.loading).empty(empty).view()
        }
        SearchTab::Artists => {
            ListView::new(state, id, &r.artists).no_header().loading(s.loading).empty(empty).view()
        }
        SearchTab::Playlists => {
            ListView::new(state, id, &r.playlists).no_header().loading(s.loading).empty(empty).view()
        }
    };
    body.push(list).into()
}
