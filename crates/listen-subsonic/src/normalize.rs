//! Maps raw Subsonic types onto `listen_core::model` (cf. Feishin's
//! `src/shared/api/subsonic/subsonic-normalize.ts`).

use crate::types;
use listen_core::model::{
    Album, AlbumInfo, Artist, ArtistRef, GainInfo, Genre, ImageRef, MbzIds, Playlist, ServerId, Song,
    SongExtra, SourceKind,
};
use std::time::Duration;

fn refs(
    list: &[types::IdName],
    fallback_id: Option<&String>,
    fallback_name: Option<&String>,
) -> Vec<ArtistRef> {
    if !list.is_empty() {
        return list.iter().map(|a| ArtistRef { id: a.id.clone(), name: a.name.clone() }).collect();
    }
    fallback_name
        .filter(|n| !n.is_empty())
        .map(|name| vec![ArtistRef { id: fallback_id.cloned(), name: name.clone() }])
        .unwrap_or_default()
}

fn genres(list: &[types::Named], single: Option<&String>) -> Vec<String> {
    if list.is_empty() {
        single.filter(|g| !g.is_empty()).cloned().into_iter().collect()
    } else {
        list.iter().map(|g| g.name.clone()).collect()
    }
}

pub fn song(server: &ServerId, c: &types::Child) -> Song {
    let artists = refs(&c.artists, c.artist_id.as_ref(), c.artist.as_ref().or(c.display_artist.as_ref()));
    let album_artists = if c.album_artists.is_empty() {
        refs(&[], None, c.display_album_artist.as_ref())
    } else {
        refs(&c.album_artists, None, None)
    };
    let gain = c.replay_gain.clone().unwrap_or_default();
    Song {
        id: c.id.clone(),
        server_id: server.clone(),
        source: SourceKind::Subsonic,
        title: if c.title.is_empty() { c.name.clone().unwrap_or_default() } else { c.title.clone() },
        album_artists: if album_artists.is_empty() { artists.clone() } else { album_artists },
        artists,
        album: c.album.clone(),
        album_id: c.album_id.clone(),
        image: c.cover_art.clone().map(ImageRef::Id),
        duration: Duration::from_secs(c.duration.unwrap_or(0)),
        track_number: c.track,
        disc_number: c.disc_number,
        year: c.year.filter(|y| *y > 0),
        genres: genres(&c.genres, c.genre.as_ref()),
        codec: c.suffix.as_ref().map(|s| s.to_uppercase()),
        bit_rate: c.bit_rate,
        sample_rate: c.sampling_rate,
        bit_depth: c.bit_depth,
        bpm: c.bpm.filter(|b| *b > 0),
        favorite: c.starred.is_some(),
        rating: c.user_rating.filter(|r| *r > 0),
        play_count: c.play_count,
        gain: GainInfo {
            track_gain: gain.track_gain,
            track_peak: gain.track_peak,
            album_gain: gain.album_gain,
            album_peak: gain.album_peak,
        },
        explicit: c.explicit_status.as_deref() == Some("explicit"),
        path: c.path.clone().filter(|p| !p.is_empty()),
        size: c.size,
        suffix: c.suffix.as_ref().map(|s| s.to_lowercase()),
        channels: c.channel_count,
        created: c.created.clone(),
        comment: c.comment.clone().filter(|c| !c.is_empty()),
        mbz: MbzIds { recording: c.music_brainz_id.clone().filter(|i| !i.is_empty()), ..Default::default() },
        extra: SongExtra {
            composers: c
                .contributors
                .iter()
                .filter(|x| x.role.eq_ignore_ascii_case("composer") && !x.artist.name.is_empty())
                .map(|x| x.artist.name.clone())
                .collect(),
            date: c.original_release_date.as_ref().and_then(item_date),
            release_date: c.release_date.as_ref().and_then(item_date),
            last_played: c.played.clone().filter(|p| !p.is_empty()),
        },
    }
}

pub fn album(server: &ServerId, a: &types::AlbumId3) -> Album {
    let songs: Vec<Song> = a.song.iter().map(|c| song(server, c)).collect();
    Album {
        id: a.id.clone(),
        server_id: server.clone(),
        source: SourceKind::Subsonic,
        name: a.name.clone(),
        album_artists: refs(
            &a.artists,
            a.artist_id.as_ref(),
            a.artist.as_ref().or(a.display_artist.as_ref()),
        ),
        image: a.cover_art.clone().map(ImageRef::Id),
        year: a.year.filter(|y| *y > 0),
        release_type: a.release_types.first().cloned(),
        release_types: a.release_types.clone(),
        genres: genres(&a.genres, a.genre.as_ref()),
        song_count: a.song_count,
        duration: Duration::from_secs(a.duration.unwrap_or(0)),
        favorite: a.starred.is_some(),
        rating: a.user_rating.filter(|r| *r > 0),
        play_count: a.play_count,
        info: album_info(a, &songs),
        songs,
    }
}

