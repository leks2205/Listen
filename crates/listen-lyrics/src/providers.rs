//! The lyrics providers Feishin has in its main process: NetEase, lrclib.net,
//! Genius and SimpMusic. Each has a search (artist / title -> candidates) and a
//! get (candidate id -> LRC or plain text). The parsers take the response body, so
//! they run on fixtures in tests; the fetchers wait for their provider's gate.

use crate::{gate, lrc, rank};
use serde_json::Value;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(8);
const LRCLIB: &str = "https://lrclib.net/api";
const NETEASE_SEARCH: &str = "https://music.163.com/api/search/get";
const NETEASE_LYRICS: &str = "https://music.163.com/api/song/lyric";
const GENIUS_SEARCH: &str = "https://genius.com/api/search/song";
const SIMPMUSIC: &str = "https://api-lyrics.simpmusic.org/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    NetEase = 0,
    Lrclib = 1,
    Genius = 2,
    SimpMusic = 3,
}

impl Source {
    pub const ALL: [Source; 4] = [Source::NetEase, Source::Lrclib, Source::Genius, Source::SimpMusic];

    /// The name Feishin's settings use.
    pub fn name(self) -> &'static str {
        match self {
            Source::NetEase => "NetEase",
            Source::Lrclib => "lrclib.net",
            Source::Genius => "Genius",
            Source::SimpMusic => "SimpMusic",
        }
    }

    pub fn from_name(name: &str) -> Option<Source> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The configured providers in order, unknown names dropped, duplicates ignored.
    pub fn parse_list(names: &[String]) -> Vec<Source> {
        let mut out = Vec::new();
        for s in names.iter().filter_map(|n| Source::from_name(n)) {
            if !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }
}

/// What to look for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: f64,
}

/// One candidate a provider found.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub source: Source,
    pub id: String,
    pub artist: String,
    pub name: String,
    /// `None` when the provider does not say.
    pub synced: Option<bool>,
    /// 0 (perfect) to 1, set by [`rank::order`].
    pub score: f32,
}

impl SearchHit {
    fn new(source: Source, id: String, artist: String, name: String, synced: Option<bool>) -> Self {
        Self { source, id, artist, name, synced, score: 1.0 }
    }

    /// "title — artist", what the lyrics view shows as the match.
    pub fn matched(&self) -> String {
        format!("{} — {}", self.name, self.artist)
    }
}

// ---- parsing -----------------------------------------------------------------

fn parse(body: &str) -> Option<Value> {
    serde_json::from_str(body).ok()
}

fn s(v: &Value) -> String {
    v.as_str().map(str::to_string).or_else(|| v.as_i64().map(|n| n.to_string())).unwrap_or_default()
}

pub mod netease {
    use super::*;

