//! The YouTube Music home feed (`FEmusic_home`: Quick picks, mixes, recommended
//! albums, …). ytmapi-rs 0.3 has no query for it, so this sends the browse
//! request itself and reads the shelves from the JSON.

use serde_json::{Map, Value, json};
use std::borrow::Cow;
use ytmapi_rs::auth::AuthToken;
use ytmapi_rs::common::Thumbnail;
use ytmapi_rs::parse::ParseFrom;
use ytmapi_rs::query::{PostMethod, PostQuery, Query};

/// Home page, or one of its continuations (more shelves further down).
pub struct HomeQuery {
    pub continuation: Option<String>,
}

/// The JSON is read by [`parse`]; ytmapi-rs only needs some output type.
#[derive(Debug)]
pub struct Unparsed;

impl ParseFrom<HomeQuery> for Unparsed {
    fn parse_from(_: ytmapi_rs::ProcessedResult<HomeQuery>) -> ytmapi_rs::Result<Self> {
        Ok(Unparsed)
    }
}

impl<A: AuthToken> Query<A> for HomeQuery {
    type Output = Unparsed;
    type Method = PostMethod;
}

impl PostQuery for HomeQuery {
    fn header(&self) -> Map<String, Value> {
        match self.continuation {
            None => Map::from_iter([("browseId".to_string(), json!("FEmusic_home"))]),
            Some(_) => Map::new(),
        }
    }
    fn params(&self) -> Vec<(&str, Cow<'_, str>)> {
        match &self.continuation {
            None => vec![],
            Some(c) => vec![
                ("ctoken", Cow::Borrowed(c.as_str())),
                ("continuation", Cow::Borrowed(c.as_str())),
                ("type", Cow::Borrowed("next")),
            ],
        }
    }
    fn path(&self) -> &str {
        "browse"
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Album {
        id: String,
        title: String,
        artists: Vec<Named>,
        year: String,
        thumbs: Vec<Thumbnail>,
    },
    Playlist {
        id: String,
        title: String,
        subtitle: String,
        thumbs: Vec<Thumbnail>,
    },
    Song {
        video_id: String,
        title: String,
        artists: Vec<Named>,
        album: Option<Named>,
        thumbs: Vec<Thumbnail>,
        /// "3:45", or empty.
        duration: String,
        /// The id of this entry in the playlist it was listed in (what
        /// removing and moving it address); `None` outside playlists.
        set_video_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shelf {
    pub title: String,
    pub items: Vec<Item>,
}

pub(crate) fn text(v: &Value, ptr: &str) -> String {
    v.pointer(ptr)
        .and_then(|r| r.get("runs"))
        .and_then(Value::as_array)
        .map(|runs| runs.iter().filter_map(|r| r.get("text").and_then(Value::as_str)).collect())
        .unwrap_or_default()
}

pub(crate) fn thumbs(v: &Value, ptr: &str) -> Vec<Thumbnail> {
    v.pointer(ptr).and_then(|t| serde_json::from_value(t.clone()).ok()).unwrap_or_default()
}

fn browse_id(run: &Value) -> Option<&str> {
    run.pointer("/navigationEndpoint/browseEndpoint/browseId").and_then(Value::as_str)
}

pub(crate) fn page_type(v: &Value) -> &str {
    v.pointer(
        "/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType",
    )
    .and_then(Value::as_str)
    .unwrap_or("")
}

/// Artists (runs linking to a channel) and album (run linking to MPRE…).
fn people(runs: &[Value]) -> (Vec<Named>, Option<Named>) {
    let mut artists = Vec::new();
    let mut album = None;
    for r in runs {
        let name = r.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        match browse_id(r) {
            Some(id) if id.starts_with("MPRE") => album = Some(Named { id: Some(id.to_string()), name }),
            Some(id) if id.starts_with("UC") => artists.push(Named { id: Some(id.to_string()), name }),
            _ => {}
        }
    }
    (artists, album)
}

pub(crate) fn runs<'a>(v: &'a Value, ptr: &str) -> &'a [Value] {
    v.pointer(ptr).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

pub(crate) fn two_row(r: &Value) -> Option<Item> {
    let title = text(r, "/title");
    let thumbs = thumbs(r, "/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails");
    let subtitle_runs = runs(r, "/subtitle/runs");
    if let Some(video_id) = r.pointer("/navigationEndpoint/watchEndpoint/videoId").and_then(Value::as_str) {
        let (artists, album) = people(subtitle_runs);
        return Some(Item::Song {
            video_id: video_id.to_string(),
            title,
            artists,
            album,
            thumbs,
            duration: String::new(),
            set_video_id: None,
        });
    }
    let id = r.pointer("/navigationEndpoint/browseEndpoint/browseId").and_then(Value::as_str)?.to_string();
    match page_type(r) {
        "MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK" => {
            let (mut artists, _) = people(subtitle_runs);
            if artists.is_empty() {
                // "Album • Various Artists • 2020": the artist is plain text.
                let texts: Vec<&str> = subtitle_runs.iter().filter_map(|x| x.get("text")?.as_str()).collect();
                if let Some(name) = texts.get(2).filter(|t| !t.chars().all(|c| c.is_ascii_digit())) {
                    artists.push(Named { id: None, name: name.to_string() });
                }
            }
            let year = subtitle_runs
                .iter()
                .filter_map(|x| x.get("text")?.as_str())
                .find(|t| t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()))
                .unwrap_or("")
                .to_string();
            Some(Item::Album { id, title, artists, year, thumbs })
        }
        "MUSIC_PAGE_TYPE_PLAYLIST" => {
            Some(Item::Playlist { id, title, subtitle: text(r, "/subtitle"), thumbs })
        }
        _ => None, // artists, podcasts, …
    }
}

/// A library row (list view) that links to an album or a playlist, read like a
/// tile: title from the first column, subtitle from the others. Song rows are
/// left to [`list_item`].
fn list_tile(r: &Value) -> Option<Item> {
    let col = |i: usize| format!("/flexColumns/{i}/musicResponsiveListItemFlexColumnRenderer/text/runs");
    let endpoint =
        r.get("navigationEndpoint").or_else(|| r.pointer(&format!("{}/0/navigationEndpoint", col(0))))?;
    endpoint.get("browseEndpoint")?;
    let mut subtitle: Vec<Value> = Vec::new();
    for i in 1..4 {
        let part = runs(r, &col(i));
        if part.is_empty() {
            continue;
        }
        if !subtitle.is_empty() {
            subtitle.push(json!({"text": " • "}));
        }
        subtitle.extend(part.iter().cloned());
    }
    let tile = json!({
        "title": {"runs": runs(r, &col(0))},
        "subtitle": {"runs": subtitle},
        "navigationEndpoint": endpoint,
        "thumbnailRenderer": {"musicThumbnailRenderer": r.pointer("/thumbnail/musicThumbnailRenderer").cloned().unwrap_or(Value::Null)},
    });
    two_row(&tile)
}

/// The key structure of a response (no values), for logging what a parser
/// didn't understand without logging anyone's library.
pub fn outline(v: &Value, max_lines: usize) -> String {
    fn walk(v: &Value, depth: usize, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    if matches!(
                        k.as_str(),
                        "responseContext" | "trackingParams" | "frameworkUpdates" | "menu"
                    ) {
                        continue;
                    }
                    if let Value::Array(a) = x {
                        out.push(format!("{}{k}[{}]", "  ".repeat(depth), a.len()));
                    } else if x.is_object() {
                        out.push(format!("{}{k}", "  ".repeat(depth)));
                    }
                    if depth < 14 && (x.is_object() || x.is_array()) {
                        walk(x, depth + 1, out);
                    }
                }
            }
            Value::Array(a) => a.iter().take(1).for_each(|x| walk(x, depth, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, 0, &mut out);
    out.truncate(max_lines);
    out.join("\n")
}

/// The playlist-entry id of a list item: `playlistItemData.playlistSetVideoId`,
/// else the `setVideoId` of its "Remove from playlist" menu action.
pub(crate) fn set_video_id(r: &Value) -> Option<String> {
    if let Some(id) = r.pointer("/playlistItemData/playlistSetVideoId").and_then(Value::as_str) {
        return Some(id.to_string());
    }
    fn find(v: &Value) -> Option<String> {
        match v {
            Value::Object(m) => {
                if let Some(id) = m
                    .get("playlistEditEndpoint")
                    .and_then(|e| e.pointer("/actions"))
                    .and_then(Value::as_array)
                    .and_then(|a| a.iter().find_map(|x| x.get("setVideoId").and_then(Value::as_str)))
                {
                    return Some(id.to_string());
                }
                m.values().find_map(find)
            }
            Value::Array(a) => a.iter().find_map(find),
            _ => None,
        }
    }
    r.get("menu").and_then(find)
}

pub(crate) fn list_item(r: &Value) -> Option<Item> {
    let col = |i: usize| format!("/flexColumns/{i}/musicResponsiveListItemFlexColumnRenderer/text");
    let video_id = r
        .pointer("/playlistItemData/videoId")
        .or_else(|| r.pointer(&format!("{}/runs/0/navigationEndpoint/watchEndpoint/videoId", col(0))))
        .and_then(Value::as_str)?
        .to_string();
    let (mut artists, mut album) = people(runs(r, &format!("{}/runs", col(1))));
    if album.is_none() {
        album = people(runs(r, &format!("{}/runs", col(2)))).1;
    }
    if artists.is_empty() {
        let first = text(r, &col(1));
        if let Some(name) = first.split(" • ").next().filter(|n| !n.is_empty()) {
            artists.push(Named { id: None, name: name.to_string() });
        }
    }
    Some(Item::Song {
        video_id,
        title: text(r, &col(0)),
        artists,
        album,
        thumbs: thumbs(r, "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"),
        duration: text(r, "/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text"),
        set_video_id: set_video_id(r),
    })
}
/// A browse request for song lists (library songs, playlists incl. Liked
/// Music), or one of its continuations.
pub struct BrowseQuery {
    pub browse_id: Option<String>,
    pub continuation: Option<Continuation>,
}

/// Continuation tokens come in two styles.
#[derive(Debug, Clone, PartialEq)]
pub enum Continuation {
    /// `continuationItemRenderer` → sent in the body.
    Body(String),
    /// `nextContinuationData` → sent as URL parameters.
    Params(String),
}

impl Continuation {
    pub fn token(&self) -> &str {
        match self {
            Continuation::Body(t) | Continuation::Params(t) => t,
        }
    }
}

impl ParseFrom<BrowseQuery> for Unparsed {
    fn parse_from(_: ytmapi_rs::ProcessedResult<BrowseQuery>) -> ytmapi_rs::Result<Self> {
        Ok(Unparsed)
    }
}

impl<A: AuthToken> Query<A> for BrowseQuery {
    type Output = Unparsed;
    type Method = PostMethod;
}

impl PostQuery for BrowseQuery {
    fn header(&self) -> Map<String, Value> {
        match (&self.continuation, &self.browse_id) {
            (Some(Continuation::Body(t)), _) => Map::from_iter([("continuation".to_string(), json!(t))]),
            (Some(Continuation::Params(_)), _) => Map::new(),
            (None, Some(id)) => Map::from_iter([("browseId".to_string(), json!(id))]),
            (None, None) => Map::new(),
        }
    }
    fn params(&self) -> Vec<(&str, Cow<'_, str>)> {
        match &self.continuation {
            Some(Continuation::Params(c)) => vec![
                ("ctoken", Cow::Borrowed(c.as_str())),
                ("continuation", Cow::Borrowed(c.as_str())),
                ("type", Cow::Borrowed("next")),
            ],
            _ => vec![],
        }
    }
    fn path(&self) -> &str {
        "browse"
    }
}

/// Containers of song lists, first pages and continuations.
const SONG_SHELVES: [&str; 5] = [
    "musicShelfRenderer",
    "musicPlaylistShelfRenderer",
    "musicShelfContinuation",
    "musicPlaylistShelfContinuation",
    "appendContinuationItemsAction",
];

pub(crate) fn shelves_in<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if SONG_SHELVES.contains(&k.as_str()) {
                    out.push(x);
                } else {
                    shelves_in(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| shelves_in(x, out)),
        _ => {}
    }
}

