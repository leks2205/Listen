//! Album page: header, comment, track list, pills and links, and "More from" carousels (Feishin:
//! `album-detail-{header,content}.tsx`). Everything around the track list is the list's lead or tail,
//! so the page scrolls as one.

pub mod content;
pub mod expanded;
pub mod format;
pub mod header;

use super::label;
use super::list::{ListView, RowGroup};
use crate::app::album_page::AlbumPageMsg;
use crate::app::list::ListId;
use crate::app::{Listen, Message};
use crate::style::metrics;
use iced::Element;
use iced::widget::container;
use listen_core::model::{Album, Song};

/// Height of the header block with its background.
pub const HEADER_H: f32 = 362.0;
/// Height of the search / sort row.
pub const TOOLBAR_H: f32 = 64.0;

/// Disc header rows, when there are several discs and the list is neither searched nor sorted.
pub fn disc_groups(state: &Listen, album: &Album, rows: &[Song]) -> Vec<RowGroup> {
    if !state.album_page.grouped() {
        return Vec::new();
    }
    let disc = |s: &Song| s.disc_number.unwrap_or(1);
    let mut starts: Vec<(usize, u32)> = Vec::new();
    for (i, s) in rows.iter().enumerate() {
        if starts.last().is_none_or(|(_, d)| *d != disc(s)) {
            starts.push((i, disc(s)));
        }
    }
    if starts.len() <= 1 {
        return Vec::new();
    }
    let list = state.lists.get(&ListId::Album(album.id.clone()));
    starts
        .iter()
        .map(|&(start, d)| {
            let all_selected = rows
                .iter()
                .enumerate()
                .filter(|(_, s)| disc(s) == d)
                .all(|(i, _)| list.selection.contains(i));
            let title = match album.info.disc_titles.iter().find(|(n, _)| *n == d) {
                Some((_, t)) => format!("Disc {d} - {t}"),
                None => format!("Disc {d}"),
            };
            RowGroup {
                before: start,
                title,
                checked: all_selected,
                on_toggle: Message::AlbumPage(AlbumPageMsg::Disc(d)),
                look: Default::default(),
            }
        })
        .collect()
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let Some(album) = &state.library.album else {
        let text = state.library.album_error.clone().unwrap_or_else(|| "Loading…".into());
        return container(label(text, metrics::font_md(), t.muted)).padding(48).into();
    };
    let rows = state.album_rows(&album.id);
    let groups = disc_groups(state, album, rows);
    let lead_h = header::lead_height(state, album);
    ListView::new(state, ListId::Album(album.id.clone()), rows)
        .no_header()
        .lead(lead_h, move || header::lead(state, album))
        .tail(0.0, move || content::tail(state, album))
        .groups(groups)
        .view()
}
