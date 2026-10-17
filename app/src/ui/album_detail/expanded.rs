//! A reusable "expanded album" panel for album lists: the cover, the title,
//! the artists and the tracks, with Play / Add next / Add last. The Albums
//! list shows it under an album when the album is expanded (Feishin
//! `expanded-album-list-item.tsx`). The caller owns what the buttons send.

use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{album_cover, ellipsize, icon, label, label_w};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, row, scrollable, space};
use iced::{Alignment, Color, Element, Length, Padding};
use listen_core::model::{Album, Song, format_duration};
use listen_core::settings::PlayBehavior;

pub const COVER: f32 = 200.0;
pub const TRACK_ROW: f32 = 32.0;
/// Most tracks shown before the list scrolls.
pub const MAX_ROWS: usize = 5;
/// Height of the panel in a list (Feishin `EXPANDED_HEIGHT` is 300).
pub const PANEL_H: f32 = 316.0;

/// What the panel sends.
pub struct Actions<'a> {
    pub close: Message,
    /// A play button (now / next / last) for the whole album.
    pub play: Box<dyn Fn(PlayBehavior) -> Message + 'a>,
    /// A double press of track `n`.
    pub track: Box<dyn Fn(usize) -> Message + 'a>,
}

/// The panel for `album` with its `songs`.
pub fn expanded<'a>(
    state: &'a Listen,
    album: &'a Album,
    songs: &'a [Song],
    actions: Actions<'a>,
) -> Element<'a, Message> {
    let t = state.tokens;
    let mut tracks = Column::new();
    for (i, s) in songs.iter().enumerate() {
        let number = format!("{} - {}", s.disc_number.unwrap_or(1), s.track_number.unwrap_or(i as u32 + 1));
        tracks = tracks.push(
            button(
                row![
                    container(label(number, metrics::font_sm(), t.muted)).width(48),
                    container(label(ellipsize(&s.title, 60), metrics::font_md(), t.foreground))
                        .width(Length::Fill),
                    label(format_duration(s.duration), metrics::font_sm(), t.muted),
                ]
                .spacing(metrics::space_sm())
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .height(TRACK_ROW)
            .padding(Padding::from([0.0, metrics::space_sm()]))
            .style(style::row_button(t, false))
            .on_press((actions.track)(i)),
        );
    }
    let list_h = (songs.len().min(MAX_ROWS) as f32 * TRACK_ROW).max(TRACK_ROW);
    let btn = |glyph: Icon, name: &'static str, b: PlayBehavior| {
        button(
            row![
                icon(glyph, metrics::font_md(), t.foreground),
                label_w(name, metrics::font_sm(), t.foreground, Weight::Medium)
            ]
            .spacing(metrics::space_xs())
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([6.0, 12.0]))
        .style(style::default_button(t))
        .on_press((actions.play)(b))
    };
    let play = button(
        row![
            icon(Icon::LuPlay, metrics::font_md(), Color::BLACK),
            label_w("Play", metrics::font_sm(), Color::BLACK, Weight::Semibold)
        ]
        .spacing(metrics::space_xs())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([6.0, 16.0]))
    .style(style::wide_play_button(t))
    .on_press((actions.play)(PlayBehavior::Now));
    let close = crate::ui::tip::tip(
        state,
        button(icon(Icon::LuX, metrics::font_lg(), t.muted))
            .padding(6)
            .style(style::icon_button(t, false))
            .on_press(actions.close),
        "Close",
        None,
        crate::ui::tip::Position::Bottom,
    );
    let info = column![
        row![
            label_w(ellipsize(&album.name, 60), 22.0, t.foreground, Weight::Bold),
            space::horizontal(),
            close
        ]
        .align_y(Alignment::Center),
        label(album.artist_line(), metrics::font_md(), t.muted),
        container(scrollable(tracks).style(style::scrollbar(t))).height(list_h),
        Row::new()
            .spacing(metrics::space_sm())
            .push(play)
            .push(btn(Icon::LuSkipForward, "Add next", PlayBehavior::Next))
            .push(btn(Icon::RiPlayListAddLine, "Add last", PlayBehavior::Last)),
    ]
    .spacing(metrics::space_sm())
    .width(Length::Fill);
    container(
        row![album_cover(state, album, COVER, metrics::radius_md()), info]
            .spacing(metrics::space_md())
            .align_y(Alignment::Start),
    )
    .width(Length::Fill)
    .padding(metrics::space_md())
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(style::alpha(t.foreground, 0.05))),
        border: iced::border::rounded(metrics::radius_md()),
        ..Default::default()
    })
    .into()
}
