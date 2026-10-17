//! The detail display type for albums: every album is a block with its cover,
//! facts and track list (Feishin's "detail" list). Blocks have a known height
//! (from the album's song count), so the list stays virtualized.

use super::Body;
use super::table::interactive;
use crate::app::list::ListMsg;
use crate::app::{Listen, Message, ScrollState};
use crate::style::{self, metrics};
use crate::ui::{ellipsize_px, label, label_w, plural};
use iced::font::Weight;
use iced::widget::{Column, Row, column, container, mouse_area, row, scrollable, space};
use iced::{Alignment, Element, Length, Padding};
use listen_core::model::{Album, Song, format_duration};

/// Feishin's detail row: a 240 px column (cover, centred facts below) next to the
/// track table, 12 px padding and gap.
const COVER: f32 = 240.0;
const PAD_V: f32 = 12.0;
/// Title, artists and facts below the cover (gap included).
const INFO: f32 = 76.0;
const TRACK_ROW: f32 = 32.0;
const OVERSCAN: f32 = 400.0;

fn track_count(album: &Album) -> usize {
    album.song_count.map(|n| n as usize).unwrap_or(album.songs.len()).max(1)
}

/// Height of an album's block.
pub fn block_height(album: &Album) -> f32 {
    2.0 * PAD_V + (COVER + INFO).max(track_count(album) as f32 * TRACK_ROW) + 1.0
}

/// `[first, last)` blocks intersecting the viewport.
pub fn visible_blocks(scroll: ScrollState, albums: &[Album]) -> (usize, usize) {
    let height = if scroll.height > 0.0 { scroll.height } else { 1200.0 };
    let (from, to) = (scroll.offset - OVERSCAN, scroll.offset + height + OVERSCAN);
    let (mut first, mut last, mut y) = (None, albums.len(), 0.0);
    for (i, a) in albums.iter().enumerate() {
        let h = block_height(a);
        if first.is_none() && y + h >= from {
            first = Some(i);
        }
        if y > to {
            last = i;
            break;
        }
        y += h;
    }
    (first.unwrap_or(albums.len()), last)
}

/// Top and height of block `index`.
pub fn block_span(albums: &[Album], index: usize) -> (f32, f32) {
    let top: f32 = albums.iter().take(index).map(block_height).sum();
    (top, albums.get(index).map_or(0.0, block_height))
}

/// The tracks of an album: the ones it carries, or the ones fetched for the detail view.
pub fn album_tracks<'a>(state: &'a Listen, album: &'a Album) -> &'a [Song] {
    if !album.songs.is_empty() {
        return &album.songs;
    }
    state.lists.detail.get(&(album.server_id.clone(), album.id.clone())).map(Vec::as_slice).unwrap_or(&[])
}

pub fn view<'a>(body: &Body<'a, Album>) -> Element<'a, Message> {
    let t = body.state.tokens;
    let albums = &body.items[body.start..body.end];
    let scroll =
        ScrollState { offset: body.list.scroll.offset, height: body.size.height, width: body.size.width };
    let (first, last) = visible_blocks(scroll, albums);
    let before: f32 = albums.iter().take(first).map(block_height).sum();
    let after: f32 = albums.iter().skip(last).map(block_height).sum();

    let mut out = Column::new().push(space().height(before));
    for (local, album) in albums.iter().enumerate().take(last).skip(first) {
        out = out.push(block(body, body.start + local, album));
    }
    out = out.push(space().height(after));
    if body.loading {
        out = out.push(container(label("Loading…", metrics::font_md(), t.muted)).padding(12));
    }
    let id = body.id.clone();
    scrollable(container(out).padding(Padding::from([0.0, super::table::PAD])))
        .id(body.id.scroll_id())
        .on_scroll(move |v| super::scrolled(&id, &v))
        .style(style::scrollbar(t))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn block<'a>(body: &Body<'a, Album>, index: usize, album: &'a Album) -> Element<'a, Message> {
    let t = body.state.tokens;
    let selected = body.list.selection.contains(index);
    let cx = super::columns::Cx {
        state: body.state,
        id: body.id.clone(),
        index,
        size: listen_core::settings::RowSize::Default,
        hovered: body.list.hover == Some(index),
        selected,
        width: (body.size.width - COVER - 120.0).max(120.0),
        card: false,
    };
    let name = super::columns::link(
        t,
        ellipsize_px(&album.name, COVER, metrics::font_md()),
        metrics::font_md(),
        t.foreground,
        Some(Message::OpenAlbum(album.id.clone())),
    );
    let artists = album_artists(&cx, album);
    let mut facts = Vec::new();
    if let Some(y) = album.year {
        facts.push(y.to_string());
    }
    if !album.genres.is_empty() {
        facts.push(album.genres.join(", "));
    }
    facts.push(plural(track_count(album), "song"));
    if !album.duration.is_zero() {
        facts.push(format_duration(album.duration));
    }
    let cover = interactive(&body.id, index, cover_with_badges(body, &cx, album));
    let facts = ellipsize_px(&facts.join(" · "), COVER, metrics::font_sm());
    let info = interactive(
        &body.id,
        index,
        column![
            container(name).center_x(Length::Fill),
            container(artists).center_x(Length::Fill),
            container(label(facts, metrics::font_sm(), t.muted)).center_x(Length::Fill)
        ]
        .spacing(metrics::space_xs())
        .width(COVER)
        .into(),
    );

    let tracks = album_tracks(body.state, album);
    let mut list = Column::new().width(Length::Fill);
    if tracks.is_empty() {
        list = list.push(label("Loading tracks…", metrics::font_sm(), t.muted));
    }
    for (n, song) in tracks.iter().enumerate() {
        list = list.push(track(body, index, n, song));
    }
    let left =
        column![cover, container(info).padding(Padding::from([metrics::space_sm(), 0.0]))].width(COVER);
    let stacked = row![left, container(list).width(Length::Fill)].spacing(metrics::space_md());
    let framed = container(stacked)
        .height(block_height(album) - 1.0)
        .padding(Padding::from([PAD_V, PAD_V]))
        .style(move |_| container::Style {
            background: selected.then(|| iced::Background::Color(style::alpha(t.primary, 0.14))),
            border: iced::border::rounded(t.radius_md),
            ..Default::default()
        });
    column![framed, super::divider_h(t)].into()
}

