//! Request signing with browser cookies (like ytmapi-rs's `BrowserToken`) plus the channel to act as:
//! requests for a brand-account channel carry its page id in `X-Goog-PageId`, as the website sends
//! after "Switch account".
//!
//! YouTube rotates the session cookie `__Secure-1PSIDTS` while a session is in use, so a cookie header
//! read once and kept answers as *signed out* a few hours later: the library, playlists and uploads
//! look empty while playback keeps working (yt-dlp resolves streams without the account's cookies,
//! see `ytdlp.rs`). A [`Session`] therefore remembers where the cookies came from and reads them
//! again.

use crate::browser;
use serde_json::Value;
use sha1::Digest;
use std::borrow::Cow;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use ytmapi_rs::auth::{AuthToken, LoggedIn, RawResult};
use ytmapi_rs::error::ErrorKind;

const YTM: &str = "https://music.youtube.com";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0";

/// Cookies read from the browser are used for this long before they are read again: YouTube rotates
/// the session token on its own schedule, and an old one is answered as signed out.
const SESSION_MAX_AGE: Duration = Duration::from_secs(10 * 60);

/// Set by [`Session::mark_stale`] when YouTube answered signed out: every token reads its cookies
/// again before the next request, whatever their age.
static SIGNED_OUT: AtomicBool = AtomicBool::new(false);

/// How long a refresh forced by a signed-out answer waits before it tries again: the browser may
/// need a moment to store the rotated token, and a locked keyring is not asked on every request.
const RETRY_AFTER: Duration = Duration::from_secs(2);

/// Whether a raw InnerTube answer is YouTube's signed-out page ("Sign in to listen to your liked
/// tracks"): every one of them carries a sign-in endpoint, and no signed-in page does.
pub fn signed_out_page(json: &str) -> bool {
    json.contains("signInEndpoint")
}

tokio::task_local! {
    /// The running request is a guest's: every page a guest gets has the sign-in button, so
    /// [`signed_out_page`] says nothing about it. ytmapi-rs hands [`CookieToken::deserialize_response`]
    /// no token, so the guest flag travels with the task.
    pub static GUEST: bool;
}

/// The answer to an account's request (not a guest's) is the signed-out page.
fn rotated_out(json: &str) -> bool {
    signed_out_page(json) && !GUEST.try_with(|guest| *guest).unwrap_or(false)
}

/// Reads the cookie header again (blocking): a browser profile's database, or a test's stand-in.
type Reader = Arc<dyn Fn() -> Result<String, String> + Send + Sync>;

/// The cookie header in use, and where to read it from again.
#[derive(Debug, Clone)]
pub struct Session(Arc<SessionInner>);

struct SessionInner {
    /// Reads the cookies from the browser profile again; `None` for a cookie export or a guest.
    reader: Option<Reader>,
    fresh: Mutex<Fresh>,
    /// YouTube answered signed out (see [`signed_out_page`]): read the cookies again.
    stale: AtomicBool,
    /// A re-read is under way (one at a time).
    refreshing: AtomicBool,
}

impl fmt::Debug for SessionInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionInner")
            .field("can_refresh", &self.reader.is_some())
            .field("fresh", &self.fresh)
            .field("stale", &self.stale)
            .field("refreshing", &self.refreshing)
            .finish()
    }
}

/// Never prints the cookies or the SAPISID (a `{:?}` of a token must be safe to log): only sizes.
impl fmt::Debug for Fresh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fresh")
            .field("cookies", &format_args!("<{} B>", self.cookies.len()))
            .field("read_at", &self.read_at)
            .finish_non_exhaustive()
    }
}

/// Clears the `refreshing` flag when a re-read ends, also if the reader panicked.
struct RefreshDone<'a>(&'a AtomicBool);

impl Drop for RefreshDone<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

struct Fresh {
    cookies: String,
    sapisid: String,
    /// When these cookies were read (they are read again after [`SESSION_MAX_AGE`]).
    read_at: Instant,
    /// When reading them was last *tried*, so a failed read is not repeated at once.
    tried_at: Instant,
}

