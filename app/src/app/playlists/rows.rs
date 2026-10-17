//! The rows of a playlist page: search, the client-side filters and sort
//! (Feishin runs them in the browser too: `client-side-song-filters.tsx`),
//! and the albums view (the songs grouped into album cards).

use crate::app::album_page::{TrackSort, filter_songs, sort_items};
use listen_core::model::{Album, ArtistRef, Song};
use std::time::Duration;

/// Which artist list of a song a filter looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Artists,
    AlbumArtists,
}

/// Feishin's client-side song filters for a playlist.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SongFilters {
    pub favorite: Option<bool>,
    pub rated: Option<bool>,
    pub genres: Vec<String>,
    /// Songs must have all the genres (else any of them).
    pub genres_all: bool,
    /// Artist keys (see [`artist_key`]).
    pub artists: Vec<String>,
    pub artists_all: bool,
    pub album_artists: Vec<String>,
    pub album_artists_all: bool,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
}

/// What identifies an artist in a filter: its id, else its name.
pub fn artist_key(a: &ArtistRef) -> String {
    a.id.clone().filter(|i| !i.is_empty()).unwrap_or_else(|| a.name.clone())
}

fn has_all_or_any(wanted: &[String], have: &[String], all: bool) -> bool {
    if wanted.is_empty() {
        return true;
    }
    if all { wanted.iter().all(|w| have.contains(w)) } else { wanted.iter().any(|w| have.contains(w)) }
}

impl SongFilters {
    pub fn active(&self) -> bool {
        let fresh = SongFilters::default();
        // The AND / OR switches alone filter nothing.
        SongFilters { genres_all: false, artists_all: false, album_artists_all: false, ..self.clone() }
            != fresh
    }

    pub fn matches(&self, song: &Song) -> bool {
        if self.favorite.is_some_and(|f| song.favorite != f) {
            return false;
        }
        if self.rated.is_some_and(|r| song.rating.is_some_and(|x| x > 0) != r) {
            return false;
        }
        if self.year_min.is_some_and(|min| song.year.is_none_or(|y| y < min))
            || self.year_max.is_some_and(|max| song.year.is_none_or(|y| y > max))
        {
            return false;
        }
        let genres: Vec<String> = song.genres.clone();
        let artists: Vec<String> = song.artists.iter().map(artist_key).collect();
        let album_artists: Vec<String> = song.album_artists.iter().map(artist_key).collect();
        has_all_or_any(&self.genres, &genres, self.genres_all)
            && has_all_or_any(&self.artists, &artists, self.artists_all)
            && has_all_or_any(&self.album_artists, &album_artists, self.album_artists_all)
    }
}

/// The rows the playlist page lists: the songs that match the search and the
/// filters, in the chosen order. The second vector gives each row's position
/// in the playlist (what removing and reordering address).
pub fn visible(
    songs: &[Song],
    search: &str,
    filters: &SongFilters,
    sort: TrackSort,
    desc: bool,
    seed: u64,
) -> (Vec<Song>, Vec<usize>) {
    let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
    let mut rows: Vec<(usize, Song)> = songs
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, s)| filters.matches(s))
        .filter(|(_, s)| words.is_empty() || !filter_songs(std::slice::from_ref(s), search).is_empty())
        .collect();
    rows = sort_items(rows, |(_, s)| s, sort, desc, seed);
    let (pos, songs): (Vec<usize>, Vec<Song>) = rows.into_iter().unzip();
    (songs, pos)
}

/// The values a filter can pick from (key, label, number of songs), most songs first.
pub fn options(songs: &[Song], what: FilterKind) -> Vec<(String, String, usize)> {
    let mut found: Vec<(String, String, usize)> = Vec::new();
    let mut add = |key: String, label: String| match found.iter_mut().find(|(k, ..)| *k == key) {
        Some(e) => e.2 += 1,
        None => found.push((key, label, 1)),
    };
    for s in songs {
        match what {
            FilterKind::Genres => s.genres.iter().for_each(|g| add(g.clone(), g.clone())),
            FilterKind::Artists => s.artists.iter().for_each(|a| add(artist_key(a), a.name.clone())),
            FilterKind::AlbumArtists => {
                s.album_artists.iter().for_each(|a| add(artist_key(a), a.name.clone()))
            }
        }
    }
    found.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    found
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Genres,
    Artists,
    AlbumArtists,
}