fn item_date(d: &types::ItemDate) -> Option<String> {
    let year = d.year.filter(|y| *y > 0)?;
    Some(match (d.month.filter(|m| (1..=12).contains(m)), d.day.filter(|d| (1..=31).contains(d))) {
        (Some(m), Some(day)) => format!("{year:04}-{m:02}-{day:02}"),
        (Some(m), None) => format!("{year:04}-{m:02}"),
        _ => format!("{year:04}"),
    })
}

fn album_info(a: &types::AlbumId3, songs: &[Song]) -> AlbumInfo {
    let sizes: Vec<u64> = songs.iter().filter_map(|s| s.size).collect();
    AlbumInfo {
        version: a.version.clone().filter(|v| !v.trim().is_empty()),
        original_date: a.original_release_date.as_ref().and_then(item_date),
        release_date: a.release_date.as_ref().and_then(item_date),
        last_played: a.played.clone().filter(|p| !p.is_empty()),
        explicit: a.explicit_status.as_deref() == Some("explicit"),
        size: (!sizes.is_empty()).then(|| sizes.iter().sum()),
        // Navidrome's album comment is the first track's comment tag.
        comment: songs.iter().find_map(|s| s.comment.clone()),
        record_labels: a.record_labels.iter().map(|l| l.name.clone()).filter(|n| !n.is_empty()).collect(),
        moods: a.moods.clone(),
        groupings: Vec::new(),
        tags: Vec::new(),
        musicbrainz_id: a.music_brainz_id.clone().filter(|m| !m.is_empty()),
        disc_titles: a
            .disc_titles
            .iter()
            .filter(|d| !d.title.is_empty())
            .map(|d| (d.disc, d.title.clone()))
            .collect(),
    }
}

