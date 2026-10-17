//! What a context menu acts on: one or several items of one kind.
//!
//! A menu on a row of a selection targets the whole selection, so every variant holds a `Vec`.

use crate::app::list::Picked;
use listen_core::model::{Album, Artist, FolderEntry, Genre, Playlist, ServerId, Song};

/// The kind of item a [`MenuTarget`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Song,
    Album,
    Artist,
    Playlist,
    Genre,
    Folder,
}

impl TargetKind {
    /// "song" / "songs" for a count of `n`.
    pub fn noun(self, n: usize) -> &'static str {
        match (self, n == 1) {
            (TargetKind::Song, true) => "song",
            (TargetKind::Song, false) => "songs",
            (TargetKind::Album, true) => "album",
            (TargetKind::Album, false) => "albums",
            (TargetKind::Artist, true) => "artist",
            (TargetKind::Artist, false) => "artists",
            (TargetKind::Playlist, true) => "playlist",
            (TargetKind::Playlist, false) => "playlists",
            (TargetKind::Genre, true) => "genre",
            (TargetKind::Genre, false) => "genres",
            (TargetKind::Folder, true) => "folder",
            (TargetKind::Folder, false) => "folders",
        }
    }
}

/// What was right-clicked. Never empty when a menu is open.
#[derive(Debug, Clone)]
pub enum MenuTarget {
    /// Songs; `queue_uids` / `history_entries` are filled when they are rows of
    /// the queue / of Recently Played (parallel to `songs`, or empty).
    Songs {
        songs: Vec<Song>,
        queue_uids: Vec<u64>,
        history_entries: Vec<usize>,
    },
    Albums(Vec<Album>),
    Artists(Vec<Artist>),
    Playlists(Vec<Playlist>),
    /// Genres belong to the server being browsed.
    Genres(Vec<Genre>),
    /// Folders of the folder browser (the server being browsed).
    Folders(Vec<FolderEntry>),
}

impl MenuTarget {
    pub fn song(song: &Song) -> Self {
        Self::songs(std::slice::from_ref(song))
    }

    pub fn songs(songs: &[Song]) -> Self {
        MenuTarget::Songs { songs: songs.to_vec(), queue_uids: Vec::new(), history_entries: Vec::new() }
    }

    pub fn history_item(song: &Song, entry: usize) -> Self {
        MenuTarget::Songs { songs: vec![song.clone()], queue_uids: Vec::new(), history_entries: vec![entry] }
    }

    pub fn queue_item(song: &Song, uid: u64) -> Self {
        MenuTarget::Songs { songs: vec![song.clone()], queue_uids: vec![uid], history_entries: Vec::new() }
    }

    pub fn album(album: &Album) -> Self {
        MenuTarget::Albums(vec![album.clone()])
    }

    pub fn artist(artist: &Artist) -> Self {
        MenuTarget::Artists(vec![artist.clone()])
    }

    pub fn playlist(playlist: &Playlist) -> Self {
        MenuTarget::Playlists(vec![playlist.clone()])
    }

    pub fn genre(genre: &Genre) -> Self {
        MenuTarget::Genres(vec![genre.clone()])
    }

    pub fn folder(folder: &FolderEntry) -> Self {
        MenuTarget::Folders(vec![folder.clone()])
    }

    /// The rows of a list (all of one kind) as one target; `None` when empty.
    pub fn from_picks(picks: &[Picked]) -> Option<Self> {
        Some(match picks.first()? {
            Picked::Song(_) => MenuTarget::songs(
                &picks
                    .iter()
                    .filter_map(|p| if let Picked::Song(s) = p { Some(s.clone()) } else { None })
                    .collect::<Vec<_>>(),
            ),
            Picked::Album(_) => MenuTarget::Albums(
                picks
                    .iter()
                    .filter_map(|p| if let Picked::Album(a) = p { Some(a.clone()) } else { None })
                    .collect(),
            ),
            Picked::Artist(_) => MenuTarget::Artists(
                picks
                    .iter()
                    .filter_map(|p| if let Picked::Artist(a) = p { Some(a.clone()) } else { None })
                    .collect(),
            ),
            Picked::Playlist(_) => MenuTarget::Playlists(
                picks
                    .iter()
                    .filter_map(|p| if let Picked::Playlist(a) = p { Some(a.clone()) } else { None })
                    .collect(),
            ),
            Picked::Genre(_) => MenuTarget::Genres(
                picks
                    .iter()
                    .filter_map(|p| if let Picked::Genre(a) = p { Some(a.clone()) } else { None })
                    .collect(),
            ),
        })
    }

    pub fn kind(&self) -> TargetKind {
        match self {
            MenuTarget::Songs { .. } => TargetKind::Song,
            MenuTarget::Albums(_) => TargetKind::Album,
            MenuTarget::Artists(_) => TargetKind::Artist,
            MenuTarget::Playlists(_) => TargetKind::Playlist,
            MenuTarget::Genres(_) => TargetKind::Genre,
            MenuTarget::Folders(_) => TargetKind::Folder,
        }
    }

    /// How many items are targeted.
    pub fn len(&self) -> usize {
        match self {
            MenuTarget::Songs { songs, .. } => songs.len(),
            MenuTarget::Albums(v) => v.len(),
            MenuTarget::Artists(v) => v.len(),
            MenuTarget::Playlists(v) => v.len(),
            MenuTarget::Genres(v) => v.len(),
            MenuTarget::Folders(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// More than one item: entries that need exactly one (go to, rename) are left out.
    pub fn is_multi(&self) -> bool {
        self.len() > 1
    }

    /// The server of the items; `None` for genres and folders, which belong to
    /// the server being browsed.
    pub fn server(&self) -> Option<&ServerId> {
        match self {
            MenuTarget::Songs { songs, .. } => songs.first().map(|s| &s.server_id),
            MenuTarget::Albums(v) => v.first().map(|a| &a.server_id),
            MenuTarget::Artists(v) => v.first().map(|a| &a.server_id),
            MenuTarget::Playlists(v) => v.first().map(|p| &p.server_id),
            MenuTarget::Genres(_) | MenuTarget::Folders(_) => None,
        }
    }

    /// The songs when the target is songs (no fetching needed).
    pub fn song_list(&self) -> Option<&[Song]> {
        match self {
            MenuTarget::Songs { songs, .. } => Some(songs),
            _ => None,
        }
    }

    /// Display name of the first item (the preview footer, confirmations).
    pub fn first_name(&self) -> String {
        match self {
            MenuTarget::Songs { songs, .. } => songs.first().map(|s| s.title.clone()),
            MenuTarget::Albums(v) => v.first().map(|a| a.name.clone()),
            MenuTarget::Artists(v) => v.first().map(|a| a.name.clone()),
            MenuTarget::Playlists(v) => v.first().map(|p| p.name.clone()),
            MenuTarget::Genres(v) => v.first().map(|g| g.name.clone()),
            MenuTarget::Folders(v) => v.first().map(|f| f.name.clone()),
        }
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    #[test]
    fn kinds_counts_and_picks() {
        let songs = demo::album_songs(0);
        let t = MenuTarget::songs(&songs[..3]);
        assert_eq!((t.kind(), t.len(), t.is_multi()), (TargetKind::Song, 3, true));
        assert!(!MenuTarget::song(&songs[0]).is_multi());
        let picks: Vec<Picked> = songs[..2].iter().cloned().map(Picked::Song).collect();
        assert_eq!(MenuTarget::from_picks(&picks).unwrap().len(), 2);
        assert!(MenuTarget::from_picks(&[]).is_none());
        assert_eq!(TargetKind::Album.noun(2), "albums");
    }
}