/// The songs of one page of a song list. Entries that can't be read are
/// skipped instead of failing the whole list.
pub fn song_page(json: &Value) -> (Vec<Item>, Option<Continuation>) {
    let mut shelves = Vec::new();
    shelves_in(json, &mut shelves);
    let mut songs = Vec::new();
    let mut next = None;
    for shelf in shelves {
        let items = shelf.get("contents").or_else(|| shelf.get("continuationItems"));
        for item in items.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(r) = item.get("musicResponsiveListItemRenderer") {
                songs.extend(list_item(r));
            } else if let Some(t) = item
                .pointer("/continuationItemRenderer/continuationEndpoint/continuationCommand/token")
                .and_then(Value::as_str)
            {
                next = Some(Continuation::Body(t.to_string()));
            }
        }
        if let Some(t) =
            shelf.pointer("/continuations/0/nextContinuationData/continuation").and_then(Value::as_str)
        {
            next = Some(Continuation::Params(t.to_string()));
        }
    }
    (songs, next)
}

/// Containers of library tiles or rows, first pages and continuations.
const LIBRARY_GRIDS: [&str; 5] = [
    "gridRenderer",
    "gridContinuation",
    "musicShelfRenderer",
    "musicShelfContinuation",
    "appendContinuationItemsAction",
];

