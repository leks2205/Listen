//! Browser cookie exports to a `Cookie` header for music.youtube.com, keeping only unexpired
//! cookies a browser would send there. Inputs:
//! * JSON arrays (Get cookies.txt LOCALLY, Cookie-Editor, EditThisCookie), optionally `{"cookies": [...]}`
//! * Netscape `cookies.txt` (tab-separated, `#HttpOnly_` prefixes allowed)
//! * a raw header value (`SAPISID=…; SID=…`, optionally prefixed `Cookie:`)

use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct Cookie {
    pub domain: String,
    pub name: String,
    pub value: String,
    /// Unix seconds; `None` for session cookies.
    pub expires: Option<f64>,
}

/// Would a browser send a cookie for `domain` to music.youtube.com?
fn applies(domain: &str) -> bool {
    let d = domain.trim().trim_start_matches('.').to_ascii_lowercase();
    d == "youtube.com" || d == "music.youtube.com"
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn from_json(v: &Value) -> Option<Vec<Cookie>> {
    // Some exporters wrap the list: {"cookies": [...]}.
    let list = v.as_array().or_else(|| v.get("cookies").and_then(Value::as_array))?;
    Some(
        list.iter()
            .filter_map(|c| {
                Some(Cookie {
                    domain: c.get("domain")?.as_str()?.to_string(),
                    name: c.get("name")?.as_str()?.to_string(),
                    value: c.get("value")?.as_str()?.to_string(),
                    // "session" cookies have no expiry.
                    expires: c.get("expirationDate").or_else(|| c.get("expires")).and_then(Value::as_f64),
                })
            })
            .collect(),
    )
}

fn from_netscape(text: &str) -> Vec<Cookie> {
    text.lines()
        .filter_map(|line| {
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            if line.starts_with('#') || line.trim().is_empty() {
                return None;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 7 {
                return None;
            }
            let expires = f[4].trim().parse::<f64>().ok().filter(|e| *e > 0.0);
            Some(Cookie { domain: f[0].into(), name: f[5].into(), value: f[6].trim_end().into(), expires })
        })
        .collect()
}

/// Builds the header; errors explain what is missing.
pub fn to_header(input: &str) -> Result<String, String> {
    let text = input.trim().trim_start_matches('\u{feff}');
    let cookies = if text.starts_with('[') || text.starts_with('{') {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not a valid cookie export: {e}"))?;
        from_json(&v).ok_or("the JSON is not a list of cookies")?
    } else if text.lines().any(|l| l.split('\t').count() >= 7) {
        from_netscape(text)
    } else {
        // Raw header.
        let header = text.trim_start_matches("Cookie:").trim().to_string();
        return if header.is_empty() { Err("no cookies given".into()) } else { Ok(header) };
    };

    let had_sapisid = input.contains("\"SAPISID\"") || input.contains("\tSAPISID\t");
    header(cookies, had_sapisid)
}

/// Builds the header from the cookies a browser would send; `had_sapisid`: a missing SAPISID expired.
pub(crate) fn header(cookies: Vec<Cookie>, had_sapisid: bool) -> Result<String, String> {
    let now = now();
    // Host-specific music.youtube.com cookies win over .youtube.com ones.
    let mut picked: Vec<Cookie> = Vec::new();
    for c in cookies.into_iter().filter(|c| applies(&c.domain)) {
        if c.expires.is_some_and(|e| e < now) {
            continue;
        }
        match picked.iter_mut().find(|p| p.name == c.name) {
            Some(existing) if c.domain.contains("music.") => *existing = c,
            Some(_) => {}
            None => picked.push(c),
        }
    }
    if picked.is_empty() {
        return Err("the export contains no youtube.com cookies — export them on music.youtube.com".into());
    }
    if !picked.iter().any(|c| c.name == "SAPISID") {
        return Err(if had_sapisid {
            "the SAPISID cookie in this export has expired — sign in on music.youtube.com and export again"
                .into()
        } else {
            "no SAPISID cookie — make sure you're signed in on music.youtube.com when exporting".into()
        });
    }
    Ok(picked.iter().map(|c| format!("{}={}", c.name, c.value)).collect::<Vec<_>>().join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAR: &str = "4102444800"; // 2100-01-01

    #[test]
    fn json_export_from_get_cookies_txt_locally() {
        let json = format!(
            r#"[{{"domain":".youtube.com","expirationDate":{FAR},"hostOnly":false,"httpOnly":false,"name":"SOCS","path":"/","sameSite":"lax","secure":true,"session":false,"storeId":"0","value":"CAI"}},
               {{"domain":".youtube.com","expirationDate":{FAR},"hostOnly":false,"httpOnly":false,"name":"SAPISID","path":"/","sameSite":"unspecified","secure":true,"session":false,"storeId":"0","value":"abc/DEF"}},
               {{"domain":".google.com","expirationDate":{FAR},"name":"NID","value":"skip","path":"/"}},
               {{"domain":".youtube.com","expirationDate":1000,"name":"OLD","value":"expired","path":"/"}},
               {{"domain":"music.youtube.com","session":true,"name":"YSC","value":"sess","path":"/"}}]"#
        );
        let h = to_header(&json).unwrap();
        assert_eq!(h, "SOCS=CAI; SAPISID=abc/DEF; YSC=sess");
    }

    #[test]
    fn netscape_cookies_txt() {
        let txt = format!(
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t{FAR}\tSAPISID\txyz\n#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t{FAR}\t__Secure-3PSID\tsecret\n.example.com\tTRUE\t/\tFALSE\t{FAR}\tX\ty\n"
        );
        assert_eq!(to_header(&txt).unwrap(), "SAPISID=xyz; __Secure-3PSID=secret");
    }

    #[test]
    fn raw_header_passes_through() {
        assert_eq!(to_header("Cookie: SAPISID=a; SID=b").unwrap(), "SAPISID=a; SID=b");
    }

    #[test]
    fn explains_missing_or_expired_sapisid() {
        let expired = r#"[{"domain":".youtube.com","expirationDate":1000,"name":"SAPISID","value":"old"},
                          {"domain":".youtube.com","name":"SOCS","value":"CAI"}]"#;
        assert!(to_header(expired).unwrap_err().contains("expired"));
        let none = r#"[{"domain":".youtube.com","name":"SOCS","value":"CAI"}]"#;
        assert!(to_header(none).unwrap_err().contains("signed in"));
        assert!(to_header("[1,2]").is_err());
    }
}
