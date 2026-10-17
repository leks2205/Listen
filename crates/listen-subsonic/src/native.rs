//! Navidrome's native REST API (`/auth/login`, `/api/album`, `/api/song`, `/api/artist`)
//! for the list pages, which can sort and filter where Subsonic cannot (as Feishin's
//! `navidrome-controller.ts`). Maps queries to request parameters and JSON rows to
//! `listen_core::model` types.

use listen_core::model::{
    Album, Artist, ArtistRef, GainInfo, ImageRef, MbzIds, ServerId, Song, SongExtra, SourceKind,
};
use listen_source::nsp::Nsp;
use listen_source::{
    AlbumQuery, AlbumSort, ArtistQuery, ArtistSort, SmartPlaylistDef, SongQuery, SongSort, SortOrder,
};
use serde_json::Value;
use std::time::Duration;

pub type Params = Vec<(&'static str, String)>;

/// What the server version allows (`BFR`: Navidrome 0.55 and newer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Features {
    pub bfr: bool,
    /// Playlist cover upload (Navidrome 0.61).
    pub playlist_image: bool,
}

impl Features {
    /// From the ping's `serverVersion` ("0.55.1 (abc123)"). Unknown: no.
    pub fn from_version(version: Option<&str>) -> Self {
        let mut parts = version.unwrap_or("").split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty());
        let major: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        Self { bfr: (major, minor) >= (0, 55), playlist_image: (major, minor) >= (0, 61) }
    }
}

fn order(o: SortOrder) -> String {
    match o {
        SortOrder::Ascending => "ASC".into(),
        SortOrder::Descending => "DESC".into(),
    }
}

fn paging(p: &mut Params, start: usize, limit: usize) {
    p.push(("_start", start.to_string()));
    p.push(("_end", (start + if limit == 0 { 100 } else { limit }).to_string()));
}

/// Navidrome's `_sort` for an album sort (`albumListSortMap.navidrome`); `None`: the server's order.
pub fn album_sort(s: AlbumSort) -> Option<&'static str> {
    Some(match s {
        AlbumSort::AlbumArtist => "album_artist",
        AlbumSort::Artist => "artist",
        AlbumSort::Duration => "duration",
        AlbumSort::Favorited => "starred_at",
        AlbumSort::Name => "name",
        AlbumSort::MostPlayed => "play_count",
        AlbumSort::Random => "random",
        AlbumSort::Rating => "rating",
        AlbumSort::RecentlyAdded => "recently_added",
        AlbumSort::RecentlyPlayed => "play_date",
        AlbumSort::SongCount => "songCount",
        // Recent versions sort by release date; Feishin falls back to the year.
        AlbumSort::Year | AlbumSort::ReleaseDate => "max_year",
        AlbumSort::Id | AlbumSort::CommunityRating | AlbumSort::CriticRating => return None,
    })
}

pub fn song_sort(s: SongSort) -> Option<&'static str> {
    Some(match s {
        SongSort::Album => "album",
        SongSort::AlbumArtist => "order_album_artist_name",
        SongSort::Artist => "artist",
        SongSort::Bpm => "bpm",
        SongSort::Channels => "channels",
        SongSort::Comment => "comment",
        SongSort::Duration => "duration",
        SongSort::Favorited => "starred_at",
        SongSort::Genre => "genre",
        SongSort::Id => "id",
        SongSort::Name => "title",
        SongSort::MostPlayed => "playCount",
        SongSort::RecentlyPlayed => "playDate",
        SongSort::Random => "random",
        SongSort::Rating => "rating",
        SongSort::RecentlyAdded => "createdAt",
        SongSort::ReleaseYear | SongSort::ReleaseDate => "releaseYear",
        SongSort::Year => "year",
    })
}