fn grids_in<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if LIBRARY_GRIDS.contains(&k.as_str()) {
                    out.push(x);
                } else {
                    grids_in(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| grids_in(x, out)),
        _ => {}
    }
}

/// The items of one page of a library grid (Library > Albums, > Playlists). YouTube sends tiles
/// (`gridRenderer`) or rows (`musicShelfRenderer`), bare or inside an `itemSectionRenderer`, or
/// just a message when empty, so they are looked for anywhere.
pub fn grid_page(json: &Value) -> (Vec<Item>, Option<Continuation>) {
    let mut grids = Vec::new();
    grids_in(json, &mut grids);
    let mut out = Vec::new();
    let mut next = None;
    for grid in grids {
        let items =
            grid.get("items").or_else(|| grid.get("contents")).or_else(|| grid.get("continuationItems"));
        for item in items.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(r) = item.get("musicTwoRowItemRenderer") {
                out.extend(two_row(r));
            } else if let Some(r) = item.get("musicResponsiveListItemRenderer") {
                out.extend(list_tile(r));
            } else if let Some(t) = item
                .pointer("/continuationItemRenderer/continuationEndpoint/continuationCommand/token")
                .and_then(Value::as_str)
            {
                next = Some(Continuation::Body(t.to_string()));
            }
        }
        if let Some(t) =
            grid.pointer("/continuations/0/nextContinuationData/continuation").and_then(Value::as_str)
        {
            next = Some(Continuation::Params(t.to_string()));
        }
    }
    (out, next)
}

