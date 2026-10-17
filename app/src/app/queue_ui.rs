//! The play queue's surfaces: side panel, drawer, Now Playing page and toolbar (Feishin
//! `features/now-playing/*`, `player/components/queue*`); widgets in `ui/queue.rs`.
//!
//! - Rows are play-order positions ([`QRow`]) read from `Listen::queue`, never copies;
//!   with a search only the matching positions are rows (`QueueUi::found`).
//! - Tables: [`ListId::Queue`] on the page, [`ListId::SideQueue`] in the panel, drawer and
//!   full-screen player. Rows drag via `DropTarget::QueueAt`; other lists drop between rows.
//! - The DJs see the queue only through `Queue` (uids, `auto`, `queued`). Songs the DJs
//!   add skip the player filters; songs the user adds don't.
//! - "Play next" / "Add to queue" go to the top / end of the Up Next block after the current
//!   song (Spotify style), ahead of the Auto-DJ songs.

use super::list::{DragPayload, ListId};
use super::menu::{MenuAction, MenuTarget};
use super::overlay::Confirm;
use super::*;
use crate::ui::queue::QRow;
use listen_core::settings::{FilterField, FilterOp, PlayerFilter, SideQueueLayout, SideQueueStyle};
use listen_source::{Capabilities, PlayQueue};

/// How long the pointer rests on the edge strip before the drawer slides in.
pub const DRAWER_DELAY: Duration = Duration::from_millis(500);
/// Smallest height of the queue panel under the page (vertical layout).
pub const PANEL_MIN_H: f32 = 160.0;

/// What a panel divider changes while dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeAxis {
    /// The height of the queue panel under the page.
    QueueHeight,
    /// The height of the panels (lyrics, visualizer) under the queue; they grow when dragged up.
    PanelHeight,
    /// The height of a panel above the queue; it grows when dragged down.
    PanelHeightFromTop,
}

#[derive(Debug, Clone, Copy)]
pub struct PanelResize {
    pub axis: ResizeAxis,
    start_y: Option<f32>,
    start: f32,
}

/// State of the queue surfaces.
#[derive(Debug, Default)]
pub struct QueueUi {
    /// The search text of the toolbar.
    pub search: String,
    /// The rows that match the search (empty search: every position is a row).
    pub found: Vec<QRow>,
    /// Uids of the rows when the selection was last remapped.
    prev: Vec<u64>,
    stamp: (u64, u64),
    /// The pointer rests on the drawer's edge strip.
    pub edge: bool,
    /// Bumped to cancel a pending drawer opening.
    pub drawer_gen: u64,
    /// The drawer was opened by the pointer (and closes when it leaves).
    pub drawer_open: bool,
    /// The pointer is over the drawer.
    pub drawer_inside: bool,
    /// Where the toolbar's "…" menu is open.
    pub more_menu: Option<iced::Point>,
    /// A save to / restore from the server is running.
    pub busy: bool,
    /// Track changes since the queue was last saved to the server.
    changes: u32,
    /// Don't ask again: the replacement was confirmed.
    bypass_confirm: bool,
    /// The queue list that was on screen after the last update.
    shown: Option<ListId>,
    pub resize: Option<PanelResize>,
}

#[derive(Debug, Clone)]
pub enum QueueMsg {
    Search(String),
    Shuffle,
    Clear,
    ClearConfirmed,
    JumpToCurrent,
    CreatePlaylist,
    RemoveSelected,
    MoreMenu(Option<iced::Point>),
    SaveToServer,
    Saved(Result<usize, String>),
    RestoreFromServer,
    Restored(Result<Option<PlayQueue>, String>),
    RestoreConfirmed(PlayQueue),
    /// The drawer's edge strip was entered / left.
    EdgeEnter,
    EdgeExit,
    DrawerOpen(u64),
    DrawerEnter,
    DrawerExit,
    DrawerClose,
    ResizeStart(ResizeAxis),
    ResizeMove(iced::Point),
    ResizeEnd,
    /// "Play now" was confirmed (the queue is replaced).
    PlayConfirmed(Vec<Song>, usize),
    ApplyConfirmed(MenuAction, Vec<Song>),
    Setting(QueueSetting),
}

