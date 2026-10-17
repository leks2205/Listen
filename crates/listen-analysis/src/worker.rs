//! Analysis in a short-lived helper process (`listen --analysis-worker`). Decoding and the beat
//! model peak at a few hundred MB per song that glibc never returns, so the helper exits when
//! idle and the next job starts a fresh one. Protocol: one JSON line per request on stdin, one
//! per answer on stdout, one job at a time.

use crate::{AnalysisError, BeatModel, BeatSource, Cancel, Opts, analyze_url};
use listen_core::analysis::Features;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// The helper exits after the queue has been empty this long.
pub const IDLE_EXIT: Duration = Duration::from_secs(30);

/// A helper whose resident memory is above this after a job is replaced:
/// it keeps roughly 15 MB per song that glibc does not return.
pub const RECYCLE_MB: u64 = 300;

#[derive(Serialize, Deserialize)]
struct Request {
    input: String,
    model: Option<(PathBuf, PathBuf)>,
    bpm_hint: Option<f32>,
}

#[derive(Serialize, Deserialize)]
enum Answer {
    /// `beats` carries the grid's beat times at full precision: the
    /// features' own serialisation (the store's) rounds them to 1 ms.
    Ok {
        features: Box<Features>,
        beats: Vec<f32>,
    },
    Decode {
        reason: String,
        transient: bool,
    },
    TooLong,
    Cancelled,
    Model(String),
}

impl From<Result<Features, AnalysisError>> for Answer {
    fn from(r: Result<Features, AnalysisError>) -> Self {
        match r {
            Ok(f) => {
                let beats = f.grid.as_ref().map(|g| g.beats.clone()).unwrap_or_default();
                Answer::Ok { features: Box::new(f), beats }
            }
            Err(AnalysisError::Decode { reason, transient }) => Answer::Decode { reason, transient },
            Err(AnalysisError::TooLong) => Answer::TooLong,
            Err(AnalysisError::Cancelled) => Answer::Cancelled,
            Err(AnalysisError::Model(m)) => Answer::Model(m),
        }
    }
}

impl From<Answer> for Result<Features, AnalysisError> {
    fn from(a: Answer) -> Self {
        match a {
            Answer::Ok { features, beats } => {
                let mut f = *features;
                if let Some(grid) = f.grid.as_mut() {
                    grid.beats = beats;
                }
                Ok(f)
            }
            Answer::Decode { reason, transient } => Err(AnalysisError::Decode { reason, transient }),
            Answer::TooLong => Err(AnalysisError::TooLong),
            Answer::Cancelled => Err(AnalysisError::Cancelled),
            Answer::Model(m) => Err(AnalysisError::Model(m)),
        }
    }
}

/// The helper's main loop (`listen --analysis-worker`): serves requests
/// until stdin closes. Returns the exit code.
pub fn run_worker() -> i32 {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return 1 };
    rt.block_on(async {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        let mut out = tokio::io::stdout();
        while let Ok(Some(line)) = lines.next_line().await {
            let answer = match serde_json::from_str::<Request>(&line) {
                Ok(req) => {
                    let beats = match req.model {
                        Some((mel, beat)) => BeatSource::BeatThis(BeatModel { mel, beat }),
                        None => BeatSource::Own,
                    };
                    let opts = Opts { beats, bpm_hint: req.bpm_hint };
                    analyze_url(&req.input, &opts, &Cancel::default()).await.into()
                }
                Err(e) => Answer::Model(format!("bad request: {e}")),
            };
            let Ok(mut json) = serde_json::to_vec(&answer) else { return 1 };
            json.push(b'\n');
            if out.write_all(&json).await.is_err() || out.flush().await.is_err() {
                return 1;
            }
        }
        0
    })
}

