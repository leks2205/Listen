//! The playlist's client-side song filters (Feishin
//! `client-side-song-filters.tsx`): favorited, rated, genres, artists, album
//! artists with "match all / any", and a year range.

use crate::app::playlists::FilterEdit;
use crate::app::playlists::PlaylistPageMsg as P;
use crate::app::playlists::rows::{FilterKind, options};
use crate::app::{Listen, Message};
use crate::style::{self, metrics};
use crate::ui::overlay::ModalParts;
use crate::ui::{label, label_w};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, row, scrollable, text_input};
use iced::{Alignment, Color, Element, Length, Padding};

/// Most values a list filter offers.
const MAX_OPTIONS: usize = 60;

fn edit(e: FilterEdit) -> Message {
    Message::PlaylistPage(P::Filter(e))
}

fn chip<'a>(state: &Listen, text: String, on: bool, m: Message) -> Element<'a, Message> {
    let t = state.tokens;
    button(label_w(text, metrics::font_sm(), if on { Color::WHITE } else { t.foreground }, Weight::Medium))
        .padding(Padding::from([4.0, 10.0]))
        .style(move |theme, status| {
            if on { style::primary_button(t)(theme, status) } else { style::default_button(t)(theme, status) }
        })
        .on_press(m)
        .into()
}

fn tri<'a>(
    state: &Listen,
    title: &'static str,
    value: Option<bool>,
    names: [&'static str; 3],
    make: fn(Option<bool>) -> FilterEdit,
) -> Element<'a, Message> {
    let t = state.tokens;
    column![
        label_w(title, metrics::font_sm(), t.muted, Weight::Semibold),
        row![
            chip(state, names[0].into(), value.is_none(), edit(make(None))),
            chip(state, names[1].into(), value == Some(true), edit(make(Some(true)))),
            chip(state, names[2].into(), value == Some(false), edit(make(Some(false)))),
        ]
        .spacing(metrics::space_xs())
    ]
    .spacing(metrics::space_xs())
    .into()
}

fn values<'a>(
    state: &'a Listen,
    title: &'static str,
    kind: FilterKind,
    chosen: &[String],
    all: bool,
) -> Option<Element<'a, Message>> {
    let t = state.tokens;
    let songs = state.open_playlist_detail().map(|d| d.songs.as_slice()).unwrap_or_default();
    let found = options(songs, kind);
    if found.is_empty() {
        return None;
    }
    let mut chips = Row::new().spacing(metrics::space_xs());
    for (key, name, count) in found.into_iter().take(MAX_OPTIONS) {
        let on = chosen.contains(&key);
        chips = chips.push(chip(state, format!("{name} ({count})"), on, edit(FilterEdit::Toggle(kind, key))));
    }
    let mode = row![
        chip(state, "All".into(), all, edit(FilterEdit::MatchAll(kind, true))),
        chip(state, "Any".into(), !all, edit(FilterEdit::MatchAll(kind, false))),
    ]
    .spacing(metrics::space_xs());
    Some(
        column![
            row![
                label_w(title, metrics::font_sm(), t.muted, Weight::Semibold),
                iced::widget::space::horizontal(),
                label("Match", metrics::font_xs(), t.muted),
                mode
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center),
            chips.wrap().vertical_spacing(metrics::space_xs())
        ]
        .spacing(metrics::space_xs())
        .into(),
    )
}

pub fn modal(state: &Listen) -> Option<ModalParts<'_>> {
    let t = state.tokens;
    let f = &state.playlist_page.filters;
    let year = |hint: &'static str, v: Option<i32>, make: fn(String) -> FilterEdit| {
        text_input(crate::i18n::tr(hint), &v.map(|y| y.to_string()).unwrap_or_default())
            .size(metrics::font_md())
            .padding(Padding::from([6.0, 8.0]))
            .style(style::input(t))
            .on_input(move |s| edit(make(s)))
            .width(Length::Fill)
    };
    let mut body = Column::new().spacing(metrics::space_md());
    body = body
        .push(tri(
            state,
            "Favorites",
            f.favorite,
            ["Any", "Favorited", "Not favorited"],
            FilterEdit::Favorite,
        ))
        .push(tri(state, "Rating", f.rated, ["Any", "Rated", "Not rated"], FilterEdit::Rated));
    body = body.push(
        column![
            label_w("Year", metrics::font_sm(), t.muted, Weight::Semibold),
            row![year("From", f.year_min, FilterEdit::YearMin), year("To", f.year_max, FilterEdit::YearMax)]
                .spacing(metrics::space_sm())
        ]
        .spacing(metrics::space_xs()),
    );
    for (title, kind, chosen, all) in [
        ("Genres", FilterKind::Genres, &f.genres, f.genres_all),
        ("Artists", FilterKind::Artists, &f.artists, f.artists_all),
        ("Album artists", FilterKind::AlbumArtists, &f.album_artists, f.album_artists_all),
    ] {
        body = body.extend(values(state, title, kind, chosen, all));
    }
    let scroll = container(
        scrollable(container(body).padding(Padding { right: 12.0, ..Padding::ZERO }))
            .style(style::scrollbar(t)),
    )
    .height(Length::Fixed(380.0));
    let reset = button(label(" Reset ", metrics::font_md(), t.foreground))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::default_button(t))
        .on_press_maybe(f.active().then(|| Message::PlaylistPage(P::ResetFilters)));
    let close = button(label("Close", metrics::font_md(), Color::WHITE))
        .padding(Padding::from([6.0, 16.0]))
        .style(style::primary_button(t))
        .on_press(Message::Overlay(crate::app::overlay::OverlayMsg::CloseModal));
    Some(("Filters", scroll.into(), vec![reset.into(), close.into()]))
}
