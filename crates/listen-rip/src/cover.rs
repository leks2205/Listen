//! Front cover for a rip: Cover Art Archive (release, then its release
//! group), then the iTunes store as a last resort. 1200 px JPEG.

use crate::musicbrainz::Release;
use listen_core::text::fold;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Cover {
    pub bytes: Vec<u8>,
    /// Where it came from, for the log ("Cover Art Archive (release group)").
    pub source: String,
}

impl Cover {
    pub fn is_jpeg(&self) -> bool {
        self.bytes.starts_with(&[0xFF, 0xD8, 0xFF])
    }
    pub fn is_png(&self) -> bool {
        self.bytes.starts_with(b"\x89PNG")
    }
    /// `cover.jpg` / `cover.png`.
    pub fn file_name(&self) -> &'static str {
        if self.is_png() { "cover.png" } else { "cover.jpg" }
    }
}

async fn image(http: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let resp = http.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?.to_vec();
    let ok = bytes.starts_with(&[0xFF, 0xD8, 0xFF]) || bytes.starts_with(b"\x89PNG");
    (ok && bytes.len() > 1000).then_some(bytes)
}

/// The 1200 px artwork URL of the iTunes album matching artist + title.
pub fn itunes_pick(body: &Value, artist: &str, album: &str) -> Option<String> {
    let (a, t) = (fold(artist), fold(album));
    body["results"].as_array()?.iter().find_map(|r| {
        let ra = fold(r["artistName"].as_str().unwrap_or(""));
        let rt = fold(r["collectionName"].as_str().unwrap_or(""));
        let fits = !a.is_empty()
            && (ra.contains(&a) || a.contains(&ra))
            && (rt.starts_with(&t) || t.starts_with(&rt));
        fits.then(|| r["artworkUrl100"].as_str().map(|u| u.replace("100x100bb", "1200x1200bb")))?
    })
}

/// The release's front cover: Cover Art Archive by release, then by release group, then iTunes.
pub async fn fetch(http: &reqwest::Client, release: &Release) -> Option<Cover> {
    let caa = [
        ("release", release.id.as_str(), "Cover Art Archive"),
        ("release-group", release.release_group_id.as_str(), "Cover Art Archive (release group)"),
    ];
    // A release parsed from an incomplete answer can lack either id: skip it, don't ask for "release//".
    for (kind, id, source) in caa.into_iter().filter(|(_, id, _)| !id.is_empty()) {
        let url = format!("https://coverartarchive.org/{kind}/{id}/front-1200");
        if let Some(bytes) = image(http, &url).await {
            return Some(Cover { bytes, source: source.into() });
        }
    }
    fetch_itunes(http, &release.artist(), &release.title).await
}

pub async fn fetch_itunes(http: &reqwest::Client, artist: &str, album: &str) -> Option<Cover> {
    let term = format!("{artist} {album}");
    let resp = http
        .get("https://itunes.apple.com/search")
        .query(&[("term", term.as_str()), ("entity", "album"), ("limit", "10")])
        .send()
        .await
        .ok()?;
    let body: Value = resp.json().await.ok()?;
    let url = itunes_pick(&body, artist, album)?;
    image(http, &url).await.map(|bytes| Cover { bytes, source: "iTunes".into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_matching_itunes_album() {
        let body = serde_json::json!({"results": [
            {"artistName": "Someone Else", "collectionName": "REVIVE+", "artworkUrl100": "https://x/a/100x100bb.jpg"},
            {"artistName": "IVE", "collectionName": "REVIVE+ - EP", "artworkUrl100": "https://x/b/100x100bb.jpg"}
        ]});
        assert_eq!(itunes_pick(&body, "IVE", "REVIVE+").as_deref(), Some("https://x/b/1200x1200bb.jpg"));
        assert_eq!(itunes_pick(&body, "IVE", "Other"), None);
    }

    /// Live (`--ignored`): REVIVE+ has no release cover, its release group has one.
    #[tokio::test]
    #[ignore = "live: musicbrainz.org and the Cover Art Archive"]
    async fn live_cover_chain() {
        let rs = crate::musicbrainz::parse_lookup(
            &serde_json::from_str(include_str!("../tests/fixtures/discid_revive.json")).unwrap(),
        );
        let cover = fetch(&crate::musicbrainz::http_client(), &rs[0]).await.expect("a cover");
        assert!(cover.is_jpeg());
        assert_eq!(cover.source, "Cover Art Archive (release group)");
    }
}
