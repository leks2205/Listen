//! The menu of rows of the play queue, in Feishin's order
//! (menus/queue-context-menu.tsx): Remove from queue, Play, Move items,
//! Shuffle, Track radio, then what every song menu has.

use super::entry::{self, Ctx, Entry};
use super::{MenuAction, MenuTarget, QueueMove, song};
use crate::icons::Icon;

pub fn entries(cx: &Ctx, out: &mut Vec<Entry>) {
    let MenuTarget::Songs { songs, queue_uids, .. } = cx.target else { return };
    // The playing song cannot be removed.
    let playing = cx.state.queue.current().map(|i| i.uid);
    let removable: Vec<u64> = queue_uids.iter().copied().filter(|u| Some(*u) != playing).collect();
    out.push(
        Entry::action(Icon::LuX, "Remove from queue", MenuAction::RemoveFromQueue(removable.clone()))
            .when(!removable.is_empty()),
    );
    out.push(Entry::divider());
    out.push(entry::play_submenu());
    let uids = queue_uids.clone();
    out.push(Entry::submenu(
        Icon::LuMenu,
        "Move items",
        vec![
            Entry::action(
                Icon::LuArrowUp,
                "Move to top",
                MenuAction::MoveQueue(QueueMove::Top, uids.clone()),
            ),
            Entry::action(
                Icon::LuSkipForward,
                "Move to next",
                MenuAction::MoveQueue(QueueMove::Next, uids.clone()),
            ),
            Entry::action(
                Icon::LuArrowDown,
                "Move to bottom",
                MenuAction::MoveQueue(QueueMove::Bottom, uids.clone()),
            ),
        ],
    ));
    out.push(Entry::submenu(
        Icon::LuShuffle,
        "Shuffle",
        vec![
            Entry::action(Icon::LuShuffle, "Shuffle selected", MenuAction::ShuffleQueue(Some(uids))),
            Entry::action(Icon::LuShuffle, "Shuffle all", MenuAction::ShuffleQueue(None)),
        ],
    ));
    out.push(Entry::divider());
    out.push(song::track_radio(cx));
    out.push(Entry::divider());
    song::library_block(cx, songs, out);
    out.push(entry::clear_selection(cx));
}
