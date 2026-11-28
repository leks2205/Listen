//! gnudb (the FreeDB/CDDB successor): names a disc MusicBrainz doesn't know.
//!
//! Two requests, as <https://gnudb.org/howtognudb.php> asks:
//! 1. `cddb query <CDDB disc ID> <tracks> <offsets…> <seconds>` lists the entries for this TOC;
//! 2. `cddb read <category> <id>` with the category and id **the query returned** (it can differ
//!    from the computed disc ID; reading by the computed one may return another disc's entry).
//!
//! Every request carries `hello=<user> <host> ListenMusicPlayer <version>` built from the e-mail
//! address the user entered in Settings; with no address set Listen never asks gnudb.
//!
//! The query sends the disc's length as lead-out / 75. gnudb refuses some lengths outright
//! ("500 Command syntax error." for 949 s, while 948 and 950 are answered); a refused query is
//! asked once more with the length the CDDB disc ID counts (from the first track to the lead-out,
//! [`cddb_length`]). A `202` (no match) is an answer and is not asked again.
//!
//! How sure the answer is: `200` (one exact match) and `210` (a list of exact matches) fit this
//! disc's TOC. A `211` list is inexact — gnudb's guess at a disc that is merely close, the same
//! mistake as a MusicBrainz TOC match that only fits the total length. Its entry is only used when
//! it has exactly as many tracks as the disc has audio tracks. Every listed entry (up to
//! [`MAX_CANDIDATES`]) is read; one whose stored frame offsets or track count differ from the
//! disc's is dropped. One left: it names the disc. Several left — different records typed in
//! for the same TOC, which the TOC cannot tell apart — and only the one an outside source
//! confirms ([`crate::confirm`]) is used; none or several confirmed, none is used. Whatever gnudb
//! can't name falls through to CD-Text. gnudb entries are typed in by other people and can be
//! wrong either way, which is why a gnudb-named album stays flagged for fixing on MusicBrainz.
//!
//! The network is behind [`Net`] ([`Recorded`] answers for tests); everything else is pure.

use crate::accuraterip::{cddb_id, cddb_length};
use crate::confirm::{self, Verdict};
use crate::toc::{PREGAP, Toc};
use serde_json::Value;
use std::future::Future;
use std::sync::{Arc, Mutex};

const URL: &str = "https://gnudb.gnudb.org/~cddb/cddb.cgi";
/// gnudb refuses generic names ("500 Unknown application"); this one is accepted.
const APP: &str = "ListenMusicPlayer";
const FRAMES_PER_SECOND: u32 = 75;
/// Entries of one query answer that are read, at most.
pub const MAX_CANDIDATES: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum GnudbError {
    #[error("“{0}” is not an e-mail address")]
    Address(String),
    #[error("gnudb: {0}")]
    Http(#[from] reqwest::Error),
    /// An error code line ("403 Database entry is corrupt", "429 …", "500 …").
    #[error("gnudb answered “{0}”")]
    Refused(String),
    #[error("gnudb sent an answer Listen doesn't understand: {0}")]
    Malformed(String),
}

/// One row of a query answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub category: String,
    pub id: String,
    /// "Artist / Album" as listed.
    pub title: String,
    /// `false` for a row of a `211` (inexact) list.
    pub exact: bool,
}

/// A disc's entry (`cddb read`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub category: String,
    pub id: String,
    pub artist: String,
    pub album: String,
    /// `DYEAR` when it is a plausible year.
    pub year: Option<u16>,
    pub genre: String,
    /// `TTITLEn` in order (empty where the entry has none).
    pub tracks: Vec<String>,
    /// The frame offsets stored with the entry (`# Track frame offsets:`; empty when it has none).
    pub offsets: Vec<u32>,
}

/// `cddb query …` for this disc: the CDDB disc ID, every track's start frame (lead-in included)
/// and the disc's length in seconds (lead-out / 75) — the numbers [`cddb_id`] is computed from.
pub fn query_command(toc: &Toc) -> String {
    query_with_length(toc, (toc.leadout_lba + PREGAP) / FRAMES_PER_SECOND)
}

/// The query asked when gnudb refused [`query_command`]: the same with [`cddb_length`] as the
/// disc's length; `None` when that is the same number.
pub fn retry_command(toc: &Toc) -> Option<String> {
    let retry = query_with_length(toc, cddb_length(toc));
    (retry != query_command(toc)).then_some(retry)
}

fn query_with_length(toc: &Toc, seconds: u32) -> String {
    let mut s = format!("cddb query {:08x} {}", cddb_id(toc), toc.tracks.len());
    for t in &toc.tracks {
        s.push_str(&format!(" {}", t.lba + PREGAP));
    }
    s.push_str(&format!(" {seconds}"));
    s
}