impl Session {
    fn new(cookies: &str, profile: Option<browser::Profile>) -> Result<Self, String> {
        let reader = profile.map(|profile| Arc::new(move || profile.youtube_header()) as Reader);
        Self::with_reader(cookies, reader)
    }

    fn with_reader(cookies: &str, reader: Option<Reader>) -> Result<Self, String> {
        let cookies = cookies.trim().trim_end_matches(';').to_string();
        let sapisid = sapisid(&cookies)?;
        Ok(Self(Arc::new(SessionInner {
            reader,
            fresh: Mutex::new(Fresh { cookies, sapisid, read_at: Instant::now(), tried_at: Instant::now() }),
            stale: AtomicBool::new(false),
            refreshing: AtomicBool::new(false),
        })))
    }

    /// Whether the cookies of this session can be read again at all.
    pub fn can_refresh(&self) -> bool {
        self.0.reader.is_some()
    }

    /// The cookie header and its SAPISID. When they are old or YouTube answered signed out, a
    /// re-read starts in the background and this request still goes out with the cookies held now
    /// ([`Session::refresh_now`] waits for the new ones). A browser that cannot be read keeps the
    /// cookies Listen has.
    fn header(&self) -> (String, String) {
        if self.due() {
            self.refresh_in_background();
        }
        let fresh = self.0.fresh.lock().unwrap_or_else(|e| e.into_inner());
        (fresh.cookies.clone(), fresh.sapisid.clone())
    }

    /// YouTube answered signed out: read the cookies again before the next request, at once (the
    /// wait between attempts is for repeats, not for the answer that just came back signed out).
    pub fn mark_stale(&self) {
        self.0.stale.store(true, Ordering::Relaxed);
        SIGNED_OUT.store(true, Ordering::Relaxed);
        if let Ok(mut fresh) = self.0.fresh.lock() {
            fresh.tried_at = Instant::now() - RETRY_AFTER;
        }
    }

    /// An answer that is not the signed-out page: the cookies in use are good again.
    pub fn clear_stale(&self) {
        self.0.stale.store(false, Ordering::Relaxed);
        SIGNED_OUT.store(false, Ordering::Relaxed);
    }

    fn due(&self) -> bool {
        if self.0.reader.is_none() {
            return false;
        }
        let Ok(fresh) = self.0.fresh.lock() else { return false };
        if fresh.read_at.elapsed() >= SESSION_MAX_AGE {
            return true;
        }
        let stale = self.0.stale.load(Ordering::Relaxed) || SIGNED_OUT.load(Ordering::Relaxed);
        stale && fresh.tried_at.elapsed() >= RETRY_AFTER
    }

    /// Starts a re-read without waiting for it. ytmapi-rs calls `headers()` inside its request
    /// future, and a re-read (a database copy, `secret-tool`, maybe a keyring unlock prompt) must
    /// not block an async worker.
    fn refresh_in_background(&self) {
        if self.0.reader.is_none() || self.0.refreshing.swap(true, Ordering::SeqCst) {
            return;
        }
        let session = self.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn_blocking(move || session.read_again())),
            // Not inside a runtime: the caller is synchronous anyway.
            Err(_) => session.read_again(),
        }
    }

    /// Reads the cookies from the browser again and waits for them (off the async workers); a
    /// re-read already under way is waited for instead of starting another.
    pub async fn refresh_now(&self) {
        let session = self.clone();
        let _ = tokio::task::spawn_blocking(move || session.refresh()).await;
    }

    /// Blocking form of [`Session::refresh_now`].
    fn refresh(&self) {
        if self.0.reader.is_none() {
            return;
        }
        if self.0.refreshing.swap(true, Ordering::SeqCst) {
            while self.0.refreshing.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            return;
        }
        self.read_again();
    }

    /// Reads the cookies again (blocking). The caller has set `refreshing`; it is cleared once the
    /// new cookies are stored, so a waiting [`Session::refresh`] sees them.
    fn read_again(&self) {
        let _done = RefreshDone(&self.0.refreshing);
        let read =
            self.0.reader.as_ref().map_or_else(|| Err("no browser profile".to_string()), |reader| reader());
        let read = read.and_then(|header| match sapisid(&header) {
            Ok(id) => Ok((header, id)),
            Err(_) => Err("the browser cookies have no SAPISID now".to_string()),
        });
        {
            // Whether it worked or not, the next attempt waits (a locked keyring must not be asked
            // on every request).
            let mut fresh = self.0.fresh.lock().unwrap_or_else(|e| e.into_inner());
            fresh.tried_at = Instant::now();
            if let Ok((header, id)) = &read {
                fresh.cookies = header.clone();
                fresh.sapisid = id.clone();
                fresh.read_at = Instant::now();
            }
        }
        match read {
            Ok((header, _)) => {
                self.0.stale.store(false, Ordering::Relaxed);
                SIGNED_OUT.store(false, Ordering::Relaxed);
                tracing::info!("read the browser cookies again ({} B)", header.len());
            }
            Err(e) => tracing::warn!("could not read the browser cookies again: {e}"),
        }
    }
}

