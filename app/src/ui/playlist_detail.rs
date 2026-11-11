//! A titled song list (Recently Played and the mixes). Playlists have their own page, `ui/playlist_page`.

use super::{cover, icon, label_w};
use crate::app::{Listen, Message, ScrollState};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, button, column, container, row, scrollable, space};
use iced::{Alignment, Color, Element, Length, Padding};
use listen_core::model::{Song, format_duration};

/// Id of the song list's scrollable (scrolled to the top when another list opens).
pub const LIST_ID: &str = "song-list";
/// The header's padding above and below it, its (cover) height and the page's side padding.
const TOP_PAD: f32 = 80.0;
const BOTTOM_PAD: f32 = 32.0;
const HEADER_H: f32 = 250.0;
const SIDE: f32 = 32.0;
/// Height above the first row: padding, the fixed-height header, padding, column header (the rows are
/// virtualized from it).
const HEADER_BLOCK: f32 = TOP_PAD + HEADER_H + BOTTOM_PAD + super::songs::HEADER;

/// `[first, last)` rows of a song list that intersect the viewport.
pub fn visible_rows(scroll: ScrollState, total: usize) -> (usize, usize) {
    let rows = ScrollState { offset: (scroll.offset - HEADER_BLOCK).max(0.0), ..scroll };
    let (first, last) = super::songs::visible_rows(rows, total);
    (first, (last + 4).min(total))
}

/// Layout for a titled song list; only the visible rows are built.
pub fn song_list<'a>(
    state: &'a Listen,
    kind: &'static str,
    title: &'a str,
    subtitle: String,
    image: Option<&'a listen_core::model::ImageRef>,
    songs: &'a [Song],
    play: fn(usize) -> Message,
) -> Element<'a, Message> {
    let t = state.tokens;
    let total: std::time::Duration = songs.iter().map(|s| s.duration).sum();
    let play_btn = button(
        row![
            icon(Icon::LuPlay, metrics::font_lg(), Color::BLACK),
            label_w("Play", metrics::font_md(), Color::BLACK, Weight::Semibold)
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([9.0, 24.0]))
    .style(style::wide_play_button(t))
    .on_press_maybe((!songs.is_empty()).then(|| play(0)));
    // `subtitle` comes translated from the caller; the rest is translated here.
    let meta =
        format!("{subtitle}{}", crate::i18n::trf("{} songs • {}", &[&songs.len(), &format_duration(total)]));
    let header = row![
        cover(state, image, HEADER_H, metrics::radius_md(), Icon::LuListMusic),
        column![
            label_w(crate::i18n::tr(kind).to_uppercase(), metrics::font_md(), t.foreground, Weight::Semibold),
            super::header_title(crate::i18n::tr(title), t.foreground),
            super::data_label(meta, metrics::font_md(), t.muted).wrapping(iced::widget::text::Wrapping::Word),
            play_btn,
        ]
        .spacing(metrics::space_sm()),
    ]
    .spacing(metrics::space_md())
    .align_y(Alignment::End)
    .height(HEADER_H);

    let (first, last) = visible_rows(state.list_scroll, songs.len());
    let row_h = super::songs::ROW;
    let mut rows = Column::new().push(space().height(first as f32 * row_h));
    for (i, song) in songs.iter().enumerate().take(last).skip(first) {
        rows = rows.push(super::songs::song_row(state, i, song, play(i), state.page_width() - 2.0 * SIDE));
    }
    rows = rows.push(space().height(songs.len().saturating_sub(last) as f32 * row_h));
    let page = column![
        container(header).padding(Padding { top: TOP_PAD, right: SIDE, bottom: BOTTOM_PAD, left: SIDE }),
        container(super::songs::header(state)).padding(Padding::from([0.0, SIDE])),
        container(rows).padding(Padding { top: 0.0, right: SIDE, bottom: 80.0, left: SIDE }),
    ];
    scrollable(container(page).max_width(1800).center_x(Length::Fill))
        .id(iced::widget::Id::new(LIST_ID))
        .on_scroll(Message::ListScrolled)
        .style(style::scrollbar(t))
        .height(Length::Fill)
        .into()
}
