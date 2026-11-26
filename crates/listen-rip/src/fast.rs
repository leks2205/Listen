//! The Fast rip mode: Listen reads the disc itself with `cd-paranoia -Z` (burst, one pass, the
//! drive at full speed, no table scan), checks every track against AccurateRip and leaves the same
//! things in the output folder as whipper does — `NN.flac`, a whipper-style `rip.log` and, when the
//! disc has CD-Text, a cdrdao-style `rip.toc` — so the pipeline can't tell the two apart.
//!
//! What it gives up against Secure: no second read and no comparison, so a read error that
//! cd-paranoia doesn't notice is only caught when AccurateRip knows the disc. A track cd-paranoia
//! can't read in burst mode is read once more with paranoia's checks on before it counts as skipped.

use crate::accuraterip::{self, DiscIds};
use crate::flac;
use crate::ripper::{MockRipper, RipError, RipEvent, RipRequest, stop_group, write_wav};
use crate::tags::CdText;
use crate::toc::Toc;
use crate::whipper::{self, Ar};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, watch};

/// Frames between an audio session's end and the next session (enhanced CDs).
const SESSION_GAP: u32 = 11_400;
const BYTES_PER_FRAME: u64 = 2352;
const WORDS_PER_FRAME: i64 = 1176;
const WAV_HEADER: u64 = 44;

#[derive(Debug, Clone)]
pub struct FastRipper {
    pub reader: Reader,
    pub accuraterip: ArSource,
}

impl FastRipper {
    /// The real drive, AccurateRip online.
    pub fn drive(http: reqwest::Client) -> Self {
        Self { reader: Reader::Drive, accuraterip: ArSource::Online(http) }
    }
}

/// Where the audio comes from.
#[derive(Debug, Clone)]
pub enum Reader {
    /// `cd-paranoia` on the request's device.
    Drive,
    /// Sine waves (tests): the mock's tracks, failures and CD-Text (from its `toc_text`).
    Mock(MockRipper),
}

/// Where the AccurateRip answer comes from.
#[derive(Debug, Clone)]
pub enum ArSource {
    Online(reqwest::Client),
    /// This `.bin` (tests); `None`: the disc isn't in the database.
    Given(Option<Vec<u8>>),
}

/// "HH:MM:SS.FF" (frames), cd-paranoia's span notation as whipper writes it.
fn hmsf(frames: u32) -> String {
    let f = frames % 75;
    let s = frames / 75 % 60;
    let m = frames / 75 / 60 % 60;
    let h = frames / 75 / 3600;
    format!("{h:02}:{m:02}:{s:02}.{f:02}")
}

/// (first frame, frame count) of audio track `number`: from its index 1 to the next track's
/// (minus the session gap before a data track), the last one to the lead-out — whipper's ranges.
pub fn track_span(toc: &Toc, number: u8) -> Option<(u32, u32)> {
    let i = toc.tracks.iter().position(|t| t.number == number && t.audio)?;
    let start = toc.tracks[i].lba;
    let end = match toc.tracks.get(i + 1) {
        Some(next) if !next.audio => next.lba.saturating_sub(SESSION_GAP),
        Some(next) => next.lba,
        None => toc.leadout_lba,
    };
    (end > start).then_some((start, end - start))
}

/// `cd-paranoia` for one track into `wav`: burst (`-Z`) or with paranoia's checks.
pub fn paranoia_args(
    device: &Path,
    offset: i32,
    number: u8,
    frames: u32,
    wav: &Path,
    burst: bool,
) -> Vec<String> {
    let mut a = Vec::new();
    if burst {
        a.push("-Z".to_string());
    }
    a.extend([
        "--stderr-progress".to_string(),
        format!("--sample-offset={offset}"),
        "-d".into(),
        device.to_string_lossy().into_owned(),
        format!("{number}[{}]-{number}[{}]", hmsf(0), hmsf(frames.saturating_sub(1))),
        wav.to_string_lossy().into_owned(),
    ]);
    a
}

/// A `--stderr-progress` line: `##: 0 [read] @ 24696` (offsets in 16-bit words, absolute).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Read(i64),
    Wrote(i64),
    /// `scsi_read error: …`
    Error,
}

