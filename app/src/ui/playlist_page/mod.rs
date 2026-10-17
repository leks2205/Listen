//! The playlist page: header, toolbar, songs or albums list, and the filters and create / edit dialogs
//! (Feishin: `features/playlists`). Header, toolbar and description are the list's lead, so the page
//! scrolls as one.

pub mod filters;
pub mod forms;
pub mod header;

use super::label;
use super::list::ListView;
use crate::app::list::ListId;
use crate::app::{Listen, Message};
use crate::style::metrics;
use iced::Element;
use iced::widget::{column, container};
use iced::{Length, Padding};
use listen_core::settings::PlaylistTarget;

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let Some(detail) = state.open_playlist_detail() else {
        return container(label("Loading…", metrics::font_md(), t.muted)).padding(48).into();
    };
    let id = detail.playlist.id.clone();
    let list = state.playlist_list_id(&id);
    let lead_h = header::lead_height(state, detail);
    match (&list, state.config.settings.lists.playlist_target) {
        (ListId::PlaylistAlbums(_), PlaylistTarget::Albums) => {
            let albums = &state.playlist_page.albums;
            column![
                container(header::lead(state, detail)).height(lead_h),
                ListView::new(state, list, albums)
                    .no_header()
                    .loading(state.library.playlist_loading_more)
                    .empty("No albums in this playlist.")
                    .view()
            ]
            .into()
        }
        _ => {
            let rows = state.playlist_rows(&id);
            let none = rows.is_empty() && !state.library.playlist_loading_more;
            let narrowed = state.playlist_page.narrowed();
            let note = if narrowed { "No songs match." } else { "This playlist is empty." };
            let mut view = ListView::new(state, list, rows)
                .no_header()
                .loading(state.library.playlist_loading_more)
                .lead(lead_h, move || header::lead(state, detail));
            if none {
                view = view.tail(96.0, move || {
                    container(label(note, metrics::font_md(), state.tokens.muted))
                        .padding(Padding::from([32.0, metrics::PAGE_PADDING_X]))
                        .height(Length::Fixed(96.0))
                        .into()
                });
            } else {
                view = view.tail(80.0, || iced::widget::space().height(80.0).into());
            }
            view.view()
        }
    }
}
