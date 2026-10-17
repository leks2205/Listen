//! Edit mode of the playlist page (Feishin: Edit, a drag column, "Save and
//! replace" after a confirmation, Cancel) and removing songs.

use super::*;
use crate::app::overlay::Confirm;

/// The playlist's order being edited.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    /// `order[i]` is the playlist position of the song now at row `i`.
    pub order: Vec<usize>,
    pub rows: Vec<Song>,
    /// A save is under way.
    pub saving: bool,
}

impl Edit {
    pub fn new(songs: &[Song]) -> Self {
        Edit { order: (0..songs.len()).collect(), rows: songs.to_vec(), saving: false }
    }

    /// Moves row `from` to row `to`.
    pub fn move_row(&mut self, from: usize, to: usize) {
        if from == to || from >= self.rows.len() || to >= self.rows.len() {
            return;
        }
        let song = self.rows.remove(from);
        self.rows.insert(to, song);
        let at = self.order.remove(from);
        self.order.insert(to, at);
    }

    /// The order differs from the playlist's.
    pub fn dirty(&self) -> bool {
        self.order.iter().enumerate().any(|(i, p)| i != *p)
    }
}

impl Listen {
    /// Edit mode is on for this list.
    pub fn playlist_editing(&self, list: &ListId) -> bool {
        matches!(list, ListId::Playlist(id)
            if self.playlist_page.edit.is_some() && self.playlist_page.id.as_deref() == Some(id.as_str()))
    }

    /// The row a dragged row would land on (the list marks it).
    pub fn playlist_landing(&self, list: &ListId, index: usize) -> bool {
        self.playlist_editing(list)
            && self.playlist_drag.is_some_and(|(from, over)| over == index && from != index)
    }

