//! Entries for playlists (single or several), and the playlist-only entries of
//! songs (Remove from playlist).

use super::entry::{self, Ctx, Entry};
use super::{MenuAction, MenuTarget};
use crate::app::list::ListId;
use crate::icons::Icon;
use listen_core::model::{Playlist, SourceKind};
use listen_source::Capabilities;

/// A link to a YouTube Music playlist (`None` for other sources and for
/// Liked Music and the uploads, which are not playlists to share).
pub fn share_url(p: &Playlist) -> Option<String> {
    if p.source != SourceKind::YouTubeMusic {
        return None;
    }
    let id = p.id.strip_prefix("VL").unwrap_or(&p.id);
    if id.is_empty() || matches!(id, "LM" | "SE" | "WL") || id.starts_with("FEmusic") {
        return None;
    }
    Some(format!("https://music.youtube.com/playlist?list={id}"))
}

pub fn entries(cx: &Ctx, out: &mut Vec<Entry>) {
    let MenuTarget::Playlists(playlists) = cx.target else { return };
    let Some(first) = playlists.first() else { return };
    // What the playlist's server does (demo playlists behave like their source).
    let caps = cx.state.playlist_caps(first);
    out.extend(entry::play_shuffle());
    out.extend(entry::queue_entries());
    out.push(entry::play_submenu());
    out.push(entry::add_to_playlist());
    out.push(
        Entry::action(Icon::LuDownload, "Download", MenuAction::Download).requires(Capabilities::DOWNLOAD),
    );
    out.push(
        Entry::action(Icon::LuListMusic, "Open playlist", MenuAction::Open).when(cx.browsing && cx.single()),
    );
    let owned = cx.single() && first.editable;
    out.push(
        Entry::action(Icon::LuMenu, "Edit playlist…", MenuAction::EditPlaylist)
            .when(owned && caps.intersects(Capabilities::PLAYLIST_UPDATE | Capabilities::PLAYLIST_RENAME)),
    );
    out.push(
        Entry::action(Icon::LuMenu, "Edit smart rules…", MenuAction::EditSmart(first.id.clone()))
            .when(owned && cx.state.smart_supported() && cx.state.is_smart(&first.id)),
    );
    if let Some(url) = share_url(first).filter(|_| cx.single()) {
        out.push(Entry::action(Icon::LuLink, "Copy link", MenuAction::CopyLink(url)));
    }
    out.push(
        Entry::action(Icon::LuX, "Delete playlist…", MenuAction::ChooseDelete)
            .when(owned && caps.contains(Capabilities::PLAYLIST_DELETE)),
    );
    // Details / share / download entries.
    crate::app::item::menu::entries(cx, out);
    out.push(entry::clear_selection(cx));
}

/// Entries songs get only inside an open playlist: "Remove from playlist".
pub fn song_entries(cx: &Ctx, out: &mut Vec<Entry>) {
    let Some(list @ ListId::Playlist(id)) = cx.list else { return };
    let Some(detail) = cx.state.open_playlist_detail().filter(|d| &d.playlist.id == id) else { return };
    let positions = cx.state.playlist_selected_positions(list);
    let allowed = detail.playlist.editable
        && cx.state.playlist_page.edit.is_none()
        && cx.state.playlist_caps(&detail.playlist).contains(Capabilities::PLAYLIST_REMOVE);
    out.push(
        Entry::action(
            Icon::LuTrash2,
            "Remove from playlist",
            MenuAction::RemoveFromPlaylist(id.clone(), positions.clone()),
        )
        .when(allowed && !positions.is_empty()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_youtube_music_playlists_have_a_link() {
        let mut p = crate::demo::ytm_playlist().playlist;
        p.id = "VLPL123".into();
        assert_eq!(share_url(&p).as_deref(), Some("https://music.youtube.com/playlist?list=PL123"));
        p.id = "PL123".into();
        assert!(share_url(&p).is_some());
        p.id = "VLLM".into();
        assert_eq!(share_url(&p), None);
        p.id = "FEmusic_library_privately_owned_tracks".into();
        assert_eq!(share_url(&p), None);
        assert_eq!(share_url(&crate::demo::navidrome_playlist().playlist), None);
    }
}