pub fn artist_sort(s: ArtistSort) -> Option<&'static str> {
    Some(match s {
        ArtistSort::AlbumCount => "albumCount",
        ArtistSort::Favorited => "starred_at",
        ArtistSort::MostPlayed => "playCount",
        ArtistSort::Name => "name",
        ArtistSort::Rating => "rating",
        ArtistSort::SongCount => "songCount",
        ArtistSort::Random => "random",
        ArtistSort::RecentlyAdded => "createdAt",
        ArtistSort::Duration | ArtistSort::Album => return None,
    })
}

fn push_ids(p: &mut Params, key: &'static str, ids: &[String], features: Features) {
    // Older servers read a single value.
    let take = if features.bfr { ids.len() } else { ids.len().min(1) };
    for id in &ids[..take] {
        p.push((key, id.clone()));
    }
}

/// Tag names live as long as the program: each distinct one is kept once
/// (a server has a few dozen), so request parameters can borrow it.
fn intern(name: &str) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(known) = names.iter().find(|n| **n == name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.push(leaked);
    leaked
}

/// The custom tag filters as request parameters (`mood=<id>&mood=<id>`).
/// Names that are not plain tag names are not sent.
fn push_tags(p: &mut Params, tags: &[(String, Vec<String>)], features: Features) {
    if !features.bfr {
        return;
    }
    for (name, ids) in tags {
        let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if plain {
            push_ids(p, intern(name), ids, features);
        }
    }
}

fn year(min: Option<i32>, max: Option<i32>) -> Option<String> {
    max.or(min).filter(|y| *y > 0).map(|y| y.to_string())
}

/// `GET /api/album` parameters. `genre_ids` are Navidrome genre ids (see `Native::genre_ids`).
pub fn album_params(q: &AlbumQuery, genre_ids: &[String], features: Features) -> Params {
    let mut p = Params::new();
    paging(&mut p, q.start, q.limit);
    p.push(("_order", order(q.order)));
    if let Some(s) = album_sort(q.sort) {
        p.push(("_sort", s.into()));
    }
    let mut artists = q.artist_ids.clone();
    artists.extend(q.artist_id.clone());
    push_ids(&mut p, "artist_id", &artists, features);
    if let Some(c) = q.compilation {
        p.push(("compilation", c.to_string()));
    }
    push_ids(&mut p, "genre_id", genre_ids, features);
    if let Some(r) = q.has_rating {
        p.push(("has_rating", r.to_string()));
    }
    if let Some(t) = q.search.as_ref().filter(|t| !t.is_empty()) {
        p.push(("name", t.clone()));
    }
    if q.recently_played {
        p.push(("recently_played", "true".into()));
    }
    if let Some(f) = q.favorite_filter() {
        p.push(("starred", f.to_string()));
    }
    if let Some(y) = year(q.year_min, q.year_max) {
        p.push(("year", y));
    }
    push_tags(&mut p, &q.tags, features);
    if features.bfr {
        p.push(("missing", "false".into()));
    }
    p
}

pub fn song_params(q: &SongQuery, genre_ids: &[String], features: Features) -> Params {
    let mut p = Params::new();
    paging(&mut p, q.start, q.limit);
    p.push(("_order", order(q.order)));
    if let Some(s) = song_sort(q.sort) {
        p.push(("_sort", s.into()));
    }
    push_ids(&mut p, "genre_id", genre_ids, features);
    let key = if features.bfr { "artists_id" } else { "album_artist_id" };
    push_ids(&mut p, key, &q.artist_ids, features);
    if features.bfr
        && let Some(r) = q.has_rating
    {
        p.push(("has_rating", r.to_string()));
    }
    if let Some(f) = q.favorite_filter() {
        p.push(("starred", f.to_string()));
    }
    if let Some(t) = q.search.as_ref().filter(|t| !t.is_empty()) {
        p.push(("title", t.clone()));
    }
    if let Some(y) = year(q.year_min, q.year_max) {
        p.push(("year", y));
    }
    push_tags(&mut p, &q.tags, features);
    if features.bfr {
        p.push(("missing", "false".into()));
    }
    p
}

