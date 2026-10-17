//! whipper (the secure ripper): its command line, the drive read offset,
//! and parsers for what it prints, its rip log (AccurateRip results) and the
//! cdrdao `.toc` it keeps (CD-Text).

use crate::drive::DriveInfo;
use crate::tags::CdText;
use std::path::{Path, PathBuf};

/// AccurateRip's drive offset table: model, offset, submissions.
const DRIVE_OFFSETS: &str = include_str!("drive_offsets.tsv");

fn norm_model(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_uppercase()
}

/// A drive's read offset from AccurateRip's table (e.g. +48 for the GU70N).
pub fn table_offset(model: &str) -> Option<i32> {
    let want = norm_model(model);
    if want.is_empty() {
        return None;
    }
    DRIVE_OFFSETS
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some((f.next()?, f.next()?.parse::<i32>().ok()?))
        })
        .find(|(m, _)| *m == want)
        .map(|(_, o)| o)
}

/// whipper's own config (after `whipper offset find`).
pub fn conf_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("whipper").join("whipper.conf"))
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) =
                std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `read_offset` from whipper.conf's `[drive:VENDOR:MODEL:RELEASE]` section
/// for this model (names are URL-encoded there).
pub fn configured_offset(conf: &str, model: &str) -> Option<i32> {
    let want = norm_model(model);
    let mut in_drive = false;
    for line in conf.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[') {
            let section = url_decode(section.trim_end_matches(']'));
            in_drive = section.starts_with("drive:")
                && section.split(':').any(|part| !want.is_empty() && norm_model(part) == want);
        } else if in_drive
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == "read_offset"
        {
            return v.trim().parse().ok();
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetSource {
    /// Listen's setting.
    Setting,
    /// whipper.conf (`whipper offset find`).
    Whipper,
    /// AccurateRip's drive table.
    AccurateRip,
}

impl OffsetSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Setting => "your setting",
            Self::Whipper => "whipper offset find",
            Self::AccurateRip => "AccurateRip drive database",
        }
    }
}

/// The read offset to rip with: the setting, else whipper's config, else
/// AccurateRip's table. `None`: unknown drive (ask for "Verify offset").
pub fn resolve_offset(setting: Option<i32>, drive: &DriveInfo) -> Option<(i32, OffsetSource)> {
    if let Some(o) = setting {
        return Some((o, OffsetSource::Setting));
    }
    if let Some(o) = conf_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|c| configured_offset(&c, &drive.model))
    {
        return Some((o, OffsetSource::Whipper));
    }
    table_offset(&drive.model).map(|o| (o, OffsetSource::AccurateRip))
}

/// Arguments for `whipper`: rip everything into `out_dir` as `NN.flac`
/// (Listen names and tags the files afterwards); keep going past
/// unreadable tracks (reported as skipped), CD-Rs and unknown discs allowed.
pub fn rip_args(device: &Path, offset: i32, out_dir: &Path, release_id: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "cd".into(),
        "-d".into(),
        device.to_string_lossy().into_owned(),
        "rip".into(),
        format!("--offset={offset}"),
        "--unknown".into(),
        "--cdr".into(),
        "--keep-going".into(),
        "--max-retries=5".into(),
        format!("--output-directory={}", out_dir.display()),
        "--disc-template=rip".into(),
        "--track-template=%t".into(),
    ];
    if let Some(id) = release_id.filter(|i| !i.is_empty()) {
        a.push(format!("--release-id={id}"));
    }
    a
}

/// `whipper offset find` (tries this offset first; needs a disc that is in
/// AccurateRip's database).
pub fn offset_find_args(device: &Path, first_try: Option<i32>) -> Vec<String> {
    let mut a = vec!["offset".into(), "find".into(), "-d".into(), device.to_string_lossy().into_owned()];
    if let Some(o) = first_try {
        a.push(format!("--offsets={o}"));
    }
    a
}