/// `hello=` from an e-mail address: "user host ListenMusicPlayer 0.43.0".
pub fn hello(email: &str) -> Result<String, GnudbError> {
    let email = email.trim();
    let bad = || GnudbError::Address(email.to_string());
    let (user, host) = email.split_once('@').ok_or_else(bad)?;
    let fine = |s: &str| !s.is_empty() && !s.contains('@') && !s.chars().any(char::is_whitespace);
    if !fine(user) || !fine(host) || !host.contains('.') || host.starts_with('.') || host.ends_with('.') {
        return Err(bad());
    }
    Ok(format!("{user} {host} {APP} {}", env!("CARGO_PKG_VERSION")))
}

/// The code line's number and the lines after it up to the lone `.` (none for single-line answers).
fn split(text: &str) -> Result<(u16, &str, Vec<&str>), GnudbError> {
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    let first = lines.next().unwrap_or("").trim();
    let code = first
        .get(..3)
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| GnudbError::Malformed(first.chars().take(80).collect()))?;
    let multi = matches!(code, 210..=213);
    let body = if multi { lines.take_while(|l| *l != ".").collect() } else { Vec::new() };
    Ok((code, first, body))
}

fn row(line: &str, exact: bool) -> Option<Match> {
    let mut parts = line.trim().splitn(3, ' ');
    let category = parts.next().filter(|s| !s.is_empty())?.to_string();
    let id = parts.next().filter(|s| !s.is_empty())?.to_string();
    Some(Match { category, id, title: parts.next().unwrap_or("").trim().to_string(), exact })
}

/// The matches of a query answer, best first (`202`: none).
pub fn parse_query(text: &str) -> Result<Vec<Match>, GnudbError> {
    let (code, first, body) = split(text)?;
    match code {
        200 => Ok(row(&first[3..], true).into_iter().collect()),
        210 | 211 => Ok(body.iter().filter_map(|l| row(l, code == 210)).collect()),
        202 => Ok(Vec::new()),
        _ => Err(GnudbError::Refused(first.to_string())),
    }
}

/// `\n`, `\t` and `\\` in a value.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// A read answer's entry. A key that repeats continues its value (long titles are stored that way).
pub fn parse_read(text: &str) -> Result<Entry, GnudbError> {
    let (code, first, body) = split(text)?;
    if code != 210 {
        return Err(GnudbError::Refused(first.to_string()));
    }
    let head = row(&first[3..], true).ok_or_else(|| GnudbError::Malformed(first.to_string()))?;
    let (mut dtitle, mut dyear, mut genre) = (String::new(), String::new(), String::new());
    let mut tracks: Vec<String> = Vec::new();
    let mut offsets: Vec<u32> = Vec::new();
    let mut in_offsets = false;
    for line in body {
        if let Some(comment) = line.strip_prefix('#') {
            // "# Track frame offsets:", then one "#<space or tab>150" per track.
            let comment = comment.trim();
            if comment.eq_ignore_ascii_case("Track frame offsets:") {
                in_offsets = true;
            } else if in_offsets {
                match comment.parse::<u32>() {
                    Ok(o) => offsets.push(o),
                    Err(_) => in_offsets = false,
                }
            }
            continue;
        }
        in_offsets = false;
        let Some((key, value)) = line.split_once('=') else { continue };
        match key {
            "DTITLE" => dtitle.push_str(value),
            "DYEAR" => dyear.push_str(value),
            "DGENRE" => genre.push_str(value),
            k => {
                if let Some(n) = k.strip_prefix("TTITLE").and_then(|n| n.parse::<usize>().ok())
                    && n < 100
                {
                    if tracks.len() <= n {
                        tracks.resize(n + 1, String::new());
                    }
                    tracks[n].push_str(value);
                }
            }
        }
    }
    let dtitle = unescape(&dtitle);
    let (artist, album) = match dtitle.split_once(" / ") {
        Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
        None => (dtitle.trim().to_string(), dtitle.trim().to_string()),
    };
    let year = dyear.trim().parse::<u16>().ok().filter(|y| (1900..=2099).contains(y));
    Ok(Entry {
        category: head.category,
        id: head.id,
        artist,
        album,
        year,
        genre: unescape(&genre).trim().to_string(),
        tracks: tracks.iter().map(|t| unescape(t).trim().to_string()).collect(),
        offsets,
    })
}