/// The cover with a heart badge for a favourite, the rating as a number, and
/// the card controls while the pointer is over it (Feishin `item-detail-list.tsx`).
fn cover_with_badges<'a>(
    body: &Body<'a, Album>,
    cx: &super::columns::Cx<'a>,
    album: &'a Album,
) -> Element<'a, Message> {
    use super::columns::ListItem;
    let t = body.state.tokens;
    let mut layers: Vec<Element<'a, Message>> = vec![album.cover(body.state, COVER, metrics::radius_md())];
    let badge = |content: Element<'a, Message>, at: iced::alignment::Horizontal| -> Element<'a, Message> {
        container(container(content).padding(Padding::from([2.0, 6.0])).style(move |_| container::Style {
            background: Some(iced::Background::Color(style::alpha(iced::Color::BLACK, 0.6))),
            border: iced::border::rounded(999),
            ..Default::default()
        }))
        .width(Length::Fill)
        .padding(8)
        .align_x(at)
        .into()
    };
    if cx.state.show_favorites() && album.favorite {
        layers.push(badge(
            crate::ui::icon(crate::icons::Icon::RiHeartFill, metrics::font_sm(), t.primary),
            iced::alignment::Horizontal::Left,
        ));
    }
    if let Some(r) = album.rating.filter(|r| *r > 0 && cx.state.show_ratings()) {
        layers.push(badge(
            label(r.to_string(), metrics::font_sm(), iced::Color::WHITE).into(),
            iced::alignment::Horizontal::Right,
        ));
    }
    if cx.hovered {
        layers.push(super::grid::controls(cx, album, COVER));
    }
    iced::widget::Stack::with_children(layers).width(COVER).height(COVER).into()
}

fn album_artists<'a>(cx: &super::columns::Cx<'a>, album: &'a Album) -> Element<'a, Message> {
    super::columns::artist_links(cx, &album.album_artists, true)
}

/// One track of a block: number, title, like, duration. Double-click plays the album from it.
fn track<'a>(body: &Body<'a, Album>, album: usize, n: usize, song: &'a Song) -> Element<'a, Message> {
    let t = body.state.tokens;
    let title_w = (body.size.width - COVER - 300.0).max(100.0);
    let content = Row::new()
        .push(container(label(format!("{}", n + 1), metrics::font_sm(), t.muted)).width(36))
        .push(
            container(label_w(
                ellipsize_px(&song.title, title_w, metrics::font_sm()),
                metrics::font_sm(),
                t.foreground,
                Weight::Normal,
            ))
            .width(Length::Fill)
            .clip(true),
        )
        .push(container(label(format_duration(song.duration), metrics::font_sm(), t.muted)).width(64))
        .align_y(Alignment::Center);
    mouse_area(container(content).height(TRACK_ROW).center_y(TRACK_ROW).width(Length::Fill))
        .on_double_click(Message::List(ListMsg::PlayDetailTrack(body.id.clone(), album, n)))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn albums(n: usize, songs: u32) -> Vec<Album> {
        (0..n)
            .map(|i| {
                let mut a = crate::demo::albums()[i % crate::demo::ALBUMS.len()].clone();
                a.songs.clear();
                a.song_count = Some(songs);
                a
            })
            .collect()
    }

    #[test]
    fn blocks_grow_with_their_tracks() {
        let short = albums(1, 3);
        let long = albums(1, 20);
        assert!(block_height(&long[0]) > block_height(&short[0]));
        // A short album is as tall as its cover.
        assert!(block_height(&short[0]) >= COVER + 2.0 * PAD_V);
    }

    #[test]
    fn only_blocks_in_view_are_built() {
        let list = albums(200, 10);
        let h = block_height(&list[0]);
        let at = |offset: f32| ScrollState { offset, height: 800.0, width: 1000.0 };
        let (first, last) = visible_blocks(at(0.0), &list);
        assert_eq!(first, 0);
        assert!((3..10).contains(&last), "{last}");
        let (first, last) = visible_blocks(at(h * 100.0), &list);
        assert!((97..=100).contains(&first), "{first}");
        assert!(last > first && last <= 108, "{last}");
        let (top, height) = block_span(&list, 50);
        assert_eq!(top, 50.0 * h);
        assert_eq!(height, h);
        assert_eq!(visible_blocks(at(1e9), &list).0, list.len());
    }
}
