//! Rips a disc into a local folder as `NN.flac` plus whipper's log and TOC.
//! `Ripper::Whipper` runs the real thing; `Ripper::Mock` makes sine-wave
//! FLACs and a whipper-style log so the whole pipeline can be tested
//! without a drive.

use crate::whipper::{self, Ar, Line, RipLog};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone)]
pub struct RipRequest {
    pub device: PathBuf,
    pub offset: i32,
    /// Local, empty folder for whipper's output.
    pub out_dir: PathBuf,
    /// Everything whipper prints is copied here (Listen's logs dir).
    pub output_log: PathBuf,
    pub audio_tracks: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RipEvent {
    /// What whipper is doing ("Reading table", "Reading track 3 of 12", …).
    Stage(String),
    Track {
        number: u32,
        total: u32,
    },
    Percent(u32),
    Warning(String),
}

#[derive(Debug, Clone, Default)]
pub struct RipOutcome {
    /// (track number, file); track 0 is hidden track one audio.
    pub files: Vec<(u32, PathBuf)>,
    pub log: RipLog,
    pub log_file: Option<PathBuf>,
    pub toc_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RipError {
    #[error("cancelled")]
    Cancelled,
    #[error("{0} is not installed")]
    NotInstalled(String),
    #[error("{0}")]
    Failed(String),
}

/// A fake drive for tests.
#[derive(Debug, Clone)]
pub struct MockRipper {
    /// Length of each track in seconds.
    pub track_secs: Vec<f32>,
    /// AccurateRip result per track (v1); missing = exact match.
    pub accuraterip: Vec<Ar>,
    /// This track can't be read (skipped, like `--keep-going`).
    pub skip_track: Option<u32>,
    /// The disc "disappears" while reading this track.
    pub fail_track: Option<u32>,
    /// Pause per progress step (slow rips for cancel tests).
    pub step: Duration,
    /// cdrdao `.toc` text to leave behind (CD-Text).
    pub toc_text: Option<String>,
}

impl Default for MockRipper {
    fn default() -> Self {
        Self {
            track_secs: vec![3.0, 2.0, 4.0],
            accuraterip: Vec::new(),
            skip_track: None,
            fail_track: None,
            step: Duration::from_millis(5),
            toc_text: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Ripper {
    Whipper,
    Mock(MockRipper),
}

impl Ripper {
    pub async fn rip(
        &self,
        req: &RipRequest,
        events: &mpsc::UnboundedSender<RipEvent>,
        cancel: watch::Receiver<bool>,
    ) -> Result<RipOutcome, RipError> {
        std::fs::create_dir_all(&req.out_dir).map_err(|e| RipError::Failed(e.to_string()))?;
        match self {
            Ripper::Whipper => rip_whipper(req, events, cancel).await?,
            Ripper::Mock(m) => rip_mock(m, req, events, cancel).await?,
        }
        collect(&req.out_dir)
    }
}

/// `NN.flac`, `rip.log`, `rip.toc` in whipper's output folder.
fn collect(dir: &Path) -> Result<RipOutcome, RipError> {
    let mut out = RipOutcome::default();
    for entry in std::fs::read_dir(dir).map_err(|e| RipError::Failed(e.to_string()))?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(n) = name.strip_suffix(".flac").and_then(|n| n.parse::<u32>().ok()) {
            out.files.push((n, path));
        } else if name.ends_with(".log") {
            out.log = whipper::parse_log(&std::fs::read_to_string(&path).unwrap_or_default());
            out.log_file = Some(path);
        } else if name.ends_with(".toc") {
            out.toc_file = Some(path);
        }
    }
    out.files.sort();
    Ok(out)
}

fn stop_group(child: &tokio::process::Child, signal: i32) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // SAFETY: signalling our own child's process group (whipper and the
        // cd-paranoia/flac processes it starts).
        unsafe {
            libc::kill(-(pid as i32), signal);
        }
    }
    #[cfg(not(unix))]
    let _ = (child, signal);
}

async fn rip_whipper(
    req: &RipRequest,
    events: &mpsc::UnboundedSender<RipEvent>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), RipError> {
    let args = whipper::rip_args(&req.device, req.offset, &req.out_dir, None);
    tracing::info!("whipper {}", args.join(" "));
    let mut cmd = tokio::process::Command::new("whipper");
    cmd.args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("PYTHONUNBUFFERED", "1")
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => RipError::NotInstalled("whipper".into()),
        _ => RipError::Failed(format!("whipper: {e}")),
    })?;
    if let Some(dir) = req.output_log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut log = std::fs::File::create(&req.output_log).ok();
    if let Some(f) = log.as_mut() {
        let _ = writeln!(f, "$ whipper {}", args.join(" "));
    }
    let (line_tx, mut lines) = mpsc::unbounded_channel::<String>();
    for stream in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = line_tx.clone();
        tokio::spawn(async move {
            let mut stream = stream;
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
    }
    drop(line_tx);
    let mut criticals = Vec::new();
    let mut last_stage = String::new();
    let mut handle = |line: String, criticals: &mut Vec<String>| {
        let parsed = whipper::parse_line(&line);
        if let Some(f) = log.as_mut()
            && !matches!(parsed, Line::Progress { .. })
        {
            let _ = writeln!(f, "{}", line.trim_end());
        }
        match parsed {
            Line::Progress { stage, track, percent } => {
                if stage != last_stage {
                    last_stage = stage.clone();
                    if let Some(f) = log.as_mut() {
                        let _ = writeln!(f, "{stage}");
                    }
                    let _ = events.send(RipEvent::Stage(stage));
                }
                if let Some((number, total)) = track {
                    let _ = events.send(RipEvent::Track { number, total });
                }
                let _ = events.send(RipEvent::Percent(percent));
            }
            Line::Track { number, total } => {
                let _ = events.send(RipEvent::Track { number, total });
            }
            Line::Critical(m) => {
                tracing::warn!("whipper: {m}");
                criticals.push(m);
            }
            Line::Warning(m) => {
                tracing::info!("whipper warning: {m}");
                let _ = events.send(RipEvent::Warning(m));
            }
            Line::Other(_) => {}
        }
    };
    let status = loop {
        tokio::select! {
            line = lines.recv() => match line {
                Some(l) => handle(l, &mut criticals),
                None => break child.wait().await,
            },
            status = child.wait() => {
                while let Some(l) = lines.recv().await {
                    handle(l, &mut criticals);
                }
                break status;
            }
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    tracing::info!("stopping whipper");
                    stop_group(&child, libc_sigterm());
                    if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                        stop_group(&child, libc_sigkill());
                        let _ = child.kill().await;
                    }
                    return Err(RipError::Cancelled);
                }
            }
        }
    };
    let status = status.map_err(|e| RipError::Failed(e.to_string()))?;
    match status.code() {
        // 5: some tracks couldn't be read (--keep-going); the pipeline asks.
        Some(0) | Some(5) => Ok(()),
        code => Err(RipError::Failed(criticals.pop().unwrap_or_else(|| {
            format!("whipper stopped ({})", code.map_or("signal".to_string(), |c| format!("exit code {c}")))
        }))),
    }
}