/// One line of whipper's output.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// A progress line: "Reading track 3 of 12  45 %".
    Progress {
        stage: String,
        track: Option<(u32, u32)>,
        percent: u32,
    },
    /// "ripping track 3 of 12: 03.flac" (log line when a track starts).
    Track {
        number: u32,
        total: u32,
    },
    Critical(String),
    Warning(String),
    Other(String),
}

/// "track 3 of 12" anywhere in `s`.
fn track_of(s: &str) -> Option<(u32, u32)> {
    let lower = s.to_lowercase();
    let i = lower.find("track ")?;
    let rest = &lower[i + 6..];
    let (n, rest) = rest.split_once(" of ")?;
    let total: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((n.trim().parse().ok()?, total.parse().ok()?))
}

pub fn parse_line(raw: &str) -> Line {
    let s = raw.trim();
    // Python logging: "LEVEL:logger.name:message".
    for (prefix, kind) in [("CRITICAL:", 2), ("ERROR:", 2), ("WARNING:", 1), ("INFO:", 0)] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let msg = rest.split_once(':').map_or(rest, |(_, m)| m).trim().to_string();
            return match kind {
                2 => Line::Critical(msg),
                1 => Line::Warning(msg),
                _ => match track_of(&msg).filter(|_| msg.starts_with("ripping track")) {
                    Some((number, total)) => Line::Track { number, total },
                    None => Line::Other(msg),
                },
            };
        }
    }
    if let Some(stage) = s.strip_suffix('%') {
        let stage = stage.trim_end();
        let digits: String = stage.chars().rev().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            let percent = digits.chars().rev().collect::<String>().parse().unwrap_or(0);
            let stage = stage[..stage.len() - digits.len()].trim().to_string();
            return Line::Progress { track: track_of(&stage), stage, percent };
        }
    }
    Line::Other(s.to_string())
}

/// An AccurateRip result for one track and one checksum version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ar {
    Match {
        confidence: u32,
    },
    Mismatch,
    NotInDb,
    #[default]
    Missing,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackLog {
    pub number: u32,
    pub filename: String,
    pub test_crc: String,
    pub copy_crc: String,
    pub v1: Ar,
    pub v2: Ar,
    pub status: String,
}

impl TrackLog {
    pub fn verified(&self) -> bool {
        matches!(self.v1, Ar::Match { .. }) || matches!(self.v2, Ar::Match { .. })
    }
    pub fn in_db(&self) -> bool {
        matches!(self.v1, Ar::Match { .. } | Ar::Mismatch)
            || matches!(self.v2, Ar::Match { .. } | Ar::Mismatch)
    }
    pub fn skipped(&self) -> bool {
        self.status.contains("skipped")
    }
    pub fn crc_ok(&self) -> bool {
        self.status == "Copy OK"
    }
}

/// whipper's rip log (the YAML "EAC-style" log).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RipLog {
    pub tracks: Vec<TrackLog>,
    pub accuraterip_summary: String,
    pub health: String,
}

/// Counts for the notification: "AccurateRip: 11/12 verified".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArSummary {
    pub tracks: u32,
    pub verified: u32,
    /// In the database, but the checksum differs.
    pub mismatched: u32,
    pub not_in_db: u32,
    pub skipped: u32,
}

impl RipLog {
    /// Real tracks only (HTOA, track 0, isn't in AccurateRip).
    pub fn summary(&self) -> ArSummary {
        let mut s = ArSummary::default();
        for t in self.tracks.iter().filter(|t| t.number > 0) {
            s.tracks += 1;
            if t.skipped() {
                s.skipped += 1;
            } else if t.verified() {
                s.verified += 1;
            } else if t.in_db() {
                s.mismatched += 1;
            } else {
                s.not_in_db += 1;
            }
        }
        s
    }
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2
        && ((v.starts_with('\'') && v.ends_with('\'')) || (v.starts_with('"') && v.ends_with('"')))
    {
        v[1..v.len() - 1].replace("''", "'")
    } else {
        v.to_string()
    }
}

