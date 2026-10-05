//! Domain types shared by the database, scheduler, and IPC layers.

pub mod status;

use serde::{Deserialize, Serialize};

pub use status::{AttentionReason, CheckpointStatus, TaskStatus, ValidationStatus};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub created_at: String,
    /// False when the directory no longer exists on disk.
    pub path_exists: bool,
    pub is_git_repo: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub priority: i64,
    pub queue_position: i64,
    pub status: TaskStatus,
    /// Weighted checkpoint completion in [0, 1]. `None` when progress cannot be
    /// determined reliably (no plan yet, or Claude never reported checkpoints).
    pub progress: Option<f64>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub claude_session_id: Option<String>,
    pub validation_status: Option<ValidationStatus>,
    pub attention_reason: Option<AttentionReason>,
    pub attention_detail: Option<String>,
    pub current_activity: Option<String>,
    /// Number of fix rounds requested so far (ASK CLAUDE TO FIX / ADJUST).
    pub fix_attempts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Checkpoint {
    pub id: String,
    pub task_id: String,
    pub ordinal: i64,
    pub title: String,
    pub status: CheckpointStatus,
    pub weight: f64,
    /// "claude" for plan steps, "taskkiln" for the validation step TaskKiln owns.
    pub owner: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskEvent {
    pub id: i64,
    pub task_id: Option<String>,
    pub kind: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogLine {
    pub id: i64,
    pub task_id: String,
    pub stream: String,
    pub line: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    Info,
    Warning,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Finding {
    pub source: String,
    pub severity: FindingSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandOutcome {
    pub kind: String,
    pub program: String,
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub duration_ms: u64,
    pub output_tail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationResult {
    pub id: String,
    pub task_id: String,
    pub status: ValidationStatus,
    pub summary: String,
    pub findings: Vec<Finding>,
    pub commands: Vec<CommandOutcome>,
    pub changed_files: Vec<String>,
    pub created_at: String,
}

/// A validation command TaskKiln may run inside a project. Detected commands
/// start unapproved; the user must confirm them before TaskKiln runs anything.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationCommand {
    pub id: String,
    pub project_id: String,
    /// "build" | "test" | "lint"
    pub kind: String,
    pub program: String,
    pub args: Vec<String>,
    /// Where it came from, e.g. "package.json#scripts.test".
    pub source: String,
    pub approved: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Empty means auto-detect.
    pub claude_path: String,
    pub always_on_top: bool,
    pub notifications: bool,
    pub auto_run_next: bool,
    pub check_build: bool,
    pub check_test: bool,
    pub check_lint: bool,
    pub claude_review: bool,
    /// Claude Code `--permission-mode` for execution runs.
    pub permission_mode: String,
    pub allowed_tools: String,
    pub disallowed_tools: String,
    /// Optional `--model` value; empty uses Claude Code's default.
    pub model: String,
    pub show_status_bar: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            claude_path: String::new(),
            always_on_top: true,
            notifications: true,
            auto_run_next: true,
            check_build: true,
            check_test: true,
            check_lint: true,
            claude_review: true,
            permission_mode: "acceptEdits".into(),
            allowed_tools: DEFAULT_ALLOWED_TOOLS.into(),
            disallowed_tools: DEFAULT_DISALLOWED_TOOLS.into(),
            model: String::new(),
            show_status_bar: true,
        }
    }
}

/// Bash patterns Claude may run without a prompt in non-interactive mode.
/// Anything else that needs approval is denied (`--permission-prompts none`).
pub const DEFAULT_ALLOWED_TOOLS: &str = "Read Edit Write Glob Grep TodoWrite \
Bash(npm run *) Bash(npm test*) Bash(npm install*) Bash(npx tsc*) Bash(npx vitest*) \
Bash(pnpm *) Bash(yarn *) Bash(cargo *) Bash(python -m pytest*) Bash(pytest*) \
Bash(go build*) Bash(go test*) Bash(git status*) Bash(git diff*) Bash(git log*) Bash(ls*) Bash(cat *)";

/// Destructive operations TaskKiln never lets Claude run unattended.
pub const DEFAULT_DISALLOWED_TOOLS: &str = "Bash(git push*) Bash(git reset --hard*) \
Bash(git clean*) Bash(rm -rf*) Bash(sudo *) Bash(git checkout -- *)";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewTask {
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub priority: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskUpdate {
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub priority: i64,
}
