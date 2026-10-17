//! The Artists and Album Artists pages, on the item list component
//! (`ui::list`): table, grid or detail, selection, keyboard, hover controls.
//! The sort, search and filter widgets of the header come from `ui::list::header`.

use super::list::ListView;
use super::plural;
use crate::app::list::ListId;
use crate::app::{Listen, Message};
use iced::Element;

pub fn view(state: &Listen, id: ListId) -> Element<'_, Message> {
    let artists = &state.library.artists;
    let title = if id == ListId::AlbumArtists { "Album Artists" } else { "Artists" };
    ListView::new(state, id, artists)
        .title(title)
        .subtitle(plural(artists.len(), "artist"))
        .loading(state.library.artists_loading)
        .done(state.library.artists_done)
        .empty("No artists.")
        .view()
}
