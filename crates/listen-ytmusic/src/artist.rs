//! The artist page (`browse` with an artist channel id). ytmapi-rs 0.3 only
//! reads pages with a `musicImmersiveHeaderRenderer`; small and auto-generated
//! artist channels use a `musicVisualHeaderRenderer` instead. This reads both,
//! tolerantly, from the raw response.

use crate::home::{Item, Named, list_item, page_type, runs, text, thumbs, two_row};
use serde_json::Value;
use ytmapi_rs::common::Thumbnail;

#[derive(Debug, Clone, PartialEq)]
pub struct TopSong {
    pub video_id: String,
    pub title: String,
    pub artists: Vec<Named>,
    pub album: Option<Named>,
    /// `None` when the page does not say.
    pub liked: Option<bool>,
    pub explicit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub id: String,
    pub title: String,
    pub year: String,
    pub thumbs: Vec<Thumbnail>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoEntry {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub length: String,
    pub thumbs: Vec<Thumbnail>,
}

/// What Listen reads from an artist page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ArtistData {
    pub name: String,
    pub thumbnails: Vec<Thumbnail>,
    pub description: Option<String>,
    pub subscribed: bool,
    pub songs: Vec<TopSong>,
    /// The "see all" playlist behind the Songs shelf.
    pub songs_playlist: Option<String>,
    pub albums: Vec<Release>,
    pub singles: Vec<Release>,
    pub videos: Vec<VideoEntry>,
    /// Related artists ("Fans might also like"): channel id and name.
    pub related: Vec<Named>,
}

impl From<&ytmapi_rs::parse::GetArtist> for ArtistData {
    fn from(a: &ytmapi_rs::parse::GetArtist) -> Self {
        use ytmapi_rs::common::{Explicit, LikeStatus, YoutubeID};
        let t = &a.top_releases;
        let releases = |g: &Option<ytmapi_rs::parse::GetArtistAlbums>| -> Vec<Release> {
            g.iter()
                .flat_map(|g| &g.results)
                .map(|r| Release {
                    id: r.album_id.get_raw().to_string(),
                    title: r.title.clone(),
                    year: r.year.clone(),
                    thumbs: r.thumbnails.clone(),
                })
                .collect()
        };
        ArtistData {
            name: a.name.clone(),
            thumbnails: a.thumbnails.clone(),
            description: a.description.clone(),
            subscribed: a.subscribed,
            songs: t
                .songs
                .iter()
                .flat_map(|s| &s.results)
                .map(|s| TopSong {
                    video_id: s.video_id.get_raw().to_string(),
                    title: s.title.clone(),
                    artists: s
                        .artists
                        .iter()
                        .map(|x| Named {
                            id: x.id.as_ref().map(|i| i.get_raw().to_string()),
                            name: x.name.clone(),
                        })
                        .collect(),
                    album: Some(Named {
                        id: Some(s.album.id.get_raw().to_string()),
                        name: s.album.name.clone(),
                    }),
                    liked: Some(matches!(s.like_status, LikeStatus::Liked)),
                    explicit: matches!(s.explicit, Explicit::IsExplicit),
                })
                .collect(),
            songs_playlist: t.songs.as_ref().map(|s| s.browse_id.get_raw().to_string()),
            albums: releases(&t.albums),
            singles: releases(&t.singles),
            videos: t
                .videos
                .iter()
                .flat_map(|v| &v.results)
                .filter_map(|v| match v {
                    ytmapi_rs::parse::SearchResultVideo::Video {
                        title,
                        channel_name,
                        video_id,
                        length,
                        thumbnails,
                        ..
                    } => Some(VideoEntry {
                        video_id: video_id.get_raw().to_string(),
                        title: title.clone(),
                        channel: channel_name.clone(),
                        length: length.clone(),
                        thumbs: thumbnails.clone(),
                    }),
                    _ => None,
                })
                .collect(),
            related: t
                .related
                .iter()
                .flat_map(|r| &r.results)
                .map(|s| Named { id: Some(s.browse_id.get_raw().to_string()), name: s.title.clone() })
                .collect(),
        }
    }
}