fn ar_result(result: &str, confidence: Option<u32>) -> Ar {
    let r = result.to_lowercase();
    if r.contains("no exact match") {
        Ar::Mismatch
    } else if r.contains("exact match") {
        Ar::Match { confidence: confidence.unwrap_or(0) }
    } else if r.contains("not present") {
        Ar::NotInDb
    } else {
        Ar::Missing
    }
}

pub fn parse_log(text: &str) -> RipLog {
    let mut log = RipLog::default();
    let mut section = "";
    let mut track: Option<TrackLog> = None;
    // The AccurateRip block being parsed (1, 2) and its result/confidence so far.
    let mut ar: Option<(u8, String, Option<u32>)> = None;
    let flush_ar = |t: &mut Option<TrackLog>, ar: &mut Option<(u8, String, Option<u32>)>| {
        if let (Some(t), Some((v, result, conf))) = (t.as_mut(), ar.take()) {
            let r = ar_result(&result, conf);
            if v == 1 { t.v1 = r } else { t.v2 = r }
        }
    };
    for raw in text.lines() {
        let indent = raw.len() - raw.trim_start().len();
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = match line.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (line, ""),
        };
        if indent == 0 {
            flush_ar(&mut track, &mut ar);
            if let Some(t) = track.take() {
                log.tracks.push(t);
            }
            section = match key {
                "Tracks" => "tracks",
                "Conclusive status report" => "status",
                _ => "",
            };
            continue;
        }
        match section {
            "tracks" if indent == 2 => {
                flush_ar(&mut track, &mut ar);
                if let Some(t) = track.take() {
                    log.tracks.push(t);
                }
                if let Ok(n) = unquote(key).parse() {
                    track = Some(TrackLog { number: n, ..Default::default() });
                }
            }
            "tracks" if indent == 4 => {
                flush_ar(&mut track, &mut ar);
                let Some(t) = track.as_mut() else { continue };
                match key {
                    "Filename" => t.filename = unquote(value),
                    "Test CRC" => t.test_crc = unquote(value),
                    "Copy CRC" => t.copy_crc = unquote(value),
                    "Status" => t.status = unquote(value),
                    "AccurateRip v1" => ar = Some((1, String::new(), None)),
                    "AccurateRip v2" => ar = Some((2, String::new(), None)),
                    _ => {}
                }
            }
            "tracks" if indent >= 6 => {
                if let Some((_, result, conf)) = ar.as_mut() {
                    match key {
                        "Result" => *result = unquote(value),
                        "Confidence" => *conf = unquote(value).parse().ok(),
                        _ => {}
                    }
                }
            }
            "status" => match key {
                "AccurateRip summary" => log.accuraterip_summary = unquote(value),
                "Health status" => log.health = unquote(value),
                _ => {}
            },
            _ => {}
        }
    }
    flush_ar(&mut track, &mut ar);
    if let Some(t) = track.take() {
        log.tracks.push(t);
    }
    log
}

/// A cdrdao string: `"…"` with `\"`, `\\` and octal `\303\244` escapes.
fn cdrdao_string(s: &str) -> String {
    let s = s.trim();
    let inner = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s);
    let b = inner.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            let oct = &b[i + 1..(i + 4).min(b.len())];
            if oct.len() == 3 && oct.iter().all(|c| (b'0'..=b'7').contains(c)) {
                let v = oct.iter().fold(0u32, |acc, c| acc * 8 + u32::from(c - b'0'));
                out.push((v & 0xFF) as u8);
                i += 4;
            } else {
                out.push(b[i + 1]);
                i += 2;
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    // CD-Text is often Latin-1; take UTF-8 when it is valid.
    match String::from_utf8(out) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&c| c as char).collect(),
    }
}