/// `Ok` when the entry may name this disc, else why not. An entry without a title names nothing;
/// stored frame offsets must be the disc's; the track count must be the disc's (all tracks, or
/// its audio tracks); an inexact match needs exactly as many tracks as the disc has audio tracks.
pub fn check(m: &Match, entry: &Entry, toc: &Toc) -> Result<(), String> {
    if entry.album.is_empty() {
        return Err("has no title".into());
    }
    let disc: Vec<u32> = toc.tracks.iter().map(|t| t.lba + PREGAP).collect();
    if !entry.offsets.is_empty() && entry.offsets != disc {
        return Err(format!("its frame offsets ({} tracks) are not the disc's", entry.offsets.len()));
    }
    let n = entry.tracks.len();
    let fits = if m.exact { n == disc.len() || n == toc.audio_count() } else { n == toc.audio_count() };
    if !fits {
        let kind = if m.exact { "" } else { "inexact match, " };
        return Err(format!("{kind}{n} tracks, the disc has {}", toc.audio_count()));
    }
    Ok(())
}

/// The entry, if [`check`] lets it name this disc.
pub fn accept(m: &Match, entry: Entry, toc: &Toc) -> Option<Entry> {
    check(m, &entry, toc).is_ok().then_some(entry)
}

/// What a lookup asks of the network: gnudb commands, and JSON from the stores that confirm an
/// entry ([`confirm`]). [`Recorded`] answers from fixtures.
pub trait Net: Sync {
    /// gnudb's answer to `cmd`, as text.
    fn cddb(&self, cmd: &str) -> impl Future<Output = Result<String, GnudbError>> + Send;
    /// A JSON GET of `url` with `query`; `None` on any failure.
    fn json(&self, url: &str, query: &[(&str, &str)]) -> impl Future<Output = Option<Value>> + Send;
}

/// gnudb.org, iTunes and Deezer.
pub struct Live<'a> {
    http: &'a reqwest::Client,
    hello: String,
}

impl<'a> Live<'a> {
    pub fn new(http: &'a reqwest::Client, email: &str) -> Result<Self, GnudbError> {
        Ok(Self { http, hello: hello(email)? })
    }
}

impl Net for Live<'_> {
    async fn cddb(&self, cmd: &str) -> Result<String, GnudbError> {
        post(self.http, &self.hello, cmd).await
    }

    async fn json(&self, url: &str, query: &[(&str, &str)]) -> Option<Value> {
        let resp = match self.http.get(url).query(query).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::info!("gnudb: {url} answered {}", r.status());
                return None;
            }
            Err(e) => {
                tracing::info!("gnudb: {url}: {e}");
                return None;
            }
        };
        resp.json().await.inspect_err(|e| tracing::info!("gnudb: {url}: {e}")).ok()
    }
}

/// Answers recorded beforehand (tests, [`crate::pipeline::Lookup::Given`]): gnudb commands and
/// JSON URLs (`url?key=value&…`, unencoded, in the order asked) that aren't recorded fail. Every
/// command and URL asked is kept in [`Recorded::asked`].
#[derive(Debug, Clone, Default)]
pub struct Recorded {
    cddb: Vec<(String, String)>,
    json: Vec<(String, Value)>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Recorded {
    pub fn new() -> Self {
        Self::default()
    }

    /// gnudb answers `answer` to `cmd` (instead of what was recorded for it before).
    pub fn cddb(mut self, cmd: &str, answer: &str) -> Self {
        self.cddb.retain(|(c, _)| c != cmd);
        self.cddb.push((cmd.to_string(), answer.to_string()));
        self
    }

    /// `url` (with its query, see the type) answers `body` (instead of what was recorded before).
    pub fn json(mut self, url: &str, body: &str) -> Self {
        let body = serde_json::from_str(body).expect("recorded JSON");
        self.json.retain(|(u, _)| u != url);
        self.json.push((url.to_string(), body));
        self
    }

    /// `cmd` is not answered (a failed request).
    pub fn without_cddb(mut self, cmd: &str) -> Self {
        self.cddb.retain(|(c, _)| c != cmd);
        self
    }

    /// Every command and URL asked so far, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("not poisoned").clone()
    }

    fn note(&self, what: String) {
        self.asked.lock().expect("not poisoned").push(what);
    }
}

impl Net for Recorded {
    async fn cddb(&self, cmd: &str) -> Result<String, GnudbError> {
        self.note(cmd.to_string());
        self.cddb
            .iter()
            .find(|(c, _)| c == cmd)
            .map(|(_, a)| a.clone())
            .ok_or_else(|| GnudbError::Malformed(format!("no recorded answer to “{cmd}”")))
    }

    async fn json(&self, url: &str, query: &[(&str, &str)]) -> Option<Value> {
        let q: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let key = format!("{url}?{}", q.join("&"));
        self.note(key.clone());
        self.json.iter().find(|(u, _)| *u == key).map(|(_, b)| b.clone())
    }
}

