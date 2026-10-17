//! Client-side filtering, sorting and paging for sources that can't (YouTube Music library, local
//! folders, plain Subsonic starred lists).

use crate::{AlbumQuery, AlbumSort, ArtistQuery, ArtistSort, SongQuery, SongSort, SortOrder};
use listen_core::model::{Album, Artist, ArtistRef, Page, Song};
use rand::seq::SliceRandom;

fn lower(s: &str) -> String {
    s.to_lowercase()
}

fn has_any(have: &[String], want: &[String], single: &Option<String>) -> bool {
    if want.is_empty() && single.is_none() {
        return true;
    }
    have.iter().any(|g| want.iter().chain(single.iter()).any(|w| g.eq_ignore_ascii_case(w)))
}

fn by_artist(have: &[ArtistRef], want: &[String], single: &Option<String>) -> bool {
    if want.is_empty() && single.is_none() {
        return true;
    }
    have.iter().any(|a| a.id.as_ref().is_some_and(|id| want.iter().chain(single.iter()).any(|w| w == id)))
}

fn in_years(year: Option<i32>, min: Option<i32>, max: Option<i32>) -> bool {
    if min.is_none() && max.is_none() {
        return true;
    }
    match year {
        Some(y) => min.is_none_or(|m| y >= m) && max.is_none_or(|m| y <= m),
        None => false,
    }
}

fn apply_order<T>(items: &mut [T], order: SortOrder, random: bool) {
    if random {
        items.shuffle(&mut rand::rng());
    } else if order == SortOrder::Descending {
        items.reverse();
    }
}

/// The albums that match the filters and the search, in the query's order.
pub fn albums(mut items: Vec<Album>, q: &AlbumQuery) -> Vec<Album> {
    let needle = q.search.as_deref().map(lower).filter(|s| !s.is_empty());
    let fav = q.favorite_filter();
    items.retain(|a| {
        needle.as_ref().is_none_or(|n| lower(&a.name).contains(n) || lower(&a.artist_line()).contains(n))
            && fav.is_none_or(|f| a.favorite == f)
            && q.has_rating.is_none_or(|r| a.rating.is_some() == r)
            && q.min_rating.is_none_or(|m| a.rating.unwrap_or(0) >= m)
            && has_any(&a.genres, &q.genres, &q.genre)
            && by_artist(&a.album_artists, &q.artist_ids, &q.artist_id)
            && in_years(a.year, q.year_min, q.year_max)
    });
    match q.sort {
        AlbumSort::Name => items.sort_by_key(|a| lower(&a.name)),
        AlbumSort::Artist | AlbumSort::AlbumArtist => {
            items.sort_by_key(|a| (lower(&a.artist_line()), lower(&a.name)))
        }
        AlbumSort::Year | AlbumSort::ReleaseDate => items.sort_by_key(|a| a.year.unwrap_or(0)),
        AlbumSort::Duration => items.sort_by_key(|a| a.duration),
        AlbumSort::Rating | AlbumSort::CommunityRating | AlbumSort::CriticRating => {
            items.sort_by_key(|a| a.rating.unwrap_or(0))
        }
        AlbumSort::SongCount => items.sort_by_key(|a| a.song_count.unwrap_or(0)),
        AlbumSort::MostPlayed => items.sort_by_key(|a| a.play_count.unwrap_or(0)),
        AlbumSort::Favorited => items.sort_by_key(|a| a.favorite),
        AlbumSort::Random | AlbumSort::Id | AlbumSort::RecentlyAdded | AlbumSort::RecentlyPlayed => {}
    }
    let random = q.sort == AlbumSort::Random;
    // "Id" keeps the server's order unflipped, as in Feishin's client sort.
    apply_order(&mut items, q.order, random);
    items
}

/// The songs that match the filters and the search, in the query's order.
pub fn songs(mut items: Vec<Song>, q: &SongQuery) -> Vec<Song> {
    let needle = q.search.as_deref().map(lower).filter(|s| !s.is_empty());
    let fav = q.favorite_filter();
    items.retain(|s| {
        needle.as_ref().is_none_or(|n| {
            lower(&s.title).contains(n)
                || lower(&s.artist_line()).contains(n)
                || s.album.as_deref().is_some_and(|a| lower(a).contains(n))
        }) && fav.is_none_or(|f| s.favorite == f)
            && q.has_rating.is_none_or(|r| s.rating.is_some() == r)
            && q.min_rating.is_none_or(|m| s.rating.unwrap_or(0) >= m)
            && has_any(&s.genres, &q.genres, &q.genre)
            && by_artist(&s.artists, &q.artist_ids, &None)
            && in_years(s.year, q.year_min, q.year_max)
    });
    match q.sort {
        SongSort::Name => items.sort_by_key(|s| lower(&s.title)),
        SongSort::Album => items.sort_by_key(|s| {
            (lower(s.album.as_deref().unwrap_or("")), s.disc_number.unwrap_or(0), s.track_number.unwrap_or(0))
        }),
        SongSort::Artist => items.sort_by_key(|s| lower(&s.artist_line())),
        SongSort::AlbumArtist => {
            items.sort_by_key(|s| lower(&listen_core::model::join_names(&s.album_artists)))
        }
        SongSort::Year | SongSort::ReleaseYear | SongSort::ReleaseDate => {
            items.sort_by_key(|s| s.year.unwrap_or(0))
        }
        SongSort::Bpm => items.sort_by_key(|s| s.bpm.unwrap_or(0)),
        SongSort::Channels => items.sort_by_key(|s| s.channels.unwrap_or(0)),
        SongSort::Comment => items.sort_by_key(|s| lower(s.comment.as_deref().unwrap_or(""))),
        SongSort::Duration => items.sort_by_key(|s| s.duration),
        SongSort::Favorited => items.sort_by_key(|s| s.favorite),
        SongSort::Genre => items.sort_by_key(|s| lower(s.genres.first().map_or("", |g| g.as_str()))),
        SongSort::Rating => items.sort_by_key(|s| s.rating.unwrap_or(0)),
        SongSort::MostPlayed => items.sort_by_key(|s| s.play_count.unwrap_or(0)),
        SongSort::RecentlyAdded => items.sort_by(|a, b| a.created.cmp(&b.created)),
        SongSort::Random | SongSort::Id | SongSort::RecentlyPlayed => {}
    }
    apply_order(&mut items, q.order, q.sort == SongSort::Random);
    items
}