    /// Why the open playlist cannot be edited now (`None`: it can).
    pub fn playlist_edit_blocked(&self) -> Option<&'static str> {
        let d = self.open_playlist_detail()?;
        if !d.playlist.editable {
            return Some("This playlist is not yours to change.");
        }
        if !self.playlist_caps(&d.playlist).contains(Capabilities::PLAYLIST_REORDER) {
            return Some("This server cannot reorder playlists.");
        }
        if self.library.playlist_loading_more {
            return Some("The playlist is still loading.");
        }
        None
    }

    /// Whether the page offers Edit.
    pub fn playlist_can_edit(&self) -> bool {
        self.open_playlist_detail().is_some_and(|d| {
            d.playlist.editable && self.playlist_caps(&d.playlist).contains(Capabilities::PLAYLIST_REORDER)
        })
    }

    pub(in crate::app) fn playlist_drag_start(&mut self, row: usize) {
        if self.playlist_page.edit.is_some() {
            self.playlist_drag = Some((row, row));
        }
    }

    pub(in crate::app) fn playlist_drag_over(&mut self, row: usize) {
        if let Some((_, over)) = &mut self.playlist_drag {
            *over = row;
        }
    }

    pub(in crate::app) fn playlist_drag_end(&mut self) {
        let Some((from, to)) = self.playlist_drag.take() else { return };
        if let Some(edit) = &mut self.playlist_page.edit {
            edit.move_row(from, to);
            if let Some(id) = self.playlist_page.id.clone() {
                let list = ListId::Playlist(id);
                // The moved row stays selected where it landed.
                self.lists.entry(&list).selection.set_only(to);
            }
        }
    }

    /// Positions in the playlist of the rows selected in the open playlist's list.
    pub fn playlist_selected_positions(&self, list: &ListId) -> Vec<usize> {
        let selected = self.lists.get(list).selection.indices();
        let pos = &self.playlist_page.pos;
        let mut out: Vec<usize> = selected.into_iter().filter_map(|i| pos.get(i).copied()).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub(super) fn on_playlist_edit(&mut self, msg: PlaylistPageMsg) -> Task<Message> {
        use PlaylistPageMsg as P;
        match msg {
            P::EditStart => {
                if let Some(why) = self.playlist_edit_blocked() {
                    self.toast_info(why.into());
                    return Task::none();
                }
                let Some(detail) = self.open_playlist_detail() else { return Task::none() };
                let edit = Edit::new(&detail.songs);
                // The edited order is the playlist's own.
                self.playlist_page.search.clear();
                self.playlist_page.filters = SongFilters::default();
                self.playlist_page.sort = TrackSort::Id;
                self.playlist_page.desc = false;
                self.playlist_page.edit = Some(edit);
                self.playlist_drag = None;
                self.playlist_rows_sync();
                if let Some(id) = self.playlist_page.id.clone() {
                    // Edit mode works on tracks.
                    self.config.settings.lists.playlist_target = PlaylistTarget::Tracks;
                    self.lists.reset(&ListId::Playlist(id));
                }
                return self.playlist_list_visible();
            }
            P::EditCancel => {
                self.playlist_page.edit = None;
                self.playlist_drag = None;
                self.playlist_rows_sync();
                return self.playlist_list_visible();
            }
            P::EditSave => {
                let Some(edit) = self.playlist_page.edit.as_ref() else { return Task::none() };
                if !edit.dirty() {
                    self.toast_info("The order did not change.".into());
                    return Task::none();
                }
                self.confirm(
                    Confirm::new(
                        "Save and replace",
                        "Replace the playlist with the new order? Are you sure?",
                        Message::PlaylistPage(P::EditSaveConfirmed),
                    )
                    .labels("Save and replace", "Cancel"),
                );
            }
            P::EditSaveConfirmed => return self.playlist_save_order(),
            P::EditSaved(id, result) => {
                let current = self.playlist_page.id.as_deref() == Some(id.as_str());
                match result {
                    Ok(()) => {
                        if let (true, Some(edit)) = (current, self.playlist_page.edit.take())
                            && let Some(d) = self.library.playlist.as_mut()
                        {
                            d.songs = edit.rows;
                        }
                        self.playlist_drag = None;
                        self.playlist_rows_sync();
                        self.toast_success("Playlist updated.".into());
                        return self.playlist_list_visible();
                    }
                    Err(e) => {
                        if let Some(edit) = self.playlist_page.edit.as_mut() {
                            edit.saving = false;
                        }
                        self.toast_error(format!("Could not save the order: {e}"));
                    }
                }
            }
            P::Remove(id, positions) => {
                if positions.is_empty() {
                    return Task::none();
                }
                let name = self
                    .open_playlist_detail()
                    .map(|d| d.playlist.name.clone())
                    .unwrap_or_else(|| "the playlist".into());
                let what = if positions.len() == 1 {
                    "this song".to_string()
                } else {
                    format!("these {} songs", positions.len())
                };
                self.confirm(
                    Confirm::new(
                        "Remove from playlist",
                        format!("Are you sure you want to remove {what} from “{name}”?"),
                        Message::PlaylistPage(P::RemoveConfirmed(id, positions)),
                    )
                    .labels("Remove", "Cancel")
                    .danger(),
                );
            }
            P::RemoveConfirmed(id, positions) => return self.playlist_remove(id, positions),
            P::Removed(id, positions, result) => match result {
                Ok(()) => return self.playlist_removed(&id, &positions),
                Err(e) => self.toast_error(format!("Could not remove from the playlist: {e}")),
            },
            other => return self.on_playlist_form(other),
        }
        Task::none()
    }

    fn playlist_save_order(&mut self) -> Task<Message> {
        let Some(detail) = self.open_playlist_detail() else { return Task::none() };
        let (id, server) = (detail.playlist.id.clone(), detail.playlist.server_id.clone());
        let Some(edit) = self.playlist_page.edit.as_mut() else { return Task::none() };
        edit.saving = true;
        let order = edit.order.clone();
        match self.playlist_source(&server) {
            Some(source) if !self.demo => Task::perform(
                {
                    let id = id.clone();
                    async move { source.set_playlist_order(&id, &order).await.map_err(err) }
                },
                move |r| Message::PlaylistPage(PlaylistPageMsg::EditSaved(id.clone(), r)),
            ),
            _ if self.demo => Task::done(Message::PlaylistPage(PlaylistPageMsg::EditSaved(id, Ok(())))),
            _ => {
                Task::done(Message::PlaylistPage(PlaylistPageMsg::EditSaved(id, Err("not connected".into()))))
            }
        }
    }

    fn playlist_remove(&mut self, id: String, positions: Vec<usize>) -> Task<Message> {
        let Some(detail) = self.open_playlist_detail().filter(|d| d.playlist.id == id) else {
            return Task::none();
        };
        let server = detail.playlist.server_id.clone();
        let done = {
            let (id, positions) = (id.clone(), positions.clone());
            move |r| Message::PlaylistPage(PlaylistPageMsg::Removed(id.clone(), positions.clone(), r))
        };
        match self.playlist_source(&server) {
            Some(source) if !self.demo => Task::perform(
                async move { source.remove_from_playlist(&id, &positions).await.map_err(err) },
                done,
            ),
            _ if self.demo => Task::done(done(Ok(()))),
            _ => Task::done(done(Err("not connected".into()))),
        }
    }

    /// The server removed the entries at `positions`: drop them from the page.
    fn playlist_removed(&mut self, id: &str, positions: &[usize]) -> Task<Message> {
        let mut gone = positions.to_vec();
        gone.sort_unstable_by(|a, b| b.cmp(a));
        gone.dedup();
        let mut removed = 0usize;
        if let Some(d) = self.library.playlist.as_mut().filter(|d| d.playlist.id == id) {
            for p in gone {
                if p < d.songs.len() {
                    d.songs.remove(p);
                    removed += 1;
                }
            }
            d.playlist.song_count = Some(d.songs.len() as u32);
            d.playlist.duration = rows::total_duration(&d.songs);
        }
        if let Some(p) = self.library.playlists.iter_mut().find(|p| p.id == id) {
            p.song_count = p.song_count.map(|n| n.saturating_sub(removed as u32));
        }
        self.playlist_rows_sync();
        for list in [ListId::Playlist(id.to_string()), ListId::PlaylistAlbums(id.to_string())] {
            self.lists.entry(&list).selection.clear();
        }
        self.toast_success(if removed == 1 {
            "Removed 1 song from the playlist.".into()
        } else {
            format!("Removed {removed} songs from the playlist.")
        });
        // Counts and covers of the playlist lists change.
        Task::batch([self.load_playlists(), self.playlist_list_visible()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn songs(n: usize) -> Vec<Song> {
        (0..n)
            .map(|i| {
                let mut s = crate::demo::album_songs(0).remove(0);
                s.id = format!("s{i}");
                s
            })
            .collect()
    }

    #[test]
    fn moving_rows_keeps_the_order_of_positions() {
        let mut e = Edit::new(&songs(5));
        assert!(!e.dirty());
        e.move_row(4, 0);
        assert_eq!(e.order, [4, 0, 1, 2, 3]);
        assert_eq!(e.rows[0].id, "s4");
        e.move_row(0, 2);
        assert_eq!(e.order, [0, 1, 4, 2, 3]);
        assert!(e.dirty());
        // Out of range and no-op moves change nothing.
        e.move_row(9, 0);
        e.move_row(2, 2);
        assert_eq!(e.order, [0, 1, 4, 2, 3]);
        // Moving back restores the playlist's order.
        e.move_row(2, 4);
        assert_eq!(e.order, [0, 1, 2, 3, 4]);
        assert!(!e.dirty());
    }
}