/// A field of `/proc/<pid>/status` in MB ("VmRSS", "VmHWM"); Linux only.
fn proc_mb(pid: Option<u32>, field: &str) -> Option<u64> {
    let path = pid.map_or("/proc/self/status".to_string(), |p| format!("/proc/{p}/status"));
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().find_map(|l| l.strip_prefix(field)?.strip_prefix(':'))?;
    Some(line.split_whitespace().next()?.parse::<u64>().ok()? / 1024)
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// The client side: starts the helper on demand and stops it when idle.
pub struct Worker {
    program: PathBuf,
    args: Vec<String>,
    running: Option<Running>,
    last_used: Instant,
}

impl Worker {
    /// Runs `listen --analysis-worker` from the current executable.
    pub fn new() -> Self {
        // Linux: /proc/self/exe still reaches the running binary after an update replaced it on
        // disk; elsewhere the current executable.
        let program = if cfg!(target_os = "linux") {
            PathBuf::from("/proc/self/exe")
        } else {
            std::env::current_exe().unwrap_or_else(|_| PathBuf::from("listen"))
        };
        Self::with_command(program, vec!["--analysis-worker".into()])
    }

    pub fn with_command(program: PathBuf, args: Vec<String>) -> Self {
        Worker { program, args, running: None, last_used: Instant::now() }
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    fn start(&self) -> Result<Running, AnalysisError> {
        let mut cmd = Command::new(&self.program);
        #[cfg(windows)]
        cmd.creation_flags(listen_core::platform::CREATE_NO_WINDOW);
        let mut child = cmd
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AnalysisError::Decode {
                reason: format!("cannot start the analysis helper: {e}"),
                transient: true,
            })?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        Ok(Running { child, stdin, stdout })
    }

    /// Like [`analyze_url`], in the helper. A helper that dies mid-job is a
    /// transient failure; the next job starts a new one.
    pub async fn analyze(
        &mut self,
        input: &str,
        opts: &Opts,
        cancel: &Cancel,
    ) -> Result<Features, AnalysisError> {
        if cancel.is_cancelled() {
            return Err(AnalysisError::Cancelled);
        }
        if self.running.is_none() {
            self.running = Some(self.start()?);
        }
        let model = match &opts.beats {
            BeatSource::BeatThis(m) => Some((m.mel.clone(), m.beat.clone())),
            BeatSource::Own => None,
        };
        let mut line = serde_json::to_vec(&Request { input: input.into(), model, bpm_hint: opts.bpm_hint })
            .map_err(|e| AnalysisError::Model(e.to_string()))?;
        line.push(b'\n');
        let run = self.running.as_mut().expect("started above");
        let pid = run.child.id();
        let exchange = async {
            run.stdin.write_all(&line).await?;
            run.stdin.flush().await?;
            let mut answer = String::new();
            let n = run.stdout.read_line(&mut answer).await?;
            Ok::<_, std::io::Error>((n, answer))
        };
        let result = tokio::select! {
            r = exchange => Some(r),
            () = cancel.cancelled() => None,
        };
        self.last_used = Instant::now();
        let now_mb = proc_mb(pid, "VmRSS").unwrap_or(0);
        tracing::info!(
            "analysis memory: helper peak {} MB (now {} MB), main {} MB",
            proc_mb(pid, "VmHWM").unwrap_or(0),
            now_mb,
            proc_mb(None, "VmRSS").unwrap_or(0)
        );
        let died = |what: String| AnalysisError::Decode {
            reason: format!("the analysis helper stopped: {what}"),
            transient: true,
        };
        let outcome = match result {
            None => {
                self.stop().await;
                return Err(AnalysisError::Cancelled);
            }
            Some(Ok((n, answer))) if n > 0 => serde_json::from_str::<Answer>(&answer)
                .map(Result::from)
                .map_err(|e| died(format!("unreadable answer: {e}"))),
            Some(Ok(_)) => Err(died("it exited".into())),
            Some(Err(e)) => Err(died(e.to_string())),
        };
        match outcome {
            Ok(r) => {
                if now_mb > RECYCLE_MB {
                    self.stop().await;
                }
                r
            }
            Err(e) => {
                self.stop().await;
                Err(e)
            }
        }
    }

    /// Stops the helper if it has been idle for [`IDLE_EXIT`]; call it when
    /// the queue is empty.
    pub async fn stop_if_idle(&mut self) {
        if self.running.is_some() && self.last_used.elapsed() >= IDLE_EXIT {
            self.stop().await;
        }
    }

    /// Closes the helper's stdin (it exits and its memory goes back to the
    /// system) and waits briefly; kills it otherwise.
    pub async fn stop(&mut self) {
        let Some(Running { mut child, stdin, .. }) = self.running.take() else { return };
        drop(stdin);
        if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
            let _ = child.kill().await;
        }
        tracing::info!("analysis helper stopped; main {} MB", proc_mb(None, "VmRSS").unwrap_or(0));
    }
}

impl Default for Worker {
    fn default() -> Self {
        Self::new()
    }
}
