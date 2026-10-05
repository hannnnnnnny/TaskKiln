//! Locating the Claude Code CLI and probing what the installed version supports.
//!
//! GUI apps (especially on macOS) do not inherit the user's shell PATH, so
//! besides PATH we check the documented install locations explicitly.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::process::{run_capture, SpawnSpec};

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Capabilities {
    pub print_mode: bool,
    pub stream_json: bool,
    pub session_id: bool,
    pub resume: bool,
    pub json_schema: bool,
    pub permission_prompts: bool,
    pub allowed_tools: bool,
    pub tools: bool,
}

impl Capabilities {
    /// The minimum TaskKiln needs to drive Claude non-interactively.
    pub fn missing_required(&self) -> Vec<&'static str> {
        let mut missing = vec![];
        if !self.print_mode { missing.push("--print"); }
        if !self.stream_json { missing.push("--output-format stream-json"); }
        if !self.session_id { missing.push("--session-id"); }
        if !self.resume { missing.push("--resume"); }
        missing
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ClaudeStatus {
    pub found: bool,
    pub path: Option<String>,
    /// "custom" | "PATH" | "known-location"
    pub source: Option<String>,
    pub version: Option<String>,
    pub capabilities: Capabilities,
    /// `None` when the CLI could not report auth state.
    pub logged_in: Option<bool>,
    pub auth_method: Option<String>,
    pub error: Option<String>,
}

impl ClaudeStatus {
    pub fn usable(&self) -> bool {
        self.found && self.capabilities.missing_required().is_empty()
    }
}

/// Environment needed to compute install candidates (injectable for tests).
pub struct SearchEnv {
    pub home: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub localappdata: Option<PathBuf>,
}

impl SearchEnv {
    pub fn from_process() -> Self {
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Self {
            home: var("HOME").or_else(|| var("USERPROFILE")),
            appdata: var("APPDATA"),
            localappdata: var("LOCALAPPDATA"),
        }
    }
}

/// Well-known install locations, most preferred first.
pub fn candidate_paths(env: &SearchEnv) -> Vec<PathBuf> {
    let mut out = vec![];
    let exe = if cfg!(windows) { "claude.exe" } else { "claude" };
    if let Some(home) = &env.home {
        out.push(home.join(".local").join("bin").join(exe));
        out.push(home.join(".claude").join("local").join(exe));
        if !cfg!(windows) {
            out.push(home.join(".npm-global").join("bin").join("claude"));
            out.push(home.join(".bun").join("bin").join("claude"));
        }
    }
    if cfg!(windows) {
        if let Some(appdata) = &env.appdata {
            out.push(appdata.join("npm").join("claude.cmd"));
            // Copy bundled with the Claude desktop app: claude-code/<version>/<hash>/claude.exe
            out.extend(bundled_desktop_cli(&appdata.join("Claude").join("claude-code"), exe));
        }
        if let Some(local) = &env.localappdata {
            out.push(local.join("Programs").join("claude").join(exe));
        }
    } else {
        out.push(PathBuf::from("/opt/homebrew/bin/claude"));
        out.push(PathBuf::from("/usr/local/bin/claude"));
        out.push(PathBuf::from("/usr/bin/claude"));
        if let Some(home) = &env.home {
            let support = home.join("Library").join("Application Support").join("Claude").join("claude-code");
            out.extend(bundled_desktop_cli(&support, exe));
        }
    }
    out
}

/// Find `<root>/<version>/<hash>/<exe>` entries, newest version first.
fn bundled_desktop_cli(root: &Path, exe: &str) -> Vec<PathBuf> {
    let Ok(versions) = std::fs::read_dir(root) else { return vec![] };
    let mut found: Vec<(Vec<u64>, PathBuf)> = vec![];
    for v in versions.flatten() {
        let key = version_key(&v.file_name().to_string_lossy());
        let Ok(hashes) = std::fs::read_dir(v.path()) else { continue };
        for h in hashes.flatten() {
            let candidate = h.path().join(exe);
            if candidate.is_file() {
                found.push((key.clone(), candidate));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, p)| p).collect()
}

fn version_key(s: &str) -> Vec<u64> {
    s.split(|c: char| !c.is_ascii_digit()).filter_map(|p| p.parse().ok()).collect()
}

/// Resolve the binary: explicit custom path, then PATH, then known locations.
pub fn locate(custom: &str, env: &SearchEnv) -> Option<(PathBuf, &'static str)> {
    if !custom.trim().is_empty() {
        let p = PathBuf::from(custom.trim());
        return p.is_file().then_some((p, "custom"));
    }
    if let Ok(p) = which::which("claude") {
        return Some((p, "PATH"));
    }
    candidate_paths(env).into_iter().find(|p| p.is_file()).map(|p| (p, "known-location"))
}

pub fn parse_version(output: &str) -> Option<String> {
    let re = regex::Regex::new(r"\d+\.\d+\.\d+[0-9A-Za-z.\-]*").expect("static regex");
    re.find(output).map(|m| m.as_str().to_string())
}

/// Feature detection from `claude --help` text, so we never pass a flag the
/// installed version does not understand.
pub fn parse_capabilities(help: &str) -> Capabilities {
    let has = |flag: &str| help.contains(flag);
    Capabilities {
        print_mode: has("--print"),
        stream_json: has("stream-json"),
        session_id: has("--session-id"),
        resume: has("--resume"),
        json_schema: has("--json-schema"),
        permission_prompts: has("--permission-prompts"),
        allowed_tools: has("--allowedTools") || has("--allowed-tools"),
        tools: has("--tools "),
    }
}

/// Parse `claude auth status` JSON output. Unknown formats yield `None`.
pub fn parse_auth(stdout: &str) -> (Option<bool>, Option<String>) {
    match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        Ok(v) => (
            v.get("loggedIn").and_then(|b| b.as_bool()),
            v.get("authMethod").and_then(|m| m.as_str()).map(str::to_string),
        ),
        Err(_) => (None, None),
    }
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

pub async fn probe(custom: &str) -> ClaudeStatus {
    let Some((path, source)) = locate(custom, &SearchEnv::from_process()) else {
        return ClaudeStatus {
            error: Some(if custom.trim().is_empty() {
                "Claude Code CLI was not found on PATH or in known install locations".into()
            } else {
                "The custom Claude CLI path does not exist".into()
            }),
            ..Default::default()
        };
    };
    let cwd = std::env::temp_dir();
    let run = |args: &[&str]| {
        let spec = SpawnSpec::new(&path, args.iter().map(|s| s.to_string()).collect(), &cwd);
        async move { run_capture(&spec, PROBE_TIMEOUT, 200_000).await }
    };
    let mut status = ClaudeStatus {
        found: true,
        path: Some(path.to_string_lossy().to_string()),
        source: Some(source.into()),
        ..Default::default()
    };
    match run(&["--version"]).await {
        Ok(out) => status.version = parse_version(&out.stdout),
        Err(e) => {
            status.found = false;
            status.error = Some(format!("Found {} but it could not be started: {e}", path.display()));
            return status;
        }
    }
    if let Ok(out) = run(&["--help"]).await {
        status.capabilities = parse_capabilities(&out.stdout);
    }
    if let Ok(out) = run(&["auth", "status"]).await {
        (status.logged_in, status.auth_method) = parse_auth(&out.stdout);
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_strings() {
        assert_eq!(parse_version("2.1.286 (Claude Code)").as_deref(), Some("2.1.286"));
        assert_eq!(parse_version("nothing"), None);
    }

    #[test]
    fn detects_capabilities_from_help() {
        let help = "-p, --print  Print\n --output-format <format> \"stream-json\"\n --session-id <uuid>\n -r, --resume [value]\n --json-schema <schema>\n --tools <tools...>\n --allowedTools, --allowed-tools";
        let caps = parse_capabilities(help);
        assert!(caps.missing_required().is_empty());
        assert!(caps.json_schema && caps.tools && caps.allowed_tools);
        assert!(!caps.permission_prompts);
        let old = parse_capabilities("-p, --print");
        assert!(old.missing_required().contains(&"--session-id"));
    }

    #[test]
    fn parses_auth_status_json() {
        let (logged, method) = parse_auth(r#"{"loggedIn": false, "authMethod": "none"}"#);
        assert_eq!(logged, Some(false));
        assert_eq!(method.as_deref(), Some("none"));
        assert_eq!(parse_auth("Not JSON").0, None);
    }

    #[test]
    fn finds_newest_bundled_desktop_cli() {
        let dir = tempfile::tempdir().unwrap();
        let exe = if cfg!(windows) { "claude.exe" } else { "claude" };
        for v in ["2.1.9", "2.1.284", "2.1.30"] {
            let d = dir.path().join(v).join("abc123");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(exe), b"").unwrap();
        }
        let found = bundled_desktop_cli(dir.path(), exe);
        assert_eq!(found.len(), 3);
        assert!(found[0].to_string_lossy().contains("2.1.284"));
    }

    #[test]
    fn custom_path_must_exist() {
        let env = SearchEnv { home: None, appdata: None, localappdata: None };
        assert!(locate("Z:/nope/claude.exe", &env).is_none());
        let f = tempfile::NamedTempFile::new().unwrap();
        let (p, src) = locate(f.path().to_str().unwrap(), &env).unwrap();
        assert_eq!(src, "custom");
        assert_eq!(p, f.path());
    }
}