pub fn parse_progress(line: &str) -> Option<Progress> {
    let line = line.trim();
    if line.starts_with("scsi_read error:") {
        return Some(Progress::Error);
    }
    let rest = line.strip_prefix("##:")?;
    let (_, rest) = rest.split_once('[')?;
    let (function, rest) = rest.split_once(']')?;
    let words: i64 = rest.trim().strip_prefix('@')?.trim().parse().ok()?;
    match function {
        "read" => Some(Progress::Read(words)),
        "wrote" => Some(Progress::Wrote(words)),
        _ => None,
    }
}

/// Percent of a track at `start` with `frames` frames once cd-paranoia wrote up to `words`.
pub fn percent_written(words: i64, start: u32, frames: u32) -> u32 {
    // cd-paranoia reports most writes one word short of a frame.
    let frame = (words + 1) / WORDS_PER_FRAME;
    let done = frame - i64::from(start);
    (done.clamp(0, i64::from(frames.max(1))) * 100 / i64::from(frames.max(1))) as u32
}

/// cdrdao's string notation: quotes and backslashes escaped, everything outside printable ASCII as
/// octal bytes of its UTF-8 (what [`whipper::parse_toc_cdtext`] reads back).
fn cdrdao_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for &b in s.as_bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push('"');
    out
}

/// CD-Text as a cdrdao `.toc` (the file whipper's table scan leaves; the pipeline reads it).
pub fn cdtext_toc(text: &CdText) -> String {
    let block = |title: &str, performer: &str| {
        format!(
            "CD_TEXT {{\n  LANGUAGE 0 {{\n    TITLE {}\n    PERFORMER {}\n  }}\n}}\n",
            cdrdao_quote(title),
            cdrdao_quote(performer)
        )
    };
    let mut out = String::from("CD_DA\n\n");
    out.push_str(&format!(
        "CD_TEXT {{\n  LANGUAGE_MAP {{\n    0 : EN\n  }}\n  LANGUAGE 0 {{\n    TITLE {}\n    PERFORMER {}\n  }}\n}}\n",
        cdrdao_quote(&text.title),
        cdrdao_quote(&text.performer)
    ));
    for (i, (title, performer)) in text.tracks.iter().enumerate() {
        out.push_str(&format!("\n// Track {}\nTRACK AUDIO\n", i + 1));
        out.push_str(&block(title, performer));
    }
    out
}

/// CRC-32 (IEEE) table.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

/// Continues a CRC-32 (start with 0).
fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    !data.iter().fold(!crc, |c, &b| CRC_TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8))
}

/// CRC-32 (IEEE) of the track's audio: whipper's "Copy CRC".
#[cfg(test)]
fn crc32(data: &[u8]) -> u32 {
    crc32_update(0, data)
}

/// (audio bytes, copy CRC, AccurateRip v1, v2) of a ripped WAV, read in pieces.
fn checksum_wav(wav: &Path, number: u32, total: u32) -> std::io::Result<(u64, u32, u32, u32)> {
    use std::io::Read as _;
    let mut f = std::fs::File::open(wav)?;
    let len = f.metadata()?.len().saturating_sub(WAV_HEADER);
    let mut header = [0u8; WAV_HEADER as usize];
    f.read_exact(&mut header)?;
    let mut ar = accuraterip::Checksum::new(number, total, (len / 4) as u32);
    let mut crc = 0u32;
    let mut buf = vec![0u8; 1 << 20];
    let mut carry = 0usize;
    loop {
        let n = f.read(&mut buf[carry..])?;
        if n == 0 {
            break;
        }
        let have = carry + n;
        let whole = have / 4 * 4;
        crc = crc32_update(crc, &buf[..whole]);
        ar.update(&buf[..whole]);
        buf.copy_within(whole..have, 0);
        carry = have - whole;
    }
    crc = crc32_update(crc, &buf[..carry]);
    let (v1, v2) = ar.finish();
    Ok((len, crc, v1, v2))
}

/// One track's result, for the log.
#[derive(Debug, Clone, Default)]
struct Ripped {
    number: u32,
    flac: PathBuf,
    /// `None`: unreadable (skipped).
    crcs: Option<(u32, u32, u32)>,
    speed: f32,
}

