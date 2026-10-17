//! Entries for artists (single or several), in Feishin's order
//! (menus/artist-context-menu.tsx, album-artist-context-menu.tsx).

use super::entry::{self, Ctx, Entry};
use super::{MenuTarget, TargetKind};
use listen_core::model::SourceKind;

pub fn entries(cx: &Ctx, out: &mut Vec<Entry>) {
    let MenuTarget::Artists(artists) = cx.target else { return };
    let Some(first) = artists.first() else { return };
    out.push(entry::play_submenu());
    out.push(entry::radio_submenu(cx, "Artist radio"));
    out.push(Entry::divider());
    out.push(entry::add_to_playlist());
    out.push(Entry::divider());
    let ytm = first.source == SourceKind::YouTubeMusic;
    out.push(entry::favorite_submenu(cx, TargetKind::Artist, ytm));
    let rating = entry::shared_rating(artists.iter().map(|a| a.rating));
    out.push(entry::rating_submenu(cx, rating).when(!ytm));
    out.push(Entry::divider());
    // Details, share and download entries.
    crate::app::item::menu::entries(cx, out);
    out.push(entry::clear_selection(cx));
}
