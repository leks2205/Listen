//! Decoding with the installed `mpv`, so beat times match playback: mpv compensates MP3/AAC
//! encoder delay and padding, and a 20–50 ms offset ruins a beat-matched mix.

use crate::{AnalysisError, Cancel};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

/// Analysis sample rate in Hz; beat-this and bliss both expect 22050.
pub const SAMPLE_RATE: u32 = 22050;
/// mpv gives up on a stalled or endless stream after this.
pub const DECODE_TIMEOUT: Duration = Duration::from_secs(120);
/// Longer inputs are not analysed (30 minutes is ~320 MB of float PCM).
pub const MAX_SECONDS: u32 = 30 * 60;

/// Decoded audio, 22050 Hz stereo float.
#[derive(Debug, Clone, Default)]
pub struct Pcm {
    pub sample_rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl Pcm {
    pub fn duration(&self) -> f32 {
        self.left.len() as f32 / self.sample_rate.max(1) as f32
    }

    /// (L + R) / 2: what the beat models and bliss expect.
    pub fn mono(&self) -> Vec<f32> {
        self.left.iter().zip(&self.right).map(|(l, r)| 0.5 * (l + r)).collect()
    }

    /// Splits interleaved stereo samples.
    pub fn from_interleaved(sample_rate: u32, samples: &[f32]) -> Self {
        let n = samples.len() / 2;
        let (mut left, mut right) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for pair in samples.as_chunks::<2>().0 {
            left.push(pair[0]);
            right.push(pair[1]);
        }
        Pcm { sample_rate, left, right }
    }
}

/// "mpv 0.41.0" (cached; "mpv" when the version can't be read).
pub fn mpv_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let mut cmd = std::process::Command::new(listen_core::platform::tool("mpv"));
        listen_core::platform::no_window(&mut cmd)
            .arg("--version")
            .stderr(Stdio::null())
            .output()
            .ok()
            .and_then(|o| {
                let text = String::from_utf8_lossy(&o.stdout).into_owned();
                let first =
                    text.lines().next()?.split_whitespace().nth(1)?.trim_start_matches('v').to_string();
                Some(format!("mpv {first}"))
            })
            .unwrap_or_else(|| "mpv".into())
    })
}

#[cfg(unix)]
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// Where mpv writes the PCM stream: its own stdout on Linux (`/dev/stdout`).
#[cfg(unix)]
const PCM_OUT: &str = "/dev/stdout";

/// The mpv command line, under `nice`/`ionice` when available so a crawl never competes with playback.
fn command(input: &str, pcm_out: &str) -> Command {
    let mut argv: Vec<String> = Vec::new();
    #[cfg(unix)]
    {
        if on_path("nice") {
            argv.extend(["nice", "-n", "19"].map(String::from));
        }
        if on_path("ionice") {
            argv.extend(["ionice", "-c3"].map(String::from));
        }
    }
    argv.push(listen_core::platform::tool("mpv").to_string_lossy().into_owned());
    argv.extend(
        [
            "--no-config",
            "--load-scripts=no",
            "--ytdl=no",
            "--no-video",
            "--really-quiet",
            "--network-timeout=30",
            "--ao=pcm",
            "--ao-pcm-waveheader=no",
            "--audio-format=float",
            "--audio-channels=stereo",
            "--af=",
        ]
        .map(String::from),
    );
    if cfg!(windows) {
        // Not a player: no media session of its own (Discord would show it).
        argv.push("--media-controls=no".into());
    }
    argv.push(format!("--ao-pcm-file={pcm_out}"));
    argv.push(format!("--audio-samplerate={SAMPLE_RATE}"));
    argv.extend(["--".to_string(), input.to_string()]);
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(listen_core::platform::CREATE_NO_WINDOW);
    cmd
}

/// Starts mpv; returns its PCM stream and stderr. On Windows (no `/dev/stdout`) the PCM comes
/// through a named pipe this process serves.
#[cfg(unix)]
async fn start(input: &str) -> std::io::Result<(tokio::process::Child, Box<dyn AsyncRead + Unpin + Send>)> {
    let mut child = command(input, PCM_OUT).spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    Ok((child, Box::new(stdout)))
}

