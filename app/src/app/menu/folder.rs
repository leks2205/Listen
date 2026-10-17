//! Entries for folders of the folder browser (single or several), in Feishin's order
//! (menus/folder-context-menu.tsx). They act on every song below the folder, recursively.

use super::MenuTarget;
use super::entry::{self, Ctx, Entry};

pub fn entries(cx: &Ctx, out: &mut Vec<Entry>) {
    let MenuTarget::Folders(_) = cx.target else { return };
    out.push(entry::play_submenu());
    out.push(Entry::divider());
    out.push(entry::add_to_playlist());
    out.push(Entry::divider());
    // Details, share and download entries.
    crate::app::item::menu::entries(cx, out);
    out.push(entry::clear_selection(cx));
}
