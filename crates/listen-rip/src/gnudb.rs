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
//! How sure the answer is: `200` (one exact match) and `210` (a list of exact matches) fit this
//! disc's TOC. A `211` list is inexact — gnudb's guess at a disc that is merely close, the same
//! mistake as a MusicBrainz TOC match that only fits the total length. Its entry is only used when
//! it has exactly as many tracks as the disc has audio tracks; otherwise it is ignored and the
//! disc falls through to CD-Text. gnudb entries are typed in by other people and can be wrong
//! either way, which is why a gnudb-named album stays flagged for fixing on MusicBrainz.
//!
//! The HTTP is in [`post`]; everything else is pure and tested against recorded answers.

use crate::accuraterip::cddb_id;
use crate::toc::{PREGAP, Toc};

const URL: &str = "https://gnudb.gnudb.org/~cddb/cddb.cgi";
/// gnudb refuses generic names ("500 Unknown application"); this one is accepted.
const APP: &str = "ListenMusicPlayer";
const FRAMES_PER_SECOND: u32 = 75;

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
}

/// `cddb query …` for this disc: the CDDB disc ID, every track's start frame (lead-in included)
/// and the disc's length in seconds — the numbers [`cddb_id`] is computed from.
pub fn query_command(toc: &Toc) -> String {
    let mut s = format!("cddb query {:08x} {}", cddb_id(toc), toc.tracks.len());
    for t in &toc.tracks {
        s.push_str(&format!(" {}", t.lba + PREGAP));
    }
    s.push_str(&format!(" {}", (toc.leadout_lba + PREGAP) / FRAMES_PER_SECOND));
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
    for line in body {
        if line.starts_with('#') {
            continue;
        }
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
    })
}

/// The entry, if it may name this disc: an exact match always, an inexact one only with exactly
/// as many tracks as the disc has audio tracks. An entry without a title names nothing.
pub fn accept(m: &Match, entry: Entry, toc: &Toc) -> Option<Entry> {
    if entry.album.is_empty() {
        tracing::info!("gnudb: {} {} has no title, not used", m.category, m.id);
        return None;
    }
    if !m.exact && entry.tracks.len() != toc.audio_count() {
        tracing::info!(
            "gnudb: inexact match {} ({} tracks, the disc has {}) not used",
            m.title,
            entry.tracks.len(),
            toc.audio_count()
        );
        return None;
    }
    Some(entry)
}

/// One request (form-encoded POST, UTF-8): gnudb's answer as text.
async fn post(http: &reqwest::Client, hello: &str, cmd: &str) -> Result<String, GnudbError> {
    tracing::info!("gnudb: {cmd}");
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
    let hello = hello(email)?;
    let matches = parse_query(&post(http, &hello, &query_command(toc)).await?)?;
    let Some(m) = matches.first() else {
        tracing::info!("gnudb: unknown disc");
        return Ok(None);
    };
    let entry = parse_read(&post(http, &hello, &format!("cddb read {} {}", m.category, m.id)).await?)?;
    let entry = accept(m, entry, toc);
    if let Some(e) = &entry {
        tracing::info!("gnudb: {} {} is {} / {}", e.category, e.id, e.artist, e.album);
    }
    Ok(entry)
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
        assert_eq!(accept(&q[0], e.clone(), &heart_toc()), None);
        // An exact match is trusted whatever its track count.
        let exact = Match { exact: true, ..q[0].clone() };
        assert!(accept(&exact, e, &heart_toc()).is_some());
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
