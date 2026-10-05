//! Validation engine building blocks: command detection, git inspection,
//! command execution, and verdict aggregation. Orchestration lives in the
//! scheduler pipeline.

pub mod aggregate;
pub mod detect;
pub mod git;

use std::path::Path;
use std::time::{Duration, Instant};

use crate::models::{CommandOutcome, ValidationCommand};
use crate::process::{run_streaming, CancelToken, SpawnSpec, Stream};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const TAIL_BYTES: usize = 4_000;

/// Build the spawn spec for a validation command. The program is resolved on
/// PATH (so `npm` finds `npm.cmd` on Windows) and run without a shell.
pub fn command_spec(cmd: &ValidationCommand, root: &Path) -> Result<SpawnSpec, String> {
    let program = which::which(&cmd.program).map_err(|_| format!("'{}' is not installed or not on PATH", cmd.program))?;
    let mut spec = SpawnSpec::new(program, cmd.args.clone(), root);
    // Non-interactive, single-run mode for test runners such as vitest/jest.
    spec.env_set = vec![("CI".into(), "true".into()), ("FORCE_COLOR".into(), "0".into())];
    Ok(spec)
}

/// Run one validation command, streaming its output through `on_line`.
pub async fn run_command<F>(cmd: &ValidationCommand, root: &Path, cancel: &CancelToken, mut on_line: F) -> CommandOutcome
where
    F: FnMut(Stream, &str),
{
    let started = Instant::now();
    let mut outcome = CommandOutcome {
        kind: cmd.kind.clone(),
        program: cmd.program.clone(),
        args: cmd.args.clone(),
        exit_code: None,
        success: false,
        duration_ms: 0,
        output_tail: String::new(),
    };
    let spec = match command_spec(cmd, root) {
        Ok(s) => s,
        Err(e) => {
            outcome.output_tail = e;
            return outcome;
        }
    };
    let mut tail = String::new();
    let result = run_streaming(&spec, cancel, Some(COMMAND_TIMEOUT), |stream, line| {
        on_line(stream, &line);
        tail.push_str(&line);
        tail.push('\n');
        if tail.len() > TAIL_BYTES * 2 {
            let cut = tail.len() - TAIL_BYTES;
            let cut = (cut..tail.len()).find(|i| tail.is_char_boundary(*i)).unwrap_or(0);
            tail.drain(..cut);
        }
    })
    .await;
    outcome.duration_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(p) => {
            outcome.exit_code = p.exit_code;
            outcome.success = p.success;
            if p.timed_out {
                tail.push_str("\n[TaskKiln] command timed out");
            }
        }
        Err(e) => tail.push_str(&format!("\n[TaskKiln] {e}")),
    }
    outcome.output_tail = crate::logging::sanitize(&crate::logging::truncate(&tail, TAIL_BYTES * 2));
    outcome
}