/// Tags that duplicate the genre filter.
const EXCLUDED_TAGS: &[&str] = &["genre"];
/// Tags that make no sense as an album filter (Feishin `EXCLUDED_ALBUM_TAGS`).
const EXCLUDED_ALBUM_TAGS: &[&str] = &[
    "asin",
    "barcode",
    "copyright",
    "encodedby",
    "isrc",
    "key",
    "language",
    "musicbrainz_workid",
    "script",
    "website",
    "work",
];
const EXCLUDED_SONG_TAGS: &[&str] = &["disctotal", "tracktotal"];

/// The rows of `GET /api/tag` grouped by tag, values sorted naturally, tags
/// by name, without the tags that are useless as a filter for `songs` / albums.
pub fn tag_filters(rows: &[Value], songs: bool) -> Vec<listen_source::TagFilter> {
    let excluded = if songs { EXCLUDED_SONG_TAGS } else { EXCLUDED_ALBUM_TAGS };
    let mut tags: Vec<listen_source::TagFilter> = Vec::new();
    for row in rows {
        let (Some(id), Some(name), Some(value)) = (s(row, "id"), s(row, "tagName"), s(row, "tagValue"))
        else {
            continue;
        };
        if EXCLUDED_TAGS.contains(&name.as_str()) || excluded.contains(&name.as_str()) {
            continue;
        }
        match tags.iter_mut().find(|t| t.name == name) {
            Some(t) => t.options.push((id, value)),
            None => tags.push(listen_source::TagFilter { name, options: vec![(id, value)] }),
        }
    }
    for t in &mut tags {
        t.options.sort_by(|a, b| natural(&a.1.to_lowercase(), &b.1.to_lowercase()));
    }
    tags.sort_by_key(|t| t.name.to_lowercase());
    tags
}

/// Compares "Disc 2" before "Disc 10" (digit runs by value).
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        n.push(c);
                        it.next();
                    }
                    n.trim_start_matches('0').to_string()
                };
                let (na, nb) = (run(&mut a), run(&mut b));
                let o = na.len().cmp(&nb.len()).then(na.cmp(&nb));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                a.next();
                b.next();
            }
        }
    }
}

pub fn artist_params(q: &ArtistQuery, features: Features) -> Params {
    let mut p = Params::new();
    paging(&mut p, q.start, q.limit);
    p.push(("_order", order(q.order)));
    if let Some(s) = artist_sort(q.sort) {
        p.push(("_sort", s.into()));
    }
    if let Some(t) = q.search.as_ref().filter(|t| !t.is_empty()) {
        p.push(("name", t.clone()));
    }
    let fav = if q.favorites_only { Some(true) } else { q.favorite };
    if let Some(f) = fav {
        p.push(("starred", f.to_string()));
    }
    if features.bfr {
        p.push(("missing", "false".into()));
    }
    p
}

// ---- rows -------------------------------------------------------------------

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).filter(|x| !x.is_empty()).map(str::to_string)
}

fn n(v: &Value, k: &str) -> Option<u32> {
    v.get(k).and_then(Value::as_f64).filter(|x| *x > 0.0).map(|x| x as u32)
}

fn b(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool).unwrap_or(false)
}

fn secs(v: &Value, k: &str) -> Duration {
    Duration::from_secs_f64(v.get(k).and_then(Value::as_f64).unwrap_or(0.0).max(0.0))
}

fn genres(v: &Value) -> Vec<String> {
    match v.get("genres").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => list.iter().filter_map(|g| s(g, "name")).collect(),
        _ => s(v, "genre").into_iter().collect(),
    }
}

fn artist_ref(id: Option<String>, name: Option<String>) -> Vec<ArtistRef> {
    name.map(|name| vec![ArtistRef { id, name }]).unwrap_or_default()
}