/// A change on the settings page.
#[derive(Debug, Clone)]
pub enum QueueSetting {
    FollowCurrent(bool),
    UpNext(bool),
    Confirm(bool),
    SavePlayQueue(bool),
    AutosaveServer(bool),
    AutosaveEvery(u32),
    Style(SideQueueStyle),
    Layout(SideQueueLayout),
    LyricsInSidebar(bool),
    Combined(bool),
    QueuePanel(bool),
    /// Moves a panel (`queue`, `lyrics`, `visualizer`) up (-1) or down (+1).
    MovePanel(String, i8),
    FilterAdd,
    FilterRemove(usize),
    FilterField(usize, FilterField),
    FilterOp(usize, FilterOp),
    FilterValue(usize, String),
    FilterEnabled(usize, bool),
}

impl Listen {
    // ---- which surfaces are on screen --------------------------------------------

    /// The queue sits in a panel next to (or under) the page.
    pub fn queue_attached(&self) -> bool {
        self.show_queue
            && !self.fullscreen
            && self.route != Route::NowPlaying
            && self.config.settings.queue.side_style == SideQueueStyle::Attached
    }

    /// The drawer is on screen: pinned open with the queue button, or slid in
    /// by the pointer.
    pub fn queue_drawer_visible(&self) -> bool {
        !self.fullscreen
            && self.route != Route::NowPlaying
            && self.config.settings.queue.side_style == SideQueueStyle::Detached
            && (self.show_queue || self.queue_ui.drawer_open)
    }

    /// The edge strip that opens the drawer is on screen.
    pub fn queue_edge_visible(&self) -> bool {
        !self.fullscreen
            && self.route != Route::NowPlaying
            && self.config.settings.queue.side_style == SideQueueStyle::Detached
            && !self.show_queue
            && !self.queue_ui.drawer_open
    }

    /// Some right-hand surface shows the queue: the panel, the drawer, the page.
    pub fn queue_on_screen(&self) -> bool {
        self.queue_attached() || self.queue_drawer_visible() || self.route == Route::NowPlaying
    }

    /// The queue list that is on screen (`None`: none).
    pub fn queue_list_shown(&self) -> Option<ListId> {
        if self.fullscreen {
            return (self.fullscreen_tab == FullscreenTab::Queue).then_some(ListId::SideQueue);
        }
        if self.route == Route::NowPlaying {
            return Some(ListId::Queue);
        }
        (self.queue_attached() || self.queue_drawer_visible()).then_some(ListId::SideQueue)
    }

    /// A queue list appeared (panel, page or full-screen tab): scrolls it to the playing
    /// song. `None` when nothing changed.
    pub(super) fn queue_surface_changed(&mut self) -> Option<Task<Message>> {
        let now = self.queue_list_shown();
        if now == self.queue_ui.shown {
            return None;
        }
        self.queue_ui.shown = now.clone();
        let id = now?;
        self.lists.entry(&id).scroll.offset = 0.0;
        Some(Task::batch([self.queue_follow(), self.queue_list_visible(&id)]))
    }

    // ---- the rows ----------------------------------------------------------------

    /// The rows of the queue lists: every position, or those that match the search.
    pub fn queue_rows(&self) -> &[QRow] {
        if self.queue_ui.search.trim().is_empty() {
            crate::ui::queue::identity(self.queue.len())
        } else {
            &self.queue_ui.found
        }
    }

    pub fn queue_row_count(&self) -> usize {
        self.queue_rows().len()
    }

    /// The play-order position of a row.
    pub fn queue_row_pos(&self, row: usize) -> Option<usize> {
        self.queue_rows().get(row).map(|r| r.0).filter(|p| *p < self.queue.len())
    }

    /// The songs of the rows (what the list actions work on).
    pub fn queue_row_songs(&self) -> Vec<Song> {
        self.queue_rows().iter().filter_map(|r| self.queue.item_at(r.0)).map(|i| i.song.clone()).collect()
    }

    /// The row that shows play-order position `pos`.
    pub fn queue_row_of(&self, pos: usize) -> Option<usize> {
        if self.queue_ui.search.trim().is_empty() {
            (pos < self.queue.len()).then_some(pos)
        } else {
            self.queue_ui.found.iter().position(|r| r.0 == pos)
        }
    }

    /// The selected rows of a queue list as (row, uid), ascending.
    fn queue_selected(&self, id: &ListId) -> Vec<(usize, u64)> {
        self.lists
            .get(id)
            .selection
            .indices()
            .into_iter()
            .filter_map(|row| Some((row, self.queue.item_at(self.queue_row_pos(row)?)?.uid)))
            .collect()
    }

