//! Request signing with browser cookies (like ytmapi-rs's `BrowserToken`) plus the channel to act as:
//! requests for a brand-account channel carry its page id in `X-Goog-PageId`, as the website sends
//! after "Switch account".

use serde_json::Value;
use sha1::Digest;
use std::borrow::Cow;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use ytmapi_rs::auth::{AuthToken, LoggedIn, RawResult};
use ytmapi_rs::error::ErrorKind;

const YTM: &str = "https://music.youtube.com";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0";

#[derive(Debug, Clone)]
pub struct CookieToken {
    cookies: String,
    sapisid: String,
    /// Brand-account channel to act as (`None` = the account's own identity).
    page_id: Option<String>,
    client_version: String,
}

impl CookieToken {
    /// `cookies`: a `Cookie` header containing `SAPISID`.
    pub async fn new(cookies: &str, page_id: Option<String>) -> Result<Self, String> {
        let cookies = cookies.trim().trim_end_matches(';').to_string();
        let sapisid = cookies
            .split(';')
            .find_map(|c| c.trim().strip_prefix("SAPISID="))
            .ok_or("no SAPISID cookie — sign in to music.youtube.com first")?
            .to_string();
        Ok(Self { client_version: client_version(&cookies).await, cookies, sapisid, page_id })
    }

    fn sapisid_hash(&self) -> String {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let hash = sha1::Sha1::digest(format!("{now} {} {YTM}", self.sapisid).as_bytes());
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        format!("{now}_{hex}")
    }
}

/// The web client version from the page (like ytmapi-rs), or ytmusicapi's
/// date-based fallback.
async fn client_version(cookies: &str) -> String {
    // Right after a boot or resume the network (Tailscale, DNS) is not answering yet:
    // without a bound this request sits there and the whole connect never completes.
    let page = async {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?
            .get(YTM)
            .header("User-Agent", USER_AGENT)
            .header("Cookie", cookies)
            .send()
            .await
            .ok()?
            .text()
            .await
            .ok()
    };
    page.await
        .and_then(|html| {
            let (_, rest) = html.split_once("INNERTUBE_CLIENT_VERSION\":\"")?;
            rest.split_once('"').map(|(v, _)| v.to_string())
        })
        .unwrap_or_else(|| format!("1.{}.01.00", today()))
}

/// Today's UTC date as `YYYYMMDD`.
fn today() -> String {
    let days = (SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) / 86_400) as i64;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}")
}

impl AuthToken for CookieToken {
    fn headers(&self) -> ytmapi_rs::Result<impl IntoIterator<Item = (&str, Cow<'_, str>)>> {
        let mut headers: Vec<(&str, Cow<'_, str>)> = vec![
            ("User-Agent", USER_AGENT.into()),
            ("X-Origin", YTM.into()),
            ("Origin", YTM.into()),
            ("Content-Type", "application/json".into()),
            ("Authorization", format!("SAPISIDHASH {}", self.sapisid_hash()).into()),
            ("Cookie", self.cookies.as_str().into()),
            ("Accept", "*/*".into()),
            ("X-Goog-AuthUser", "0".into()),
        ];
        if let Some(page_id) = &self.page_id {
            headers.push(("X-Goog-PageId", page_id.as_str().into()));
        }
        Ok(headers)
    }

    fn client_version(&self) -> Cow<'_, str> {
        self.client_version.as_str().into()
    }

    fn deserialize_response<Q>(raw: RawResult<Q, Self>) -> ytmapi_rs::Result<ytmapi_rs::ProcessedResult<Q>> {
        // Same guard as ytmapi-rs's own tokens: error objects become errors.
        if let Ok(v) = serde_json::from_str::<Value>(&raw.json)
            && let Some(error) = v.get("error")
        {
            let code = error.get("code").and_then(Value::as_u64).unwrap_or(0);
            let message = error.get("message").and_then(Value::as_str).unwrap_or("").to_string();
            return Err(ErrorKind::OtherErrorCodeInResponse { code, message }.into());
        }
        ytmapi_rs::ProcessedResult::try_from(raw)
    }
}

impl LoggedIn for CookieToken {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_carry_the_channel() {
        let t = CookieToken {
            cookies: "SAPISID=abc; SID=x".into(),
            sapisid: "abc".into(),
            page_id: Some("1234567890".into()),
            client_version: "1.20260921.01.00".into(),
        };
        let headers: Vec<_> = t.headers().unwrap().into_iter().collect();
        assert!(headers.iter().any(|(k, v)| *k == "X-Goog-PageId" && v == "1234567890"));
        assert!(headers.iter().any(|(k, v)| *k == "Authorization" && v.starts_with("SAPISIDHASH ")));
        let own = CookieToken { page_id: None, ..t };
        assert!(!own.headers().unwrap().into_iter().any(|(k, _)| k == "X-Goog-PageId"));
        assert_eq!(today().len(), 8);
    }
}