#[cfg(windows)]
async fn start(input: &str) -> std::io::Result<(tokio::process::Child, Box<dyn AsyncRead + Unpin + Send>)> {
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::net::windows::named_pipe::ServerOptions;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name =
        format!(r"\\.\pipe\listen-decode-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    let server = ServerOptions::new().first_pipe_instance(true).access_inbound(true).create(&name)?;
    let mut cmd = command(input, &name);
    cmd.stdout(Stdio::null());
    let child = cmd.spawn()?;
    // A failed start (bad URL) never connects; the caller's timeout and exit code report it.
    let connected = tokio::time::timeout(Duration::from_secs(30), server.connect()).await;
    match connected {
        Ok(Ok(())) => Ok((child, Box::new(server))),
        Ok(Err(e)) => Err(e),
        // Never connected: hand over the pipe anyway; it reads EOF once mpv is gone.
        Err(_) => Ok((child, Box::new(server))),
    }
}

/// Decodes a URL or local path to 22050 Hz stereo float. Cancelling or dropping the future kills mpv.
pub async fn decode(input: &str, cancel: &Cancel) -> Result<Pcm, AnalysisError> {
    let (mut child, mut stdout) = start(input)
        .await
        .map_err(|e| AnalysisError::Decode { reason: format!("cannot start mpv: {e}"), transient: false })?;
    let mut stderr = child.stderr.take().expect("piped stderr");
    let max_frames = MAX_SECONDS as usize * SAMPLE_RATE as usize;

    // Read straight into the two channel buffers, without a raw or interleaved copy.
    let read = async {
        let cap = 4 * 60 * SAMPLE_RATE as usize;
        let (mut left, mut right) = (Vec::<f32>::with_capacity(cap), Vec::<f32>::with_capacity(cap));
        let mut buf = vec![0u8; 1 << 16];
        // Bytes of a frame (8: L + R) cut by a read boundary.
        let mut carry = 0usize;
        loop {
            let n = stdout.read(&mut buf[carry..]).await?;
            if n == 0 {
                return Ok::<_, std::io::Error>(Some((left, right)));
            }
            let filled = carry + n;
            let whole = filled / 8 * 8;
            for frame in buf[..whole].as_chunks::<8>().0 {
                left.push(f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]));
                right.push(f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]));
            }
            buf.copy_within(whole..filled, 0);
            carry = filled - whole;
            if left.len() > max_frames {
                return Ok(None);
            }
        }
    };
    let (left, right) = tokio::select! {
        r = tokio::time::timeout(DECODE_TIMEOUT, read) => match r {
            Err(_) => return Err(AnalysisError::Decode { reason: "mpv timed out".into(), transient: true }),
            Ok(Err(e)) => {
                return Err(AnalysisError::Decode { reason: format!("reading mpv output: {e}"), transient: true });
            }
            Ok(Ok(None)) => return Err(AnalysisError::TooLong),
            Ok(Ok(Some(b))) => b,
        },
        () = cancel.cancelled() => return Err(AnalysisError::Cancelled),
    };
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .map_err(|_| AnalysisError::Decode { reason: "mpv did not exit".into(), transient: true })?
        .map_err(|e| AnalysisError::Decode { reason: e.to_string(), transient: true })?;
    if !status.success() || left.is_empty() {
        let mut err = String::new();
        let _ = stderr.read_to_string(&mut err).await;
        let err = err.lines().last().unwrap_or("").trim().to_string();
        // Exit code 2: unplayable (network, 404, bad file); transient unless mpv ran and produced nothing.
        return Err(AnalysisError::Decode {
            reason: if err.is_empty() { format!("mpv failed ({status})") } else { err },
            transient: !status.success(),
        });
    }
    Ok(Pcm { sample_rate: SAMPLE_RATE, left, right })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mpv_available() -> bool {
        std::process::Command::new(listen_core::platform::tool("mpv")).arg("--version").output().is_ok()
    }

    #[test]
    fn deinterleaves() {
        let p = Pcm::from_interleaved(22050, &[1.0, -1.0, 0.5, 0.25, 9.0]);
        assert_eq!(p.left, [1.0, 0.5]);
        assert_eq!(p.right, [-1.0, 0.25]);
        assert_eq!(p.mono(), [0.0, 0.375]);
    }

    /// Left and right stay apart across read boundaries that split frames.
    #[tokio::test]
    async fn keeps_channels_apart_across_read_boundaries() {
        if !mpv_available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("listen-analysis-stereo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stereo.wav");
        let sr = SAMPLE_RATE;
        let n = sr as usize * 5;
        let mut b = Vec::new();
        let data_len = (n * 4) as u32;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&sr.to_le_bytes());
        b.extend_from_slice(&(sr * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..n {
            let l = (0.5 * (i as f32 * 0.05).sin() * 30000.0) as i16;
            b.extend_from_slice(&l.to_le_bytes());
            b.extend_from_slice(&0i16.to_le_bytes());
        }
        std::fs::write(&path, b).unwrap();
        let pcm = decode(path.to_str().unwrap(), &Cancel::default()).await.unwrap();
        assert_eq!(pcm.left.len(), pcm.right.len());
        assert!(pcm.right.iter().all(|x| x.abs() < 1e-6), "right channel must stay silent");
        assert!(pcm.left.iter().fold(0f32, |m, x| m.max(x.abs())) > 0.3);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Needs mpv; skipped quietly where it isn't installed.
    #[tokio::test]
    async fn decodes_a_wav_with_mpv() {
        if !mpv_available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("listen-analysis-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        crate::test_util::write_wav(&path, 44100, &crate::test_util::sine(44100, 440.0, 1.0));
        let pcm = decode(path.to_str().unwrap(), &Cancel::default()).await.unwrap();
        assert_eq!(pcm.sample_rate, SAMPLE_RATE);
        assert!((pcm.duration() - 1.0).abs() < 0.01, "{}", pcm.duration());
        let peak = pcm.left.iter().fold(0f32, |m, x| m.max(x.abs()));
        // mpv upmixes mono at −3 dB.
        assert!(peak > 0.3, "{peak}");
        assert!(mpv_version().starts_with("mpv"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