/// The YAML block of one AccurateRip version.
fn ar_block(out: &mut String, version: u8, ar: Ar, local: u32, remote: Option<u32>) {
    out.push_str(&format!("    AccurateRip v{version}:\n"));
    let (result, confidence) = match ar {
        Ar::Match { confidence } => ("Found, exact match", Some(confidence)),
        Ar::Mismatch => ("Found, NO exact match", None),
        Ar::NotInDb => ("Track not present in AccurateRip database", None),
        Ar::Missing => ("AccurateRip database could not be reached", None),
    };
    out.push_str(&format!("      Result: {result}\n"));
    if let Some(c) = confidence {
        out.push_str(&format!("      Confidence: {c}\n"));
    }
    out.push_str(&format!("      Local CRC: {local:08X}\n"));
    if let Some(r) = remote {
        out.push_str(&format!("      Remote CRC: {r:08X}\n"));
    }
}

/// The rip log in whipper's layout (so [`whipper::parse_log`] reads it).
fn write_log(
    drive: &str,
    offset: i32,
    speed: Option<u32>,
    tracks: &[Ripped],
    responses: Option<&[accuraterip::Response]>,
) -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (y, mo, d, h, mi, s) = crate::pipeline::civil(secs);
    let mut out = format!(
        "Log created by: Listen {} (Fast mode)\nLog creation date: {y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z\n\n\
         Ripping phase information:\n  Drive: {drive}\n  Extraction engine: cd-paranoia, burst mode (one read)\n  \
         Read offset correction: {offset}\n  Drive speed: {}\n\nTracks:\n",
        env!("CARGO_PKG_VERSION"),
        speed.map_or("unchanged".to_string(), |s| format!("{s}x")),
    );
    let mut verified = 0;
    let mut skipped = 0;
    for (i, t) in tracks.iter().enumerate() {
        out.push_str(&format!("  {}:\n    Filename: {}\n", t.number, t.flac.display()));
        let Some((copy, v1, v2)) = t.crcs else {
            skipped += 1;
            out.push_str(
                "    AccurateRip v1: {}\n    AccurateRip v2: {}\n    Status: Track not ripped (skipped)\n",
            );
            continue;
        };
        out.push_str(&format!("    Extraction speed: {:.1} X\n    Copy CRC: {copy:08X}\n", t.speed));
        let (a1, a2) = match responses {
            Some(r) => accuraterip::verify(r, i, v1, v2),
            None => (Ar::Missing, Ar::Missing),
        };
        // The database's checksum shown next to ours: the match, else the most confident entry.
        let remote = |local: u32, ar: Ar| -> Option<u32> {
            let entries = responses?.iter().filter_map(|r| r.tracks.get(i).copied());
            match ar {
                Ar::Match { .. } => Some(local),
                Ar::Mismatch => entries.max_by_key(|(c, _)| *c).map(|(_, crc)| crc),
                _ => None,
            }
        };
        if matches!(a1, Ar::Match { .. }) || matches!(a2, Ar::Match { .. }) {
            verified += 1;
        }
        ar_block(&mut out, 1, a1, v1, remote(v1, a1));
        ar_block(&mut out, 2, a2, v2, remote(v2, a2));
        out.push_str("    Status: Copy finished (one read)\n");
    }
    let total = tracks.len();
    let summary = if verified == total {
        "All tracks accurately ripped".to_string()
    } else {
        format!("Some tracks could not be verified as accurate ({}/{total} got no match)", total - verified)
    };
    let health = if skipped == 0 {
        "No errors occurred".to_string()
    } else {
        "Some tracks were not ripped (skipped)".into()
    };
    out.push_str(&format!(
        "\nConclusive status report:\n  AccurateRip summary: {summary}\n  Health status: {health}\n  EOF: End of status report\n"
    ));
    out
}

/// What reading one track came to.
enum Read {
    Ok,
    /// cd-paranoia couldn't read it (why).
    Unreadable(String),
}

/// Lines of a child's stderr, split at `\n` and `\r` (cd-paranoia redraws its progress with `\r`).
fn lines_of(
    mut stream: impl tokio::io::AsyncRead + Unpin + Send + 'static,
) -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        let mut pending = Vec::new();
        while let Ok(n) = stream.read(&mut buf).await {
            if n == 0 {
                break;
            }
            for &b in &buf[..n] {
                if b == b'\n' || b == b'\r' {
                    if !pending.is_empty() {
                        let _ = tx.send(String::from_utf8_lossy(&pending).into_owned());
                        pending.clear();
                    }
                } else {
                    pending.push(b);
                }
            }
        }
        if !pending.is_empty() {
            let _ = tx.send(String::from_utf8_lossy(&pending).into_owned());
        }
    });
    rx
}

