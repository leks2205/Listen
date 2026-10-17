//! Sort options and filters per server kind for the list pages, as in Feishin's
//! `list-sort-by-dropdown.tsx` and `features/albums/components/*-album-filters.tsx`.

use crate::{AlbumSort, ArtistSort, SongSort, SortOrder};
use bitflags::bitflags;

/// One entry of a sort dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortOption<S: 'static> {
    pub sort: S,
    pub label: &'static str,
    /// The order that is set when the option is picked.
    pub default_order: SortOrder,
}

const fn opt<S>(sort: S, label: &'static str, default_order: SortOrder) -> SortOption<S> {
    SortOption { sort, label, default_order }
}

use SortOrder::{Ascending as A, Descending as D};

bitflags! {
    /// Filters a server can apply to a list.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct QueryFilters: u16 {
        /// Favourited: any / yes / no.
        const FAVORITE        = 1 << 0;
        /// Rated: any / yes / no.
        const RATED           = 1 << 1;
        /// Compilation: any / yes / no.
        const COMPILATION     = 1 << 2;
        const RECENTLY_PLAYED = 1 << 3;
        const GENRES          = 1 << 4;
        const ARTISTS         = 1 << 5;
        /// From / to year.
        const YEAR_RANGE      = 1 << 6;
        /// One year only (Navidrome).
        const YEAR            = 1 << 7;
        /// More than one genre / artist can be picked.
        const MULTI           = 1 << 8;
        /// Custom tag filters ([`crate::MusicSource::tag_filters`]).
        const TAGS            = 1 << 9;
        /// Only one of favourite, artists, genres and year at a time (plain Subsonic album lists).
        const EXCLUSIVE       = 1 << 10;
    }
}

pub const NAVIDROME_ALBUMS: &[SortOption<AlbumSort>] = &[
    opt(AlbumSort::AlbumArtist, "Album artist", A),
    opt(AlbumSort::Id, "ID", A),
    opt(AlbumSort::Artist, "Artist", A),
    opt(AlbumSort::Duration, "Duration", D),
    opt(AlbumSort::MostPlayed, "Most played", D),
    opt(AlbumSort::Name, "Name", A),
    opt(AlbumSort::Random, "Random", A),
    opt(AlbumSort::Rating, "Rating", D),
    opt(AlbumSort::RecentlyAdded, "Recently added", D),
    opt(AlbumSort::RecentlyPlayed, "Recently played", D),
    opt(AlbumSort::SongCount, "Song count", D),
    opt(AlbumSort::Favorited, "Favorited", D),
    opt(AlbumSort::Year, "Release year", D),
];

pub const JELLYFIN_ALBUMS: &[SortOption<AlbumSort>] = &[
    opt(AlbumSort::AlbumArtist, "Album artist", A),
    opt(AlbumSort::Id, "ID", A),
    opt(AlbumSort::CommunityRating, "Community rating", D),
    opt(AlbumSort::CriticRating, "Critic rating", D),
    opt(AlbumSort::Name, "Name", A),
    opt(AlbumSort::MostPlayed, "Most played", D),
    opt(AlbumSort::Random, "Random", A),
    opt(AlbumSort::RecentlyAdded, "Recently added", D),
    opt(AlbumSort::ReleaseDate, "Release date", D),
];

pub const SUBSONIC_ALBUMS: &[SortOption<AlbumSort>] = &[
    opt(AlbumSort::AlbumArtist, "Album artist", A),
    opt(AlbumSort::Id, "ID", A),
    opt(AlbumSort::MostPlayed, "Most played", D),
    opt(AlbumSort::Name, "Name", A),
    opt(AlbumSort::Random, "Random", A),
    opt(AlbumSort::RecentlyAdded, "Recently added", D),
    opt(AlbumSort::RecentlyPlayed, "Recently played", D),
    opt(AlbumSort::Favorited, "Favorited", D),
    opt(AlbumSort::Year, "Release year", D),
];

/// A source that sorts its whole library itself (YouTube Music, local folders).
pub const LIBRARY_ALBUMS: &[SortOption<AlbumSort>] = &[
    opt(AlbumSort::Name, "Name", A),
    opt(AlbumSort::AlbumArtist, "Album artist", A),
    opt(AlbumSort::Year, "Release year", D),
    opt(AlbumSort::RecentlyAdded, "Recently added", D),
    opt(AlbumSort::RecentlyPlayed, "Recently played", D),
    opt(AlbumSort::Random, "Random", A),
];

pub const LOCAL_ALBUMS: &[SortOption<AlbumSort>] = &[
    opt(AlbumSort::Name, "Name", A),
    opt(AlbumSort::AlbumArtist, "Album artist", A),
    opt(AlbumSort::Year, "Release year", D),
    opt(AlbumSort::RecentlyAdded, "Recently added", D),
    opt(AlbumSort::Random, "Random", A),
];