    pub fn parse_search(body: &str) -> Vec<SearchHit> {
        let Some(v) = parse(body) else { return vec![] };
        v["result"]["songs"]
            .as_array()
            .map(|songs| {
                songs
                    .iter()
                    .map(|song| {
                        let artist = song["artists"]
                            .as_array()
                            .map(|a| a.iter().map(|x| s(&x["name"])).collect::<Vec<_>>().join(", "))
                            .unwrap_or_default();
                        SearchHit::new(Source::NetEase, s(&song["id"]), artist, s(&song["name"]), None)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The original LRC, merged with the translation when `translation` is on.
    pub fn parse_lyrics(body: &str, translation: bool) -> Option<String> {
        let v = parse(body)?;
        let original = v["lrc"]["lyric"].as_str().filter(|t| !t.trim().is_empty())?;
        if !translation {
            return Some(original.to_string());
        }
        match v["tlyric"]["lyric"].as_str() {
            Some(t) => Some(lrc::merge_translation(original, t)),
            None => Some(original.to_string()),
        }
    }
}

pub mod lrclib {
    use super::*;

    pub fn parse_search(body: &str) -> Vec<SearchHit> {
        let Some(v) = parse(body) else { return vec![] };
        v.as_array()
            .map(|a| {
                a.iter()
                    .map(|song| {
                        SearchHit::new(
                            Source::Lrclib,
                            s(&song["id"]),
                            s(&song["artistName"]),
                            s(&song["name"]),
                            Some(song["syncedLyrics"].as_str().is_some_and(|t| !t.is_empty())),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Synced lyrics when there are, else the plain ones.
    pub fn parse_track(body: &str) -> Option<String> {
        let v = parse(body)?;
        ["syncedLyrics", "plainLyrics"]
            .iter()
            .filter_map(|k| v[*k].as_str())
            .find(|t| !t.trim().is_empty())
            .map(str::to_string)
    }
}

pub mod simpmusic {
    use super::*;

    pub fn parse_search(body: &str) -> Vec<SearchHit> {
        let Some(v) = parse(body) else { return vec![] };
        v["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|song| {
                        SearchHit::new(
                            Source::SimpMusic,
                            s(&song["videoId"]),
                            s(&song["artistName"]),
                            s(&song["songTitle"]),
                            Some(song["syncedLyrics"].as_str().is_some_and(|t| !t.is_empty())),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn parse_lyrics(body: &str) -> Option<String> {
        let v = parse(body)?;
        let first = if v["data"].is_array() { &v["data"][0] } else { &v["data"] };
        ["syncedLyrics", "plainLyric"]
            .iter()
            .filter_map(|k| first[*k].as_str())
            .find(|t| !t.trim().is_empty())
            .map(str::to_string)
    }
}

pub mod genius {
    use super::*;

    pub fn parse_search(body: &str) -> Vec<SearchHit> {
        let Some(v) = parse(body) else { return vec![] };
        v["response"]["sections"][0]["hits"]
            .as_array()
            .map(|hits| {
                hits.iter()
                    .map(|h| &h["result"])
                    .map(|r| {
                        SearchHit::new(
                            Source::Genius,
                            s(&r["url"]),
                            s(&r["artist_names"]),
                            s(&r["full_title"]),
                            None,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The lyrics out of a Genius song page: the `data-lyrics-container` blocks
    /// (minus the `data-exclude-from-selection` parts), or an old `div.lyrics`.
    pub fn parse_page(html: &str) -> Option<String> {
        let html = html.replace("<br/>", "\n").replace("<br>", "\n").replace("<br />", "\n");
        let mut blocks = Vec::new();
        let mut from = 0;
        while let Some(i) = html[from..].find("data-lyrics-container=\"true\"") {
            let at = from + i;
            let Some(open) = html[..at].rfind('<') else { break };
            let (inner, next) = element_inner(&html, open);
            blocks.push(text_of(inner));
            from = next.max(at + 1);
        }
        if blocks.is_empty()
            && let Some(i) = html.find("<div class=\"lyrics\"")
        {
            blocks.push(text_of(element_inner(&html, i).0));
        }
        let text = blocks.join("\n");
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    /// The inner HTML of the element opening at `open`, and where it ends.
    fn element_inner(html: &str, open: usize) -> (&str, usize) {
        let name_end =
            html[open + 1..].find(|c: char| !c.is_ascii_alphanumeric()).map_or(html.len(), |n| open + 1 + n);
        let name = &html[open + 1..name_end];
        let Some(gt) = html[open..].find('>') else { return ("", html.len()) };
        let start = open + gt + 1;
        let (open_pat, close_pat) = (format!("<{name}"), format!("</{name}>"));
        let mut depth = 1;
        let mut pos = start;
        while pos < html.len() {
            let next_open = html[pos..].find(&open_pat).map(|n| pos + n);
            let next_close = html[pos..].find(&close_pat).map(|n| pos + n);
            match (next_open, next_close) {
                (Some(o), Some(c)) if o < c => {
                    // `<divider` etc. is another element name.
                    let after = html[o + open_pat.len()..].chars().next();
                    if after.is_some_and(|ch| ch.is_ascii_alphanumeric()) {
                        pos = o + open_pat.len();
                    } else {
                        depth += 1;
                        pos = o + open_pat.len();
                    }
                }
                (_, Some(c)) => {
                    depth -= 1;
                    if depth == 0 {
                        return (&html[start..c], c + close_pat.len());
                    }
                    pos = c + close_pat.len();
                }
                _ => break,
            }
        }
        (&html[start..], html.len())
    }

    /// Visible text of an HTML fragment, without the excluded subtrees.
    fn text_of(fragment: &str) -> String {
        let mut out = String::new();
        let mut rest = fragment;
        while let Some(i) = rest.find('<') {
            out.push_str(&decode(&rest[..i]));
            let tag = &rest[i..];
            let Some(gt) = tag.find('>') else {
                rest = "";
                break;
            };
            let head = &tag[..gt];
            let excluded = head.contains("data-exclude-from-selection=\"true\"");
            rest = &tag[gt + 1..];
            if excluded && !head.ends_with('/') {
                let (_, next) = element_inner(tag, 0);
                rest = tag.get(next..).unwrap_or("");
            }
        }
        out.push_str(&decode(rest));
        out
    }

    fn decode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(i) = rest.find('&') {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            let Some(semi) = tail.find(';').filter(|n| *n <= 10) else {
                out.push('&');
                rest = &tail[1..];
                continue;
            };
            let ent = &tail[1..semi];
            let ch = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => ent
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            match ch {
                Some(c) => {
                    out.push(c);
                    rest = &tail[semi + 1..];
                }
                None => {
                    out.push('&');
                    rest = &tail[1..];
                }
            }
        }
        out.push_str(rest);
        out
    }
}

// ---- fetching ----------------------------------------------------------------

async fn get_text(http: &reqwest::Client, source: Source, url: url::Url) -> Option<String> {
    gate::wait(source).await;
    let resp = http.get(url).timeout(TIMEOUT).send().await.ok()?;
    if !resp.status().is_success() {
        tracing::debug!("{} answered {}", source.name(), resp.status());
        return None;
    }
    resp.text().await.ok()
}

fn url_with(base: &str, path: &str, params: &[(&str, &str)]) -> Option<url::Url> {
    let mut u = url::Url::parse(&format!("{base}{path}")).ok()?;
    u.query_pairs_mut().extend_pairs(params);
    Some(u)
}

/// Searches one provider and returns its candidates ranked for `q`.
pub async fn search(http: &reqwest::Client, source: Source, q: &Query) -> Vec<SearchHit> {
    let both = format!("{} {}", q.artist, q.name).trim().to_string();
    let hits = match source {
        Source::NetEase => {
            if both.is_empty() {
                return vec![];
            }
            let Some(u) =
                url_with(NETEASE_SEARCH, "", &[("limit", "5"), ("offset", "0"), ("s", &both), ("type", "1")])
            else {
                return vec![];
            };
            get_text(http, source, u).await.map(|b| netease::parse_search(&b))
        }
        Source::Lrclib => {
            let query = format!("{} {}", q.name, q.artist).trim().to_string();
            if query.is_empty() {
                return vec![];
            }
            let Some(u) = url_with(LRCLIB, "/search", &[("q", &query)]) else { return vec![] };
            get_text(http, source, u).await.map(|b| lrclib::parse_search(&b))
        }
        Source::Genius => {
            if both.is_empty() {
                return vec![];
            }
            let Some(u) = url_with(GENIUS_SEARCH, "", &[("per_page", "5"), ("q", &both)]) else {
                return vec![];
            };
            get_text(http, source, u).await.map(|b| genius::parse_search(&b))
        }
        Source::SimpMusic => {
            if q.name.is_empty() {
                return vec![];
            }
            let Some(u) = url_with(SIMPMUSIC, "/search", &[("q", &q.name)]) else { return vec![] };
            get_text(http, source, u).await.map(|b| simpmusic::parse_search(&b))
        }
    };
    rank::order(q, hits.unwrap_or_default())
}

/// The lyrics text (LRC or plain) of one candidate.
pub async fn fetch_text(
    http: &reqwest::Client,
    source: Source,
    id: &str,
    netease_translation: bool,
) -> Option<String> {
    match source {
        Source::NetEase => {
            let u = url_with(NETEASE_LYRICS, "", &[("id", id), ("kv", "-1"), ("lv", "-1"), ("tv", "-1")])?;
            netease::parse_lyrics(&get_text(http, source, u).await?, netease_translation)
        }
        Source::Lrclib => {
            let u = url::Url::parse(&format!("{LRCLIB}/get/{}", urlencode(id))).ok()?;
            lrclib::parse_track(&get_text(http, source, u).await?)
        }
        Source::Genius => {
            // The id is the song page's URL; only genius.com pages are fetched.
            let u = url::Url::parse(id).ok()?;
            if u.scheme() != "https"
                || !u.host_str().is_some_and(|h| h == "genius.com" || h.ends_with(".genius.com"))
            {
                return None;
            }
            genius::parse_page(&get_text(http, source, u).await?)
        }
        Source::SimpMusic => {
            let u = url::Url::parse(&format!("{SIMPMUSIC}/{}", urlencode(id))).ok()?;
            simpmusic::parse_lyrics(&get_text(http, source, u).await?)
        }
    }
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_round_trip() {
        for s in Source::ALL {
            assert_eq!(Source::from_name(s.name()), Some(s));
        }
        let list = Source::parse_list(&["Genius".into(), "nope".into(), "NetEase".into(), "Genius".into()]);
        assert_eq!(list, [Source::Genius, Source::NetEase]);
    }

    #[test]
    fn netease_search_fixture() {
        let body = r#"{"code":200,"result":{"songCount":2,"songs":[
            {"id":186016,"name":"晴天","artists":[{"id":6452,"name":"周杰伦"}],"album":{"name":"叶惠美"}},
            {"id":7,"name":"Other","artists":[{"name":"A"},{"name":"B"}]}]}}"#;
        let hits = netease::parse_search(body);
        assert_eq!(hits.len(), 2);
        assert_eq!(
            (hits[0].id.as_str(), hits[0].name.as_str(), hits[0].artist.as_str()),
            ("186016", "晴天", "周杰伦")
        );
        assert_eq!(hits[1].artist, "A, B");
        assert_eq!(hits[0].source, Source::NetEase);
        assert!(netease::parse_search("not json").is_empty());
        assert!(netease::parse_search(r#"{"result":{}}"#).is_empty());
    }

    #[test]
    fn netease_lyrics_with_and_without_translation() {
        let body =
            r#"{"lrc":{"lyric":"[00:01.00]Hello\n[00:02.00]Bye"},"tlyric":{"lyric":"[00:01.00]Hallo"}}"#;
        assert_eq!(netease::parse_lyrics(body, false).unwrap(), "[00:01.00]Hello\n[00:02.00]Bye");
        let merged = netease::parse_lyrics(body, true).unwrap();
        assert_eq!(merged, "[00:01.00]Hello_BREAK_Hallo\n[00:02.00]Bye");
        assert!(netease::parse_lyrics(r#"{"lrc":{"lyric":" "}}"#, true).is_none());
        assert_eq!(netease::parse_lyrics(r#"{"lrc":{"lyric":"[00:01.00]a"}}"#, true).unwrap(), "[00:01.00]a");
    }

    #[test]
    fn lrclib_fixtures() {
        let body = r#"[{"id":1,"name":"Song","artistName":"Art","syncedLyrics":"[00:01.00]x","plainLyrics":"x"},
                       {"id":2,"name":"Song","artistName":"Art","syncedLyrics":null,"plainLyrics":"x"}]"#;
        let hits = lrclib::parse_search(body);
        assert_eq!(hits.iter().map(|h| h.synced).collect::<Vec<_>>(), [Some(true), Some(false)]);
        assert_eq!(hits[0].id, "1");
        assert_eq!(
            lrclib::parse_track(r#"{"syncedLyrics":"[00:01.00]x","plainLyrics":"x"}"#).unwrap(),
            "[00:01.00]x"
        );
        assert_eq!(lrclib::parse_track(r#"{"syncedLyrics":"","plainLyrics":"plain"}"#).unwrap(), "plain");
        assert!(lrclib::parse_track(r#"{"syncedLyrics":null,"plainLyrics":null}"#).is_none());
    }

    #[test]
    fn simpmusic_fixtures() {
        let body = r#"{"success":true,"data":[{"videoId":"abc","songTitle":"T","artistName":"A",
            "syncedLyrics":"[00:01.00]x","plainLyric":"x"},{"videoId":"d","songTitle":"U","artistName":"B","plainLyric":"y"}]}"#;
        let hits = simpmusic::parse_search(body);
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[0].id.as_str(), hits[0].synced, hits[1].synced), ("abc", Some(true), Some(false)));
        assert_eq!(simpmusic::parse_lyrics(body).unwrap(), "[00:01.00]x");
        assert_eq!(simpmusic::parse_lyrics(r#"{"data":{"plainLyric":"only"}}"#).unwrap(), "only");
        assert!(simpmusic::parse_lyrics(r#"{"data":[]}"#).is_none());
    }

    #[test]
    fn genius_search_fixture() {
        let body = r#"{"meta":{"status":200},"response":{"sections":[{"type":"song","hits":[
            {"type":"song","result":{"url":"https://genius.com/Rick-astley-never-gonna-give-you-up-lyrics",
             "artist_names":"Rick Astley","full_title":"Never Gonna Give You Up by Rick Astley"}}]}]}}"#;
        let hits = genius::parse_search(body);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "https://genius.com/Rick-astley-never-gonna-give-you-up-lyrics");
        assert_eq!(hits[0].artist, "Rick Astley");
    }

    #[test]
    fn genius_page_fixture() {
        let html = r#"<html><body><div class="header">no</div>
          <div data-lyrics-container="true" class="Lyrics__Container">
            <div data-exclude-from-selection="true"><div>1 Contributor</div>Lyrics</div>
            [Verse 1]<br/>We&#x27;re no strangers to love<br/>You know the rules &amp; so do I<br/>
            <a href="/x"><span>Inner <i>link</i></span></a><br/>Last line
          </div>
          <div data-lyrics-container="true"><div>nested</div>Second block</div>
          <div class="footer">no</div></body></html>"#;
        let text = genius::parse_page(html).unwrap();
        assert!(
            text.contains("[Verse 1]\nWe're no strangers to love\nYou know the rules & so do I\n"),
            "{text}"
        );
        assert!(text.contains("Inner link\nLast line"), "{text}");
        assert!(text.contains("Second block"), "{text}");
        assert!(!text.contains("Contributor"), "{text}");
        assert!(!text.contains("no\n"), "{text}");
        assert!(genius::parse_page("<html>nothing</html>").is_none());
    }

    #[test]
    fn genius_old_markup() {
        let html = r#"<div class="lyrics"><p>Old<br>school</p></div>"#;
        assert_eq!(genius::parse_page(html).unwrap(), "Old\nschool");
    }
}