pub fn artist(server: &ServerId, a: &types::ArtistId3, info: Option<&types::ArtistInfo>) -> Artist {
    Artist {
        id: a.id.clone(),
        server_id: server.clone(),
        source: SourceKind::Subsonic,
        name: a.name.clone(),
        image: a
            .cover_art
            .clone()
            .map(ImageRef::Id)
            .or_else(|| a.artist_image_url.clone().filter(|u| !u.is_empty()).map(ImageRef::Url)),
        album_count: a.album_count,
        song_count: None,
        favorite: a.starred.is_some(),
        rating: a.user_rating.filter(|r| (1..=5).contains(r)),
        biography: info.and_then(|i| i.biography.clone()).filter(|b| !b.trim().is_empty()),
        similar: info
            .map(|i| {
                i.similar_artist
                    .iter()
                    .map(|s| ArtistRef { id: library_id(&s.id), name: s.name.clone() })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The id of a similar artist; `None` when not in the library (Navidrome sends `-1` or nothing).
fn library_id(id: &str) -> Option<String> {
    (!id.is_empty() && id != "-1").then(|| id.to_string())
}

/// `getArtistInfo2` data: biography, links and the similar artists that are in the library.
pub fn artist_info(server: &ServerId, info: &types::ArtistInfo) -> listen_source::ArtistInfo {
    let link = |s: &Option<String>| s.clone().filter(|u| !u.trim().is_empty());
    let musicbrainz_id = link(&info.music_brainz_id);
    listen_source::ArtistInfo {
        biography: info.biography.clone().filter(|b| !b.trim().is_empty()),
        links: listen_source::ExternalLinks {
            musicbrainz_url: musicbrainz_id.as_ref().map(|id| format!("https://musicbrainz.org/artist/{id}")),
            lastfm_url: link(&info.last_fm_url),
        },
        musicbrainz_id,
        similar: info
            .similar_artist
            .iter()
            .map(|s| ArtistRef { id: library_id(&s.id), name: s.name.clone() })
            .collect(),
        similar_artists: info
            .similar_artist
            .iter()
            .filter(|s| library_id(&s.id).is_some())
            .map(|s| artist(server, s, None))
            .collect(),
        genres: Vec::new(),
    }
}

pub fn playlist(server: &ServerId, p: &types::Playlist, username: &str) -> Playlist {
    let owned = p.owner.as_deref().is_none_or(|o| o.eq_ignore_ascii_case(username));
    let editable = !p.readonly.unwrap_or(false) && owned;
    tracing::debug!(name = %p.name, owner = ?p.owner, ro = ?p.readonly, username, editable, "playlist");
    Playlist {
        id: p.id.clone(),
        server_id: server.clone(),
        source: SourceKind::Subsonic,
        name: p.name.clone(),
        description: p.comment.clone().filter(|c| !c.is_empty()),
        owner: p.owner.clone(),
        image: p.cover_art.clone().map(ImageRef::Id),
        song_count: p.song_count,
        duration: Duration::from_secs(p.duration.unwrap_or(0)),
        public: p.public.unwrap_or(false),
        editable,
    }
}

pub fn genre(g: &types::Genre) -> Genre {
    Genre { id: g.value.clone(), name: g.value.clone(), album_count: g.album_count, song_count: g.song_count }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_keeps_the_musicbrainz_recording_id() {
        let c: types::Child =
            serde_json::from_str(r#"{"id":"1","title":"T","musicBrainzId":"rec-1"}"#).unwrap();
        let s = song(&ServerId("s".into()), &c);
        assert_eq!(s.mbz.recording.as_deref(), Some("rec-1"));
        let none: types::Child =
            serde_json::from_str(r#"{"id":"2","title":"T","musicBrainzId":""}"#).unwrap();
        assert_eq!(song(&ServerId("s".into()), &none).mbz, Default::default());
    }

    #[test]
    fn song_keeps_the_file_details_the_server_sends() {
        let c: types::Child = serde_json::from_str(
            r#"{"id":"1","title":"T","path":"A/B/01.flac","size":31337,"suffix":"FLAC","channelCount":2,
                "created":"2024-05-01T10:00:00Z","comment":"hi"}"#,
        )
        .unwrap();
        let s = song(&ServerId("s".into()), &c);
        assert_eq!(s.path.as_deref(), Some("A/B/01.flac"));
        assert_eq!((s.size, s.channels), (Some(31337), Some(2)));
        assert_eq!(s.suffix.as_deref(), Some("flac"));
        assert_eq!(s.created.as_deref(), Some("2024-05-01T10:00:00Z"));
        assert_eq!(s.comment.as_deref(), Some("hi"));
        let bare = song(&ServerId("s".into()), &types::Child::default());
        assert_eq!((bare.path, bare.size, bare.comment), (None, None, None));
    }

    #[test]
    fn composers_dates_and_the_last_play_come_from_opensubsonic() {
        let c: types::Child = serde_json::from_str(
            r#"{"id":"1","title":"T","played":"2026-10-14T21:30:00Z",
                "releaseDate":{"year":2021,"month":3,"day":12},
                "originalReleaseDate":{"year":2019},
                "contributors":[{"role":"composer","artist":{"id":"c1","name":"Lena Voss"}},
                                {"role":"Composer","artist":{"name":"Marc Ostrow"}},
                                {"role":"lyricist","artist":{"name":"Not Listed"}}]}"#,
        )
        .unwrap();
        let s = song(&ServerId("s".into()), &c);
        assert_eq!(s.extra.composers, ["Lena Voss", "Marc Ostrow"]);
        assert_eq!(s.extra.release_date.as_deref(), Some("2021-03-12"));
        assert_eq!(s.extra.date.as_deref(), Some("2019"));
        assert_eq!(s.extra.last_played.as_deref(), Some("2026-10-14T21:30:00Z"));
        let bare = song(&ServerId("s".into()), &types::Child::default());
        assert!(bare.extra.composers.is_empty() && bare.extra.last_played.is_none());
    }

    #[test]
    fn an_album_keeps_every_release_type_and_its_last_play() {
        let a: types::AlbumId3 = serde_json::from_str(
            r#"{"id":"a","name":"Live at Night","releaseTypes":["album","live"],"played":"2026-01-02T03:04:05Z"}"#,
        )
        .unwrap();
        let album = album(&ServerId("s".into()), &a);
        assert_eq!(album.release_types, ["album", "live"]);
        assert_eq!(album.release_type.as_deref(), Some("album"), "the first stays the single type");
        assert_eq!(album.info.last_played.as_deref(), Some("2026-01-02T03:04:05Z"));
    }
}
