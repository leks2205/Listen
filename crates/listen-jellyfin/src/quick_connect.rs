//! Jellyfin Quick Connect, as in Feishin's `use-jellyfin-quick-connect.ts`: show a code, poll until
//! another device approves it (`/web/#/quickconnect`), then trade the secret for a token.

use crate::{Options, auth_header, base_url, http_client, net};
use listen_source::{Result, SourceError};
use serde_json::{Value, json};

/// Seconds between two polls (Feishin: 5 s).
pub const POLL_SECS: u64 = 5;

/// A started request: show `code`, poll with `secret`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Initiated {
    pub code: String,
    pub secret: String,
}

/// The signed-in session handed out after approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub token: String,
    pub user_id: String,
    pub user_name: String,
}

/// Where the user approves the code.
pub fn approval_url(server_url: &str) -> Option<String> {
    let url = server_url.trim().trim_end_matches('/');
    (url.starts_with("http://") || url.starts_with("https://")).then(|| format!("{url}/web/#/quickconnect"))
}

/// `POST /QuickConnect/Initiate`. `device_id` is the id the token will belong to.
pub async fn initiate(url: &str, device_id: &str, options: Options) -> Result<Initiated> {
    let (base, http) = (base_url(url)?, http_client(options.ignore_ssl)?);
    let resp = http
        .post(base.join("QuickConnect/Initiate").expect("valid path"))
        .header("Authorization", auth_header(device_id, None))
        .send()
        .await
        .map_err(net)?;
    if matches!(resp.status().as_u16(), 401 | 403 | 400) {
        return Err(SourceError::Auth("Quick Connect is not enabled on this server".into()));
    }
    if !resp.status().is_success() {
        return Err(SourceError::Server {
            code: resp.status().as_u16().into(),
            message: "Quick Connect".into(),
        });
    }
    let body: Value = resp.json().await.map_err(|e| SourceError::Parse(e.to_string()))?;
    let code = body["Code"].as_str().ok_or_else(|| SourceError::Parse("no Code".into()))?;
    let secret = body["Secret"].as_str().ok_or_else(|| SourceError::Parse("no Secret".into()))?;
    Ok(Initiated { code: code.into(), secret: secret.into() })
}

/// `GET /QuickConnect/Connect?secret=…`: has the code been approved yet?
pub async fn is_approved(url: &str, device_id: &str, secret: &str, options: Options) -> Result<bool> {
    let (base, http) = (base_url(url)?, http_client(options.ignore_ssl)?);
    let resp = http
        .get(base.join("QuickConnect/Connect").expect("valid path"))
        .query(&[("secret", secret)])
        .header("Authorization", auth_header(device_id, None))
        .send()
        .await
        .map_err(net)?;
    if !resp.status().is_success() {
        return Err(SourceError::Auth("Quick Connect was deactivated or the code expired".into()));
    }
    let body: Value = resp.json().await.map_err(|e| SourceError::Parse(e.to_string()))?;
    Ok(body["Authenticated"].as_bool().unwrap_or(false))
}

/// `POST /Users/AuthenticateWithQuickConnect`: the token for an approved secret.
pub async fn authenticate(url: &str, device_id: &str, secret: &str, options: Options) -> Result<Session> {
    let (base, http) = (base_url(url)?, http_client(options.ignore_ssl)?);
    let resp = http
        .post(base.join("Users/AuthenticateWithQuickConnect").expect("valid path"))
        .header("Authorization", auth_header(device_id, None))
        .json(&json!({ "Secret": secret }))
        .send()
        .await
        .map_err(net)?;
    if !resp.status().is_success() {
        return Err(SourceError::Auth("Quick Connect sign-in failed".into()));
    }
    let body: Value = resp.json().await.map_err(|e| SourceError::Parse(e.to_string()))?;
    let field = |v: &Value, what: &str| {
        v.as_str().map(str::to_string).ok_or_else(|| SourceError::Parse(format!("no {what}")))
    };
    Ok(Session {
        token: field(&body["AccessToken"], "AccessToken")?,
        user_id: field(&body["User"]["Id"], "user id")?,
        user_name: body["User"]["Name"].as_str().unwrap_or_default().to_string(),
    })
}