/// Sends the percentage when it went up.
fn report_percent(events: &mpsc::UnboundedSender<RipEvent>, p: u32, percent: &mut Option<u32>) {
    let p = p.min(100);
    if percent.is_none_or(|old| p > old) {
        *percent = Some(p);
        let _ = events.send(RipEvent::Percent(p));
    }
}

/// Reads one track with cd-paranoia into `wav`.
#[allow(clippy::too_many_arguments)]
async fn read_with_paranoia(
    req: &RipRequest,
    number: u8,
    (start, frames): (u32, u32),
    wav: &Path,
    burst: bool,
    events: &mpsc::UnboundedSender<RipEvent>,
    cancel: &mut watch::Receiver<bool>,
    log: &mut Option<std::fs::File>,
) -> Result<Read, RipError> {
    let args = paranoia_args(&req.device, req.offset, number, frames, wav, burst);
    tracing::info!("cd-paranoia {}", args.join(" "));
    if let Some(f) = log.as_mut() {
        let _ = writeln!(f, "$ cd-paranoia {}", args.join(" "));
    }
    let mut cmd = tokio::process::Command::new("cd-paranoia");
    cmd.args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => RipError::NotInstalled("cd-paranoia".into()),
        _ => RipError::Failed(format!("cd-paranoia: {e}")),
    })?;
    let mut lines = match child.stderr.take() {
        Some(s) => lines_of(s),
        None => mpsc::unbounded_channel().1,
    };
    let mut percent = None;
    let mut errors = 0u32;
    let mut said: Vec<String> = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    let expected = u64::from(frames) * BYTES_PER_FRAME;
    let report = |p: u32, percent: &mut Option<u32>| report_percent(events, p, percent);
    let mut handle = |line: String, percent: &mut Option<u32>, errors: &mut u32, said: &mut Vec<String>| {
        match parse_progress(&line) {
            Some(Progress::Wrote(words)) => report(percent_written(words, start, frames), percent),
            Some(Progress::Read(_)) => {}
            Some(Progress::Error) => {
                *errors += 1;
                if let Some(f) = log.as_mut() {
                    let _ = writeln!(f, "{}", line.trim_end());
                }
            }
            None => {
                if let Some(f) = log.as_mut() {
                    let _ = writeln!(f, "{}", line.trim_end());
                }
                said.push(line.trim().to_string());
            }
        }
    };
    report(0, &mut percent);
    let status = loop {
        tokio::select! {
            line = lines.recv() => match line {
                Some(l) => handle(l, &mut percent, &mut errors, &mut said),
                None => break child.wait().await,
            },
            status = child.wait() => {
                while let Some(l) = lines.recv().await {
                    handle(l, &mut percent, &mut errors, &mut said);
                }
                break status;
            }
            _ = tick.tick() => {
                // Also from the file's size, in case a cd-paranoia build prints no [wrote] lines.
                if let Ok(m) = std::fs::metadata(wav) && expected > 0 {
                    report_percent(events, (m.len().saturating_sub(WAV_HEADER) * 100 / expected) as u32, &mut percent);
                }
            }
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    tracing::info!("stopping cd-paranoia");
                    stop_group(&child, crate::ripper::libc_sigterm());
                    if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                        stop_group(&child, crate::ripper::libc_sigkill());
                        let _ = child.kill().await;
                    }
                    return Err(RipError::Cancelled);
                }
            }
        }
    };
    let status = status.map_err(|e| RipError::Failed(e.to_string()))?;
    let size = std::fs::metadata(wav).map_or(0, |m| m.len());
    if status.success() && size == expected + WAV_HEADER {
        if errors > 0 {
            tracing::info!("track {number}: {errors} read error(s) reported, read anyway");
        }
        return Ok(Read::Ok);
    }
    let why = said.iter().rev().find(|l| !l.is_empty()).cloned().unwrap_or_else(|| {
        if status.success() {
            format!("{size} bytes instead of {}", expected + WAV_HEADER)
        } else {
            format!(
                "cd-paranoia stopped ({})",
                status.code().map_or("signal".into(), |c| format!("exit code {c}"))
            )
        }
    });
    Ok(Read::Unreadable(why))
}

