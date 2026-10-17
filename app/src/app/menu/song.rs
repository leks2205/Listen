//! Entries for songs (single or several), in Feishin's order
//! (menus/song-context-menu.tsx): Play, Track radio, Add to playlist,
//! Favorite / rating, download / share, Go to, then details (another package).
//! Rows of the queue get [`queue::entries`] instead.

use super::entry::{self, Ctx, Entry};
use super::{MenuAction, MenuTarget, TargetKind, queue};
use crate::icons::Icon;
use listen_core::model::{Song, SourceKind};

pub fn entries(cx: &Ctx, out: &mut Vec<Entry>) {
    let MenuTarget::Songs { songs, history_entries, queue_uids } = cx.target else { return };
    if songs.is_empty() {
        return;
    }
    if !queue_uids.is_empty() {
        return queue::entries(cx, out);
    }
    // Recently Played rows (not in Feishin): remove the plays first, like "Remove from queue".
    out.push(
        Entry::action(Icon::LuX, "Remove from history", MenuAction::RemoveFromHistory)
            .when(!history_entries.is_empty()),
    );
    out.push(Entry::divider());
    out.push(entry::play_submenu());
    out.push(track_radio(cx));
    out.push(Entry::divider());
    library_block(cx, songs, out);
    out.push(entry::clear_selection(cx));
}

/// Listen's "Start radio" (Feishin: Track radio), greyed out for several songs.
pub fn track_radio(cx: &Ctx) -> Entry {
    Entry::action(Icon::RiRadioLine, "Start radio", MenuAction::StartRadio).enabled_if(cx.single())
}

/// Entries song and queue menus share, from "Add to playlist" down to "Go to".
pub fn library_block(cx: &Ctx, songs: &[Song], out: &mut Vec<Entry>) {
    let Some(first) = songs.first() else { return };
    let state = cx.state;
    out.push(entry::add_to_playlist());
    super::playlist::song_entries(cx, out);
    out.push(Entry::divider());

    let ytm = first.source == SourceKind::YouTubeMusic;
    out.push(entry::favorite_submenu(cx, TargetKind::Song, ytm));
    // Ratings: Navidrome only (the RATINGS capability), never YouTube Music.
    let rateable = songs.iter().all(|s| s.source != SourceKind::YouTubeMusic);
    let rating = entry::shared_rating(songs.iter().map(|s| s.rating));
    out.push(entry::rating_submenu(cx, rating).when(rateable));
    out.push(Entry::divider());

    // Download / share (Listen: offline copy, YouTube Music link).
    if songs.iter().all(|s| state.downloaded(s)) {
        out.push(Entry::action(Icon::LuTrash2, "Remove download", MenuAction::RemoveDownload));
    } else if songs.iter().any(|s| state.can_download(s)) {
        out.push(Entry::action(Icon::LuDownload, "Download", MenuAction::Download));
    }
    if let Some(url) = cx.source.as_ref().and_then(|s| s.share_url(first)).filter(|_| cx.single()) {
        out.push(Entry::action(Icon::LuLink, "Copy link", MenuAction::CopyLink(url)));
    }
    out.push(Entry::divider());

    // Go to: the album, then each artist (the song's artists, else its album artists).
    let artists = if first.artists.is_empty() { &first.album_artists } else { &first.artists };
    let artists: Vec<(String, String)> = artists
        .iter()
        .filter_map(|a| Some((a.id.clone().filter(|i| !i.is_empty())?, a.name.clone())))
        .collect();
    out.push(entry::go_to_submenu(cx, first.album_id.clone(), &artists));
    out.push(Entry::divider());
    // Details / share / download entries.
    crate::app::item::menu::entries(cx, out);
}