/// The SAPISID of a cookie header.
fn sapisid(cookies: &str) -> Result<String, String> {
    cookies
        .split(';')
        .find_map(|c| c.trim().strip_prefix("SAPISID="))
        .map(|v| v.to_string())
        .ok_or_else(|| "no SAPISID cookie — sign in to music.youtube.com first".to_string())
}

fn sapisid_hash(sapisid: &str) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let hash = sha1::Sha1::digest(format!("{now} {sapisid} {YTM}").as_bytes());
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("{now}_{hex}")
}

#[derive(Debug, Clone)]
pub struct CookieToken {
    session: Session,
    /// Brand-account channel to act as (`None` = the account's own identity).
    page_id: Option<String>,
    client_version: String,
}

impl CookieToken {
    /// `cookies`: a `Cookie` header containing `SAPISID`. Fetches music.youtube.com once for the
    /// client version, even for the guest cookie.
    pub async fn new(cookies: &str, page_id: Option<String>) -> Result<Self, String> {
        let cookies = cookies.trim().trim_end_matches(';').to_string();
        let session = Session::new(&cookies, None)?;
        Ok(Self { client_version: client_version(&cookies).await, session, page_id })
    }

    /// Like [`CookieToken::new`], but the cookies are read from `profile` again while Listen runs
    /// (see [`Session`]).
    pub async fn from_profile(profile: browser::Profile, page_id: Option<String>) -> Result<Self, String> {
        let reader = profile.clone();
        let header = tokio::task::spawn_blocking(move || reader.youtube_header())
            .await
            .map_err(|e| e.to_string())??;
        let session = Session::new(&header, Some(profile))?;
        Ok(Self { client_version: client_version(&header).await, session, page_id })
    }

    /// A token built without any request (the date-based client version), for unit tests: they
    /// never reach a real host, and [`CookieToken::new`] fetches music.youtube.com.
    #[cfg(test)]
    pub(crate) fn offline(cookies: &str) -> Result<Self, String> {
        let cookies = cookies.trim().trim_end_matches(';').to_string();
        let session = Session::new(&cookies, None)?;
        Ok(Self { client_version: fallback_version(), session, page_id: None })
    }

    /// The cookies in use, to ask for them to be read again (see [`Session::mark_stale`]).
    pub fn session(&self) -> Session {
        self.session.clone()
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
        .unwrap_or_else(fallback_version)
}

/// ytmusicapi's date-based client version.
fn fallback_version() -> String {
    format!("1.{}.01.00", today())
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
        let (cookies, sapisid) = self.session.header();
        let mut headers: Vec<(&str, Cow<'_, str>)> = vec![
            ("User-Agent", USER_AGENT.into()),
            ("X-Origin", YTM.into()),
            ("Origin", YTM.into()),
            ("Content-Type", "application/json".into()),
            ("Authorization", format!("SAPISIDHASH {}", sapisid_hash(&sapisid)).into()),
            ("Cookie", Cow::Owned(cookies)),
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
        // The signed-out page parses into nothing useful: report it as an error so the caller asks
        // again with the cookies read from the browser (code 0 = not a wrong password).
        if rotated_out(&raw.json) {
            SIGNED_OUT.store(true, Ordering::Relaxed);
            return Err(ErrorKind::OtherErrorCodeInResponse {
                code: 0,
                message: "signed out: the browser session token rotated".into(),
            }
            .into());
        }
        ytmapi_rs::ProcessedResult::try_from(raw)
    }
}

impl LoggedIn for CookieToken {}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(page_id: Option<&str>) -> CookieToken {
        CookieToken {
            session: Session::new("SAPISID=abc; SID=x", None).unwrap(),
            page_id: page_id.map(str::to_string),
            client_version: "1.20260921.01.00".into(),
        }
    }