pub const NAVIDROME_SONGS: &[SortOption<SongSort>] = &[
    opt(SongSort::Album, "Album", A),
    opt(SongSort::AlbumArtist, "Album artist", A),
    opt(SongSort::Artist, "Artist", A),
    opt(SongSort::Bpm, "BPM", D),
    opt(SongSort::Channels, "Channels", A),
    opt(SongSort::Comment, "Comment", A),
    opt(SongSort::Duration, "Duration", D),
    opt(SongSort::Favorited, "Is favorited", D),
    opt(SongSort::Genre, "Genre", A),
    opt(SongSort::Name, "Name", A),
    opt(SongSort::MostPlayed, "Play count", D),
    opt(SongSort::Random, "Random", A),
    opt(SongSort::Rating, "Rating", D),
    opt(SongSort::RecentlyAdded, "Recently added", D),
    opt(SongSort::RecentlyPlayed, "Recently played", D),
    opt(SongSort::ReleaseYear, "Release year", D),
    opt(SongSort::Year, "Year", D),
];

pub const JELLYFIN_SONGS: &[SortOption<SongSort>] = &[
    opt(SongSort::Album, "Album", A),
    opt(SongSort::AlbumArtist, "Album artist", A),
    opt(SongSort::Artist, "Artist", A),
    opt(SongSort::Duration, "Duration", A),
    opt(SongSort::MostPlayed, "Play count", A),
    opt(SongSort::Name, "Name", A),
    opt(SongSort::Random, "Random", A),
    opt(SongSort::RecentlyAdded, "Recently added", A),
    opt(SongSort::RecentlyPlayed, "Recently played", A),
    opt(SongSort::ReleaseDate, "Release date", A),
];

pub const SUBSONIC_SONGS: &[SortOption<SongSort>] = &[opt(SongSort::Name, "Name", A)];

pub const LIBRARY_SONGS: &[SortOption<SongSort>] = &[
    opt(SongSort::Name, "Name", A),
    opt(SongSort::Album, "Album", A),
    opt(SongSort::Artist, "Artist", A),
    opt(SongSort::Duration, "Duration", D),
    opt(SongSort::RecentlyAdded, "Recently added", D),
    opt(SongSort::Random, "Random", A),
];

pub const LOCAL_SONGS: &[SortOption<SongSort>] = &[
    opt(SongSort::Name, "Name", A),
    opt(SongSort::Album, "Album", A),
    opt(SongSort::Artist, "Artist", A),
    opt(SongSort::Year, "Year", D),
    opt(SongSort::RecentlyAdded, "Recently added", D),
    opt(SongSort::Random, "Random", A),
];

pub const NAVIDROME_ARTISTS: &[SortOption<ArtistSort>] = &[
    opt(ArtistSort::AlbumCount, "Album count", D),
    opt(ArtistSort::Favorited, "Is favorited", D),
    opt(ArtistSort::MostPlayed, "Most played", D),
    opt(ArtistSort::Name, "Name", A),
    opt(ArtistSort::Rating, "Rating", D),
    opt(ArtistSort::SongCount, "Song count", D),
];

pub const JELLYFIN_ARTISTS: &[SortOption<ArtistSort>] = &[
    opt(ArtistSort::Album, "Album", A),
    opt(ArtistSort::Duration, "Duration", D),
    opt(ArtistSort::Name, "Name", A),
    opt(ArtistSort::Random, "Random", A),
    opt(ArtistSort::RecentlyAdded, "Recently added", D),
];

pub const SUBSONIC_ARTISTS: &[SortOption<ArtistSort>] = &[
    opt(ArtistSort::AlbumCount, "Album count", D),
    opt(ArtistSort::Favorited, "Is favorited", D),
    opt(ArtistSort::Name, "Name", A),
    opt(ArtistSort::Rating, "Rating", D),
];

pub const LIBRARY_ARTISTS: &[SortOption<ArtistSort>] = &[opt(ArtistSort::Name, "Name", A)];

/// The option for `sort` in `options`, or the first (the list's default).
pub fn find<S: PartialEq + Copy>(options: &'static [SortOption<S>], sort: S) -> SortOption<S> {
    options.iter().find(|o| o.sort == sort).copied().unwrap_or(options[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navidrome_has_every_album_sort_feishin_lists() {
        assert_eq!(NAVIDROME_ALBUMS.len(), 13);
        assert_eq!(SUBSONIC_SONGS.len(), 1);
        assert_eq!(find(NAVIDROME_ALBUMS, AlbumSort::Rating).default_order, D);
        // An option the server does not have falls back to the first.
        assert_eq!(find(SUBSONIC_ALBUMS, AlbumSort::Rating).sort, AlbumSort::AlbumArtist);
    }
}
