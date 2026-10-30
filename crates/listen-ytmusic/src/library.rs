//! Library calls ytmapi-rs lacks: subscribed artists with their page type
//! (only real artist channels are kept), the "Save to playlist" list, and
//! adding to / creating playlists with the result checked. Request shapes
//! follow ytmusicapi and YouTube.js.

use crate::home::{Continuation, Unparsed, page_type, shelves_in, text, thumbs};
use serde_json::{Map, Value, json};
use std::borrow::Cow;
use ytmapi_rs::auth::AuthToken;
use ytmapi_rs::common::Thumbnail;
use ytmapi_rs::parse::ParseFrom;
use ytmapi_rs::query::{PostMethod, PostQuery, Query};

/// Any InnerTube POST: `path` with `body` (plus the client context ytmapi-rs adds).
pub struct RawPost {
    pub path: &'static str,
    pub body: Map<String, Value>,
    pub continuation: Option<Continuation>,
}

impl RawPost {
    pub fn new(path: &'static str, body: Value) -> Self {
        let body = match body {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        RawPost { path, body, continuation: None }
    }
}

impl ParseFrom<RawPost> for Unparsed {
    fn parse_from(_: ytmapi_rs::ProcessedResult<RawPost>) -> ytmapi_rs::Result<Self> {
        Ok(Unparsed)
    }
}

impl<A: AuthToken> Query<A> for RawPost {
    type Output = Unparsed;
    type Method = PostMethod;
}

impl PostQuery for RawPost {
    fn header(&self) -> Map<String, Value> {
        match &self.continuation {
            Some(Continuation::Body(t)) => Map::from_iter([("continuation".to_string(), json!(t))]),
            Some(Continuation::Params(_)) => Map::new(),
            None => self.body.clone(),
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
        self.path
    }
}

/// An entry of the library's artist lists.
#[derive(Debug, Clone, PartialEq)]
pub struct LibArtist {
    pub id: String,
    pub name: String,
    pub thumbs: Vec<Thumbnail>,
    /// A music artist's channel (`MUSIC_PAGE_TYPE_ARTIST`), not a user channel.
    pub artist: bool,
}

/// One page of `FEmusic_library_corpus_artists` (subscriptions) and the next page's token.
pub fn artist_page(json: &Value) -> (Vec<LibArtist>, Option<Continuation>) {
    let mut shelves = Vec::new();
    shelves_in(json, &mut shelves);
    let mut out = Vec::new();
    let mut next = None;
    for shelf in shelves {
        let items = shelf.get("contents").or_else(|| shelf.get("continuationItems"));
        for item in items.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(r) = item.get("musicResponsiveListItemRenderer") {
                let Some(id) =
                    r.pointer("/navigationEndpoint/browseEndpoint/browseId").and_then(Value::as_str)
                else {
                    continue;
                };
                let name = text(r, "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text");
                if name.is_empty() {
                    continue;
                }
                out.push(LibArtist {
                    id: id.to_string(),
                    name,
                    thumbs: thumbs(r, "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"),
                    artist: page_type(r) == "MUSIC_PAGE_TYPE_ARTIST",
                });
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
    (out, next)
}

/// A playlist offered by YouTube Music's "Save to playlist" dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct AddOption {
    pub id: String,
    pub title: String,
    /// The song is in it already.
    pub contains: bool,
}

/// The user's own playlists in a `playlist/get_add_to_playlist` answer.
pub fn add_options(json: &Value) -> Vec<AddOption> {
    fn walk(v: &Value, out: &mut Vec<AddOption>) {
        match v {
            Value::Object(m) => {
                if let Some(r) = m.get("playlistAddToOptionRenderer") {
                    let id = r.get("playlistId").and_then(Value::as_str).unwrap_or("");
                    let title = match r.pointer("/title/simpleText").and_then(Value::as_str) {
                        Some(t) => t.to_string(),
                        None => text(r, "/title"),
                    };
                    // Liked Music, episodes for later and Watch later aren't edited this way.
                    if !id.is_empty() && !title.is_empty() && !matches!(id, "LM" | "SE" | "WL") {
                        out.push(AddOption {
                            id: id.to_string(),
                            title,
                            contains: r.get("containsSelectedVideos").and_then(Value::as_str) == Some("ALL"),
                        });
                    }
                    return;
                }
                m.values().for_each(|x| walk(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(json, &mut out);
    out
}

/// The other version of a watched song: the music video of a song, the
/// song of a music video (`counterpart` of the first queue entry in a `next` answer).
pub fn counterpart_id(json: &Value) -> Option<String> {
    fn find(v: &Value) -> Option<String> {
        match v {
            Value::Object(m) => {
                if let Some(id) = m
                    .get("counterpart")
                    .and_then(|c| c.pointer("/0/counterpartRenderer/playlistPanelVideoRenderer/videoId"))
                    .and_then(Value::as_str)
                {
                    return Some(id.to_string());
                }
                m.values().find_map(find)
            }
            Value::Array(a) => a.iter().find_map(find),
            _ => None,
        }
    }
    find(json)
}

/// `musicVideoType` of the first queue entry of a `next` answer
/// (`MUSIC_VIDEO_TYPE_ATV` = the song's art track, `_OMV` = an official music video).
pub fn video_type(json: &Value) -> Option<String> {
    fn find(v: &Value) -> Option<String> {
        match v {
            Value::Object(m) => {
                if let Some(t) = m.get("musicVideoType").and_then(Value::as_str) {
                    return Some(t.to_string());
                }
                m.values().find_map(find)
            }
            Value::Array(a) => a.iter().find_map(find),
            _ => None,
        }
    }
    let first = json.pointer("/contents").unwrap_or(json);
    find(first)
}

/// The id whose stream is a real video: the music-video counterpart of an art track, or
/// `id` itself. `None` when the song only has the art track.
pub fn music_video_id(json: &Value, id: &str) -> Option<String> {
    if let Some(c) = counterpart_id(json) {
        return Some(c);
    }
    match video_type(json).as_deref() {
        Some("MUSIC_VIDEO_TYPE_OMV" | "MUSIC_VIDEO_TYPE_UGC" | "MUSIC_VIDEO_TYPE_OFFICIAL_SOURCE_MUSIC") => {
            Some(id.to_string())
        }
        _ => None,
    }
}

/// One entry of a radio / watch queue, already turned into the song where the entry was a video.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioEntry {
    pub video_id: String,
    pub title: String,
    /// (name, channel id) of each artist.
    pub artists: Vec<(String, Option<String>)>,
    /// (name, browse id) of the album; videos have none.
    pub album: Option<(String, String)>,
    pub length: String,
    pub thumbnails: Vec<Thumbnail>,
    /// 0 = the song (art track), 1 = an official video, 2 = an upload (covers, fancams, clips).
    pub kind: u8,
}

/// Music videos, uploads and performance clips (anything but the art track `MUSIC_VIDEO_TYPE_ATV`).
const VIDEO_TYPES: [&str; 3] =
    ["MUSIC_VIDEO_TYPE_OMV", "MUSIC_VIDEO_TYPE_UGC", "MUSIC_VIDEO_TYPE_OFFICIAL_SOURCE_MUSIC"];

fn entry_type(r: &Value) -> Option<&str> {
    r.pointer("/navigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType")
        .and_then(Value::as_str)
}

fn entry_is_video(r: &Value) -> bool {
    entry_type(r).is_some_and(|t| VIDEO_TYPES.contains(&t))
}

fn radio_entry(r: &Value) -> Option<RadioEntry> {
    let video_id = r.get("videoId")?.as_str()?.to_string();
    let runs = r.pointer("/longBylineText/runs").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut artists = Vec::new();
    let mut album = None;
    for run in &runs {
        let name = run.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        match run.pointer("/navigationEndpoint/browseEndpoint/browseId").and_then(Value::as_str) {
            Some(id) if id.starts_with("MPRE") => album = Some((name, id.to_string())),
            Some(id) if id.starts_with("UC") => artists.push((name, Some(id.to_string()))),
            _ => {}
        }
    }
    if artists.is_empty()
        && let Some(first) = runs.first().and_then(|r| r.get("text")).and_then(Value::as_str)
    {
        artists.push((first.to_string(), None));
    }
    Some(RadioEntry {
        video_id,
        title: text(r, "/title"),
        artists,
        album,
        length: text(r, "/lengthText"),
        thumbnails: thumbs(r, "/thumbnail/thumbnails"),
        kind: match entry_type(r) {
            Some("MUSIC_VIDEO_TYPE_ATV") | None => 0,
            Some("MUSIC_VIDEO_TYPE_OMV" | "MUSIC_VIDEO_TYPE_OFFICIAL_SOURCE_MUSIC") => 1,
            Some(_) => 2,
        },
    })
}

/// The entries of a `next` answer in order. Where YouTube pairs a music video with its song
/// (`playlistPanelVideoWrapperRenderer` / `counterpart`), the song is taken, so a radio started
/// from a video or holding stage clips gives the official audio where there is one.
pub fn radio_entries(json: &Value) -> Vec<RadioEntry> {
    fn pick(primary: &Value, counterpart: Option<&Value>) -> Option<RadioEntry> {
        match counterpart {
            Some(c) if entry_is_video(primary) && !entry_is_video(c) => radio_entry(c),
            _ => radio_entry(primary),
        }
    }
    fn walk(v: &Value, out: &mut Vec<RadioEntry>) {
        match v {
            Value::Object(m) => {
                if let Some(w) = m.get("playlistPanelVideoWrapperRenderer") {
                    if let Some(p) = w.pointer("/primaryRenderer/playlistPanelVideoRenderer") {
                        let c = w.pointer("/counterpart/0/counterpartRenderer/playlistPanelVideoRenderer");
                        out.extend(pick(p, c));
                    }
                    return;
                }
                if let Some(p) = m.get("playlistPanelVideoRenderer") {
                    let c = p.pointer("/counterpart/0/counterpartRenderer/playlistPanelVideoRenderer");
                    out.extend(pick(p, c));
                    return;
                }
                m.values().for_each(|x| walk(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(json.pointer("/contents").unwrap_or(json), &mut out);
    out
}

/// `privacyStatus` / `playlistPrivacy` for a public flag.
pub fn privacy(public: bool) -> &'static str {
    if public { "PUBLIC" } else { "PRIVATE" }
}

/// YouTube Music refuses titles with angle brackets.
pub fn clean_title(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '<' | '>')).collect()
}

/// Body of `playlist/create`.
pub fn create_body(name: &str, description: &str, public: bool, videos: &[String]) -> Value {
    json!({
        "title": clean_title(name),
        "description": clean_title(description),
        "privacyStatus": privacy(public),
        "videoIds": videos,
    })
}

/// Actions of `browse/edit_playlist` for a change of title, description or
/// privacy (only the fields that are set).
pub fn metadata_actions(name: Option<&str>, description: Option<&str>, public: Option<bool>) -> Vec<Value> {
    let mut actions = Vec::new();
    if let Some(name) = name {
        actions.push(json!({"action": "ACTION_SET_PLAYLIST_NAME", "playlistName": clean_title(name)}));
    }
    if let Some(description) = description {
        actions.push(json!({
            "action": "ACTION_SET_PLAYLIST_DESCRIPTION",
            "playlistDescription": clean_title(description),
        }));
    }
    if let Some(public) = public {
        actions.push(json!({"action": "ACTION_SET_PLAYLIST_PRIVACY", "playlistPrivacy": privacy(public)}));
    }
    actions
}

/// Actions removing entries, each addressed by its video id and its id in the playlist.
pub fn remove_actions(entries: &[(&str, &str)]) -> Vec<Value> {
    entries
        .iter()
        .map(|(video, set)| json!({"action": "ACTION_REMOVE_VIDEO", "removedVideoId": video, "setVideoId": set}))
        .collect()
}

/// The action moving the entry `set_video_id` before the entry `before`
/// (`None`: to the end of the playlist).
pub fn move_action(set_video_id: &str, before: Option<&str>) -> Value {
    let mut a = json!({"action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": set_video_id});
    if let Some(before) = before {
        a["movedSetVideoId"] = json!(before);
    }
    a
}

/// Body of `browse/edit_playlist`.
pub fn edit_body(playlist_id: &str, actions: Vec<Value>) -> Value {
    json!({"playlistId": playlist_id.strip_prefix("VL").unwrap_or(playlist_id), "actions": actions})
}

/// Why `playlist/delete` failed, `None` on success. A successful delete has no `status`
/// field, so only an `error` object means failure.
pub fn delete_failure(json: &Value) -> Option<String> {
    let error = json.get("error")?;
    Some(
        error
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| "YouTube Music refused to delete the playlist".to_string(), str::to_string),
    )
}

/// Why `browse/edit_playlist` didn't add the songs, `None` on success.
pub fn edit_failure(json: &Value) -> Option<String> {
    let status = json.get("status").and_then(Value::as_str).unwrap_or("");
    if status.contains("SUCCEEDED") {
        return None;
    }
    // YouTube Music answers a duplicate with a "add anyway?" dialog.
    if json.to_string().contains("DEDUPE_OPTION_SKIP") {
        return Some("it is already in that playlist".into());
    }
    Some(if status.is_empty() {
        "YouTube Music did not confirm the change".into()
    } else {
        status.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_names_the_video_and_its_playlist_entry() {
        let body = edit_body("VLPL1", remove_actions(&[("vidA", "SV1"), ("vidB", "SV2")]));
        assert_eq!(
            body,
            json!({"playlistId": "PL1", "actions": [
                {"action": "ACTION_REMOVE_VIDEO", "removedVideoId": "vidA", "setVideoId": "SV1"},
                {"action": "ACTION_REMOVE_VIDEO", "removedVideoId": "vidB", "setVideoId": "SV2"},
            ]})
        );
    }

    #[test]
    fn a_move_goes_before_another_entry_or_to_the_end() {
        assert_eq!(
            move_action("SV3", Some("SV1")),
            json!({"action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": "SV3", "movedSetVideoId": "SV1"})
        );
        assert_eq!(
            move_action("SV3", None),
            json!({"action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": "SV3"})
        );
    }

    #[test]
    fn metadata_sends_only_what_is_set_and_strips_angle_brackets() {
        assert!(metadata_actions(None, None, None).is_empty());
        let actions = metadata_actions(Some("A <b> mix"), Some("for <the> road"), Some(true));
        assert_eq!(
            actions,
            [
                json!({"action": "ACTION_SET_PLAYLIST_NAME", "playlistName": "A b mix"}),
                json!({"action": "ACTION_SET_PLAYLIST_DESCRIPTION", "playlistDescription": "for the road"}),
                json!({"action": "ACTION_SET_PLAYLIST_PRIVACY", "playlistPrivacy": "PUBLIC"}),
            ]
        );
    }

    #[test]
    fn create_carries_description_and_privacy() {
        let body = create_body("Road", "songs", false, &["v1".to_string()]);
        assert_eq!(
            body,
            json!({"title": "Road", "description": "songs", "privacyStatus": "PRIVATE", "videoIds": ["v1"]})
        );
        assert_eq!(create_body("x", "", true, &[])["privacyStatus"], "PUBLIC");
    }

    #[test]
    fn a_delete_without_status_is_a_success() {
        // What YouTube Music answers to playlist/delete (seen on a real account).
        assert_eq!(delete_failure(&json!({"responseContext": {"visitorData": "x"}})), None);
        assert_eq!(
            delete_failure(&json!({"error": {"code": 404, "message": "Requested entity was not found."}})),
            Some("Requested entity was not found.".into())
        );
    }

    #[test]
    fn subscriptions_keep_page_type() {
        let item = |id: &str, name: &str, kind: &str| {
            json!({"musicResponsiveListItemRenderer": {
                "navigationEndpoint": {"browseEndpoint": {"browseId": id,
                    "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": kind}}}},
                "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": name}]}}},
                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "1.2M subscribers"}]}}}],
                "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "https://x/a", "width": 60, "height": 60}]}}}
            }})
        };
        let page = json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content":
        {"sectionListRenderer": {"contents": [{"itemSectionRenderer": {"contents": [{"musicShelfRenderer": {
            "contents": [item("UC1", "IU", "MUSIC_PAGE_TYPE_ARTIST"), item("UC2", "Some Vlogger", "MUSIC_PAGE_TYPE_USER_CHANNEL")],
            "continuations": [{"nextContinuationData": {"continuation": "TOKEN"}}]
        }}]}}]}}}}]}}});
        let (artists, next) = artist_page(&page);
        assert_eq!(artists.len(), 2);
        assert_eq!(artists[0].name, "IU");
        assert!(artists[0].artist);
        assert!(!artists[1].artist);
        assert_eq!(artists[0].thumbs.len(), 1);
        assert_eq!(next, Some(Continuation::Params("TOKEN".into())));
        // A continuation page.
        let more = json!({"continuationContents": {"musicShelfContinuation": {"contents": [item("UC3", "BTS", "MUSIC_PAGE_TYPE_ARTIST")]}}});
        let (artists, next) = artist_page(&more);
        assert_eq!(artists[0].id, "UC3");
        assert_eq!(next, None);
    }

    #[test]
    fn add_options_skip_liked_music() {
        let opt = |id: &str, title: Value, contains: &str| json!({"playlistAddToOptionRenderer": {"playlistId": id, "title": title, "containsSelectedVideos": contains, "privacy": "PRIVATE"}});
        let answer = json!({"contents": [{"addToPlaylistRenderer": {"playlists": [
            opt("PLa", json!({"simpleText": "Roadtrip"}), "NONE"),
            opt("PLb", json!({"runs": [{"text": "Chill"}]}), "ALL"),
            opt("LM", json!({"simpleText": "Liked Music"}), "NONE"),
        ]}}]});
        let options = add_options(&answer);
        assert_eq!(
            options,
            [
                AddOption { id: "PLa".into(), title: "Roadtrip".into(), contains: false },
                AddOption { id: "PLb".into(), title: "Chill".into(), contains: true },
            ]
        );
    }

    #[test]
    fn radio_takes_the_song_of_a_video() {
        let video = |id: &str, kind: &str| {
            json!({"videoId": id, "title": {"runs": [{"text": "Magnetic"}]}, "lengthText": {"runs": [{"text": "3:09"}]},
                "longBylineText": {"runs": [{"text": "HYBE LABELS"}]},
                "navigationEndpoint": {"watchEndpoint": {"watchEndpointMusicSupportedConfigs":
                    {"watchEndpointMusicConfig": {"musicVideoType": kind}}}}})
        };
        let mut song = video("SONG", "MUSIC_VIDEO_TYPE_ATV");
        song["lengthText"] = json!({"runs": [{"text": "2:40"}]});
        song["longBylineText"] = json!({"runs": [
            {"text": "ILLIT", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCx"}}}, {"text": " • "},
            {"text": "SUPER REAL ME", "navigationEndpoint": {"browseEndpoint": {"browseId": "MPREb_y"}}}]});
        let answer = json!({"contents": {"queue": [
            {"playlistPanelVideoWrapperRenderer": {"primaryRenderer": {"playlistPanelVideoRenderer": video("MV", "MUSIC_VIDEO_TYPE_OMV")},
                "counterpart": [{"counterpartRenderer": {"playlistPanelVideoRenderer": song.clone()}}]}},
            {"playlistPanelVideoRenderer": video("STAGE", "MUSIC_VIDEO_TYPE_UGC")},
            {"playlistPanelVideoWrapperRenderer": {"primaryRenderer": {"playlistPanelVideoRenderer": song},
                "counterpart": [{"counterpartRenderer": {"playlistPanelVideoRenderer": video("MV2", "MUSIC_VIDEO_TYPE_OMV")}}]}},
        ]}});
        let got = radio_entries(&answer);
        assert_eq!(got.iter().map(|e| e.video_id.as_str()).collect::<Vec<_>>(), ["SONG", "STAGE", "SONG"]);
        assert_eq!(got[0].album, Some(("SUPER REAL ME".into(), "MPREb_y".into())));
        assert_eq!(got[0].artists, vec![("ILLIT".to_string(), Some("UCx".to_string()))]);
        assert_eq!(got[0].length, "2:40");
        assert_eq!(got[1].album, None, "a video without a song stays as it is");
    }

    #[test]
    fn counterpart_of_a_song() {
        let answer = json!({"contents": {"x": [{"playlistPanelVideoRenderer": {"videoId": "SONG",
            "counterpart": [{"counterpartRenderer": {"playlistPanelVideoRenderer": {"videoId": "VIDEO"}}}]}}]}});
        assert_eq!(counterpart_id(&answer).as_deref(), Some("VIDEO"));
        assert_eq!(counterpart_id(&json!({"contents": []})), None);
    }

    #[test]
    fn art_track_without_counterpart_has_no_video() {
        let art = json!({"contents": [{"playlistPanelVideoRenderer": {"videoId": "A",
            "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"}}]});
        assert_eq!(music_video_id(&art, "A"), None);
        let omv = json!({"contents": [{"playlistPanelVideoRenderer": {"videoId": "V",
            "musicVideoType": "MUSIC_VIDEO_TYPE_OMV"}}]});
        assert_eq!(music_video_id(&omv, "V").as_deref(), Some("V"));
    }

    #[test]
    fn edit_status() {
        assert_eq!(edit_failure(&json!({"status": "STATUS_SUCCEEDED"})), None);
        let dup = json!({"status": "STATUS_FAILED", "actions": [{"confirmDialogEndpoint": {"content": {"confirmDialogRenderer": {
            "confirmButton": {"buttonRenderer": {"command": {"playlistEditEndpoint": {"actions": [{"dedupeOption": "DEDUPE_OPTION_SKIP"}]}}}}}}}}]});
        assert_eq!(edit_failure(&dup).as_deref(), Some("it is already in that playlist"));
        assert!(edit_failure(&json!({})).is_some());
    }

    fn fixture(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn edit_playlist_responses_from_fixtures() {
        let ok = fixture(include_str!("../tests/fixtures/edit_playlist_ok.json"));
        assert_eq!(edit_failure(&ok), None);
        let dup = fixture(include_str!("../tests/fixtures/edit_playlist_duplicate.json"));
        assert_eq!(edit_failure(&dup).as_deref(), Some("it is already in that playlist"));
    }

    #[test]
    fn add_to_playlist_options_from_a_fixture() {
        let options = add_options(&fixture(include_str!("../tests/fixtures/add_to_playlist_options.json")));
        assert_eq!(
            options.iter().map(|o| (o.id.as_str(), o.title.as_str(), o.contains)).collect::<Vec<_>>(),
            [("PLroad", "Roadtrip", false), ("PLchill", "Chill", true)]
        );
    }
}