/// The artists that match the filters and the search, in the query's order.
pub fn artists(mut items: Vec<Artist>, q: &ArtistQuery) -> Vec<Artist> {
    let needle = q.search.as_deref().map(lower).filter(|s| !s.is_empty());
    let fav = if q.favorites_only { Some(true) } else { q.favorite };
    items.retain(|a| {
        needle.as_ref().is_none_or(|n| lower(&a.name).contains(n)) && fav.is_none_or(|f| a.favorite == f)
    });
    match q.sort {
        ArtistSort::Name => items.sort_by_key(|a| lower(&a.name)),
        ArtistSort::AlbumCount => items.sort_by_key(|a| a.album_count.unwrap_or(0)),
        ArtistSort::SongCount => items.sort_by_key(|a| a.song_count.unwrap_or(0)),
        ArtistSort::Favorited => items.sort_by_key(|a| a.favorite),
        _ => {}
    }
    apply_order(&mut items, q.order, q.sort == ArtistSort::Random);
    items
}

/// `start .. start + limit` of `items` as a page (`limit` 0: 100).
pub fn page<T: Clone>(items: &[T], start: usize, limit: usize) -> Page<T> {
    let limit = if limit == 0 { 100 } else { limit };
    let from = start.min(items.len());
    let to = (from + limit).min(items.len());
    Page { items: items[from..to].to_vec(), start_index: start, total: Some(items.len()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::{ServerId, SourceKind};
    use std::time::Duration;

    fn album(id: &str, name: &str, artist: &str, year: i32, fav: bool) -> Album {
        Album {
            release_types: Vec::new(),
            info: Default::default(),
            id: id.into(),
            server_id: ServerId("s".into()),
            source: SourceKind::Local,
            name: name.into(),
            album_artists: vec![ArtistRef { id: Some(artist.into()), name: artist.into() }],
            image: None,
            year: Some(year),
            release_type: None,
            genres: vec!["Rock".into()],
            song_count: Some(10),
            duration: Duration::from_secs(60),
            favorite: fav,
            rating: None,
            play_count: None,
            songs: vec![],
        }
    }

    fn lib() -> Vec<Album> {
        vec![
            album("1", "Beta", "Zed", 2001, false),
            album("2", "alpha", "Amy", 1999, true),
            album("3", "Gamma", "Amy", 2010, false),
        ]
    }

    fn names(v: &[Album]) -> Vec<&str> {
        v.iter().map(|a| a.name.as_str()).collect()
    }

    #[test]
    fn sorts_and_reverses() {
        let q = AlbumQuery::default();
        assert_eq!(names(&albums(lib(), &q)), ["alpha", "Beta", "Gamma"]);
        let q = AlbumQuery { order: SortOrder::Descending, ..Default::default() };
        assert_eq!(names(&albums(lib(), &q)), ["Gamma", "Beta", "alpha"]);
        let q = AlbumQuery { sort: AlbumSort::Year, ..Default::default() };
        assert_eq!(names(&albums(lib(), &q)), ["alpha", "Beta", "Gamma"]);
    }

    #[test]
    fn filters_combine() {
        let q = AlbumQuery { artist_ids: vec!["Amy".into()], year_min: Some(2000), ..Default::default() };
        assert_eq!(names(&albums(lib(), &q)), ["Gamma"]);
        let q = AlbumQuery { favorite: Some(false), ..Default::default() };
        assert_eq!(names(&albums(lib(), &q)), ["Beta", "Gamma"]);
        let q = AlbumQuery { favorites_only: true, ..Default::default() };
        assert_eq!(names(&albums(lib(), &q)), ["alpha"]);
        let q = AlbumQuery { search: Some("ZED".into()), ..Default::default() };
        assert_eq!(names(&albums(lib(), &q)), ["Beta"]);
        let q = AlbumQuery { genres: vec!["jazz".into()], ..Default::default() };
        assert!(albums(lib(), &q).is_empty());
    }

    #[test]
    fn pages_past_the_end_are_empty() {
        let all = lib();
        let p = page(&all, 2, 5);
        assert_eq!((p.items.len(), p.total), (1, Some(3)));
        assert!(page(&all, 9, 5).items.is_empty());
    }
}
