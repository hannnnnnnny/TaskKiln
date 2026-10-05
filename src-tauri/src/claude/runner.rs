//! Launching Claude Code in print mode and streaming its events.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::detect::Capabilities;
use super::stream::{parse_line, RunResult, StreamEvent};
use crate::error::AppResult;
use crate::process::{run_streaming, CancelToken, ProcessOutcome, SpawnSpec, Stream};

/// Variables set when TaskKiln itself runs inside a Claude Code session (e.g.
/// during development). Passing them on would make the child think it is a
/// nested/hosted session, so they are stripped. User auth/provider variables
/// such as ANTHROPIC_API_KEY or CLAUDE_CODE_USE_BEDROCK are left untouched.
const HOST_SESSION_ENV: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_HOST_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SDK_HAS_HOST_AUTH_REFRESH",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_AGENT_SDK_VERSION",
    "CLAUDE_PID",
];

#[derive(Debug, Clone, PartialEq)]
pub enum SessionMode {
    /// Start a new persisted session with this id.
    New(String),
    /// Continue an existing session.
    Resume(String),
    /// One-off run that is not saved (reviews, criteria drafts).
    Ephemeral,
}

#[derive(Debug, Clone)]
pub struct ClaudeRequest {
    pub cwd: PathBuf,
    pub prompt: String,
    pub session: SessionMode,
    pub json_schema: Option<&'static str>,
    /// `Some` restricts the built-in tool set (e.g. read-only planning).
    pub tools: Option<String>,
    pub permission_mode: Option<String>,
    pub allowed_tools: String,
    pub disallowed_tools: String,
    pub model: String,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct ClaudeOutcome {
    pub process: ProcessOutcome,
    pub result: Option<RunResult>,
    pub session_id: Option<String>,
    pub stderr_tail: String,
    /// The CLI reported that the session to resume does not exist.
    pub resume_failed: bool,
}

impl ClaudeOutcome {
    pub fn succeeded(&self) -> bool {
        self.process.success && self.result.as_ref().is_some_and(|r| !r.is_error)
    }