#[cfg(unix)]
fn libc_sigterm() -> i32 {
    libc::SIGTERM
}
#[cfg(unix)]
fn libc_sigkill() -> i32 {
    libc::SIGKILL
}
#[cfg(not(unix))]
fn libc_sigterm() -> i32 {
    15
}
#[cfg(not(unix))]
fn libc_sigkill() -> i32 {
    9
}

/// 44.1 kHz 16-bit stereo sine WAV.
fn write_wav(path: &Path, secs: f32, freq: f32) -> std::io::Result<()> {
    let frames = (44_100.0 * secs) as u32;
    let data_len = frames * 4;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&44_100u32.to_le_bytes())?;
    f.write_all(&(44_100u32 * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for i in 0..frames {
        let t = i as f32 / 44_100.0;
        let s = ((t * freq * std::f32::consts::TAU).sin() * 12_000.0) as i16;
        f.write_all(&s.to_le_bytes())?;
        f.write_all(&s.to_le_bytes())?;
    }
    f.flush()
}

fn ar_yaml(ar: Ar) -> String {
    match ar {
        Ar::Match { confidence } => format!(
            "      Result: Found, exact match\n      Confidence: {confidence}\n      Local CRC: ABCDEF01\n      Remote CRC: ABCDEF01\n"
        ),
        Ar::Mismatch => "      Result: Found, NO exact match\n      Confidence: 4\n      Local CRC: 12345678\n      Remote CRC: 87654321\n".into(),
        Ar::NotInDb | Ar::Missing => "      Result: Track not present in AccurateRip database\n".into(),
    }
}

async fn rip_mock(
    m: &MockRipper,
    req: &RipRequest,
    events: &mpsc::UnboundedSender<RipEvent>,
    cancel: watch::Receiver<bool>,
) -> Result<(), RipError> {
    let total = m.track_secs.len() as u32;
    let _ = events.send(RipEvent::Stage("Reading table".into()));
    let mut log = String::from(
        "Log created by: whipper 0.10.0 (internal logger)\nLog creation date: 2026-10-01T12:00:00Z\n\nRipping phase information:\n  Drive: MOCK (revision 1)\n  Read offset correction: 48\n\nTracks:\n",
    );
    for (i, secs) in m.track_secs.iter().enumerate() {
        let n = i as u32 + 1;
        let _ = events.send(RipEvent::Track { number: n, total });
        for p in [0, 50, 100] {
            if *cancel.borrow() {
                return Err(RipError::Cancelled);
            }
            if m.fail_track == Some(n) && p == 50 {
                return Err(RipError::Failed("no CD detected, please insert one and retry".into()));
            }
            let _ = events.send(RipEvent::Percent(p));
            tokio::time::sleep(m.step).await;
        }
        let name = format!("{n:02}.flac");
        if m.skip_track == Some(n) {
            log.push_str(&format!(
                "  {n}:\n    Filename: {}\n    AccurateRip v1: {{}}\n    AccurateRip v2: {{}}\n    Status: Track not ripped (skipped)\n",
                req.out_dir.join(&name).display()
            ));
            continue;
        }
        let wav = req.out_dir.join(format!("{n:02}.wav"));
        let flac = req.out_dir.join(&name);
        write_wav(&wav, *secs, 220.0 * n as f32).map_err(|e| RipError::Failed(e.to_string()))?;
        let out = tokio::process::Command::new("flac")
            .args(["-5", "--silent", "--force", "-o"])
            .arg(&flac)
            .arg(&wav)
            .output()
            .await
            .map_err(|e| RipError::Failed(format!("flac: {e}")))?;
        let _ = std::fs::remove_file(&wav);
        if !out.status.success() {
            return Err(RipError::Failed(String::from_utf8_lossy(&out.stderr).into_owned()));
        }
        let ar = m.accuraterip.get(i).copied().unwrap_or(Ar::Match { confidence: 7 });
        log.push_str(&format!(
            "  {n}:\n    Filename: {}\n    Test CRC: 0000{n:04X}\n    Copy CRC: 0000{n:04X}\n    AccurateRip v1:\n{}    AccurateRip v2:\n{}    Status: Copy OK\n",
            flac.display(),
            ar_yaml(ar),
            ar_yaml(ar)
        ));
    }
    log.push_str("\nConclusive status report:\n  AccurateRip summary: (mock)\n  Health status: (mock)\n  EOF: End of status report\n");
    std::fs::write(req.out_dir.join("rip.log"), log).map_err(|e| RipError::Failed(e.to_string()))?;
    if let Some(t) = &m.toc_text {
        std::fs::write(req.out_dir.join("rip.toc"), t).map_err(|e| RipError::Failed(e.to_string()))?;
    }
    Ok(())
}