/// One request (form-encoded POST, UTF-8): gnudb's answer as text.
async fn post(http: &reqwest::Client, hello: &str, cmd: &str) -> Result<String, GnudbError> {
    let bytes = http
        .post(URL)
        .form(&[("cmd", cmd), ("hello", hello), ("proto", "6")])
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// This disc's gnudb entry (`None`: gnudb doesn't know it, or only something it may not use).
pub async fn lookup(http: &reqwest::Client, toc: &Toc, email: &str) -> Result<Option<Entry>, GnudbError> {
    resolve(&Live::new(http, email)?, toc).await
}

async fn ask(net: &impl Net, cmd: &str) -> Result<String, GnudbError> {
    tracing::info!("gnudb: {cmd}");
    net.cddb(cmd).await
}

/// The listed entries for this disc's TOC, the query asked again once if gnudb refused it.
async fn query(net: &impl Net, toc: &Toc) -> Result<Vec<Match>, GnudbError> {
    match parse_query(&ask(net, &query_command(toc)).await?) {
        Err(GnudbError::Refused(why)) => {
            let Some(retry) = retry_command(toc) else { return Err(GnudbError::Refused(why)) };
            tracing::info!(
                "gnudb: “{why}” — asking again with the length the disc ID counts ({} s)",
                cddb_length(toc)
            );
            parse_query(&ask(net, &retry).await?)
        }
        answer => answer,
    }
}

/// [`lookup`] over any [`Net`].
pub async fn resolve(net: &impl Net, toc: &Toc) -> Result<Option<Entry>, GnudbError> {
    let matches = query(net, toc).await?;
    if matches.is_empty() {
        tracing::info!("gnudb: unknown disc");
        return Ok(None);
    }
    let label = |m: &Match| format!("{} {} “{}”", m.category, m.id, m.title);
    if let [m] = matches.as_slice() {
        // The common case: one entry, read and checked, no outside source asked.
        let entry = parse_read(&ask(net, &format!("cddb read {} {}", m.category, m.id)).await?)?;
        return Ok(match check(m, &entry, toc) {
            Ok(()) => {
                tracing::info!(
                    "gnudb: {} {} is {} / {}",
                    entry.category,
                    entry.id,
                    entry.artist,
                    entry.album
                );
                Some(entry)
            }
            Err(why) => {
                tracing::info!("gnudb: {} not used: {why}", label(m));
                None
            }
        });
    }

    // Several entries: read each (gnudb one at a time), keep those that fit the TOC. One that
    // can't be read, or isn't read (beyond the cap), may still be the right one: it can't be
    // confirmed, but it stops the others from winning unconfirmed.
    tracing::info!("gnudb: {} entries listed for this TOC", matches.len());
    let mut fitting: Vec<Entry> = Vec::new();
    let mut unchecked = 0;
    for (i, m) in matches.iter().enumerate() {
        if i >= MAX_CANDIDATES {
            tracing::info!("gnudb: candidate {}: not read (more than {MAX_CANDIDATES} entries)", label(m));
            unchecked += 1;
            continue;
        }
        let read = ask(net, &format!("cddb read {} {}", m.category, m.id)).await.and_then(|t| parse_read(&t));
        match read {
            Err(e) => {
                tracing::info!("gnudb: candidate {}: could not be read ({e})", label(m));
                unchecked += 1;
            }
            Ok(entry) => match check(m, &entry, toc) {
                Err(why) => tracing::info!("gnudb: candidate {}: dropped, {why}", label(m)),
                Ok(()) if fitting.iter().any(|f| same_record(f, &entry)) => {
                    tracing::info!("gnudb: candidate {}: the same entry as one above", label(m));
                }
                Ok(()) => fitting.push(entry),
            },
        }
    }
    let shown = |e: &Entry| format!("{} {} “{} / {}”", e.category, e.id, e.artist, e.album);
    if unchecked == 0 && fitting.len() <= 1 {
        return Ok(fitting
            .pop()
            .inspect(|e| tracing::info!("gnudb: candidate {}: the only one that fits", shown(e))));
    }

    // They fit the TOC equally well: only an outside source can tell which record this is.
    let verdicts = futures_util::future::join_all(
        fitting.iter().map(|e| confirm::confirm(net, &e.artist, &e.album, toc)),
    )
    .await;
    for (e, v) in fitting.iter().zip(&verdicts) {
        match v {
            Verdict::Confirmed(what) => tracing::info!("gnudb: candidate {}: confirmed ({what})", shown(e)),
            Verdict::Unconfirmed(why) => {
                tracing::info!("gnudb: candidate {}: not confirmed ({why})", shown(e))
            }
        }
    }
    let mut confirmed: Vec<Entry> =
        fitting.into_iter().zip(&verdicts).filter(|(_, v)| v.confirmed()).map(|(e, _)| e).collect();
    match confirmed.len() {
        1 => {
            let e = confirmed.pop().expect("one");
            tracing::info!(
                "gnudb: {} {} is {} / {} (the only confirmed candidate)",
                e.category,
                e.id,
                e.artist,
                e.album
            );
            Ok(Some(e))
        }
        n => {
            tracing::info!("gnudb: {n} of the colliding entries confirmed, none used");
            Ok(None)
        }
    }
}

/// Two listings of one record (the same entry filed under two categories): every value that
/// would be tagged is the same, so either names the disc alike.
fn same_record(a: &Entry, b: &Entry) -> bool {
    (&a.artist, &a.album, a.year, &a.genre, &a.tracks) == (&b.artist, &b.album, b.year, &b.genre, &b.tracks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 6-track disc from the bug report (HEART OF WOMAN / ETERNAL FADE).
    fn heart_toc() -> Toc {
        Toc::from_offsets(1, 71_625, &[150, 12_675, 22_950, 35_175, 46_875, 60_225])
    }

    fn five_tracks() -> Toc {
        Toc::from_offsets(1, 60_000, &[150, 12_000, 24_000, 36_000, 48_000])
    }

    #[test]
    fn query_is_the_one_gnudb_answered() {
        assert_eq!(
            query_command(&heart_toc()),
            "cddb query 4603b906 6 150 12675 22950 35175 46875 60225 955"
        );
    }

    #[test]
    fn hello_needs_an_address() {
        assert_eq!(
            hello(" someone@example.com ").unwrap(),
            format!("someone example.com ListenMusicPlayer {}", env!("CARGO_PKG_VERSION"))
        );
        for bad in
            ["", "someone", "@example.com", "someone@", "some one@example.com", "a@b@c.d", "a@localhost"]
        {
            assert!(matches!(hello(bad), Err(GnudbError::Address(_))), "{bad:?}");
        }
    }

    #[test]
    fn exact_list_then_read_by_the_returned_id() {
        let q = parse_query(include_str!("../tests/fixtures/gnudb_query_heart.txt")).unwrap();
        assert_eq!(
            q,
            [Match {
                category: "data".into(),
                id: "4603b98a".into(),
                title: "HEART OF WOMAN / ETERNAL FADE".into(),
                exact: true
            }]
        );
        // The read uses the id the query returned, not the computed 4603b906.
        assert_ne!(q[0].id, format!("{:08x}", cddb_id(&heart_toc())));
        let e = parse_read(include_str!("../tests/fixtures/gnudb_read_heart.txt")).unwrap();
        assert_eq!((e.category.as_str(), e.id.as_str()), ("data", "4603b98a"));
        assert_eq!((e.artist.as_str(), e.album.as_str()), ("HEART OF WOMAN", "ETERNAL FADE"));
        assert_eq!(e.year, Some(2026));
        assert_eq!(e.genre, "");
        assert_eq!(e.tracks, ["Footsteps", "Melt", "Blue", "Glow", "LIT IT", "Find U"]);
        assert_eq!(accept(&q[0], e.clone(), &heart_toc()), Some(e));
    }

    #[test]
    fn single_exact_match() {
        let q = parse_query("200 rock 0a0b0c03 Band / Album\r\n").unwrap();
        assert_eq!(q.len(), 1);
        assert!(q[0].exact);
        assert_eq!(
            (q[0].category.as_str(), q[0].id.as_str(), q[0].title.as_str()),
            ("rock", "0a0b0c03", "Band / Album")
        );
    }

    #[test]
    fn no_match_and_errors() {
        assert!(parse_query(include_str!("../tests/fixtures/gnudb_query_nomatch.txt")).unwrap().is_empty());
        for refused in [
            "403 Database entry is corrupt.\r\n",
            "429 Too many requests\r\n",
            "500 Unknown application / developer email\r\n",
        ] {
            assert!(matches!(parse_query(refused), Err(GnudbError::Refused(_))), "{refused}");
        }
        assert!(matches!(parse_query("<html>"), Err(GnudbError::Malformed(_))));
        assert!(matches!(
            parse_read("401 data 4603b98a No such CD entry in database.\r\n"),
            Err(GnudbError::Refused(_))
        ));
    }

    #[test]
    fn inexact_match_needs_the_same_track_count() {
        let q = parse_query(include_str!("../tests/fixtures/gnudb_query_inexact.txt")).unwrap();
        assert_eq!(q.len(), 2);
        assert!(q.iter().all(|m| !m.exact));
        assert_eq!(q[0].title, "Some Band / Close Enough");
        let e = parse_read(include_str!("../tests/fixtures/gnudb_read_inexact.txt")).unwrap();
        assert_eq!(e.tracks.len(), 5);
        assert_eq!(e.year, Some(1999));
        assert_eq!(e.genre, "Rock");
        // Five tracks: a guess that fits the disc's track count is used …
        assert_eq!(accept(&q[0], e.clone(), &five_tracks()).map(|e| e.album), Some("Close Enough".into()));
        // … one for the 6-track disc is not.
        assert_eq!(
            check(&q[0], &e, &heart_toc()),
            Err("its frame offsets (5 tracks) are not the disc's".into())
        );
        let mut no_offsets = Entry { offsets: vec![], ..e.clone() };
        assert_eq!(
            check(&q[0], &no_offsets, &heart_toc()),
            Err("inexact match, 5 tracks, the disc has 6".into())
        );
        // An exact match needs the disc's track count too.
        let exact = Match { exact: true, ..q[0].clone() };
        assert!(accept(&exact, e, &five_tracks()).is_some());
        assert_eq!(check(&exact, &no_offsets, &heart_toc()), Err("5 tracks, the disc has 6".into()));
        no_offsets.tracks.push("Six".into());
        assert_eq!(check(&exact, &no_offsets, &heart_toc()), Ok(()));
    }

    /// RESCENE – PULSE (미니4집): LBAs 0 / 10575 / 24975 / 40875 / 54525, lead-out 71025.
    fn pulse_toc() -> Toc {
        Toc::from_offsets(1, 71_175, &[150, 10_725, 25_125, 41_025, 54_675])
    }

    const QUERY_949: &str = "cddb query 3703b305 5 150 10725 25125 41025 54675 949";
    const QUERY_947: &str = "cddb query 3703b305 5 150 10725 25125 41025 54675 947";
    const READ_FADLADDER: &str = "cddb read data 3703b39a";
    const READ_PULSE: &str = "cddb read data 3703b39c";
    const ITUNES_PULSE: &str = "https://itunes.apple.com/search?term=RESCENE PULSE&entity=album&limit=10";
    const ITUNES_PULSE_TRACKS: &str = "https://itunes.apple.com/lookup?id=6828335122&entity=song";
    const ITUNES_FADLADDER: &str =
        "https://itunes.apple.com/search?term=Fadladder Fadladder Play Erik Satie&entity=album&limit=10";
    const DEEZER_FADLADDER: &str =
        "https://api.deezer.com/search/album?q=Fadladder Fadladder Play Erik Satie";

    /// gnudb as it answered for the PULSE disc: 949 s refused, 947 s two exact entries.
    fn pulse_gnudb() -> Recorded {
        Recorded::new()
            .cddb(QUERY_949, include_str!("../tests/fixtures/gnudb_query_pulse_refused.txt"))
            .cddb(QUERY_947, include_str!("../tests/fixtures/gnudb_query_pulse.txt"))
            .cddb(READ_FADLADDER, include_str!("../tests/fixtures/gnudb_read_fadladder.txt"))
            .cddb(READ_PULSE, include_str!("../tests/fixtures/gnudb_read_pulse.txt"))
    }

    /// … plus iTunes and Deezer as they answered for both candidates.
    fn pulse_world() -> Recorded {
        pulse_gnudb()
            .json(ITUNES_PULSE, include_str!("../tests/fixtures/itunes_search_pulse.json"))
            .json(ITUNES_PULSE_TRACKS, include_str!("../tests/fixtures/itunes_lookup_pulse.json"))
            .json(ITUNES_FADLADDER, include_str!("../tests/fixtures/itunes_search_fadladder.json"))
            .json(DEEZER_FADLADDER, include_str!("../tests/fixtures/deezer_search_fadladder.json"))
    }

    #[test]
    fn the_pulse_disc_lengths() {
        let toc = pulse_toc();
        assert_eq!(cddb_id(&toc), 0x3703_b305);
        assert_eq!(query_command(&toc), QUERY_949, "lead-out / 75 first");
        assert_eq!(retry_command(&toc).as_deref(), Some(QUERY_947), "then the disc ID's length");
        assert_eq!(cddb_length(&toc), 0x3b3);
    }

    #[test]
    fn the_colliding_entries_are_identical_to_the_toc() {
        let q = parse_query(include_str!("../tests/fixtures/gnudb_query_pulse.txt")).unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].title, "Fadladder / Fadladder Play Erik Satie");
        assert_eq!(q[1].title, "RESCENE / PULSE (미니4집)");
        let fad = parse_read(include_str!("../tests/fixtures/gnudb_read_fadladder.txt")).unwrap();
        let pulse = parse_read(include_str!("../tests/fixtures/gnudb_read_pulse.txt")).unwrap();
        assert_eq!(fad.offsets, [150, 10_725, 25_125, 41_025, 54_675]);
        assert_eq!(fad.offsets, pulse.offsets);
        assert_eq!(
            (pulse.artist.as_str(), pulse.album.as_str(), pulse.year),
            ("RESCENE", "PULSE (미니4집)", Some(2026))
        );
        assert_eq!(pulse.tracks, ["Finest Thing", "With", "백화요란(Inside Out)", "MEME", "Dragon"]);
        // The TOC can't tell them apart.
        assert_eq!(check(&q[0], &fad, &pulse_toc()), Ok(()));
        assert_eq!(check(&q[1], &pulse, &pulse_toc()), Ok(()));
    }

    #[tokio::test]
    async fn a_refused_query_is_asked_once_more_with_the_disc_id_length() {
        let net = pulse_world();
        resolve(&net, &pulse_toc()).await.unwrap();
        assert_eq!(net.asked()[..2], [QUERY_949, QUERY_947]);

        // Refused twice: the error, after exactly two queries.
        let refused = include_str!("../tests/fixtures/gnudb_query_pulse_refused.txt");
        let net = Recorded::new().cddb(QUERY_949, refused).cddb(QUERY_947, refused);
        let e = resolve(&net, &pulse_toc()).await.unwrap_err();
        assert!(matches!(&e, GnudbError::Refused(w) if w == "500 Command syntax error."), "{e}");
        assert_eq!(net.asked(), [QUERY_949, QUERY_947]);

        // "No match" is an answer: not asked again.
        let net = Recorded::new().cddb(QUERY_949, include_str!("../tests/fixtures/gnudb_query_nomatch.txt"));
        assert_eq!(resolve(&net, &pulse_toc()).await.unwrap(), None);
        assert_eq!(net.asked(), [QUERY_949]);
    }

    #[tokio::test]
    async fn of_two_colliding_entries_the_one_itunes_confirms_names_the_disc() {
        let net = pulse_world();
        let e = resolve(&net, &pulse_toc()).await.unwrap().expect("RESCENE confirmed");
        assert_eq!(
            (e.id.as_str(), e.artist.as_str(), e.album.as_str()),
            ("3703b39c", "RESCENE", "PULSE (미니4집)")
        );
        let asked = net.asked();
        assert_eq!(asked[2..4], [READ_FADLADDER, READ_PULSE], "both read, in list order");
        for url in [ITUNES_PULSE, ITUNES_PULSE_TRACKS, ITUNES_FADLADDER, DEEZER_FADLADDER] {
            assert!(asked.iter().any(|a| a == url), "{url} in {asked:#?}");
        }
        assert_eq!(asked.len(), 8);
    }

    #[tokio::test]
    async fn two_confirmed_entries_name_nothing() {
        let net = pulse_world()
            .json(
                ITUNES_FADLADDER,
                r#"{"resultCount":1,"results":[{"wrapperType":"collection","collectionId":1,
                    "artistName":"Fadladder","collectionName":"Fadladder Play Erik Satie","trackCount":5}]}"#,
            )
            .json(
                "https://itunes.apple.com/lookup?id=1&entity=song",
                r#"{"results":[{"wrapperType":"collection"},
                    {"wrapperType":"track","discNumber":1,"trackNumber":1,"trackTimeMillis":141000},
                    {"wrapperType":"track","discNumber":1,"trackNumber":2,"trackTimeMillis":192000},
                    {"wrapperType":"track","discNumber":1,"trackNumber":3,"trackTimeMillis":212000},
                    {"wrapperType":"track","discNumber":1,"trackNumber":4,"trackTimeMillis":182000},
                    {"wrapperType":"track","discNumber":1,"trackNumber":5,"trackTimeMillis":220000}]}"#,
            );
        for (artist, album) in [("Fadladder", "Fadladder Play Erik Satie"), ("RESCENE", "PULSE (미니4집)")]
        {
            assert!(confirm::confirm(&net, artist, album, &pulse_toc()).await.confirmed(), "{artist}");
        }
        assert_eq!(resolve(&net, &pulse_toc()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn no_confirmed_entry_names_nothing() {
        // iTunes and Deezer find nothing (or can't be reached) for either.
        let net = pulse_gnudb();
        assert_eq!(resolve(&net, &pulse_toc()).await.unwrap(), None);
        let asked = net.asked();
        assert_eq!(asked.iter().filter(|a| a.starts_with("https://api.deezer.com/")).count(), 2);
        // Found on iTunes, but with other track lengths: not confirmed, and Deezer isn't asked.
        let wrong = include_str!("../tests/fixtures/itunes_lookup_pulse.json").replace("140000", "150000");
        let net = Recorded::new()
            .json(ITUNES_PULSE, include_str!("../tests/fixtures/itunes_search_pulse.json"))
            .json(ITUNES_PULSE_TRACKS, &wrong);
        let v = confirm::confirm(&net, "RESCENE", "PULSE (미니4집)", &pulse_toc()).await;
        assert_eq!(
            v,
            Verdict::Unconfirmed("iTunes: RESCENE – PULSE - EP: track 1 is 150 s, on the disc 141 s".into())
        );
        assert_eq!(net.asked(), [ITUNES_PULSE, ITUNES_PULSE_TRACKS]);
    }

    #[tokio::test]
    async fn a_candidate_that_does_not_fit_the_toc_is_dropped_without_asking_anyone() {
        // A disc with other offsets but the same answers: Fadladder's entry doesn't fit it …
        let fad =
            include_str!("../tests/fixtures/gnudb_read_fadladder.txt").replace("#     10725", "#     10800");
        let net = pulse_gnudb().cddb(READ_FADLADDER, &fad);
        let e = resolve(&net, &pulse_toc()).await.unwrap().expect("the one that fits");
        assert_eq!(e.artist, "RESCENE");
        assert!(net.asked().iter().all(|a| !a.starts_with("https://")), "{:?}", net.asked());
    }

    #[tokio::test]
    async fn a_single_match_asks_no_outside_source() {
        let net = Recorded::new()
            .cddb(&query_command(&heart_toc()), include_str!("../tests/fixtures/gnudb_query_heart.txt"))
            .cddb("cddb read data 4603b98a", include_str!("../tests/fixtures/gnudb_read_heart.txt"));
        let e = resolve(&net, &heart_toc()).await.unwrap().expect("known");
        assert_eq!(e.album, "ETERNAL FADE");
        assert_eq!(net.asked(), [query_command(&heart_toc()).as_str(), "cddb read data 4603b98a"]);
    }

    #[tokio::test]
    async fn an_unreadable_candidate_still_collides() {
        // The RESCENE read fails: Fadladder is not used unconfirmed.
        let net = pulse_world().without_cddb(READ_PULSE);
        assert_eq!(resolve(&net, &pulse_toc()).await.unwrap(), None);
        assert!(net.asked().iter().any(|a| a == ITUNES_FADLADDER));
    }

    #[test]
    fn continued_values_are_concatenated() {
        let e = parse_read(include_str!("../tests/fixtures/gnudb_read_continued.txt")).unwrap();
        assert_eq!(e.artist, "The Long Names Orchestra");
        assert_eq!(e.album, "A Very Long Album Title That Did Not Fit On One Line");
        assert_eq!(
            e.tracks[0],
            "A song whose title is so long that the database had to split it across two lines"
        );
        assert_eq!(e.tracks[1], "Short");
        assert_eq!(e.tracks[2], "Back\\slash and\ttab");
        assert_eq!(e.year, None, "19xx is not a year");
    }

    #[test]
    fn title_without_a_slash_is_artist_and_album() {
        let e = parse_read(include_str!("../tests/fixtures/gnudb_read_noslash.txt")).unwrap();
        assert_eq!((e.artist.as_str(), e.album.as_str()), ("Soundtrack Sampler", "Soundtrack Sampler"));
        assert_eq!(e.tracks, ["Opening", "", "Closing"]);
        assert_eq!(e.year, None);
    }

    #[test]
    fn an_entry_without_a_title_names_nothing() {
        let m = Match { category: "misc".into(), id: "1".into(), title: String::new(), exact: true };
        assert_eq!(accept(&m, Entry { tracks: vec!["x".into()], ..Default::default() }, &heart_toc()), None);
    }

    /// Live: the PULSE disc on gnudb.org, iTunes and Deezer (run with --ignored and
    /// `LISTEN_GNUDB_EMAIL=you@example.org`).
    #[tokio::test]
    #[ignore = "live: gnudb.org, itunes.apple.com, api.deezer.com"]
    async fn live_colliding_lookup() {
        let email = std::env::var("LISTEN_GNUDB_EMAIL").expect("LISTEN_GNUDB_EMAIL");
        let e = lookup(&crate::musicbrainz::http_client(), &pulse_toc(), &email)
            .await
            .unwrap()
            .expect("confirmed");
        assert_eq!((e.artist.as_str(), e.album.as_str()), ("RESCENE", "PULSE (미니4집)"));
    }

    /// Live: the disc from the bug report on gnudb.org (run with --ignored and
    /// `LISTEN_GNUDB_EMAIL=you@example.org`).
    #[tokio::test]
    #[ignore = "live: gnudb.org"]
    async fn live_lookup() {
        let email = std::env::var("LISTEN_GNUDB_EMAIL").expect("LISTEN_GNUDB_EMAIL");
        let e =
            lookup(&crate::musicbrainz::http_client(), &heart_toc(), &email).await.unwrap().expect("known");
        assert_eq!(e.id, "4603b98a");
        assert_eq!((e.artist.as_str(), e.album.as_str()), ("HEART OF WOMAN", "ETERNAL FADE"));
        assert_eq!(e.tracks.len(), 6);
    }
}
