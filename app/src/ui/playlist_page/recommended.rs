//! The "Recommended" strip under a playlist's rows: songs that fit the playlist, each with a
//! "+" that adds it in one click (a Spotify-style playlist footer). The songs,
//! their cache and the error state live in [`crate::app::playlists::Recommended`].

use super::super::{ellipsize_px, icon, label, label_w, song_cover, with_menu};
use crate::app::menu::MenuTarget;
use crate::app::playlists::PlaylistPageMsg;
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{button, column, container, row, space};
use iced::{Alignment, Element, Length, Padding};
use listen_core::model::Song;

/// The title line with the refresh button.
const HEADER: f32 = 32.0;
/// One recommended row.
const ROW: f32 = 44.0;
/// Air above the strip and under its last row.
const PAD: f32 = 16.0;
const COVER: f32 = 34.0;

/// Height the strip takes under the playlist's rows.
pub fn height(state: &Listen, id: &str) -> f32 {
    let strip = &state.playlist_page.recommended;
    if !strip.shown(id) {
        return 8.0;
    }
    let error = if strip.error.is_some() { 32.0 } else { 0.0 };
    PAD + HEADER + strip.songs.len() as f32 * ROW + error + PAD
}

/// The strip of playlist `id`.
pub fn view<'a>(state: &'a Listen, id: &str) -> Element<'a, Message> {
    let t = state.tokens;
    let strip = &state.playlist_page.recommended;
    if !strip.shown(id) {
        return space().height(8.0).into();
    }
    let head = container(
        row![
            label_w("Recommended", metrics::font_sm(), t.muted, Weight::Semibold),
            space().width(Length::Fill),
            refresh(state),
        ]
        .align_y(Alignment::Center),
    )
    .height(HEADER)
    .padding(Padding::from([0.0, metrics::PAGE_PADDING_X]))
    .center_y(HEADER);
    let mut body = column![];

    if let Some(error) = &strip.error {
        body = body.push(
            row![
                label(
                    ellipsize_px(error, (state.page_width() - 200.0).max(160.0), metrics::font_sm()),
                    metrics::font_sm(),
                    t.error
                ),
                button(label_w("Retry", metrics::font_sm(), t.foreground, Weight::Semibold))
                    .padding(Padding::from([4.0, 10.0]))
                    .style(style::default_button(t))
                    .on_press(Message::PlaylistPage(PlaylistPageMsg::Recommend(true))),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .padding(Padding::from([0.0, metrics::PAGE_PADDING_X]))
            .height(32.0),
        );
    } else if strip.songs.is_empty() {
        body = body.push(
            container(label(
                if strip.loading { "Finding songs…" } else { "Nothing to recommend here." },
                metrics::font_sm(),
                t.muted,
            ))
            .padding(Padding::from([0.0, metrics::PAGE_PADDING_X])),
        );
    } else {
        for (index, song) in strip.songs.iter().enumerate() {
            body = body.push(song_row(state, index, song));
        }
    }

    container(column![head, body])
        .padding(Padding::from([PAD, 0.0]))
        .height(Length::Fixed(height(state, id)))
        .width(Length::Fill)
        .into()
}

/// One recommended song: cover, title/artist, album and its "+".
fn song_row<'a>(state: &'a Listen, index: usize, song: &'a Song) -> Element<'a, Message> {
    let t = state.tokens;
    let text_px = (state.page_width() - 320.0).max(140.0);
    let content = row![
        song_cover(state, song, COVER, metrics::radius_md()),
        container(
            column![
                label_w(
                    ellipsize_px(&song.title, text_px * 0.6, metrics::font_md()),
                    metrics::font_md(),
                    t.foreground,
                    Weight::Medium
                ),
                label(
                    ellipsize_px(&song.artist_line(), text_px * 0.6, metrics::font_xs()),
                    metrics::font_xs(),
                    t.muted
                ),
            ]
            .spacing(metrics::space_xs()),
        )
        .width(Length::FillPortion(3))
        .clip(true),
        container(label(
            ellipsize_px(song.album.as_deref().unwrap_or_default(), text_px * 0.4, metrics::font_sm()),
            metrics::font_sm(),
            t.muted
        ))
        .width(Length::FillPortion(2))
        .clip(true),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    let play = button(content)
        .width(Length::Fill)
        .height(ROW)
        .padding(Padding::from([0.0, metrics::space_sm()]))
        .style(style::row_button(t, false))
        .on_press(Message::PlaylistPage(PlaylistPageMsg::RecommendPlay(index)));
    let add = crate::ui::tip::tip(
        state,
        button(icon(Icon::LuPlus, metrics::font_md(), t.foreground))
            .padding(6)
            .style(style::icon_button(t, false))
            .on_press(Message::PlaylistPage(PlaylistPageMsg::RecommendAdd(index))),
        "Add to this playlist",
        None,
        crate::ui::tip::Position::Right,
    );
    let line = row![play, add]
        .spacing(metrics::space_xs())
        .align_y(Alignment::Center)
        .padding(Padding::from([0.0, metrics::PAGE_PADDING_X]));
    with_menu(line, MenuTarget::song(song))
}

/// The small button that asks for another set.
fn refresh(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let again = button(icon(Icon::LuRefreshCw, metrics::font_sm(), t.muted))
        .padding(6)
        .style(style::icon_button(t, false))
        .on_press(Message::PlaylistPage(PlaylistPageMsg::Recommend(true)));
    if state.playlist_page.recommended.loading {
        // Nothing to ask for twice.
        return container(icon(Icon::LuRefreshCw, metrics::font_sm(), t.muted)).padding(6).into();
    }
    crate::ui::tip::tip(state, again, "Other recommendations", None, crate::ui::tip::Position::Left)
}