pub fn album(server: &ServerId, v: &Value) -> Option<Album> {
    let id = s(v, "id")?;
    let year = n(v, "maxYear").or_else(|| n(v, "minYear")).map(|y| y as i32);
    Some(Album {
        info: listen_core::model::AlbumInfo {
            release_date: s(v, "releaseDate").or_else(|| s(v, "date")),
            original_date: s(v, "originalDate"),
            last_played: play_date(v),
            ..Default::default()
        },
        image: Some(ImageRef::Id(format!("al-{id}"))),
        id,
        server_id: server.clone(),
        source: SourceKind::Subsonic,
        name: s(v, "name").unwrap_or_default(),
        album_artists: artist_ref(s(v, "albumArtistId"), s(v, "albumArtist")),
        year,
        release_type: s(v, "mbzAlbumType"),
        release_types: release_types(v),
        genres: genres(v),
        song_count: n(v, "songCount"),
        duration: secs(v, "duration"),
        favorite: b(v, "starred"),
        rating: n(v, "rating").map(|r| r as u8),
        play_count: n(v, "playCount"),
        songs: vec![],
    })
}

pub fn song(server: &ServerId, v: &Value) -> Option<Song> {
    let id = s(v, "id")?;
    let artists = artist_ref(s(v, "artistId"), s(v, "artist"));
    let album_artists = artist_ref(s(v, "albumArtistId"), s(v, "albumArtist"));
    let album_id = s(v, "albumId");
    let f = |k: &str| v.get(k).and_then(Value::as_f64).map(|x| x as f32);
    Some(Song {
        image: album_id.as_ref().map(|a| ImageRef::Id(format!("al-{a}"))),
        id,
        server_id: server.clone(),
        source: SourceKind::Subsonic,
        title: s(v, "title").unwrap_or_default(),
        album_artists: if album_artists.is_empty() { artists.clone() } else { album_artists },
        artists,
        album: s(v, "album"),
        album_id,
        duration: secs(v, "duration"),
        track_number: n(v, "trackNumber"),
        disc_number: n(v, "discNumber"),
        year: n(v, "year").map(|y| y as i32),
        genres: genres(v),
        codec: s(v, "suffix").map(|x| x.to_uppercase()),
        bit_rate: n(v, "bitRate"),
        sample_rate: n(v, "sampleRate"),
        bit_depth: n(v, "bitDepth"),
        bpm: n(v, "bpm"),
        favorite: b(v, "starred"),
        rating: n(v, "rating").map(|r| r as u8),
        play_count: n(v, "playCount"),
        gain: GainInfo {
            track_gain: f("rgTrackGain"),
            track_peak: f("rgTrackPeak"),
            album_gain: f("rgAlbumGain"),
            album_peak: f("rgAlbumPeak"),
        },
        explicit: s(v, "explicitStatus").as_deref() == Some("e"),
        path: s(v, "path"),
        size: v.get("size").and_then(Value::as_u64),
        suffix: s(v, "suffix").map(|x| x.to_lowercase()),
        channels: n(v, "channels"),
        created: s(v, "createdAt"),
        comment: s(v, "comment"),
        mbz: MbzIds {
            track: s(v, "mbzReleaseTrackId").filter(|i| !i.is_empty()),
            recording: s(v, "mbzRecordingId").filter(|i| !i.is_empty()),
            album: s(v, "mbzAlbumId").filter(|i| !i.is_empty()),
        },
        extra: SongExtra {
            composers: composers(v),
            date: s(v, "date").or_else(|| n(v, "year").map(|y| y.to_string())),
            release_date: s(v, "releaseDate").or_else(|| n(v, "releaseYear").map(|y| y.to_string())),
            last_played: play_date(v),
        },
    })
}

/// `playDate`, unless it is Go's zero time ("never played").
fn play_date(v: &Value) -> Option<String> {
    s(v, "playDate").filter(|d| !d.contains("0001-"))
}

/// The composers of a song: `participants.composer[].name`.
fn composers(v: &Value) -> Vec<String> {
    v.pointer("/participants/composer")
        .and_then(Value::as_array)
        .map(|l| l.iter().filter_map(|p| s(p, "name")).collect())
        .unwrap_or_default()
}