/// The playlist's songs as album cards, in the order the albums first appear.
/// Songs without an album are left out.
pub fn albums_of(songs: &[Song]) -> Vec<Album> {
    let mut out: Vec<Album> = Vec::new();
    for s in songs {
        let Some(id) = s.album_id.clone().filter(|i| !i.is_empty()) else { continue };
        if let Some(a) = out.iter_mut().find(|a| a.id == id && a.server_id == s.server_id) {
            a.song_count = Some(a.song_count.unwrap_or(0) + 1);
            a.duration += s.duration;
            continue;
        }
        out.push(Album {
            release_types: Vec::new(),
            id,
            server_id: s.server_id.clone(),
            source: s.source,
            name: s.album.clone().unwrap_or_default(),
            album_artists: if s.album_artists.is_empty() {
                s.artists.clone()
            } else {
                s.album_artists.clone()
            },
            image: s.image.clone(),
            year: s.year,
            release_type: None,
            genres: s.genres.clone(),
            song_count: Some(1),
            duration: s.duration,
            favorite: false,
            rating: None,
            play_count: None,
            songs: Vec::new(),
            info: Default::default(),
        });
    }
    out
}

/// Total playing time of `songs`.
pub fn total_duration(songs: &[Song]) -> Duration {
    songs.iter().map(|s| s.duration).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str, title: &str, artist: &str, genre: &str, year: i32, album: &str) -> Song {
        let mut s = crate::demo::album_songs(0).remove(0);
        s.id = id.into();
        s.title = title.into();
        s.artists = vec![ArtistRef { id: Some(format!("ar-{artist}")), name: artist.into() }];
        s.album_artists = s.artists.clone();
        s.genres = vec![genre.into()];
        s.year = Some(year);
        s.album = Some(album.into());
        s.album_id = Some(format!("al-{album}"));
        s.favorite = false;
        s.rating = None;
        s.comment = None;
        s
    }

    fn sample() -> Vec<Song> {
        let mut a = song("1", "Alpha", "Ann", "Rock", 1999, "First");
        a.favorite = true;
        let mut b = song("2", "Bravo", "Bob", "Pop", 2005, "First");
        b.rating = Some(4);
        let c = song("3", "Charlie", "Ann", "Pop", 2011, "Second");
        vec![a, b, c]
    }

    fn ids(rows: &[Song]) -> Vec<&str> {
        rows.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn rows_keep_their_playlist_position() {
        let songs = sample();
        let (rows, pos) = visible(&songs, "", &SongFilters::default(), TrackSort::Name, true, 0);
        assert_eq!(ids(&rows), ["3", "2", "1"]);
        assert_eq!(pos, [2, 1, 0]);
        let (rows, pos) = visible(&songs, "bob", &SongFilters::default(), TrackSort::Id, false, 0);
        assert_eq!((ids(&rows), pos), (vec!["2"], vec![1]));
    }

    #[test]
    fn filters_combine_and_the_and_or_switch_matters() {
        let songs = sample();
        let by = |f: SongFilters| ids(&visible(&songs, "", &f, TrackSort::Id, false, 0).0).join("");
        assert_eq!(by(SongFilters { favorite: Some(true), ..Default::default() }), "1");
        assert_eq!(by(SongFilters { rated: Some(true), ..Default::default() }), "2");
        assert_eq!(by(SongFilters { rated: Some(false), ..Default::default() }), "13");
        assert_eq!(by(SongFilters { year_min: Some(2000), year_max: Some(2008), ..Default::default() }), "2");
        let two_genres = vec!["Rock".to_string(), "Pop".to_string()];
        assert_eq!(by(SongFilters { genres: two_genres.clone(), ..Default::default() }), "123");
        // A song has one genre, so "all of them" matches none.
        assert_eq!(by(SongFilters { genres: two_genres, genres_all: true, ..Default::default() }), "");
        assert_eq!(by(SongFilters { artists: vec!["ar-Ann".into()], ..Default::default() }), "13");
    }

    #[test]
    fn active_ignores_the_and_or_switches() {
        assert!(!SongFilters::default().active());
        assert!(!SongFilters { genres_all: true, ..Default::default() }.active());
        assert!(SongFilters { favorite: Some(false), ..Default::default() }.active());
    }

    #[test]
    fn options_count_songs_most_first() {
        let o = options(&sample(), FilterKind::Genres);
        assert_eq!(o[0], ("Pop".into(), "Pop".into(), 2));
        assert_eq!(o[1].2, 1);
        assert_eq!(options(&sample(), FilterKind::Artists)[0].1, "Ann");
    }

    #[test]
    fn the_albums_view_groups_by_album_in_first_seen_order() {
        let albums = albums_of(&sample());
        assert_eq!(albums.len(), 2);
        assert_eq!((albums[0].name.as_str(), albums[0].song_count), ("First", Some(2)));
        assert_eq!(albums[1].song_count, Some(1));
        assert_eq!(albums[0].duration, sample()[0].duration + sample()[1].duration);
        let mut none = song("9", "x", "y", "z", 2000, "A");
        none.album_id = None;
        assert!(albums_of(&[none]).is_empty());
    }
}