fn shelf(v: &Value) -> Option<Shelf> {
    let s = v.get("musicCarouselShelfRenderer")?;
    let title = text(s, "/header/musicCarouselShelfBasicHeaderRenderer/title");
    let items: Vec<Item> = runs(s, "/contents")
        .iter()
        .filter_map(|c| {
            if let Some(r) = c.get("musicTwoRowItemRenderer") {
                two_row(r)
            } else {
                list_item(c.get("musicResponsiveListItemRenderer")?)
            }
        })
        .collect();
    (!title.is_empty() && !items.is_empty()).then_some(Shelf { title, items })
}

/// Shelves of one home response, plus the token for more.
pub fn parse(json: &Value) -> (Vec<Shelf>, Option<String>) {
    let section = json
        .pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer")
        .or_else(|| json.pointer("/continuationContents/sectionListContinuation"));
    let Some(section) = section else { return (vec![], None) };
    let shelves = runs(section, "/contents").iter().filter_map(shelf).collect();
    let next = section
        .pointer("/continuations/0/nextContinuationData/continuation")
        .and_then(Value::as_str)
        .map(String::from);
    (shelves, next)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = include_str!("../tests/fixtures/home.json");

    #[test]
    fn parses_home_shelves() {
        let (shelves, next) = parse(&serde_json::from_str(HOME).unwrap());
        assert_eq!(next.as_deref(), Some("CONT"));
        assert_eq!(shelves.len(), 2);
        assert_eq!(shelves[0].title, "Quick picks");
        match &shelves[0].items[0] {
            Item::Song { video_id, title, artists, album, .. } => {
                assert_eq!(video_id, "vid1");
                assert_eq!(title, "봄날");
                assert_eq!(artists[0].name, "BTS");
                assert_eq!(album.as_ref().unwrap().id.as_deref(), Some("MPREb_x"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(shelves[1].title, "Mixed for you");
        assert!(matches!(&shelves[1].items[0], Item::Playlist { id, .. } if id == "VLRDTMAK1"));
        match &shelves[1].items[1] {
            Item::Album { id, artists, year, thumbs, .. } => {
                assert_eq!(id, "MPREb_y");
                assert_eq!(artists[0].name, "Artist");
                assert_eq!(year, "2021");
                assert_eq!(thumbs.len(), 1);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn playlist_entries_carry_their_set_video_id() {
        let json = serde_json::from_str(include_str!("../tests/fixtures/playlist_items.json")).unwrap();
        let (items, _) = song_page(&json);
        let ids: Vec<_> = items
            .iter()
            .map(|i| match i {
                Item::Song { video_id, set_video_id, .. } => (video_id.as_str(), set_video_id.as_deref()),
                other => panic!("{other:?}"),
            })
            .collect();
        // From playlistItemData, from the "remove from playlist" menu action, and absent.
        assert_eq!(ids, [("vidA", Some("SV-A")), ("vidB", Some("SV-B")), ("vidC", None)]);
    }

    #[test]
    fn parses_continuations() {
        let v = json!({"continuationContents": {"sectionListContinuation": {"contents": []}}});
        assert_eq!(parse(&v), (vec![], None));
    }

    const LIBRARY_SONGS: &str = include_str!("../tests/fixtures/library_songs.json");
    const LIBRARY_SONGS_MORE: &str = include_str!("../tests/fixtures/library_songs_continuation.json");

    #[test]
    fn parses_a_library_songs_page() {
        let (songs, next) = song_page(&serde_json::from_str(LIBRARY_SONGS).unwrap());
        assert_eq!(next, Some(Continuation::Params("NEXTPAGE".into())));
        // The entry without a video id is skipped.
        assert_eq!(songs.len(), 2);
        match &songs[0] {
            Item::Song { video_id, title, artists, album, duration, thumbs, .. } => {
                assert_eq!(video_id, "songA");
                assert_eq!(title, "Dynamite");
                assert_eq!(artists[0].name, "BTS");
                assert_eq!(artists[0].id.as_deref(), Some("UCbts"));
                assert_eq!(album.as_ref().unwrap().id.as_deref(), Some("MPREb_dyn"));
                assert_eq!(duration, "3:19");
                assert_eq!(thumbs.len(), 1);
            }
            other => panic!("{other:?}"),
        }
        // An upload with plain-text artist and album columns still gets its artist.
        match &songs[1] {
            Item::Song { video_id, artists, duration, .. } => {
                assert_eq!(video_id, "songB");
                assert_eq!(artists[0].name, "Me");
                assert_eq!(duration, "1:02:03");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_a_library_songs_continuation_with_a_body_token() {
        let (songs, next) = song_page(&serde_json::from_str(LIBRARY_SONGS_MORE).unwrap());
        assert_eq!(songs.len(), 1);
        assert!(matches!(&songs[0], Item::Song { video_id, .. } if video_id == "songC"));
        assert_eq!(next, Some(Continuation::Body("BODYTOKEN".into())));
    }

    fn album_item(id: &str) -> Value {
        json!({"musicTwoRowItemRenderer": {
            "title": {"runs": [{"text": format!("Title {id}")}]},
            "subtitle": {"runs": [
                {"text": "Album"}, {"text": " • "},
                {"text": "Rammstein", "navigationEndpoint": {"browseEndpoint": {
                    "browseId": "UCram",
                    "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": "MUSIC_PAGE_TYPE_ARTIST"}}}}},
                {"text": " • "}, {"text": "1997"}
            ]},
            "navigationEndpoint": {"browseEndpoint": {
                "browseId": id,
                "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": "MUSIC_PAGE_TYPE_ALBUM"}}}}
        }})
    }

    fn playlist_item(id: &str) -> Value {
        json!({"musicTwoRowItemRenderer": {
            "title": {"runs": [{"text": "Mix"}]},
            "subtitle": {"runs": [{"text": "Alex"}, {"text": " • "}, {"text": "1,234 tracks"}]},
            "navigationEndpoint": {"browseEndpoint": {
                "browseId": id,
                "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": "MUSIC_PAGE_TYPE_PLAYLIST"}}}}
        }})
    }

    #[test]
    fn parses_library_playlists_and_skips_the_new_playlist_tile() {
        let new_playlist = json!({"musicTwoRowItemRenderer": {
            "title": {"runs": [{"text": "New playlist"}]},
            "navigationEndpoint": {"createPlaylistEndpoint": {}}
        }});
        let page = json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content":
        {"sectionListRenderer": {"contents": [{"itemSectionRenderer": {"contents": [{"gridRenderer": {
            "items": [new_playlist, playlist_item("VLPLmix")]
        }}]}}]}}}}]}}});
        let (items, next) = grid_page(&page);
        assert_eq!(next, None);
        assert_eq!(items.len(), 1);
        match &items[0] {
            Item::Playlist { id, title, subtitle, .. } => {
                assert_eq!(id, "VLPLmix");
                assert_eq!(title, "Mix");
                assert_eq!(subtitle, "Alex • 1,234 tracks");
            }
            other => panic!("{other:?}"),
        }
    }

    fn album_ids(items: &[Item]) -> Vec<&str> {
        items
            .iter()
            .map(|i| match i {
                Item::Album { id, .. } => id.as_str(),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn parses_library_albums_in_any_wrapping() {
        let grid = json!({"gridRenderer": {
            "items": [album_item("MPREb_a"), album_item("MPREb_b")],
            "continuations": [{"nextContinuationData": {"continuation": "MORE"}}]
        }});
        let tab = |contents: Value| {
            json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content":
                {"sectionListRenderer": {"contents": contents}}}}]}}})
        };
        // Straight in the section list, and after a chip bar inside an itemSectionRenderer.
        let direct = tab(json!([grid.clone()]));
        let wrapped = tab(json!([{"itemSectionRenderer": {"contents": [{"chipCloudRenderer": {}}, grid]}}]));
        for page in [direct, wrapped] {
            let (albums, next) = grid_page(&page);
            assert_eq!(album_ids(&albums), ["MPREb_a", "MPREb_b"]);
            assert_eq!(next, Some(Continuation::Params("MORE".into())));
            match &albums[0] {
                Item::Album { artists, year, .. } => {
                    assert_eq!(artists[0].name, "Rammstein");
                    assert_eq!(year, "1997");
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn an_empty_album_library_is_no_albums() {
        let page = json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content":
        {"sectionListRenderer": {"contents": [{"itemSectionRenderer": {"contents": [
            {"messageRenderer": {"text": {"runs": [{"text": "Albums you save will show up here"}]}}}
        ]}}]}}}}]}}});
        assert_eq!(grid_page(&page), (vec![], None));
    }

    #[test]
    fn parses_library_album_continuations() {
        let legacy =
            json!({"continuationContents": {"gridContinuation": {"items": [album_item("MPREb_c")]}}});
        assert_eq!(grid_page(&legacy).1, None);
        assert_eq!(album_ids(&grid_page(&legacy).0), ["MPREb_c"]);
        let body = json!({"onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
            album_item("MPREb_d"),
            {"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"token": "BODY"}}}}
        ]}}]});
        let (albums, next) = grid_page(&body);
        assert_eq!(album_ids(&albums), ["MPREb_d"]);
        assert_eq!(next, Some(Continuation::Body("BODY".into())));
    }

    #[test]
    fn parses_library_playlists_shown_as_rows() {
        let col =
            |runs: Value| json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": runs}}});
        let row = json!({"musicResponsiveListItemRenderer": {
            "flexColumns": [
                col(json!([{"text": "Road Trip"}])),
                col(json!([{"text": "Playlist"}, {"text": " • "}, {"text": "Alex"}, {"text": " • "}, {"text": "42 tracks"}]))
            ],
            "navigationEndpoint": {"browseEndpoint": {
                "browseId": "VLPLroad",
                "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": "MUSIC_PAGE_TYPE_PLAYLIST"}}}}
        }});
        let song_row = json!({"musicResponsiveListItemRenderer": {
            "flexColumns": [col(json!([{"text": "A song", "navigationEndpoint": {"watchEndpoint": {"videoId": "v1"}}}]))]
        }});
        let page = json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content":
        {"sectionListRenderer": {"contents": [{"musicShelfRenderer": {
            "contents": [row, song_row],
            "continuations": [{"nextContinuationData": {"continuation": "ROWS"}}]
        }}]}}}}]}}});
        let (items, next) = grid_page(&page);
        assert_eq!(next, Some(Continuation::Params("ROWS".into())));
        assert_eq!(items.len(), 1);
        match &items[0] {
            Item::Playlist { id, title, subtitle, .. } => {
                assert_eq!(id, "VLPLroad");
                assert_eq!(title, "Road Trip");
                assert_eq!(subtitle, "Playlist • Alex • 42 tracks");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn outlines_keys_without_values() {
        let v = json!({"contents": {"a": [{"b": {"name": "secret"}}]}, "trackingParams": "x"});
        assert_eq!(outline(&v, 10), "contents\n  a[1]\n    b");
    }
}