    /// Short human-readable reason for a failed run.
    pub fn failure_reason(&self) -> String {
        if self.process.cancelled {
            return "stopped by user".into();
        }
        if self.process.timed_out {
            return "timed out".into();
        }
        if let Some(r) = &self.result {
            if r.is_error {
                let text = if r.text.is_empty() { r.subtype.clone() } else { r.text.clone() };
                return crate::logging::sanitize(&crate::logging::truncate(&text, 300));
            }
        }
        let tail = self.stderr_tail.trim();
        match (self.process.exit_code, tail.is_empty()) {
            (Some(c), true) => format!("Claude exited with code {c}"),
            (Some(c), false) => format!("Claude exited with code {c}: {}", crate::logging::truncate(tail, 300)),
            (None, _) => "Claude process ended unexpectedly".into(),
        }
    }
}

/// Split a tool list like `Read Edit Bash(npm run *)` into entries, keeping
/// parenthesized patterns intact, and join them with commas for the CLI.
pub fn normalize_tool_list(raw: &str) -> String {
    let mut out: Vec<String> = vec![];
    let mut cur = String::new();
    let mut depth = 0;
    for ch in raw.chars() {
        match ch {
            '(' => { depth += 1; cur.push(ch); }
            ')' => { depth -= 1; cur.push(ch); }
            c if (c.is_whitespace() || c == ',') && depth == 0 => {
                if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() { out.push(cur); }
    out.join(",")
}

/// Build the argument vector. Pure so it can be unit-tested; only flags the
/// installed CLI advertises are included.
pub fn build_args(req: &ClaudeRequest, caps: &Capabilities) -> Vec<String> {
    let mut a: Vec<String> = vec!["-p".into(), "--output-format".into(), "stream-json".into(), "--verbose".into()];
    let mut push = |k: &str, v: &str| { a.push(k.into()); a.push(v.into()); };
    match &req.session {
        SessionMode::New(id) => push("--session-id", id),
        SessionMode::Resume(id) => push("--resume", id),
        SessionMode::Ephemeral => {}
    }
    if caps.permission_prompts {
        // Non-interactive: anything needing approval is denied, never left hanging.
        push("--permission-prompts", "none");
    }
    if let Some(mode) = &req.permission_mode {
        push("--permission-mode", mode);
    }
    if !req.model.is_empty() {
        push("--model", &req.model);
    }
    if let (Some(schema), true) = (req.json_schema, caps.json_schema) {
        push("--json-schema", schema);
    }
    if let (Some(tools), true) = (&req.tools, caps.tools) {
        push("--tools", &normalize_tool_list(tools));
    }
    let allowed = normalize_tool_list(&req.allowed_tools);
    if !allowed.is_empty() && caps.allowed_tools {
        push("--allowedTools", &allowed);
    }
    let denied = normalize_tool_list(&req.disallowed_tools);
    if !denied.is_empty() && caps.allowed_tools {
        push("--disallowedTools", &denied);
    }
    if matches!(req.session, SessionMode::Ephemeral) {
        a.push("--no-session-persistence".into());
    }
    a
}

pub struct ClaudeRunner {
    pub bin: PathBuf,
    pub caps: Capabilities,
}

impl ClaudeRunner {
    pub fn new(bin: impl AsRef<Path>, caps: Capabilities) -> Self {
        Self { bin: bin.as_ref().to_path_buf(), caps }
    }

    pub fn spec(&self, req: &ClaudeRequest) -> SpawnSpec {
        let mut spec = SpawnSpec::new(&self.bin, build_args(req, &self.caps), &req.cwd);
        spec.stdin = Some(req.prompt.clone());
        spec.env_remove = HOST_SESSION_ENV.iter().map(|s| s.to_string()).collect();
        spec
    }

    /// Run Claude, calling `on_event` for each parsed stream event and
    /// `on_raw` for every output line (for the task log).
    pub async fn run<E, R>(&self, req: &ClaudeRequest, cancel: &CancelToken, mut on_event: E, mut on_raw: R)
        -> AppResult<ClaudeOutcome>
    where
        E: FnMut(&StreamEvent),
        R: FnMut(Stream, &str),
    {
        let spec = self.spec(req);
        let mut result: Option<RunResult> = None;
        let mut session_id: Option<String> = None;
        let mut stderr_tail = String::new();
        let process = run_streaming(&spec, cancel, Some(req.timeout), |stream, line| {
            on_raw(stream, &line);
            if stream == Stream::Stderr {
                push_tail(&mut stderr_tail, &line);
                return;
            }
            for ev in parse_line(&line).unwrap_or_default() {
                match &ev {
                    StreamEvent::Init { session_id: s, .. } => session_id = Some(s.clone()),
                    StreamEvent::Result(r) => result = Some(r.clone()),
                    _ => {}
                }
                on_event(&ev);
            }
        })
        .await?;
        let resume_failed = matches!(req.session, SessionMode::Resume(_))
            && (stderr_tail.contains("No conversation found")
                || result.as_ref().is_some_and(|r| r.text.contains("No conversation found")));
        Ok(ClaudeOutcome { process, result, session_id, stderr_tail, resume_failed })
    }
}

fn push_tail(buf: &mut String, line: &str) {
    buf.push_str(line);
    buf.push('\n');
    if buf.len() > 4000 {
        let cut = buf.len() - 4000;
        let cut = (cut..buf.len()).find(|i| buf.is_char_boundary(*i)).unwrap_or(0);
        buf.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> Capabilities {
        Capabilities {
            print_mode: true, stream_json: true, session_id: true, resume: true,
            json_schema: true, permission_prompts: true, allowed_tools: true, tools: true,
        }
    }

    fn req(session: SessionMode) -> ClaudeRequest {
        ClaudeRequest {
            cwd: std::env::temp_dir(),
            prompt: "do it".into(),
            session,
            json_schema: Some("{}"),
            tools: None,
            permission_mode: Some("acceptEdits".into()),
            allowed_tools: "Read Bash(npm run *)".into(),
            disallowed_tools: "Bash(git push*)".into(),
            model: String::new(),
            timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn tool_lists_keep_parenthesized_patterns() {
        assert_eq!(normalize_tool_list("Read  Edit Bash(npm run *), Bash(git diff*)"), "Read,Edit,Bash(npm run *),Bash(git diff*)");
        assert_eq!(normalize_tool_list("  "), "");
    }

    #[test]
    fn new_session_args() {
        let a = build_args(&req(SessionMode::New("uuid-1".into())), &caps());
        let joined = a.join(" ");
        assert!(joined.starts_with("-p --output-format stream-json --verbose --session-id uuid-1"));
        assert!(joined.contains("--permission-prompts none"));
        assert!(joined.contains("--permission-mode acceptEdits"));
        assert!(joined.contains("--json-schema {}"));
        assert!(a.contains(&"Read,Bash(npm run *)".to_string()));
        assert!(a.contains(&"Bash(git push*)".to_string()));
        assert!(!a.contains(&"do it".to_string()), "prompt goes via stdin, not argv");
        assert!(!a.contains(&"--no-session-persistence".to_string()));
    }

    #[test]
    fn resume_and_ephemeral_args() {
        let a = build_args(&req(SessionMode::Resume("s1".into())), &caps());
        assert!(a.windows(2).any(|w| w[0] == "--resume" && w[1] == "s1"));
        let e = build_args(&req(SessionMode::Ephemeral), &caps());
        assert!(e.contains(&"--no-session-persistence".to_string()));
        assert!(!e.contains(&"--session-id".to_string()));
    }

    #[test]
    fn unsupported_flags_are_omitted() {
        let old = Capabilities { print_mode: true, stream_json: true, session_id: true, resume: true, ..Default::default() };
        let a = build_args(&req(SessionMode::New("x".into())), &old);
        assert!(!a.contains(&"--json-schema".to_string()));
        assert!(!a.contains(&"--permission-prompts".to_string()));
        assert!(!a.contains(&"--allowedTools".to_string()));
    }

    #[test]
    fn host_session_env_is_stripped() {
        let r = ClaudeRunner::new("claude", caps());
        let spec = r.spec(&req(SessionMode::Ephemeral));
        assert!(spec.env_remove.contains(&"CLAUDECODE".to_string()));
        assert!(!spec.env_remove.iter().any(|k| k == "ANTHROPIC_API_KEY"));
        assert_eq!(spec.stdin.as_deref(), Some("do it"));
    }
}