/// Reads an artist browse response: either header flavour, then whatever
/// shelves are there. Fails only without a name (nothing to show).
pub fn parse(json: &Value) -> Result<ArtistData, String> {
    let mut d = ArtistData::default();
    // The header: immersive, visual or any other single renderer.
    let header = json.pointer("/header").and_then(Value::as_object);
    let h = header
        .and_then(|h| {
            h.get("musicImmersiveHeaderRenderer")
                .or_else(|| h.get("musicVisualHeaderRenderer"))
                .or_else(|| h.values().next())
        })
        .ok_or("no /header in the artist response")?;
    d.name = text(h, "/title");
    if d.name.is_empty() {
        return Err("artist header without a name".into());
    }
    for ptr in [
        "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails",
        "/foregroundThumbnail/musicThumbnailRenderer/thumbnail/thumbnails",
    ] {
        d.thumbnails = thumbs(h, ptr);
        if !d.thumbnails.is_empty() {
            break;
        }
    }
    d.subscribed = h
        .pointer("/subscriptionButton/subscribeButtonRenderer/subscribed")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let sections = json
        .pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    for s in sections {
        if let Some(shelf) = s.get("musicShelfRenderer") {
            if !d.songs.is_empty() {
                continue;
            }
            d.songs_playlist = shelf
                .pointer("/title/runs/0/navigationEndpoint/browseEndpoint/browseId")
                .or_else(|| shelf.pointer("/bottomEndpoint/browseEndpoint/browseId"))
                .and_then(Value::as_str)
                .map(str::to_string);
            for row in runs_of(shelf, "/contents") {
                let Some(r) = row.get("musicResponsiveListItemRenderer") else { continue };
                if let Some(Item::Song { video_id, title, artists, album, .. }) = list_item(r) {
                    d.songs.push(TopSong {
                        video_id,
                        title,
                        artists,
                        album,
                        liked: like_status(r),
                        explicit: r.to_string().contains("MUSIC_EXPLICIT_BADGE"),
                    });
                }
            }
        } else if let Some(shelf) = s.get("musicCarouselShelfRenderer") {
            let title = text(shelf, "/header/musicCarouselShelfBasicHeaderRenderer/title").to_lowercase();
            let items: Vec<&Value> =
                runs_of(shelf, "/contents").iter().filter_map(|c| c.get("musicTwoRowItemRenderer")).collect();
            let mut releases = Vec::new();
            let mut videos = Vec::new();
            let mut related = Vec::new();
            for r in items {
                if r.pointer("/navigationEndpoint/watchEndpoint/videoId").is_some() {
                    if let Some(Item::Song { video_id, title, artists, thumbs, .. }) = two_row(r) {
                        let channel =
                            artists.first().map(|a| a.name.clone()).unwrap_or_else(|| text(r, "/subtitle"));
                        videos.push(VideoEntry { video_id, title, channel, length: String::new(), thumbs });
                    }
                } else if page_type(r) == "MUSIC_PAGE_TYPE_ARTIST" {
                    if let Some(id) =
                        r.pointer("/navigationEndpoint/browseEndpoint/browseId").and_then(Value::as_str)
                    {
                        related.push(Named { id: Some(id.to_string()), name: text(r, "/title") });
                    }
                } else if let Some(Item::Album { id, title, year, thumbs, .. }) = two_row(r) {
                    releases.push(Release { id, title, year, thumbs });
                }
            }
            d.videos.extend(videos);
            d.related.extend(related);
            if title.contains("single") {
                d.singles.extend(releases);
            } else {
                d.albums.extend(releases);
            }
        } else if let Some(desc) = s.get("musicDescriptionShelfRenderer") {
            let t = text(desc, "/description");
            if !t.trim().is_empty() {
                d.description = Some(t);
            }
        }
    }
    Ok(d)
}

fn runs_of<'a>(v: &'a Value, ptr: &str) -> &'a [Value] {
    runs(v, ptr)
}

/// `LIKE`, `INDIFFERENT`, … from the row's menu.
fn like_status(r: &Value) -> Option<bool> {
    fn find(v: &Value) -> Option<&str> {
        match v {
            Value::Object(m) => {
                m.get("likeStatus").and_then(Value::as_str).or_else(|| m.values().find_map(find))
            }
            Value::Array(a) => a.iter().find_map(find),
            _ => None,
        }
    }
    find(r.get("menu")?).map(|s| s == "LIKE")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn visual_header_page() {
        // artist_visual_header.json: a small channel (UNI) with a `musicVisualHeaderRenderer`,
        // a related shelf and a description.
        let a = parse(&fixture("artist_visual_header.json")).unwrap();
        assert_eq!(a.name, "UNI");
        assert!(!a.thumbnails.is_empty());
        assert!(!a.subscribed);
        assert_eq!(a.description.as_deref(), Some("Synthetic description."));
        assert_eq!(a.songs.len(), 5);
        assert_eq!(a.songs[0].title, "Bahamut");
        assert_eq!(a.songs[0].video_id, "C8qnzHCsXpI");
        assert_eq!(
            a.songs[0].artists.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["Rinkadink", "UNI", "Tsuyoshi Suzuki"]
        );
        assert!(a.songs_playlist.as_deref().unwrap().starts_with("VLOLAK5uy_"));
        assert_eq!(a.singles.len(), 3);
        assert!(a.albums.is_empty());
        assert!(a.singles.iter().all(|r| r.id.starts_with("MPRE") && !r.title.is_empty()));
        assert_eq!(a.videos.len(), 3);
        assert!(!a.videos[0].video_id.is_empty());
        assert_eq!(
            a.related,
            [Named { id: Some("UCRxFsyMT9Kq733uMvP5ZSng".into()), name: "Rinkadink".into() }]
        );
    }

    #[test]
    fn immersive_page_reads_the_same_shelves() {
        let imm = parse(&fixture("artist_immersive.json")).unwrap();
        let vis = parse(&fixture("artist_visual_header.json")).unwrap();
        assert_eq!(imm.name, vis.name);
        assert_eq!(imm.songs, vis.songs);
        assert_eq!(imm.singles, vis.singles);
        assert_eq!(imm.videos, vis.videos);
        assert!(imm.related.is_empty());
    }

    #[test]
    fn page_without_header_is_an_error() {
        assert!(parse(&serde_json::json!({"contents": {}})).is_err());
    }
}
