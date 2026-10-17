//! Client-side sorting, searching and filtering of the YouTube Music library for the list pages
//! (via `listen_source::clientside`); the library calls return newest first with no server sort.

use listen_core::model::{Album, Song};
use listen_source::{AlbumQuery, AlbumSort, SongQuery, SongSort, SortOrder, clientside};

fn flip(o: SortOrder) -> SortOrder {
    match o {
        SortOrder::Ascending => SortOrder::Descending,
        SortOrder::Descending => SortOrder::Ascending,
    }
}

/// The library order is newest first, i.e. Descending for the "when" sorts; Ascending reverses it.
fn library_order(own: bool, order: SortOrder) -> SortOrder {
    if own { flip(order) } else { order }
}

/// `items` (library order) searched, filtered by year and sorted per `q`.
pub fn albums_in(items: Vec<Album>, q: &AlbumQuery) -> Vec<Album> {
    let own = matches!(q.sort, AlbumSort::RecentlyAdded | AlbumSort::RecentlyPlayed | AlbumSort::Id);
    let q = AlbumQuery {
        // Likes and genres are not part of YouTube Music's library albums.
        favorites_only: false,
        favorite: None,
        genre: None,
        genres: vec![],
        artist_id: None,
        artist_ids: vec![],
        sort: if own { AlbumSort::Id } else { q.sort },
        order: library_order(own, q.order),
        ..q.clone()
    };
    clientside::albums(items, &q)
}

/// `items` (library order, or the liked songs) searched and sorted per `q`.
pub fn songs_in(items: Vec<Song>, q: &SongQuery) -> Vec<Song> {
    let own = matches!(q.sort, SongSort::RecentlyAdded | SongSort::Id);
    let liked_only = q.favorite_filter() == Some(true);
    let q = SongQuery {
        // The liked list was chosen by the caller; its songs need no second check.
        favorites_only: false,
        favorite: if liked_only { None } else { q.favorite },
        sort: if own { SongSort::Id } else { q.sort },
        order: library_order(own, q.order),
        ..q.clone()
    };
    clientside::songs(items, &q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::{ArtistRef, ServerId, SourceKind};
    use std::time::Duration;

    fn album(id: &str, name: &str, artist: &str, year: i32) -> Album {
        Album {
            release_types: Vec::new(),
            info: Default::default(),
            id: id.into(),
            server_id: ServerId("yt".into()),
            source: SourceKind::YouTubeMusic,
            name: name.into(),
            album_artists: vec![ArtistRef { id: None, name: artist.into() }],
            image: None,
            year: Some(year),
            release_type: None,
            genres: vec![],
            song_count: None,
            duration: Duration::ZERO,
            favorite: false,
            rating: None,
            play_count: None,
            songs: vec![],
        }
    }

    fn song(id: &str, title: &str, album: &str, secs: u64) -> Song {
        Song {
            mbz: Default::default(),
            extra: Default::default(),
            id: id.into(),
            server_id: ServerId("yt".into()),
            source: SourceKind::YouTubeMusic,
            title: title.into(),
            artists: vec![ArtistRef { id: None, name: "Artist".into() }],
            album_artists: vec![],
            album: Some(album.into()),
            album_id: None,
            image: None,
            duration: Duration::from_secs(secs),
            track_number: None,
            disc_number: None,
            year: None,
            genres: vec![],
            codec: None,
            bit_rate: None,
            sample_rate: None,
            bit_depth: None,
            bpm: None,
            favorite: false,
            rating: None,
            play_count: None,
            gain: Default::default(),
            explicit: false,
            path: None,
            size: None,
            suffix: None,
            channels: None,
            created: None,
            comment: None,
        }
    }

    /// Library order: newest first.
    fn library() -> Vec<Album> {
        vec![
            album("3", "Zebra", "Mia", 2020),
            album("2", "apple", "Zoe", 1999),
            album("1", "Mango", "Al", 2010),
        ]
    }

    fn names(v: &[Album]) -> Vec<&str> {
        v.iter().map(|a| a.name.as_str()).collect()
    }

    #[test]
    fn name_sort_and_order() {
        assert_eq!(names(&albums_in(library(), &AlbumQuery::default())), ["apple", "Mango", "Zebra"]);
        let q = AlbumQuery { order: SortOrder::Descending, ..Default::default() };
        assert_eq!(names(&albums_in(library(), &q)), ["Zebra", "Mango", "apple"]);
    }

    #[test]
    fn recently_added_keeps_the_account_order() {
        // Descending (the option's default) is the library order; ascending reverses it.
        let q =
            AlbumQuery { sort: AlbumSort::RecentlyAdded, order: SortOrder::Descending, ..Default::default() };
        assert_eq!(names(&albums_in(library(), &q)), ["Zebra", "apple", "Mango"]);
        let q = AlbumQuery { order: SortOrder::Ascending, ..q };
        assert_eq!(names(&albums_in(library(), &q)), ["Mango", "apple", "Zebra"]);
    }

    #[test]
    fn search_and_year_filter_the_library() {
        let q = AlbumQuery { search: Some("zoe".into()), ..Default::default() };
        assert_eq!(names(&albums_in(library(), &q)), ["apple"]);
        let q = AlbumQuery { year_min: Some(2005), ..Default::default() };
        assert_eq!(names(&albums_in(library(), &q)), ["Mango", "Zebra"]);
        // A like filter on albums is ignored (the library has no likes).
        let q = AlbumQuery { favorites_only: true, ..Default::default() };
        assert_eq!(albums_in(library(), &q).len(), 3);
    }

    #[test]
    fn songs_sort_by_duration_and_search_by_album() {
        let songs =
            vec![song("a", "One", "X", 300), song("b", "Two", "Y", 100), song("c", "Three", "X", 200)];
        let q = SongQuery { sort: SongSort::Duration, order: SortOrder::Descending, ..Default::default() };
        let ids: Vec<_> = songs_in(songs.clone(), &q).iter().map(|s| s.id.clone()).collect();
        assert_eq!(ids, ["a", "c", "b"]);
        let q = SongQuery { search: Some("y".into()), ..Default::default() };
        assert_eq!(songs_in(songs.clone(), &q)[0].id, "b");
        // The liked list is already the favourites: no second check on the flag.
        let q = SongQuery { favorites_only: true, ..Default::default() };
        assert_eq!(songs_in(songs, &q).len(), 3);
    }
}
