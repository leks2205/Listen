//! Entries for albums (single or several), in Feishin's order
//! (menus/album-context-menu.tsx).

use super::entry::{self, Ctx, Entry};
use super::{MenuAction, MenuTarget, TargetKind};
use crate::icons::Icon;
use listen_core::model::SourceKind;
use listen_source::Capabilities;

pub fn entries(cx: &Ctx, out: &mut Vec<Entry>) {
    let MenuTarget::Albums(albums) = cx.target else { return };
    let Some(first) = albums.first() else { return };
    out.push(entry::play_submenu());
    out.push(entry::radio_submenu(cx, "Album radio"));
    out.push(Entry::divider());
    out.push(entry::add_to_playlist());
    out.push(Entry::divider());
    let ytm = first.source == SourceKind::YouTubeMusic;
    out.push(entry::favorite_submenu(cx, TargetKind::Album, ytm));
    let rating = entry::shared_rating(albums.iter().map(|a| a.rating));
    out.push(entry::rating_submenu(cx, rating).when(!ytm));
    out.push(Entry::divider());
    out.push(
        Entry::action(Icon::LuDownload, "Download", MenuAction::Download).requires(Capabilities::DOWNLOAD),
    );
    out.push(Entry::divider());
    let artists: Vec<(String, String)> = first
        .album_artists
        .iter()
        .filter_map(|a| Some((a.id.clone().filter(|i| !i.is_empty())?, a.name.clone())))
        .collect();
    out.push(entry::go_to_submenu(cx, Some(first.id.clone()), &artists));
    out.push(Entry::divider());
    // Details, share, download.
    crate::app::item::menu::entries(cx, out);
    out.push(entry::clear_selection(cx));
}
