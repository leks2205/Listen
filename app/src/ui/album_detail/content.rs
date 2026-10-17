//! The bottom of the album page: genre / tag / label / mood / grouping pills,
//! external links, the record label line and the "More from" carousels.

use super::format;
use crate::app::album_page::{AlbumPageMsg as P, Carousel};
use crate::app::{Listen, Message, Route};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{album_cover, ellipsize, icon, label, label_w, with_menu};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, space};
use iced::{Alignment, Element, Padding};
use listen_core::model::Album;

const CARD: f32 = 150.0;
const GAP: f32 = 16.0;

fn heading<'a>(state: &Listen, text: &str) -> Element<'a, Message> {
    label_w(text.to_uppercase(), metrics::font_sm(), state.tokens.foreground, Weight::Semibold).into()
}

/// A rounded chip; a button when it leads somewhere.
fn pill<'a>(state: &Listen, text: String, to: Option<Message>) -> Element<'a, Message> {
    let t = state.tokens;
    let body = label(text, metrics::font_md(), t.foreground);
    let style = move |_: &iced::Theme, status: button::Status| button::Style {
        background: Some(iced::Background::Color(if matches!(status, button::Status::Hovered) {
            style::alpha(t.foreground, 0.16)
        } else {
            style::alpha(t.foreground, 0.08)
        })),
        text_color: t.foreground,
        border: iced::border::rounded(999),
        ..Default::default()
    };
    match to {
        Some(msg) => button(body).padding(Padding::from([4.0, 12.0])).style(style).on_press(msg).into(),
        None => container(body)
            .padding(Padding::from([4.0, 12.0]))
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(style::alpha(t.foreground, 0.08))),
                border: iced::border::rounded(999),
                ..Default::default()
            })
            .into(),
    }
}

fn group<'a>(state: &Listen, title: &str, pills: Vec<Element<'a, Message>>) -> Option<Element<'a, Message>> {
    if pills.is_empty() {
        return None;
    }
    let mut r = Row::new().spacing(8);
    for p in pills {
        r = r.push(p);
    }
    Some(column![heading(state, title), r.wrap().vertical_spacing(8)].spacing(metrics::space_sm()).into())
}

/// The metadata groups of an album, in Feishin's order.
fn metadata<'a>(state: &Listen, album: &'a Album) -> Vec<Element<'a, Message>> {
    let info = &album.info;
    let mut out = Vec::new();
    let genres = album
        .genres
        .iter()
        .map(|g| pill(state, g.clone(), Some(Message::Navigate(Route::Genre(g.clone())))))
        .collect();
    out.extend(group(state, if album.genres.len() == 1 { "Genre" } else { "Genres" }, genres));

    let mut tags: Vec<String> = Vec::new();
    if let Some(kind) = &album.release_type {
        tags.push(format::release_type_label(kind));
    }
    for (name, values) in &info.tags {
        if matches!(name.to_lowercase().as_str(), "releasecountry" | "releasestatus") {
            tags.extend(values.iter().cloned());
        }
    }
    if info.explicit {
        tags.push("Explicit".into());
    }
    out.extend(group(state, "Tags", tags.into_iter().map(|t| pill(state, t, None)).collect()));
    out.extend(group(
        state,
        "Record label",
        info.record_labels.iter().map(|l| pill(state, l.clone(), None)).collect(),
    ));
    out.extend(group(state, "Mood", info.moods.iter().map(|m| pill(state, m.clone(), None)).collect()));
    out.extend(group(
        state,
        "Grouping",
        info.groupings.iter().map(|g| pill(state, g.clone(), None)).collect(),
    ));
    out
}

fn links<'a>(state: &Listen, album: &Album) -> Option<Element<'a, Message>> {
    let pills = format::external_links(album, &state.config.settings.artist.external_links)
        .into_iter()
        .map(|l| crate::ui::site_icon::site_button(state, l.label, Message::AlbumPage(P::OpenUrl(l.url))))
        .collect();
    group(state, "External links", pills)
}

fn card<'a>(state: &'a Listen, album: &'a Album) -> Element<'a, Message> {
    let t = state.tokens;
    let cover = with_menu(
        button(album_cover(state, album, CARD, metrics::radius_md()))
            .padding(0)
            .style(style::bare_button(t))
            .on_press(Message::OpenAlbum(album.id.clone())),
        crate::app::menu::MenuTarget::album(album),
    );
    column![
        cover,
        label_w(ellipsize(&album.name, 20), metrics::font_md(), t.foreground, Weight::Medium),
        label(ellipsize(&album.artist_line(), 22), metrics::font_sm(), t.muted),
    ]
    .spacing(2)
    .width(CARD)
    .into()
}

fn carousel<'a>(state: &'a Listen, index: usize, c: &'a Carousel) -> Element<'a, Message> {
    let t = state.tokens;
    let fit =
        (((state.page_width() - 2.0 * metrics::PAGE_PADDING_X + GAP) / (CARD + GAP)).floor() as usize).max(1);
    let mut cards = Row::new().spacing(GAP);
    for album in c.albums.iter().take(fit) {
        cards = cards.push(card(state, album));
    }
    let mut head = Row::new().align_y(Alignment::Center).push(label_w(
        c.title.clone(),
        24.0,
        t.foreground,
        Weight::Bold,
    ));
    head = head.push(space::horizontal());
    if c.genre.is_some() {
        head = head.push(crate::ui::tip::tip(
            state,
            button(icon(Icon::RiRepeat2Line, metrics::font_lg(), t.muted))
                .padding(6)
                .style(style::icon_button(t, false))
                .on_press(Message::AlbumPage(P::Refresh(index))),
            "Refresh",
            None,
            crate::ui::tip::Position::Bottom,
        ));
    }
    column![head, cards].spacing(metrics::space_md()).into()
}

pub fn tail<'a>(state: &'a Listen, album: &'a Album) -> Element<'a, Message> {
    let t = state.tokens;
    let mut col = Column::new().spacing(metrics::space_lg()).padding(Padding {
        top: 24.0,
        right: metrics::PAGE_PADDING_X,
        bottom: 80.0,
        left: metrics::PAGE_PADDING_X,
    });
    let mut groups = metadata(state, album);
    groups.extend(links(state, album));
    if !groups.is_empty() {
        let mut r = Row::new().spacing(metrics::space_xl());
        for g in groups {
            r = r.push(container(g).max_width(420));
        }
        col = col.push(r.wrap().vertical_spacing(metrics::space_lg()));
    }
    if !album.info.record_labels.is_empty() {
        let year = format::release_year(album).map(|y| format!(" {y}")).unwrap_or_default();
        for l in &album.info.record_labels {
            col = col.push(label(format!("℗{year} {l}"), metrics::font_sm(), t.muted));
        }
    }
    for (i, c) in state.album_page.carousels.iter().enumerate() {
        if let Some(c) = c.as_ref().filter(|c| !c.albums.is_empty()) {
            col = col.push(carousel(state, i, c));
        }
    }
    col.into()
}
