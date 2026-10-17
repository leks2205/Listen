//! The context-menu entries of this package, declared once for every kind
//! (each kind file calls [`entries`]): Get info, Edit
//! metadata, Share, Show in file manager and Save file.

use crate::app::menu::entry::{Ctx, Entry};
use crate::app::menu::{MenuAction, MenuTarget};
use crate::icons::Icon;
use listen_core::model::SourceKind;
use listen_source::Capabilities;

/// Whether the original files can be saved from the source of the menu.
fn can_save(cx: &Ctx) -> bool {
    cx.source.as_ref().is_some_and(|s| matches!(s.kind(), SourceKind::Subsonic | SourceKind::Jellyfin))
}

pub fn entries(cx: &Ctx, out: &mut Vec<Entry>) {
    let ytm = cx.source.as_ref().is_some_and(|s| s.kind() == SourceKind::YouTubeMusic);
    let kind_has_details = !matches!(cx.target, MenuTarget::Genres(_) | MenuTarget::Folders(_));
    out.push(Entry::divider());
    out.push(Entry::action(Icon::LuInfo, "Get info", MenuAction::GetInfo).when(kind_has_details));

    if let MenuTarget::Songs { songs, .. } = cx.target {
        out.push(
            Entry::action(Icon::LuPencil, "Edit metadata…", MenuAction::EditTags)
                .when(super::can_edit_tags(songs)),
        );
        let local = songs.first().and_then(|s| cx.state.song_file(s)).is_some();
        out.push(
            Entry::action(Icon::LuFolderOpen, "Show in file manager", MenuAction::ShowInFolder)
                .when(cx.single() && local),
        );
    }

    // Navidrome shares; YouTube Music's song menu already has "Copy link".
    let shareable = !matches!(cx.target, MenuTarget::Genres(_));
    out.push(
        Entry::action(Icon::LuShare2, "Share…", MenuAction::ShareItem)
            .requires(Capabilities::SHARES)
            .when(shareable),
    );
    let song_target = matches!(cx.target, MenuTarget::Songs { .. });
    out.push(
        Entry::action(Icon::LuLink, "Copy link", MenuAction::ShareItem)
            .when(ytm && cx.single() && !song_target && kind_has_details),
    );

    let savable = !matches!(cx.target, MenuTarget::Genres(_) | MenuTarget::Artists(_));
    out.push(
        Entry::action(Icon::LuDownload, "Save file…", MenuAction::SaveFile).when(can_save(cx) && savable),
    );
}