/// The release types of an album (`tags.releasetype`), else its `mbzAlbumType`.
fn release_types(v: &Value) -> Vec<String> {
    let tagged: Vec<String> = v
        .pointer("/tags/releasetype")
        .and_then(Value::as_array)
        .map(|l| l.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if tagged.is_empty() { s(v, "mbzAlbumType").into_iter().collect() } else { tagged }
}

pub fn artist(server: &ServerId, v: &Value) -> Option<Artist> {
    let id = s(v, "id")?;
    Some(Artist {
        rating: None,
        // The native API only has external image URLs; the Subsonic cover id is "ar-<id>".
        image: Some(ImageRef::Id(format!("ar-{id}"))),
        id,
        server_id: server.clone(),
        source: SourceKind::Subsonic,
        name: s(v, "name").unwrap_or_default(),
        album_count: n(v, "albumCount"),
        song_count: n(v, "songCount"),
        favorite: b(v, "starred"),
        biography: None,
        similar: vec![],
    })
}

// ---- smart playlists ----------------------------------------------------------

/// Body of `POST /api/playlist` and `PUT /api/playlist/{id}` for a smart playlist
/// (Feishin `createPlaylist` / `updatePlaylist` in `navidrome-controller.ts`).
pub fn playlist_body(def: &SmartPlaylistDef) -> Value {
    serde_json::json!({
        "name": def.name,
        "comment": def.comment,
        "public": def.public,
        "sync": def.sync,
        "rules": def.rules.to_value(),
    })
}

/// A playlist row of `/api/playlist`: the smart-playlist definition, `None` for a
/// plain playlist (`rules` is null) or rules this version cannot read.
pub fn smart_def(v: &Value) -> Option<SmartPlaylistDef> {
    let rules = match v.get("rules")? {
        Value::Null => return None,
        Value::String(text) if text.trim().is_empty() => return None,
        Value::String(text) => Nsp::from_json(text),
        other => Nsp::from_value(other),
    };
    match rules {
        Ok(rules) => Some(SmartPlaylistDef {
            name: s(v, "name").unwrap_or_default(),
            comment: s(v, "comment").unwrap_or_default(),
            public: b(v, "public"),
            sync: b(v, "sync"),
            rules,
        }),
        Err(e) => {
            tracing::warn!("unreadable smart playlist rules: {e}");
            None
        }
    }
}

/// Ids of the playlists in `rows` that have rules.
pub fn smart_ids(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .filter(|r| !matches!(r.get("rules"), None | Some(Value::Null)) && r["rules"] != "")
        .filter_map(|r| s(r, "id"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEW: Features = Features { bfr: true, playlist_image: false };
    const OLD: Features = Features { bfr: false, playlist_image: false };

    fn get<'a>(p: &'a Params, k: &str) -> Vec<&'a str> {
        p.iter().filter(|(n, _)| *n == k).map(|(_, v)| v.as_str()).collect()
    }

    #[test]
    fn native_rows_carry_composers_dates_last_play_and_release_types() {
        let row = serde_json::json!({
            "id": "s1", "title": "T", "year": 2001, "date": "2001-03-12", "releaseDate": "2002-04-01",
            "playDate": "2026-10-14T21:30:00Z",
            "participants": {"composer": [{"id": "1", "name": "Lena Voss"}], "artist": [{"name": "X"}]}
        });
        let s = song(&ServerId("s".into()), &row).unwrap();
        assert_eq!(s.extra.composers, ["Lena Voss"]);
        assert_eq!(s.extra.date.as_deref(), Some("2001-03-12"));
        assert_eq!(s.extra.release_date.as_deref(), Some("2002-04-01"));
        assert_eq!(s.extra.last_played.as_deref(), Some("2026-10-14T21:30:00Z"));
        // Go's zero time means never played; the year stands in for a missing date.
        let never = serde_json::json!({"id": "s2", "year": 1999, "playDate": "0001-01-01T00:00:00Z"});
        let s = song(&ServerId("s".into()), &never).unwrap();
        assert_eq!((s.extra.last_played, s.extra.date.as_deref()), (None, Some("1999")));
        // Albums: every release type of the tags, else the MusicBrainz type.
        let tagged = serde_json::json!({"id": "a1", "name": "A", "tags": {"releasetype": ["album", "live"]},
                                       "mbzAlbumType": "album", "releaseDate": "2020-05-05"});
        let a = album(&ServerId("s".into()), &tagged).unwrap();
        assert_eq!(a.release_types, ["album", "live"]);
        assert_eq!(a.info.release_date.as_deref(), Some("2020-05-05"));
        let plain = serde_json::json!({"id": "a2", "name": "B", "mbzAlbumType": "single"});
        assert_eq!(album(&ServerId("s".into()), &plain).unwrap().release_types, ["single"]);
    }

    #[test]
    fn tag_filters_become_repeated_parameters_and_need_a_new_server() {
        let q = AlbumQuery {
            tags: vec![
                ("mood".into(), vec!["t1".into(), "t2".into()]),
                ("recordlabel".into(), vec!["t3".into()]),
                // Not a plain tag name: never sent.
                ("a&b=c".into(), vec!["x".into()]),
            ],
            ..Default::default()
        };
        let p = album_params(&q, &[], NEW);
        assert_eq!(get(&p, "mood"), ["t1", "t2"]);
        assert_eq!(get(&p, "recordlabel"), ["t3"]);
        assert!(p.iter().all(|(k, _)| !k.contains('&')));
        // The same for songs.
        let sq = SongQuery { tags: q.tags.clone(), ..Default::default() };
        assert_eq!(get(&song_params(&sq, &[], NEW), "mood"), ["t1", "t2"]);
        // Servers before 0.55 have no tags.
        assert!(get(&album_params(&q, &[], OLD), "mood").is_empty());
    }

    #[test]
    fn the_tag_list_is_grouped_sorted_and_without_useless_tags() {
        let row = |id: &str, name: &str, value: &str| serde_json::json!({"id": id, "tagName": name, "tagValue": value});
        let rows = vec![
            row("1", "mood", "Happy"),
            row("2", "mood", "Disc 10"),
            row("3", "mood", "Disc 2"),
            row("4", "genre", "Rock"),
            row("5", "barcode", "123"),
            row("6", "albumtype", "x"),
        ];
        let tags = tag_filters(&rows, false);
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["albumtype", "mood"], "genre and album-useless tags are dropped");
        let mood: Vec<&str> = tags[1].options.iter().map(|o| o.1.as_str()).collect();
        assert_eq!(mood, ["Disc 2", "Disc 10", "Happy"], "natural order");
        // Song lists keep the barcode, not the track totals.
        assert!(tag_filters(&rows, true).iter().any(|t| t.name == "barcode"));
    }

    #[test]
    fn version_detection() {
        assert!(Features::from_version(Some("0.55.1 (abc)")).bfr);
        assert!(Features::from_version(Some("0.60.0")).bfr);
        assert!(!Features::from_version(Some("0.54.5")).bfr);
        assert!(!Features::from_version(None).bfr);
    }

    #[test]
    fn album_request_follows_feishin() {
        let q = AlbumQuery {
            sort: AlbumSort::RecentlyPlayed,
            order: SortOrder::Descending,
            start: 200,
            limit: 100,
            favorite: Some(true),
            compilation: Some(false),
            has_rating: Some(true),
            recently_played: true,
            artist_ids: vec!["a1".into(), "a2".into()],
            year_min: Some(1990),
            year_max: Some(1999),
            search: Some("abbey".into()),
            ..Default::default()
        };
        let p = album_params(&q, &["g1".into(), "g2".into()], NEW);
        assert_eq!(get(&p, "_sort"), ["play_date"]);
        assert_eq!(get(&p, "_order"), ["DESC"]);
        assert_eq!((get(&p, "_start"), get(&p, "_end")), (vec!["200"], vec!["300"]));
        assert_eq!(get(&p, "artist_id"), ["a1", "a2"]);
        assert_eq!(get(&p, "genre_id"), ["g1", "g2"]);
        assert_eq!(get(&p, "starred"), ["true"]);
        assert_eq!(get(&p, "compilation"), ["false"]);
        assert_eq!(get(&p, "has_rating"), ["true"]);
        assert_eq!(get(&p, "recently_played"), ["true"]);
        assert_eq!(get(&p, "name"), ["abbey"]);
        // Navidrome has one year field: the upper bound wins, like Feishin.
        assert_eq!(get(&p, "year"), ["1999"]);
        assert_eq!(get(&p, "missing"), ["false"]);
    }

    #[test]
    fn old_servers_get_single_ids_and_no_missing_flag() {
        let q = AlbumQuery { artist_ids: vec!["a1".into(), "a2".into()], ..Default::default() };
        let p = album_params(&q, &["g1".into(), "g2".into()], OLD);
        assert_eq!(get(&p, "artist_id"), ["a1"]);
        assert_eq!(get(&p, "genre_id"), ["g1"]);
        assert!(get(&p, "missing").is_empty());
        let sq = SongQuery { artist_ids: vec!["a".into()], ..Default::default() };
        assert_eq!(get(&song_params(&sq, &[], OLD), "album_artist_id"), ["a"]);
        assert_eq!(get(&song_params(&sq, &[], NEW), "artists_id"), ["a"]);
    }

    #[test]
    fn id_sort_sends_no_sort_and_names_map() {
        let q = AlbumQuery { sort: AlbumSort::Id, ..Default::default() };
        assert!(get(&album_params(&q, &[], NEW), "_sort").is_empty());
        assert_eq!(album_sort(AlbumSort::Year), Some("max_year"));
        assert_eq!(song_sort(SongSort::Name), Some("title"));
        assert_eq!(song_sort(SongSort::AlbumArtist), Some("order_album_artist_name"));
        assert_eq!(artist_sort(ArtistSort::Favorited), Some("starred_at"));
        let sq = SongQuery { search: Some("x".into()), favorite: Some(false), ..Default::default() };
        let p = song_params(&sq, &[], NEW);
        assert_eq!((get(&p, "title"), get(&p, "starred")), (vec!["x"], vec!["false"]));
    }

    #[test]
    fn maps_rows() {
        let server = ServerId("s".into());
        let a = album(
            &server,
            &serde_json::json!({"id":"al1","name":"Abbey Road","albumArtistId":"ar1","albumArtist":"Beatles",
                "maxYear":1969,"songCount":17,"duration":2832.5,"starred":true,"rating":4,"playCount":9,
                "genres":[{"id":"g","name":"Rock"}],"genre":"Rock"}),
        )
        .unwrap();
        assert_eq!(a.year, Some(1969));
        assert_eq!(a.artist_line(), "Beatles");
        assert_eq!(a.image, Some(ImageRef::Id("al-al1".into())));
        assert_eq!((a.favorite, a.rating, a.genres.clone()), (true, Some(4), vec!["Rock".to_string()]));
        let song = song(
            &server,
            &serde_json::json!({"id":"s1","title":"Come Together","album":"Abbey Road","albumId":"al1",
                "artist":"Beatles","artistId":"ar1","albumArtist":"Beatles","albumArtistId":"ar1","trackNumber":1,
                "discNumber":1,"year":1969,"duration":259.0,"bitRate":900,"suffix":"flac","bpm":0}),
        )
        .unwrap();
        assert_eq!(song.title, "Come Together");
        assert_eq!(song.image, Some(ImageRef::Id("al-al1".into())));
        assert_eq!((song.bpm, song.codec.as_deref()), (None, Some("FLAC")));
        let ar = artist(
            &server,
            &serde_json::json!({"id":"ar1","name":"Beatles","albumCount":13,"songCount":200}),
        )
        .unwrap();
        assert_eq!((ar.album_count, ar.song_count), (Some(13), Some(200)));
        assert!(album(&server, &serde_json::json!({})).is_none());
    }
}