/// The mock drive's version of [`read_with_paranoia`].
async fn read_mock(
    m: &MockRipper,
    number: u32,
    wav: &Path,
    events: &mpsc::UnboundedSender<RipEvent>,
    cancel: &watch::Receiver<bool>,
) -> Result<Read, RipError> {
    for p in [0, 50, 100] {
        if *cancel.borrow() {
            return Err(RipError::Cancelled);
        }
        if m.fail_track == Some(number) && p == 50 {
            return Err(RipError::Failed("no CD detected, please insert one and retry".into()));
        }
        if m.skip_track == Some(number) && p == 50 {
            return Ok(Read::Unreadable("mock: unreadable".into()));
        }
        let _ = events.send(RipEvent::Percent(p));
        tokio::time::sleep(m.step).await;
    }
    let secs = m.track_secs.get(number as usize - 1).copied().unwrap_or(1.0);
    write_wav(wav, secs, MockRipper::freq(number)).map_err(|e| RipError::Failed(e.to_string()))?;
    Ok(Read::Ok)
}

pub(crate) async fn rip_fast(
    fast: &FastRipper,
    req: &RipRequest,
    events: &mpsc::UnboundedSender<RipEvent>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), RipError> {
    let ids = DiscIds::from_toc(&req.toc);
    // The AccurateRip answer while the disc is read.
    let answer = match &fast.accuraterip {
        ArSource::Online(http) => {
            let http = http.clone();
            tokio::spawn(async move { accuraterip::fetch(&http, &ids).await })
        }
        ArSource::Given(bin) => {
            let bin = bin.clone();
            tokio::spawn(async move { Ok::<_, String>(bin) })
        }
    };
    if let Some(dir) = req.output_log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut log = std::fs::File::create(&req.output_log).ok();
    if let Some(f) = log.as_mut() {
        let _ = writeln!(f, "Listen Fast mode, read offset {:+}, AccurateRip {}", req.offset, ids.url());
    }

    let (drive, speed, text, numbers): (String, Option<u32>, Option<CdText>, Vec<u32>) = match &fast.reader {
        Reader::Drive => {
            let _ = events.send(RipEvent::Stage("Reading the disc".into()));
            let speed = crate::drive::full_speed(&req.device).await;
            let device = req.device.clone();
            let text =
                tokio::task::spawn_blocking(move || crate::drive::read_cdtext(&device)).await.ok().flatten();
            let info = crate::drive::info(&req.device);
            (info.label(), speed, text, req.toc.audio_tracks().map(|t| u32::from(t.number)).collect())
        }
        Reader::Mock(m) => (
            "MOCK (revision 1)".into(),
            Some(24),
            m.toc_text.as_deref().map(whipper::parse_toc_cdtext).filter(|t| !t.is_empty()),
            (1..=m.track_secs.len() as u32).collect(),
        ),
    };
    if let Some(f) = log.as_mut() {
        let _ = writeln!(
            f,
            "drive {drive}, speed {}, CD-Text {}",
            speed.map_or("unchanged".into(), |s| format!("{s}x")),
            if text.is_some() { "found" } else { "none" }
        );
    }
    if let Some(t) = &text {
        std::fs::write(req.out_dir.join("rip.toc"), cdtext_toc(t))
            .map_err(|e| RipError::Failed(e.to_string()))?;
    }

    let total = numbers.len() as u32;
    let mut ripped = Vec::new();
    let mut encodes = Vec::new();
    for (i, &number) in numbers.iter().enumerate() {
        if *cancel.borrow() {
            encodes.iter().for_each(tokio::task::JoinHandle::abort);
            return Err(RipError::Cancelled);
        }
        let _ = events.send(RipEvent::Stage(format!("Reading track {} of {total}", i + 1)));
        let _ = events.send(RipEvent::Track { number: i as u32 + 1, total });
        let wav = req.out_dir.join(format!("{number:02}.wav"));
        let flac_path = req.out_dir.join(format!("{number:02}.flac"));
        let began = Instant::now();
        let read = match &fast.reader {
            Reader::Mock(m) => read_mock(m, number, &wav, events, &cancel).await,
            Reader::Drive => {
                let Some(span) = track_span(&req.toc, number as u8) else {
                    return Err(RipError::Failed(format!("track {number} is not in the table of contents")));
                };
                let mut r =
                    read_with_paranoia(req, number as u8, span, &wav, true, events, &mut cancel, &mut log)
                        .await;
                if let Ok(Read::Unreadable(why)) = &r {
                    if crate::drive::disc_state(&req.device) != crate::drive::DiscState::Audio {
                        return Err(RipError::Failed("no CD detected, please insert one and retry".into()));
                    }
                    let w = format!("track {number}: {why}; reading it again with paranoia");
                    tracing::warn!("{w}");
                    let _ = events.send(RipEvent::Warning(w));
                    r = read_with_paranoia(
                        req,
                        number as u8,
                        span,
                        &wav,
                        false,
                        events,
                        &mut cancel,
                        &mut log,
                    )
                    .await;
                }
                r
            }
        };
        let read = match read {
            Ok(r) => r,
            Err(e) => {
                encodes.iter().for_each(tokio::task::JoinHandle::abort);
                return Err(e);
            }
        };
        match read {
            Read::Ok => {
                let elapsed = began.elapsed().as_secs_f32();
                let (path, position) = (wav.clone(), i as u32 + 1);
                let (bytes, copy, v1, v2) =
                    tokio::task::spawn_blocking(move || checksum_wav(&path, position, total))
                        .await
                        .map_err(|e| RipError::Failed(e.to_string()))?
                        .map_err(|e| RipError::Failed(format!("{}: {e}", wav.display())))?;
                let secs = bytes as f32 / 176_400.0;
                ripped.push(Ripped {
                    number,
                    flac: flac_path.clone(),
                    crcs: Some((copy, v1, v2)),
                    speed: secs / elapsed.max(0.001),
                });
                // Encode while the next track is read.
                encodes.push(tokio::spawn(async move {
                    let r = flac::encode(&wav, &flac_path).await;
                    let _ = std::fs::remove_file(&wav);
                    r
                }));
            }
            Read::Unreadable(why) => {
                let _ = std::fs::remove_file(&wav);
                tracing::warn!("track {number} not ripped: {why}");
                if let Some(f) = log.as_mut() {
                    let _ = writeln!(f, "track {number} not ripped: {why}");
                }
                ripped.push(Ripped { number, flac: flac_path, crcs: None, speed: 0.0 });
            }
        }
    }
    for e in encodes {
        match e.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(RipError::Failed(e)),
            Err(e) => return Err(RipError::Failed(e.to_string())),
        }
    }

    let responses = match tokio::time::timeout(Duration::from_secs(30), answer).await {
        Ok(Ok(Ok(Some(bin)))) => Some(accuraterip::parse(&bin)),
        Ok(Ok(Ok(None))) => Some(Vec::new()),
        Ok(Ok(Err(e))) => {
            tracing::warn!("{e}");
            None
        }
        _ => {
            tracing::warn!("AccurateRip didn't answer in time");
            None
        }
    };
    let text = write_log(&drive, req.offset, speed, &ripped, responses.as_deref());
    if let Some(f) = log.as_mut() {
        let _ = f.write_all(text.as_bytes());
    }
    std::fs::write(req.out_dir.join("rip.log"), text).map_err(|e| RipError::Failed(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revive() -> Toc {
        Toc::from_offsets(
            1,
            154_800,
            &[150, 14625, 27975, 40575, 53175, 66750, 79425, 92325, 105150, 117975, 130200, 139500],
        )
    }

    #[test]
    fn spans_like_whipper() {
        let toc = revive();
        assert_eq!(track_span(&toc, 1), Some((0, 14_475)));
        assert_eq!(track_span(&toc, 12), Some((139_350, 154_800 - 139_500)));
        assert_eq!(track_span(&toc, 13), None);
        // Enhanced CD: the last audio track stops before the session gap.
        let mut enhanced = revive();
        enhanced.tracks.push(crate::toc::TocTrack { number: 13, lba: 170_000, audio: false });
        enhanced.leadout_lba = 200_000;
        assert_eq!(track_span(&enhanced, 12), Some((139_350, 170_000 - 11_400 - 139_350)));
        assert_eq!(track_span(&enhanced, 13), None);
        assert_eq!(hmsf(14_474), "00:03:12.74");
        assert_eq!(hmsf(75 * 3600 + 1), "01:00:00.01");
    }

    #[test]
    fn command_line() {
        let a = paranoia_args(Path::new("/dev/sr0"), 48, 3, 14_475, Path::new("/tmp/r/03.wav"), true);
        assert_eq!(
            a,
            [
                "-Z",
                "--stderr-progress",
                "--sample-offset=48",
                "-d",
                "/dev/sr0",
                "3[00:00:00.00]-3[00:03:12.74]",
                "/tmp/r/03.wav"
            ]
        );
        let secure = paranoia_args(Path::new("/dev/sr0"), -6, 1, 100, Path::new("w.wav"), false);
        assert_eq!(secure[0], "--stderr-progress");
        assert!(secure.contains(&"--sample-offset=-6".to_string()));
    }

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress("##: 0 [read] @ 24696"), Some(Progress::Read(24_696)));
        assert_eq!(parse_progress("##: -2 [wrote] @ 1175"), Some(Progress::Wrote(1175)));
        assert_eq!(parse_progress("scsi_read error: sector=1234"), Some(Progress::Error));
        assert_eq!(parse_progress("##: 1 [verify] @ 100"), None);
        assert_eq!(parse_progress("Ripping from sector 0 (track 1 [0:00.00])"), None);
        // Track at frame 1000, 200 frames long.
        assert_eq!(percent_written(1000 * 1176 - 1, 1000, 200), 0);
        assert_eq!(percent_written(1100 * 1176 - 1, 1000, 200), 50);
        assert_eq!(percent_written(1300 * 1176, 1000, 200), 100);
        assert_eq!(percent_written(0, 1000, 200), 0);
    }

    #[test]
    fn cdtext_round_trips_through_the_toc_file() {
        let text = CdText {
            title: "Mix \"Tape\" \\ 1".into(),
            performer: "Café Band".into(),
            tracks: vec![("Intro".into(), String::new()), ("Müde – 夜".into(), "Guest".into())],
        };
        assert_eq!(whipper::parse_toc_cdtext(&cdtext_toc(&text)), text);
    }

    #[test]
    fn crc32_is_ieee() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32_update(crc32(b"12345"), b"6789"), 0xCBF4_3926);
    }

    #[test]
    fn wav_checksums_in_pieces_match_the_whole() {
        let dir = std::env::temp_dir().join(format!("listen-fast-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("01.wav");
        // A little over 1 MiB, so the file is read in several pieces.
        write_wav(&wav, 6.1, 440.0).unwrap();
        let bytes = std::fs::read(&wav).unwrap();
        let audio = &bytes[WAV_HEADER as usize..];
        let (v1, v2) = accuraterip::checksums(audio, 2, 3);
        assert_eq!(checksum_wav(&wav, 2, 3).unwrap(), (audio.len() as u64, crc32(audio), v1, v2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_reads_back_like_whippers() {
        let responses = vec![accuraterip::Response {
            ids: DiscIds { tracks: 3, id1: 1, id2: 2, cddb: 3 },
            tracks: vec![(7, 0x11), (5, 0x99), (4, 0x33)],
        }];
        let tracks = vec![
            Ripped { number: 1, flac: "/t/01.flac".into(), crcs: Some((0xAB, 0x11, 0x12)), speed: 3.8 },
            Ripped { number: 2, flac: "/t/02.flac".into(), crcs: Some((0xCD, 0x21, 0x22)), speed: 3.9 },
            Ripped { number: 3, flac: "/t/03.flac".into(), crcs: None, speed: 0.0 },
        ];
        let log =
            whipper::parse_log(&write_log("HL-DT-ST DVDRAM GU70N", 48, Some(24), &tracks, Some(&responses)));
        assert_eq!(log.tracks.len(), 3);
        assert_eq!(log.tracks[0].v1, Ar::Match { confidence: 7 });
        assert_eq!(log.tracks[0].copy_crc, "000000AB");
        assert_eq!((log.tracks[1].v1, log.tracks[1].v2), (Ar::Mismatch, Ar::Mismatch));
        assert!(log.tracks[2].skipped());
        let s = log.summary();
        assert_eq!((s.tracks, s.verified, s.mismatched, s.skipped), (3, 1, 1, 1));
        assert_eq!(log.health, "Some tracks were not ripped (skipped)");

        // Unknown disc, and the database not reachable: nothing verified, nothing "mismatched".
        let unknown = whipper::parse_log(&write_log("D", 0, None, &tracks[..2], Some(&[])));
        assert_eq!(unknown.summary().not_in_db, 2);
        let offline = whipper::parse_log(&write_log("D", 0, None, &tracks[..2], None));
        assert_eq!((offline.summary().not_in_db, offline.summary().mismatched), (2, 0));
        assert_eq!(offline.tracks[0].v1, Ar::Missing);
    }
}
