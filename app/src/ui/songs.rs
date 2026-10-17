//! Tracks page, on the item list component (`ui::list`). `song_row` / `header` / `visible_rows`
//! are the legacy fixed-column table, still used by other pages.

use super::list::ListView;
use super::{ellipsize_px, label, label_w, plural, song_cover};
use crate::app::list::ListId;
use crate::app::{Listen, Message, ScrollState};
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{button, column, container, row, space};
use iced::{Alignment, Element, Length, Padding};
use listen_core::model::{Song, format_duration};

pub const ROW: f32 = metrics::ROW_HEIGHT;
const HEADER: f32 = 40.0;

/// `[first, last)` rows intersecting the viewport, with a little overscan.
pub fn visible_rows(scroll: ScrollState, total: usize) -> (usize, usize) {
    let height = if scroll.height > 0.0 { scroll.height } else { 1200.0 };
    let first = (scroll.offset / ROW).floor().max(0.0) as usize;
    let last = ((scroll.offset + height) / ROW).ceil() as usize + 2;
    (first.min(total), last.min(total))
}

/// One table row: #, cover + title/artist, album, duration.
pub fn song_row<'a>(
    state: &'a Listen,
    index: usize,
    song: &'a Song,
    on_press: Message,
) -> Element<'a, Message> {
    let t = state.tokens;
    let current = state.queue.current().is_some_and(|c| c.song.id == song.id);
    let title_color = if current { t.primary } else { t.foreground };
    // Title and album share what's left of the page after the fixed columns (3 : 2).
    let flexible = (state.page_width()
        - 2.0 * 32.0
        - 2.0 * metrics::space_sm()
        - 48.0
        - 48.0
        - 80.0
        - 4.0 * metrics::space_sm()
        - 14.0)
        .max(80.0);
    let (title_px, album_px) = (flexible * 0.6, flexible * 0.4 - metrics::space_sm());
    let content = row![
        container(label(format!("{}", index + 1), metrics::font_md(), t.muted)).width(48).center_x(48),
        song_cover(state, song, 48.0, metrics::radius_md()),
        container(
            column![
                label_w(
                    ellipsize_px(&song.title, title_px, metrics::font_md()),
                    metrics::font_md(),
                    title_color,
                    Weight::Medium
                ),
                label(
                    ellipsize_px(&song.artist_line(), title_px, metrics::font_xs()),
                    metrics::font_xs(),
                    t.muted
                ),
            ]
            .spacing(metrics::space_xs()),
        )
        .width(Length::FillPortion(3))
        .clip(true),
        container(label(
            ellipsize_px(song.album.as_deref().unwrap_or_default(), album_px, metrics::font_md()),
            metrics::font_md(),
            t.muted
        ))
        .width(Length::FillPortion(2))
        .clip(true),
        container(label(format_duration(song.duration), metrics::font_md(), t.muted)).width(80),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    let row = button(content)
        .width(Length::Fill)
        .height(ROW)
        .padding(Padding::from([metrics::space_xs(), metrics::space_sm()]))
        .style(style::row_button(t, current))
        .on_press(on_press);
    let target = match state.history_view.recent_idx.get(index) {
        Some(&entry) if state.route == crate::app::Route::Recent => {
            crate::app::menu::MenuTarget::history_item(song, entry)
        }
        _ => crate::app::menu::MenuTarget::song(song),
    };
    super::with_menu(row, target)
}

pub fn header<'a>(state: &'a Listen) -> Element<'a, Message> {
    let t = state.tokens;
    let h = |s: &'static str| label_w(s, metrics::font_sm(), t.muted, Weight::Medium);
    container(
        row![
            container(h("#")).width(48).center_x(48),
            space().width(48),
            container(h("Title")).width(Length::FillPortion(3)),
            container(h("Album")).width(Length::FillPortion(2)),
            container(h("Time")).width(80),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    )
    .height(HEADER)
    .padding(Padding::from([0.0, metrics::space_sm()]))
    .center_y(HEADER)
    .into()
}

/// The Tracks page with the `ui::list::header::page_header` sort / filter / search header.
pub fn view(state: &Listen) -> Element<'_, Message> {
    let songs = &state.library.songs;
    let count = if state.library.songs_done {
        plural(songs.len(), "track")
    } else {
        format!("{}+ tracks", songs.len())
    };
    let searching = !state.list_queries.songs.search.trim().is_empty();
    column![
        super::list::header::page_header(state, &ListId::Songs, "Tracks", count),
        ListView::new(state, ListId::Songs, songs)
            .no_header()
            .loading(state.library.songs_loading)
            .done(state.library.songs_done)
            .empty(if searching || state.list_queries.songs.filters.active() {
                "No tracks match."
            } else {
                "No tracks."
            })
            .view()
    ]
    .into()
}
