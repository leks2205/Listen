//! Identities of a signed-in session: the Google account and its brand-account channels, with library
//! sizes so the sign-in page can offer the one holding the music.

use crate::token::CookieToken;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::borrow::Cow;
use ytmapi_rs::auth::AuthToken;
use ytmapi_rs::parse::ParseFrom;
use ytmapi_rs::query::{PostMethod, PostQuery, Query};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub name: String,
    /// e.g. "@mixtape"
    pub handle: String,
    /// Brand-account page id; `None` for the Google account's own identity.
    pub page_id: Option<String>,
    /// Songs in the library (first page; `more` if there are further pages).
    pub songs: usize,
    pub more: bool,
}

impl Channel {
    /// "Mixtape (@mixtape) — 26+ songs"
    pub fn label(&self) -> String {
        let who = if self.handle.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, self.handle)
        };
        let songs = match (self.songs, self.more) {
            (0, _) => "empty library".to_string(),
            (1, false) => "1 song".to_string(),
            (n, false) => format!("{n} songs"),
            (n, true) => format!("{n}+ songs"),
        };
        format!("{who} — {songs}")
    }
}

/// A browse/account request whose JSON is read here.
pub(crate) struct RawQuery {
    pub path: &'static str,
    pub body: Map<String, Value>,
}

#[derive(Debug)]
pub(crate) struct Unparsed;

impl ParseFrom<RawQuery> for Unparsed {
    fn parse_from(_: ytmapi_rs::ProcessedResult<RawQuery>) -> ytmapi_rs::Result<Self> {
        Ok(Unparsed)
    }
}

impl<A: AuthToken> Query<A> for RawQuery {
    type Output = Unparsed;
    type Method = PostMethod;
}

impl PostQuery for RawQuery {
    fn header(&self) -> Map<String, Value> {
        self.body.clone()
    }
    fn params(&self) -> Vec<(&str, Cow<'_, str>)> {
        vec![]
    }
    fn path(&self) -> &str {
        self.path
    }
}

pub(crate) fn walk<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == key {
                    out.push(x);
                }
                walk(x, key, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| walk(x, key, out)),
        _ => {}
    }
}

fn text(v: Option<&Value>) -> String {
    let Some(v) = v else { return String::new() };
    v.get("runs")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(|x| x.get("text")?.as_str()).collect())
        .or_else(|| v.get("simpleText").and_then(Value::as_str).map(String::from))
        .unwrap_or_default()
}

async fn raw(token: CookieToken, path: &'static str, body: Map<String, Value>) -> Result<Value, String> {
    let yt = ytmapi_rs::YtMusicBuilder::new_rustls_tls()
        .with_auth_token(token)
        .build()
        .map_err(|e| e.to_string())?;
    let text = yt.raw_json_query::<RawQuery>(&RawQuery { path, body }).await.map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Identities from an `account/accounts_list` response.
pub(crate) fn parse_accounts(v: &Value) -> Vec<Channel> {
    let mut items = Vec::new();
    walk(v, "accountItem", &mut items);
    items
        .into_iter()
        .map(|a| {
            let mut ids = Vec::new();
            walk(a, "pageId", &mut ids);
            Channel {
                name: text(a.get("accountName")),
                handle: text(a.get("channelHandle")),
                page_id: ids.first().and_then(|i| i.as_str()).map(String::from),
                songs: 0,
                more: false,
            }
        })
        .collect()
}

/// Library size from a `FEmusic_liked_videos` response.
pub(crate) fn parse_library_size(v: &Value) -> (usize, bool) {
    let mut songs = Vec::new();
    walk(v, "musicResponsiveListItemRenderer", &mut songs);
    let mut cont = Vec::new();
    walk(v, "continuations", &mut cont);
    let mut cont2 = Vec::new();
    walk(v, "continuationItemRenderer", &mut cont2);
    (songs.len(), !cont.is_empty() || !cont2.is_empty())
}

/// The session's identities with their library sizes (fullest first).
pub async fn list(cookies: &str) -> Result<Vec<Channel>, String> {
    let own = CookieToken::new(cookies, None).await?;
    let accounts = raw(own.clone(), "account/accounts_list", Map::new()).await?;
    let mut channels = parse_accounts(&accounts);
    if channels.is_empty() {
        return Err("YouTube didn't list any account for this sign-in".into());
    }
    for c in &mut channels {
        let token = CookieToken::new(cookies, c.page_id.clone()).await?;
        let body = Map::from_iter([("browseId".to_string(), json!("FEmusic_liked_videos"))]);
        if let Ok(v) = raw(token, "browse", body).await {
            (c.songs, c.more) = parse_library_size(&v);
        }
    }
    // Stable: keeps YouTube's order among equally full channels.
    channels.sort_by_key(|c| std::cmp::Reverse((c.more, c.songs)));
    Ok(channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_accounts_and_library_size() {
        let accounts = json!({"actions": [{"getMultiPageMenuAction": {"menu": {"multiPageMenuRenderer": {"sections": [
            {"accountSectionListRenderer": {"contents": [{"accountItemSectionRenderer": {"contents": [
                {"accountItem": {"accountName": {"runs": [{"text": "Alex"}]}, "channelHandle": {"runs": [{"text": "@own"}]},
                    "isSelected": true, "serviceEndpoint": {"selectActiveIdentityEndpoint": {"supportedTokens": [{"accountStateToken": {}}]}}}},
                {"accountItem": {"accountName": {"runs": [{"text": "Mixtape"}]}, "channelHandle": {"runs": [{"text": "@mixtape"}]},
                    "serviceEndpoint": {"selectActiveIdentityEndpoint": {"supportedTokens": [{"pageIdToken": {"pageId": "1234"}}]}}}}
            ]}}]}}
        ]}}}}]});
        let c = parse_accounts(&accounts);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].page_id, None);
        assert_eq!(c[1].page_id.as_deref(), Some("1234"));
        assert_eq!(c[1].handle, "@mixtape");

        let lib = json!({"contents": {"x": [{"musicResponsiveListItemRenderer": {}}, {"musicResponsiveListItemRenderer": {}}],
                         "continuations": [{"nextContinuationData": {}}]}});
        assert_eq!(parse_library_size(&lib), (2, true));
        let mut brand = c[1].clone();
        (brand.songs, brand.more) = (26, true);
        assert_eq!(brand.label(), "Mixtape (@mixtape) — 26+ songs");
    }
}