    /// Menu target of the selected rows (their uids are what "Remove" and
    /// "Move to top" need).
    pub(super) fn queue_selection_target(&self, id: &ListId) -> Option<MenuTarget> {
        let mut songs = Vec::new();
        let mut uids = Vec::new();
        for (row, uid) in self.queue_selected(id) {
            let pos = self.queue_row_pos(row)?;
            songs.push(self.queue.item_at(pos)?.song.clone());
            uids.push(uid);
        }
        (!songs.is_empty()).then_some(MenuTarget::Songs {
            songs,
            queue_uids: uids,
            history_entries: Vec::new(),
        })
    }

    /// Keeps the search result and the selection in step with the queue. Runs
    /// after every update.
    pub(super) fn queue_ui_sync(&mut self) {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&self.queue_ui.search, &mut h);
        let stamp = (self.queue.signature(), std::hash::Hasher::finish(&h));
        if stamp == self.queue_ui.stamp {
            return;
        }
        let searching = !self.queue_ui.search.trim().is_empty();
        let uids: Vec<u64> = self.queue.iter().map(|i| i.uid).collect();
        if searching {
            let q = self.queue_ui.search.trim().to_lowercase();
            self.queue_ui.found = self
                .queue
                .iter()
                .enumerate()
                .filter(|(_, i)| crate::ui::queue::matches(&i.song, &q))
                .map(|(n, _)| QRow(n))
                .collect();
        } else {
            self.queue_ui.found.clear();
        }
        // The selected songs stay selected where they moved to.
        if stamp.1 == self.queue_ui.stamp.1 {
            let prev = std::mem::take(&mut self.queue_ui.prev);
            for id in [ListId::Queue, ListId::SideQueue] {
                let selected = self.lists.get(&id).selection.indices();
                if selected.is_empty() {
                    continue;
                }
                let keep: Vec<u64> = selected.iter().filter_map(|&r| prev.get(r).copied()).collect();
                let rows: Vec<usize> = if searching {
                    Vec::new()
                } else {
                    keep.iter().filter_map(|u| uids.iter().position(|x| x == u)).collect()
                };
                self.lists.entry(&id).selection.set_many(rows);
            }
        } else {
            for id in [ListId::Queue, ListId::SideQueue] {
                self.lists.entry(&id).selection.clear();
            }
        }
        self.queue_ui.prev = uids;
        self.queue_ui.stamp = stamp;
    }

    // ---- scrolling ---------------------------------------------------------------

    /// Covers of the rows in view.
    pub(super) fn queue_list_visible(&mut self, id: &ListId) -> Task<Message> {
        let (first, last) = self.list_range(id, self.queue_row_count());
        let refs: Vec<(ServerId, ImageRef)> = (first..last)
            .filter_map(|r| self.queue.item_at(self.queue_row_pos(r)?))
            .filter_map(|i| Some((i.song.server_id.clone(), i.song.image.clone()?)))
            .collect();
        Task::batch(refs.iter().map(|(s, i)| self.request_image_on(s, Some(i))).collect::<Vec<_>>())
    }

    /// Scrolls the queue list `id` so `row` is in the middle of the view.
    fn queue_scroll_to_row(&mut self, id: &ListId, row: usize) -> Task<Message> {
        let config = self.config.settings.lists.config(id.kind());
        let row_h = config.table.size.height();
        let header = if config.table.header { crate::ui::list::table::HEADER_BLOCK } else { 0.0 };
        let groups = crate::ui::queue::groups(self).iter().filter(|g| g.before <= row).count();
        let top = header + groups as f32 * crate::ui::list::table::GROUP_H + row as f32 * row_h;
        let view = self.lists.scroll(id).height;
        let y = if view > 0.0 { (top - (view - row_h) / 2.0).max(0.0) } else { top };
        self.lists.entry(id).scroll.offset = y;
        let scroll = self.scroll_list_to(id, y);
        Task::batch([scroll, self.list_page_visible(id)])
    }

    /// The lyrics panel of the right sidebar is on screen.
    pub(super) fn queue_panel_lyrics_shown(&self) -> bool {
        (self.queue_attached() || self.queue_drawer_visible())
            && crate::ui::queue::panels(self)
                .iter()
                .any(|p| matches!(p, crate::ui::queue::Panel::Lyrics | crate::ui::queue::Panel::Combined))
    }

    /// Follow current song: the queue on screen scrolls to the playing row.
    pub(super) fn queue_follow(&mut self) -> Task<Message> {
        if !self.config.settings.queue.follow_current_song {
            return Task::none();
        }
        self.queue_jump_to_current()
    }

    fn queue_jump_to_current(&mut self) -> Task<Message> {
        let Some(id) = self.queue_list_shown() else { return Task::none() };
        let Some(row) = self.queue.current_position().and_then(|p| self.queue_row_of(p)) else {
            return Task::none();
        };
        self.queue_scroll_to_row(&id, row)
    }

    /// Hook of every track change: the follow scroll and the server autosave.
    pub(super) fn queue_track_changed(&mut self) -> Task<Message> {
        let follow = self.queue_follow();
        let q = &self.config.settings.queue;
        if !q.autosave_server || self.demo {
            return follow;
        }
        self.queue_ui.changes += 1;
        if self.queue_ui.changes < q.autosave_every.max(1) {
            return follow;
        }
        self.queue_ui.changes = 0;
        match self.queue_save_task(false) {
            Some(save) => Task::batch([follow, save]),
            None => follow,
        }
    }

    // ---- activating rows ---------------------------------------------------------

    /// Double-click / Enter / the play button on a row: jump to that song.
    pub(super) fn queue_activate(&mut self, row: usize) -> Task<Message> {
        let Some(uid) = self.queue_row_pos(row).and_then(|p| self.queue.item_at(p)).map(|i| i.uid) else {
            return Task::none();
        };
        self.queue.jump_to(uid);
        self.start_current()
    }

    /// The row a dragged row would land before (the list marks it).
    pub fn queue_landing(&self, list: &ListId, row: usize) -> bool {
        list.is_queue()
            && self.lists.drag.is_some()
            && self.lists.get(list).hover == Some(row)
            && self.lists.drag.as_ref().is_some_and(|d| {
                d.payload.from != *list || {
                    // Not on a row that is itself being dragged.
                    !self.lists.get(&d.payload.from).selection.contains(row)
                }
            })
    }

    /// Drop on row `row` (or past the last): queue rows move there, items from other lists
    /// are inserted (albums, playlists and artists fetched first).
    pub(super) fn queue_drop(&mut self, row: usize, payload: DragPayload) -> Task<Message> {
        let at = if row >= self.queue_row_count() {
            self.queue.len()
        } else {
            self.queue_row_pos(row).unwrap_or(self.queue.len())
        };
        if payload.from.is_queue() {
            let moved: Vec<u64> = self.queue_selected(&payload.from).into_iter().map(|(_, u)| u).collect();
            self.queue.move_uids(&moved, at);
            self.queue_ui_sync();
            // The moved rows stay selected where they landed.
            let rows: Vec<usize> = moved
                .iter()
                .filter_map(|u| self.queue.iter().position(|i| i.uid == *u))
                .filter_map(|p| self.queue_row_of(p))
                .collect();
            self.lists.entry(&payload.from).selection.set_many(rows);
            return self.refresh_next();
        }
        let before = self.queue.item_at(at).map_or(u64::MAX, |i| i.uid);
        self.apply_to_picks(MenuAction::InsertBefore(before), payload.items)
    }

    /// The songs of a drop land before the queue row with `uid` (`u64::MAX`: at the end).
    pub(super) fn queue_insert_before(&mut self, uid: u64, songs: Vec<Song>) -> Task<Message> {
        let at = self.queue.iter().position(|i| i.uid == uid).unwrap_or(self.queue.len());
        let was_idle = self.queue.current().is_none();
        self.note_listener_picks(&songs);
        let n = songs.len();
        self.queue.insert_at(at, songs);
        self.toast_info(if n == 1 {
            "Added to the queue.".into()
        } else {
            format!("{n} songs added to the queue.")
        });
        if was_idle { self.start_current() } else { self.refresh_next() }
    }

    // ---- replacing the queue, with a confirmation --------------------------------

    /// Runs `f` without the replace-queue confirmation (for callers outside the window,
    /// e.g. the MCP server).
    pub(super) fn without_queue_confirm<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let was = std::mem::replace(&mut self.queue_ui.bypass_confirm, true);
        let out = f(self);
        self.queue_ui.bypass_confirm = was;
        out
    }

    /// The setting asks before a queue with songs is replaced.
    pub(super) fn queue_confirm_needed(&self) -> bool {
        self.config.settings.queue.confirm_queue_changes
            && !self.queue.is_empty()
            && !self.queue_ui.bypass_confirm
    }

    pub(super) fn queue_confirm_play(&mut self, songs: Vec<Song>, start: usize) -> Task<Message> {
        self.confirm(
            Confirm::new(
                "Replace the queue?",
                "Playing this replaces the songs in your queue.",
                Message::Queue(QueueMsg::PlayConfirmed(songs, start)),
            )
            .labels("Replace", "Cancel"),
        );
        Task::none()
    }

    pub(super) fn queue_confirm_apply(&mut self, action: MenuAction, songs: Vec<Song>) -> Task<Message> {
        self.confirm(
            Confirm::new(
                "Replace the queue?",
                "Playing this replaces the songs in your queue.",
                Message::Queue(QueueMsg::ApplyConfirmed(action, songs)),
            )
            .labels("Replace", "Cancel"),
        );
        Task::none()
    }

    // ---- the server's play queue -------------------------------------------------

    /// The browsed server can keep a play queue (OpenSubsonic).
    pub fn queue_server_sync(&self) -> bool {
        !self.demo
            && self.source.as_ref().is_some_and(|s| s.capabilities().contains(Capabilities::PLAY_QUEUE_SYNC))
    }

    /// The queue as the server stores it: the songs of that server in play
    /// order, the playing one and how far it got.
    pub fn queue_for_server(&self, server: &ServerId) -> PlayQueue {
        let songs: Vec<Song> =
            self.queue.iter().filter(|i| &i.song.server_id == server).map(|i| i.song.clone()).collect();
        let current = self.queue.current().filter(|i| &i.song.server_id == server).map(|i| i.song.id.clone());
        PlayQueue { songs, current, position: self.playback.position, changed_by: None }
    }

    /// `savePlayQueue` for the browsed server; `loud`: report the result.
    fn queue_save_task(&mut self, loud: bool) -> Option<Task<Message>> {
        let source =
            self.source.clone().filter(|s| s.capabilities().contains(Capabilities::PLAY_QUEUE_SYNC))?;
        let server = self.config.active_server.clone()?;
        let queue = self.queue_for_server(&server);
        if queue.songs.is_empty() {
            if loud {
                self.toast_info("The queue has no songs of this server to save.".into());
            }
            return None;
        }
        let n = queue.songs.len();
        Some(Task::perform(
            async move { source.save_play_queue(&queue).await.map(|_| n).map_err(err) },
            move |r| {
                if loud {
                    Message::Queue(QueueMsg::Saved(r))
                } else {
                    if let Err(e) = r {
                        tracing::warn!("saving the play queue to the server failed: {e}");
                    }
                    Message::Noop
                }
            },
        ))
    }

    /// Replaces the queue with the one from the server. Nothing plays yet;
    /// the first play resumes where the other client stopped.
    fn queue_apply_server(&mut self, q: PlayQueue) -> Task<Message> {
        if q.songs.is_empty() {
            return Task::none();
        }
        let start = q.current.as_ref().and_then(|id| q.songs.iter().position(|s| &s.id == id)).unwrap_or(0);
        let n = q.songs.len();
        let cancel = self.mix_cancel("the queue was restored");
        self.dj_interrupt();
        self.bump_token();
        self.queue.set(q.songs, start);
        self.playback.playing = false;
        self.playback.position = q.position;
        self.playback.duration = None;
        self.restored = Some(q.position);
        self.toast_success(format!("Restored {n} songs from the server."));
        let stop = self.stop_player();
        let (image, lyrics) = match self.queue.current().map(|i| i.song.clone()) {
            Some(song) => (self.request_image_on(&song.server_id, song.image.as_ref()), self.load_lyrics()),
            None => (Task::none(), Task::none()),
        };
        Task::batch([cancel, stop, image, lyrics, self.refresh_next()])
    }

    // ---- messages ----------------------------------------------------------------

    pub(super) fn on_queue(&mut self, msg: QueueMsg) -> Task<Message> {
        match msg {
            QueueMsg::Search(text) => {
                self.queue_ui.search = text;
                self.queue_ui_sync();
                for id in [ListId::Queue, ListId::SideQueue] {
                    self.lists.entry(&id).selection.clear();
                }
            }
            QueueMsg::Shuffle => {
                self.queue.shuffle_all();
                return self.refresh_next();
            }
            QueueMsg::Clear => {
                if self.queue.is_empty() {
                    return Task::none();
                }
                if self.config.settings.queue.confirm_queue_changes {
                    self.confirm(
                        Confirm::new(
                            "Clear the queue?",
                            "Every song is removed and playback stops.",
                            Message::Queue(QueueMsg::ClearConfirmed),
                        )
                        .labels("Clear", "Cancel")
                        .danger(),
                    );
                    return Task::none();
                }
                return self.queue_clear();
            }
            QueueMsg::ClearConfirmed => return self.queue_clear(),
            QueueMsg::JumpToCurrent => return self.queue_jump_to_current(),
            QueueMsg::CreatePlaylist => {
                let songs: Vec<Song> = self.queue.iter().map(|i| i.song.clone()).collect();
                if songs.is_empty() {
                    self.toast_info("The queue is empty.".into());
                    return Task::none();
                }
                self.queue_ui.more_menu = None;
                return self.on_playlist_page(playlists::PlaylistPageMsg::Create(songs));
            }
            QueueMsg::RemoveSelected => return self.queue_remove_selected(),
            QueueMsg::MoreMenu(at) => self.queue_ui.more_menu = at,
            QueueMsg::SaveToServer => {
                self.queue_ui.more_menu = None;
                if self.queue_ui.busy {
                    return Task::none();
                }
                if let Some(task) = self.queue_save_task(true) {
                    self.queue_ui.busy = true;
                    return task;
                }
            }
            QueueMsg::Saved(result) => {
                self.queue_ui.busy = false;
                match result {
                    Ok(n) => self.toast_success(format!("Saved {n} songs to the server.")),
                    Err(e) => self.toast_error(format!("Could not save the queue to the server: {e}")),
                }
            }
            QueueMsg::RestoreFromServer => {
                self.queue_ui.more_menu = None;
                let Some(source) = self.source.clone().filter(|_| self.queue_server_sync()) else {
                    return Task::none();
                };
                if self.queue_ui.busy {
                    return Task::none();
                }
                self.queue_ui.busy = true;
                return Task::perform(async move { source.play_queue().await.map_err(err) }, |r| {
                    Message::Queue(QueueMsg::Restored(r))
                });
            }
            QueueMsg::Restored(result) => {
                self.queue_ui.busy = false;
                match result {
                    Ok(Some(q)) if !q.songs.is_empty() => {
                        if self.queue_confirm_needed() {
                            self.confirm(
                                Confirm::new(
                                    "Replace the queue?",
                                    "The queue saved on the server replaces the songs in your queue.",
                                    Message::Queue(QueueMsg::RestoreConfirmed(q)),
                                )
                                .labels("Replace", "Cancel"),
                            );
                            return Task::none();
                        }
                        return self.queue_apply_server(q);
                    }
                    Ok(_) => self.toast_info("No queue is saved on the server.".into()),
                    Err(e) => self.toast_error(format!("Could not restore the queue: {e}")),
                }
            }
            QueueMsg::RestoreConfirmed(q) => return self.queue_apply_server(q),
            QueueMsg::EdgeEnter => {
                self.queue_ui.edge = true;
                self.queue_ui.drawer_gen += 1;
                let generation = self.queue_ui.drawer_gen;
                return Task::perform(
                    async move {
                        tokio::time::sleep(DRAWER_DELAY).await;
                        generation
                    },
                    |g| Message::Queue(QueueMsg::DrawerOpen(g)),
                );
            }
            QueueMsg::EdgeExit => {
                self.queue_ui.edge = false;
                self.queue_ui.drawer_gen += 1;
            }
            QueueMsg::DrawerOpen(generation) => {
                if generation == self.queue_ui.drawer_gen && self.queue_ui.edge {
                    self.queue_ui.drawer_open = true;
                    self.queue_ui.drawer_inside = true;
                    return Task::batch([
                        self.queue_jump_to_current(),
                        self.queue_list_visible(&ListId::SideQueue),
                    ]);
                }
            }
            QueueMsg::DrawerEnter => self.queue_ui.drawer_inside = true,
            QueueMsg::DrawerExit => {
                self.queue_ui.drawer_inside = false;
                // Leaving while rows are dragged keeps it open (the drop may come from inside).
                if self.lists.drag.is_none() {
                    self.queue_ui.drawer_open = false;
                    self.queue_ui.edge = false;
                }
            }
            QueueMsg::DrawerClose => {
                self.queue_ui.drawer_open = false;
                self.queue_ui.edge = false;
                self.show_queue = false;
            }
            QueueMsg::ResizeStart(axis) => {
                let q = &self.config.settings.queue;
                let start = match axis {
                    ResizeAxis::QueueHeight => q.height,
                    ResizeAxis::PanelHeight | ResizeAxis::PanelHeightFromTop => q.panel_height,
                };
                self.queue_ui.resize = Some(PanelResize { axis, start_y: None, start });
            }
            QueueMsg::ResizeMove(at) => {
                if let Some(r) = &mut self.queue_ui.resize {
                    let y0 = *r.start_y.get_or_insert(at.y);
                    let up = r.start - (at.y - y0);
                    let down = r.start + (at.y - y0);
                    let max = (self.window_size.height - 240.0).max(PANEL_MIN_H);
                    let q = &mut self.config.settings.queue;
                    match r.axis {
                        ResizeAxis::QueueHeight => q.height = up.clamp(PANEL_MIN_H, max),
                        ResizeAxis::PanelHeight => q.panel_height = up.clamp(100.0, max),
                        ResizeAxis::PanelHeightFromTop => q.panel_height = down.clamp(100.0, max),
                    }
                }
            }
            QueueMsg::ResizeEnd => {
                if self.queue_ui.resize.take().is_some() {
                    self.save_config();
                }
            }
            QueueMsg::PlayConfirmed(songs, start) => {
                self.queue_ui.bypass_confirm = true;
                let task = self.play_picked(songs, start);
                self.queue_ui.bypass_confirm = false;
                return task;
            }
            QueueMsg::ApplyConfirmed(action, songs) => {
                self.queue_ui.bypass_confirm = true;
                let task = self.apply_to_songs(action, songs);
                self.queue_ui.bypass_confirm = false;
                return task;
            }
            QueueMsg::Setting(change) => return self.queue_setting(change),
        }
        Task::none()
    }

    /// Empties the queue and stops playback.
    fn queue_clear(&mut self) -> Task<Message> {
        let cancel = self.mix_cancel("the queue was cleared");
        self.dj_interrupt();
        self.bump_token();
        self.queue.clear();
        self.playback.position = Duration::ZERO;
        self.playback.duration = None;
        self.restored = None;
        let stop = self.stop_player();
        Task::batch([cancel, stop])
    }

    /// Delete / Backspace and the selection bar: takes the selected rows out
    /// (the playing song stays).
    fn queue_remove_selected(&mut self) -> Task<Message> {
        let Some(id) = self.queue_list_shown() else { return Task::none() };
        let playing = self.queue.current().map(|i| i.uid);
        let uids: Vec<u64> =
            self.queue_selected(&id).into_iter().map(|(_, u)| u).filter(|u| Some(*u) != playing).collect();
        if uids.is_empty() {
            return Task::none();
        }
        self.queue.remove_many(&uids);
        self.lists.entry(&id).selection.clear();
        self.refresh_next()
    }

    /// Delete / Backspace in the shown queue.
    pub(super) fn queue_key(
        &mut self,
        key: &iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        use iced::keyboard::{Key, key::Named};
        if !matches!(key, Key::Named(Named::Delete | Named::Backspace))
            || modifiers.command()
            || modifiers.alt()
            || self.menu.is_some()
            || self.overlay.modal.is_some()
        {
            return None;
        }
        let id = self.queue_list_shown()?;
        if self.fullscreen || self.lists.get(&id).selection.is_empty() {
            return None;
        }
        Some(self.queue_remove_selected())
    }

    /// The pointer moves of a panel divider drag.
    pub(super) fn queue_subscription(&self) -> Option<Subscription<Message>> {
        self.queue_ui.resize.is_some().then(|| {
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::Queue(QueueMsg::ResizeMove(position)))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
                | iced::Event::Mouse(iced::mouse::Event::CursorLeft) => {
                    Some(Message::Queue(QueueMsg::ResizeEnd))
                }
                _ => None,
            })
        })
    }

    /// The drawer closes after a drag that ended outside it.
    pub(super) fn queue_drawer_settle(&mut self) {
        if self.queue_ui.drawer_open && !self.queue_ui.drawer_inside && self.lists.drag.is_none() {
            self.queue_ui.drawer_open = false;
            self.queue_ui.edge = false;
        }
    }

    // ---- settings ----------------------------------------------------------------

    fn queue_setting(&mut self, change: QueueSetting) -> Task<Message> {
        let q = &mut self.config.settings.queue;
        let mut follow = false;
        match change {
            QueueSetting::FollowCurrent(v) => {
                q.follow_current_song = v;
                follow = v;
            }
            QueueSetting::UpNext(v) => q.up_next_queue = v,
            QueueSetting::Confirm(v) => q.confirm_queue_changes = v,
            QueueSetting::SavePlayQueue(v) => q.save_play_queue = v,
            QueueSetting::AutosaveServer(v) => q.autosave_server = v,
            QueueSetting::AutosaveEvery(n) => q.autosave_every = n.clamp(1, 100),
            QueueSetting::Style(s) => {
                q.side_style = s;
                self.queue_ui.drawer_open = false;
                self.queue_ui.edge = false;
            }
            QueueSetting::Layout(l) => q.side_layout = l,
            QueueSetting::LyricsInSidebar(v) => q.lyrics_in_sidebar = v,
            QueueSetting::Combined(v) => q.combined_panel = v,
            QueueSetting::QueuePanel(v) => self.config.settings.sidebar.show_queue = v,
            QueueSetting::MovePanel(name, by) => {
                let order = &mut self.config.settings.sidebar.panel_order;
                crate::ui::queue::normalize_order(order);
                if let Some(i) = order.iter().position(|p| *p == name) {
                    let j = (i as i64 + i64::from(by)).clamp(0, order.len() as i64 - 1) as usize;
                    order.swap(i, j);
                }
            }
            QueueSetting::FilterAdd => q.filters.push(PlayerFilter::default()),
            QueueSetting::FilterRemove(i) => {
                if i < q.filters.len() {
                    q.filters.remove(i);
                }
            }
            QueueSetting::FilterField(i, field) => {
                if let Some(f) = q.filters.get_mut(i) {
                    f.field = field;
                    // The operators of the new kind of field.
                    let ops = FilterOp::for_kind(field.kind());
                    if !ops.contains(&f.op) {
                        f.op = ops[0];
                    }
                }
            }
            QueueSetting::FilterOp(i, op) => {
                if let Some(f) = q.filters.get_mut(i) {
                    f.op = op;
                }
            }
            QueueSetting::FilterValue(i, value) => {
                if let Some(f) = q.filters.get_mut(i) {
                    f.value = value;
                }
            }
            QueueSetting::FilterEnabled(i, on) => {
                if let Some(f) = q.filters.get_mut(i) {
                    f.enabled = on;
                }
            }
        }
        self.save_config();
        if follow { self.queue_jump_to_current() } else { Task::none() }
    }

    /// `songs` without those a player filter keeps out of the queue. Says so
    /// when something was filtered.
    pub(super) fn queue_filter(&mut self, songs: Vec<Song>) -> Vec<Song> {
        let before = songs.len();
        let kept = self.config.settings.queue.filtered(songs);
        let dropped = before - kept.len();
        if dropped > 0 {
            self.toast_info(if kept.is_empty() {
                "Your player filters kept every song out of the queue.".into()
            } else {
                format!("{dropped} songs were kept out by your player filters.")
            });
        }
        kept
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Args;
    use crate::app::dj::Break;
    use crate::ui::list::GroupLook;

    fn state() -> Listen {
        let (s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
        s
    }

    /// The AI DJ's "break" marker still shows before the song it airs ahead of.
    #[test]
    fn the_ai_dj_break_is_marked_before_its_song() {
        let mut s = state();
        assert!(crate::ui::queue::groups(&s).iter().all(|g| g.look != GroupLook::DjBreak));
        let uid = s.queue.item_at(4).unwrap().uid;
        s.dj.brk = Break::Armed {
            before_uid: uid,
            clip: None,
            line: "hi".into(),
            mode: listen_dj::BreakMode::Between,
            clip_secs: 0.0,
        };
        let groups = crate::ui::queue::groups(&s);
        let marker = groups.iter().find(|g| g.look == GroupLook::DjBreak).expect("the marker");
        assert_eq!((marker.before, marker.title.as_str()), (4, "AI DJ break"));
        // It sits before the song's row even with Up Next headers around it.
        s.queue.enqueue_next(vec![s.queue.item_at(0).unwrap().song.clone()]);
        let groups = crate::ui::queue::groups(&s);
        let rows: Vec<usize> = groups.iter().map(|g| g.before).collect();
        assert!(rows.windows(2).all(|w| w[0] <= w[1]), "sorted: {rows:?}");
        assert_eq!(groups.iter().filter(|g| g.look == GroupLook::Label).count(), 3, "Queue, Up Next, Queue");
        // A search hides the headers but the marker follows its song.
        s.queue_ui.search = "last".into();
        s.queue_ui_sync();
        assert!(crate::ui::queue::groups(&s).iter().all(|g| g.look != GroupLook::Label));
    }

    /// Rows are the queue itself: no copy that could go stale.
    #[test]
    fn rows_read_the_queue_directly() {
        let mut s = state();
        assert_eq!(s.queue_row_count(), s.queue.len());
        s.queue.clear();
        assert_eq!(s.queue_row_count(), 0);
        assert!(s.queue_row_pos(0).is_none());
    }

    #[test]
    fn search_rows_keep_their_queue_positions() {
        let mut s = state();
        s.queue_ui.search = "coastl".into();
        s.queue_ui_sync();
        assert_eq!(s.queue_row_count(), 1);
        let pos = s.queue_row_pos(0).unwrap();
        assert_eq!(s.queue.item_at(pos).unwrap().song.title, "Coastline");
        assert_eq!(s.queue_row_of(pos), Some(0));
        assert_eq!(s.queue_row_of(0), None);
    }
}