/// CD-Text from cdrdao's `.toc` (the disc block, then one per `TRACK`).
pub fn parse_toc_cdtext(toc: &str) -> CdText {
    let mut text = CdText::default();
    // -1: disc level; n: nth track (0-based).
    let mut current: i32 = -1;
    for line in toc.lines() {
        let line = line.trim();
        if line.starts_with("TRACK ") {
            current += 1;
            continue;
        }
        let (field, rest) = match line.split_once(' ') {
            Some((f, r)) if f == "TITLE" || f == "PERFORMER" => (f, r),
            _ => continue,
        };
        let value = cdrdao_string(rest);
        if current < 0 {
            if field == "TITLE" {
                text.title = value;
            } else {
                text.performer = value;
            }
        } else {
            let i = current as usize;
            if text.tracks.len() <= i {
                text.tracks.resize(i + 1, Default::default());
            }
            if field == "TITLE" {
                text.tracks[i].0 = value;
            } else {
                text.tracks[i].1 = value;
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gu70n_offset_is_plus_48() {
        assert_eq!(table_offset("DVDRAM GU70N"), Some(48));
        assert_eq!(table_offset("  dvdram   gu70n "), Some(48));
        assert_eq!(table_offset("NO SUCH DRIVE 9000"), None);
        let conf = "[main]\nx = 1\n\n[drive:HL-DT-ST%20%3ADVDRAM%20GU70N%20%3A1.00]\nread_offset = 48\ndefeats_cache = True\n";
        assert_eq!(configured_offset(conf, "DVDRAM GU70N"), Some(48));
        assert_eq!(configured_offset(conf, "OTHER"), None);
        let drive =
            DriveInfo { device: "/dev/sr0".into(), vendor: "HL-DT-ST".into(), model: "DVDRAM GU70N".into() };
        assert_eq!(resolve_offset(Some(6), &drive), Some((6, OffsetSource::Setting)));
    }

    #[test]
    fn command_line() {
        let a = rip_args(Path::new("/dev/sr0"), 48, Path::new("/tmp/rip"), Some("abc"));
        assert_eq!(a[..4], ["cd", "-d", "/dev/sr0", "rip"]);
        assert!(a.contains(&"--offset=48".to_string()));
        assert!(a.contains(&"--unknown".to_string()) && a.contains(&"--cdr".to_string()));
        assert!(a.contains(&"--output-directory=/tmp/rip".to_string()));
        assert!(a.contains(&"--track-template=%t".to_string()));
        assert_eq!(a.last().unwrap(), "--release-id=abc");
        assert!(
            rip_args(Path::new("/dev/sr0"), -24, Path::new("/t"), None).contains(&"--offset=-24".to_string())
        );
    }

    #[test]
    fn output_lines() {
        assert_eq!(
            parse_line("Reading track 3 of 12  45 %   "),
            Line::Progress { stage: "Reading track 3 of 12".into(), track: Some((3, 12)), percent: 45 }
        );
        assert_eq!(
            parse_line("Reading table  12 %"),
            Line::Progress { stage: "Reading table".into(), track: None, percent: 12 }
        );
        assert_eq!(
            parse_line("INFO:whipper.command.cd:ripping track 3 of 12: 03.flac"),
            Line::Track { number: 3, total: 12 }
        );
        assert_eq!(
            parse_line("INFO:whipper.command.cd:ripping track 4 of 12 (try 2): 04.flac"),
            Line::Track { number: 4, total: 12 }
        );
        assert_eq!(
            parse_line("CRITICAL:whipper.command.cd:giving up on track 5 after 5 times"),
            Line::Critical("giving up on track 5 after 5 times".into())
        );
        assert!(matches!(parse_line("MusicBrainz disc id 7rEa"), Line::Other(_)));
    }

    pub const LOG: &str = r#"Log created by: whipper 0.10.0 (internal logger)
Log creation date: 2026-10-01T12:00:00Z

Ripping phase information:
  Drive: HL-DT-STDVDRAM GU70N (revision 1.00)
  Extraction engine: cdparanoia cdparanoia III 10.2 libcdio 2.0.0 x86_64-pc-linux-gnu
  Defeat audio cache: true
  Read offset correction: 48

Tracks:
  1:
    Filename: /tmp/rip/01.flac
    Peak level: 0.977142
    Pre-emphasis: false
    Extraction speed: 4.1 X
    Extraction quality: 100.00 %
    Test CRC: 1A2B3C4D
    Copy CRC: 1A2B3C4D
    AccurateRip v1:
      Result: Found, exact match
      Confidence: 12
      Local CRC: 11111111
      Remote CRC: 11111111
    AccurateRip v2:
      Result: Found, exact match
      Confidence: 8
      Local CRC: 22222222
      Remote CRC: 22222222
    Status: Copy OK
  2:
    Filename: '/tmp/rip/02.flac'
    Test CRC: 5A5A5A5A
    Copy CRC: 5A5A5A5A
    AccurateRip v1:
      Result: Found, NO exact match
      Confidence: 3
      Local CRC: 33333333
      Remote CRC: 44444444
    AccurateRip v2:
      Result: Track not present in AccurateRip database
    Status: Copy OK
  3:
    Filename: /tmp/rip/03.flac
    AccurateRip v1:
      Result: Track not present in AccurateRip database
    AccurateRip v2:
      Result: Track not present in AccurateRip database
    Status: Copy OK
  4:
    Filename: /tmp/rip/04.flac
    AccurateRip v1: {}
    AccurateRip v2: {}
    Status: Track not ripped (skipped)

Conclusive status report:
  AccurateRip summary: Some tracks could not be verified as accurate (3/4 got no match)
  Health status: Some tracks were not ripped (skipped)
  EOF: End of status report
"#;

    #[test]
    fn rip_log() {
        let log = parse_log(LOG);
        assert_eq!(log.tracks.len(), 4);
        let t = &log.tracks[0];
        assert_eq!((t.number, t.filename.as_str(), t.copy_crc.as_str()), (1, "/tmp/rip/01.flac", "1A2B3C4D"));
        assert_eq!(t.v1, Ar::Match { confidence: 12 });
        assert!(t.verified() && t.crc_ok());
        assert_eq!(log.tracks[1].filename, "/tmp/rip/02.flac");
        assert_eq!(log.tracks[1].v1, Ar::Mismatch);
        assert_eq!(log.tracks[2].v2, Ar::NotInDb);
        assert!(log.tracks[3].skipped());
        assert_eq!(
            log.summary(),
            ArSummary { tracks: 4, verified: 1, mismatched: 1, not_in_db: 1, skipped: 1 }
        );
        assert!(log.accuraterip_summary.starts_with("Some tracks"));
        assert_eq!(log.health, "Some tracks were not ripped (skipped)");
    }

    #[test]
    fn cdtext_from_toc() {
        let toc = r#"CD_DA

CD_TEXT {
  LANGUAGE_MAP {
    0 : EN
  }
  LANGUAGE 0 {
    TITLE "Mix \"Tape\""
    PERFORMER "Caf\303\251 Band"
  }
}

// Track 1
TRACK AUDIO
NO COPY
CD_TEXT {
  LANGUAGE 0 {
    TITLE "Intro"
    PERFORMER ""
  }
}
FILE "data.wav" 0 03:00:00

// Track 2
TRACK AUDIO
CD_TEXT {
  LANGUAGE 0 {
    TITLE "M\374de"
    PERFORMER "Guest"
  }
}
"#;
        let t = parse_toc_cdtext(toc);
        assert_eq!(t.title, "Mix \"Tape\"");
        assert_eq!(t.performer, "Café Band");
        assert_eq!(t.tracks, vec![("Intro".into(), String::new()), ("Müde".into(), "Guest".into())]);
        assert!(parse_toc_cdtext("CD_DA\nTRACK AUDIO\n").is_empty());
    }
}