    #[test]
    fn headers_carry_the_channel() {
        let t = token(Some("1234567890"));
        let headers: Vec<_> = t.headers().unwrap().into_iter().collect();
        assert!(headers.iter().any(|(k, v)| *k == "X-Goog-PageId" && v == "1234567890"));
        assert!(headers.iter().any(|(k, v)| *k == "Authorization" && v.starts_with("SAPISIDHASH ")));
        // An export or a guest has no profile to read again: the header never changes.
        assert!(!t.session().can_refresh());
        let own = token(None);
        assert!(!own.headers().unwrap().into_iter().any(|(k, _)| k == "X-Goog-PageId"));
        assert_eq!(today().len(), 8);
    }

    #[tokio::test]
    async fn a_guest_is_not_signed_out_by_the_sign_in_button() {
        let page = r#"{"contents":{"foo":{"signInEndpoint":{}}}}"#;
        assert!(rotated_out(page));
        assert!(!GUEST.scope(true, async { rotated_out(page) }).await);
        assert!(GUEST.scope(false, async { rotated_out(page) }).await);
    }

    #[test]
    fn the_signed_out_page_is_recognised() {
        assert!(signed_out_page(r#"{"contents":{"foo":{"signInEndpoint":{"clickTrackingParams":"x"}}}}"#));
        assert!(!signed_out_page(r#"{"contents":{"singleColumnBrowseResultsRenderer":{}}}"#));
    }

    /// A browser session whose re-read takes `delay` and returns `header`.
    fn reading_session(delay: Duration, header: &'static str) -> Session {
        let reader: Reader = Arc::new(move || {
            std::thread::sleep(delay);
            Ok(header.to_string())
        });
        Session::with_reader("SAPISID=old; SID=x", Some(reader)).unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn refresh_never_blocks_headers() {
        let session = reading_session(Duration::from_millis(1500), "SAPISID=new; SID=y");
        session.mark_stale();
        let started = Instant::now();
        let (cookies, id) = session.header();
        assert!(started.elapsed() < Duration::from_millis(500), "headers() waited {:?}", started.elapsed());
        assert_eq!((cookies.as_str(), id.as_str()), ("SAPISID=old; SID=x", "old"));
        // The re-read finishes in the background.
        let deadline = Instant::now() + Duration::from_secs(10);
        while session.header().1 != "new" {
            assert!(Instant::now() < deadline, "the background re-read never landed");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(session.header().0, "SAPISID=new; SID=y");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn refresh_now_waits_for_the_new_cookies() {
        let session = reading_session(Duration::from_millis(300), "SAPISID=new; SID=y");
        session.refresh_now().await;
        assert_eq!(session.header().1, "new");
        // A re-read already under way is waited for, not skipped.
        let session = reading_session(Duration::from_millis(300), "SAPISID=new; SID=y");
        session.mark_stale();
        assert_eq!(session.header().1, "old");
        session.refresh_now().await;
        assert_eq!(session.header().1, "new");
    }

    #[test]
    fn debug_output_never_contains_cookies() {
        let t = token(Some("1234567890"));
        let printed = format!("{t:?} {:?} {:#?}", t.session(), t);
        assert!(!printed.contains("abc"), "{printed}");
        assert!(!printed.contains("SID=x"), "{printed}");
        assert!(printed.contains("1234567890"), "{printed}");
    }

    #[test]
    fn a_stale_session_is_due_but_a_guest_is_not() {
        let t = token(None);
        t.session().mark_stale();
        assert!(!t.session().due());
    }
}
