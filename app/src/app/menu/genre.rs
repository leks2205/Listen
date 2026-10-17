//! Genre menu entries, in the order of Feishin's `genre-context-menu.tsx`.

use super::MenuTarget;
use super::entry::{self, Ctx, Entry};

pub fn entries(cx: &Ctx, out: &mut Vec<Entry>) {
    let MenuTarget::Genres(genres) = cx.target else { return };
    if genres.is_empty() {
        return;
    }
    out.push(entry::play_submenu());
    out.push(Entry::divider());
    out.push(entry::add_to_playlist());
    out.push(Entry::divider());
    out.push(entry::clear_selection(cx));
}
