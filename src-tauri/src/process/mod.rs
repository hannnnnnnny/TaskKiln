//! Child-process management.
//!
//! Rules this module enforces for every process TaskKiln starts:
//! - programs are spawned directly with an argument vector, never via a shell;
//! - large inputs (prompts) go through stdin, not the command line;
//! - each child runs in its own kill group (Windows Job Object / Unix process
//!   group) so stopping a task, or TaskKiln exiting, takes the whole tree down.

mod cancel;
mod group;

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::Instant;

pub use cancel::CancelToken;
use group::KillGroup;

use crate::error::{AppError, AppResult};

/// After the main process exits, how long to keep reading pipes that a
/// leftover grandchild may still hold open before the group is killed.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub stdin: Option<String>,
    pub env_remove: Vec<String>,
    pub env_set: Vec<(String, String)>,
}

impl SpawnSpec {
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>, cwd: impl Into<PathBuf>) -> Self {
        Self { program: program.into(), args, cwd: cwd.into(), stdin: None, env_remove: vec![], env_set: vec![] }
    }

    /// Human-readable rendering for the command audit log. Arguments are quoted
    /// for display only; nothing is ever executed from this string.
    pub fn describe(&self) -> String {
        let quote = |a: &str| {
            if a.is_empty() || a.contains(char::is_whitespace) || a.contains('"') {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                a.to_string()
            }
        };
        let mut s = quote(&self.program.to_string_lossy());
        for a in &self.args {
            s.push(' ');
            s.push_str(&quote(crate::logging::truncate(a, 300).as_str()));
        }
        if self.stdin.is_some() {
            s.push_str(" < [prompt via stdin]");
        }
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessOutcome {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub cancelled: bool,
    pub timed_out: bool,
}

fn build_command(spec: &SpawnSpec) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(if spec.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for k in &spec.env_remove {
        cmd.env_remove(k);
    }
    for (k, v) in &spec.env_set {
        cmd.env(k, v);
    }
    group::configure(&mut cmd);
    cmd
}

fn spawn_reader<R>(reader: R, stream: Stream, tx: mpsc::Sender<(Stream, String)>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        // Invalid UTF-8 ends the stream for that pipe; the exit code is still collected.
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send((stream, line)).await.is_err() {
                break;
            }
        }
    });
}

/// Run a process to completion, delivering each output line to `on_line` as it
/// arrives. Cancellation or timeout kills the entire process group.
pub async fn run_streaming<F>(
    spec: &SpawnSpec,
    cancel: &CancelToken,
    timeout: Option<Duration>,
    mut on_line: F,
) -> AppResult<ProcessOutcome>
where
    F: FnMut(Stream, String),
{
    if !spec.cwd.is_dir() {
        return Err(AppError::Process(format!("working directory missing: {}", spec.cwd.display())));
    }
    let mut child = build_command(spec)
        .spawn()
        .map_err(|e| AppError::Process(format!("failed to start {}: {e}", spec.program.display())))?;
    let group = KillGroup::attach(&child)?;

    let (tx, mut rx) = mpsc::channel::<(Stream, String)>(1024);
    if let Some(out) = child.stdout.take() {
        spawn_reader(out, Stream::Stdout, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        spawn_reader(err, Stream::Stderr, tx.clone());
    }
    drop(tx);
    if let (Some(input), Some(mut stdin)) = (spec.stdin.clone(), child.stdin.take()) {
        tokio::spawn(async move {
            // A child that exits early closes stdin; that's reported via its exit code.
            let _ = stdin.write_all(input.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }

    let mut state = LoopState::new(timeout);
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some((stream, text)) => on_line(stream, text),
                None => break,
            },
            status = child.wait(), if state.exit.is_none() => {
                state.exit = Some(status?);
                state.drain_deadline = Some(Instant::now() + DRAIN_GRACE);
            }
            _ = sleep_until_opt(state.drain_deadline) => { group.kill(); break; }
            _ = cancel.cancelled(), if !state.cancelled => { state.cancelled = true; group.kill(); }
            _ = sleep_until_opt(state.deadline), if !state.timed_out => { state.timed_out = true; group.kill(); }
        }
    }
    let status = match state.exit {
        Some(s) => s,
        None => child.wait().await?,
    };
    // Reap anything the child left behind (background servers, watchers, ...).
    group.kill();
    Ok(ProcessOutcome {
        exit_code: status.code(),
        success: status.success() && !state.cancelled && !state.timed_out,
        cancelled: state.cancelled,
        timed_out: state.timed_out,
    })
}

struct LoopState {
    exit: Option<std::process::ExitStatus>,
    drain_deadline: Option<Instant>,
    deadline: Option<Instant>,
    cancelled: bool,
    timed_out: bool,
}

impl LoopState {
    fn new(timeout: Option<Duration>) -> Self {
        Self {
            exit: None,
            drain_deadline: None,
            deadline: timeout.map(|t| Instant::now() + t),
            cancelled: false,
            timed_out: false,
        }
    }
}

async fn sleep_until_opt(at: Option<Instant>) {
    match at {
        Some(t) => tokio::time::sleep_until(t).await,
        None => std::future::pending().await,
    }
}

/// Collected output of a short-lived command.
#[derive(Debug, Clone)]
pub struct Captured {
    pub outcome: ProcessOutcome,
    pub stdout: String,
    pub stderr: String,
}

/// Run a command and collect its output (bounded to `max_bytes` per stream).
pub async fn run_capture(spec: &SpawnSpec, timeout: Duration, max_bytes: usize) -> AppResult<Captured> {
    let mut stdout = String::new();
    let mut stderr = String::new();
    let outcome = run_streaming(spec, &CancelToken::new(), Some(timeout), |stream, line| {
        let buf = if stream == Stream::Stdout { &mut stdout } else { &mut stderr };
        if buf.len() < max_bytes {
            buf.push_str(&line);
            buf.push('\n');
        }
    })
    .await?;
    Ok(Captured { outcome, stdout, stderr })
}

#[cfg(test)]
mod tests;
