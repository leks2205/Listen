//! The playing song's title, artists and album as links (player bar, full-screen player), cut with an
//! ellipsis when too long.

use crate::app::{Listen, Message, Route};
use crate::style::Tokens;
use crate::ui::{ellipsize_px, label_w, text_px};
use iced::font::Weight;
use iced::widget::{Row, button};
use iced::{Alignment, Color, Element};
use listen_core::model::{ArtistRef, Song};

/// Text that opens `to` when clicked (plain text without a target).
pub fn link<'a>(
    t: Tokens,
    text: String,
    size: f32,
    color: Color,
    weight: Weight,
    to: Option<Message>,
) -> Element<'a, Message> {
    let Some(to) = to else { return label_w(text, size, color, weight).into() };
    // The button's text colour (muted, foreground on hover) applies to the label.
    let content = crate::ui::text_in(text, crate::ui::font(weight))
        .size(size)
        .wrapping(iced::widget::text::Wrapping::None);
    button(content)
        .padding(0)
        .style(move |_, status| iced::widget::button::Style {
            text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                t.foreground
            } else {
                color
            },
            snap: true,
            ..Default::default()
        })
        .on_press(to)
        .into()
}

/// The title: a link to the Now Playing page.
pub fn title<'a>(state: &Listen, song: &Song, size: f32, color: Color, px: f32) -> Element<'a, Message> {
    link(
        state.tokens,
        ellipsize_px(&song.title, px, size),
        size,
        color,
        Weight::Medium,
        Some(Message::Navigate(Route::NowPlaying)),
    )
}

/// The artists, each a link to its page, joined by commas, within `px`.
pub fn artists<'a>(
    state: &Listen,
    artists: &[ArtistRef],
    size: f32,
    color: Color,
    px: f32,
) -> Element<'a, Message> {
    let mut row = Row::new().align_y(Alignment::Center);
    let mut left = px;
    for (i, artist) in artists.iter().enumerate() {
        let sep = if i == 0 { "" } else { ", " };
        let room = left - text_px(sep, size);
        // Less than a few letters of room: stop rather than show a stub.
        if room < size * 2.0 {
            break;
        }
        let name = ellipsize_px(&artist.name, room, size);
        left = room - text_px(&name, size);
        if !sep.is_empty() {
            row = row.push(label_w(sep, size, color, Weight::Normal));
        }
        let to = artist.id.clone().filter(|id| !id.is_empty()).map(|id| Message::Navigate(Route::Artist(id)));
        row = row.push(link(state.tokens, name, size, color, Weight::Normal, to));
    }
    row.into()
}

/// The album name, a link to the album page.
pub fn album<'a>(state: &Listen, song: &Song, size: f32, color: Color, px: f32) -> Element<'a, Message> {
    let name = song.album.as_deref().unwrap_or_default();
    let to = song.album_id.clone().filter(|id| !id.is_empty()).map(|id| Message::Navigate(Route::Album(id)));
    link(state.tokens, ellipsize_px(name, px, size), size, color, Weight::Medium, to)
}
