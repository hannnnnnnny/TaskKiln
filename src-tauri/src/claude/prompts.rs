//! Prompt and JSON-schema construction for each kind of Claude run.

use crate::models::{Checkpoint, Finding, FindingSeverity, Task, ValidationCommand};

pub const PLAN_SCHEMA: &str = r#"{"type":"object","properties":{"checkpoints":{"type":"array","minItems":1,"maxItems":10,"items":{"type":"object","properties":{"title":{"type":"string","maxLength":120},"weight":{"type":"integer","minimum":1,"maximum":5}},"required":["title","weight"]}},"notes":{"type":"string"}},"required":["checkpoints"]}"#;

pub const COMPLETION_SCHEMA: &str = r#"{"type":"object","properties":{"status":{"type":"string","enum":["completed","partial","blocked"]},"summary":{"type":"string"},"changed_files":{"type":"array","items":{"type":"string"}},"tests_run":{"type":"array","items":{"type":"string"}},"tests_passed":{"type":"boolean"},"remaining_issues":{"type":"array","items":{"type":"string"}},"checkpoint_results":{"type":"array","items":{"type":"object","properties":{"index":{"type":"integer"},"status":{"type":"string","enum":["completed","failed","skipped"]},"note":{"type":"string"}},"required":["index","status"]}}},"required":["status","summary","changed_files","tests_passed","remaining_issues","checkpoint_results"]}"#;

pub const REVIEW_SCHEMA: &str = r#"{"type":"object","properties":{"overall":{"type":"string","enum":["pass","warning","fail"]},"summary":{"type":"string"},"criteria":{"type":"array","items":{"type":"object","properties":{"criterion":{"type":"string"},"verdict":{"type":"string","enum":["met","partially_met","not_met","cannot_determine"]},"evidence":{"type":"string"}},"required":["criterion","verdict","evidence"]}}},"required":["overall","summary","criteria"]}"#;

pub const CRITERIA_SCHEMA: &str = r#"{"type":"object","properties":{"criteria":{"type":"array","minItems":1,"maxItems":12,"items":{"type":"string","maxLength":200}}},"required":["criteria"]}"#;

/// Project facts included in every prompt.
pub struct ProjectContext<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub is_git_repo: bool,
    pub commands: &'a [ValidationCommand],
}

fn criteria_block(criteria: &[String]) -> String {
    if criteria.is_empty() {
        return "(none specified — infer reasonable completion conditions from the description)".into();
    }
    criteria.iter().enumerate().map(|(i, c)| format!("{}. {c}", i + 1)).collect::<Vec<_>>().join("\n")
}

fn context_block(ctx: &ProjectContext) -> String {
    let cmds = ctx
        .commands
        .iter()
        .filter(|c| c.enabled)
        .map(|c| format!("- {}: {} {}", c.kind, c.program, c.args.join(" ")))
        .collect::<Vec<_>>();
    format!(
        "PROJECT\nName: {}\nRoot: {}\nGit repository: {}\nValidation commands TaskKiln will run afterwards:\n{}",
        ctx.name,
        ctx.path,
        if ctx.is_git_repo { "yes" } else { "no" },
        if cmds.is_empty() { "- (none detected)".to_string() } else { cmds.join("\n") }
    )
}

fn task_block(task: &Task) -> String {
    format!(
        "TASK\nTitle: {}\n\nDescription:\n{}\n\nAcceptance criteria:\n{}",
        task.title,
        if task.description.is_empty() { "(no description)" } else { &task.description },
        criteria_block(&task.acceptance_criteria)
    )
}

pub fn plan_prompt(ctx: &ProjectContext, task: &Task) -> String {
    format!(
        "You are working under TaskKiln, a queue manager that runs coding tasks one at a time.\n\n\
{}\n\n{}\n\n\
STEP 1 OF 2 — PLAN ONLY. Do not modify any files in this step.\n\
Inspect the relevant parts of the codebase (read-only), then produce a concise execution plan of \
3 to 8 checkpoints. Each checkpoint is a concrete, verifiable step (e.g. \"inspect current auth flow\", \
\"add comments table migration\", \"run test suite\"). Give each a weight from 1 (trivial) to 5 (largest), \
proportional to expected effort. Include a testing checkpoint when tests exist.\n\
Return the plan in the required structured output.",
        context_block(ctx),
        task_block(task)
    )
}

fn checkpoint_list(checkpoints: &[Checkpoint]) -> String {
    checkpoints
        .iter()
        .filter(|c| c.owner == "claude")
        .map(|c| format!("{}. {} [{}]", c.ordinal, c.title, c.status.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

const MARKER_RULES: &str = "PROGRESS REPORTING (required — TaskKiln parses these lines):\n\
- When you begin checkpoint N, print a line containing only: TASKKILN_CHECKPOINT_START N\n\
- When checkpoint N is finished, print a line containing only: TASKKILN_CHECKPOINT_DONE N\n\
- If checkpoint N cannot be completed, print: TASKKILN_CHECKPOINT_FAILED N\n\
Print each marker as plain text in your message, on its own line, not inside a code block.";

const WORK_RULES: &str = "RULES\n\
- Inspect the existing code before changing it; follow its conventions.\n\
- Make the minimal changes needed to satisfy the task and its acceptance criteria.\n\
- Stay inside the project root. Never push, force-push, reset --hard, delete the repository, or use sudo.\n\
- Do not commit; TaskKiln and the user review the working tree.\n\
- Test your work: run the relevant build/test commands when they exist and fix failures you introduced.\n\
- When finished, return the required structured completion report. Be honest: use status \"partial\" or \
\"blocked\" and list remaining_issues if anything is not done.";

pub fn execute_prompt(ctx: &ProjectContext, task: &Task, checkpoints: &[Checkpoint]) -> String {
    format!(
        "STEP 2 OF 2 — IMPLEMENT the task now, following the plan you made.\n\n{}\n\n{}\n\n\
CHECKPOINTS\n{}\n\n{}\n\n{}",
        context_block(ctx),
        task_block(task),
        checkpoint_list(checkpoints),
        MARKER_RULES,
        WORK_RULES
    )
}

fn findings_block(findings: &[Finding]) -> String {
    let relevant: Vec<String> = findings
        .iter()
        .filter(|f| f.severity != FindingSeverity::Info)
        .map(|f| format!("- [{}] {}: {}", format!("{:?}", f.severity).to_uppercase(), f.source, f.message))
        .collect();
    if relevant.is_empty() { "- (no specific findings recorded)".into() } else { relevant.join("\n") }
}

pub fn fix_prompt(ctx: &ProjectContext, task: &Task, checkpoints: &[Checkpoint], findings: &[Finding]) -> String {
    format!(
        "TaskKiln validated your work on this task and it did NOT pass. Fix the problems below.\n\n\
VALIDATION FINDINGS\n{}\n\n{}\n\n{}\n\nCHECKPOINTS (re-run any that the fix affects)\n{}\n\n{}\n\n{}",
        findings_block(findings),
        context_block(ctx),
        task_block(task),
        checkpoint_list(checkpoints),
        MARKER_RULES,
        WORK_RULES
    )
}

pub fn reconcile_prompt(ctx: &ProjectContext, task: &Task, checkpoints: &[Checkpoint], findings: &[Finding]) -> String {
    format!(
        "The user ADJUSTED THE REQUIREMENTS for this task after validation. The acceptance criteria below \
are the new source of truth. Reconcile the implementation with them: add what is missing and remove or \
change behaviour that contradicts them.\n\nPREVIOUS VALIDATION FINDINGS\n{}\n\n{}\n\n{}\n\n\
CHECKPOINTS\n{}\n\n{}\n\n{}",
        findings_block(findings),
        context_block(ctx),
        task_block(task),
        checkpoint_list(checkpoints),
        MARKER_RULES,
        WORK_RULES
    )
}

pub fn resume_prompt(ctx: &ProjectContext, task: &Task, checkpoints: &[Checkpoint]) -> String {
    format!(
        "Your previous run on this task was interrupted before it finished (TaskKiln or the process stopped). \
The working tree may contain partial changes. Inspect the current state, then continue the task to completion.\n\n\
{}\n\n{}\n\nCHECKPOINTS (status as last recorded)\n{}\n\n{}\n\n{}",
        context_block(ctx),
        task_block(task),
        checkpoint_list(checkpoints),
        MARKER_RULES,
        WORK_RULES
    )
}

pub struct ReviewInput<'a> {
    pub diff_stat: &'a str,
    pub diff: &'a str,
    pub changed_files: &'a [String],
    pub command_summary: &'a str,
    pub claude_summary: &'a str,
}

pub fn review_prompt(ctx: &ProjectContext, task: &Task, input: &ReviewInput) -> String {
    format!(
        "You are an independent reviewer for TaskKiln. Another agent claims to have completed the task below. \
Do NOT trust its claims; verify against the actual code. You may read files in the project. Do not modify anything.\n\n\
{}\n\n{}\n\n\
AGENT'S OWN SUMMARY (unverified)\n{}\n\n\
VALIDATION COMMAND RESULTS (run by TaskKiln)\n{}\n\n\
CHANGED FILES\n{}\n\nDIFF STAT\n{}\n\nDIFF (may be truncated)\n{}\n\n\
For EACH acceptance criterion give a verdict: met, partially_met, not_met, or cannot_determine, with concrete \
evidence (file/function names). overall = fail if any criterion is not_met or the change is clearly broken; \
warning if anything is partially_met or cannot_determine; pass only if every criterion is met.",
        context_block(ctx),
        task_block(task),
        if input.claude_summary.is_empty() { "(none)" } else { input.claude_summary },
        input.command_summary,
        if input.changed_files.is_empty() { "(none detected)".to_string() } else { input.changed_files.join("\n") },
        input.diff_stat,
        input.diff
    )
}

pub fn criteria_prompt(title: &str, description: &str) -> String {
    format!(
        "Draft acceptance criteria for this coding task. Each criterion must be a short, objectively \
checkable statement about the finished software (behaviour, persistence, permissions, build/tests passing). \
Do not inspect or modify files; answer from the text alone.\n\nTitle: {title}\n\nDescription:\n{description}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TaskStatus;

    fn task() -> Task {
        Task {
            id: "t".into(), project_id: "p".into(), title: "Add comments".into(),
            description: "Users can comment".into(),
            acceptance_criteria: vec!["comments persist".into(), "users delete only their own".into()],
            priority: 1, queue_position: 0, status: TaskStatus::Queued, progress: None,
            created_at: String::new(), started_at: None, completed_at: None, claude_session_id: None,
            validation_status: None, attention_reason: None, attention_detail: None,
            current_activity: None, fix_attempts: 0,
        }
    }

    #[test]
    fn schemas_are_valid_json() {
        for s in [PLAN_SCHEMA, COMPLETION_SCHEMA, REVIEW_SCHEMA, CRITERIA_SCHEMA] {
            serde_json::from_str::<serde_json::Value>(s).unwrap();
        }
    }

    #[test]
    fn execute_prompt_contains_task_criteria_and_marker_rules() {
        let ctx = ProjectContext { name: "demo", path: "/x/demo", is_git_repo: true, commands: &[] };
        let p = execute_prompt(&ctx, &task(), &[]);
        assert!(p.contains("1. comments persist"));
        assert!(p.contains("2. users delete only their own"));
        assert!(p.contains("TASKKILN_CHECKPOINT_START N"));
        assert!(p.contains("Inspect the existing code"));
        assert!(p.contains("minimal changes"));
    }

    #[test]
    fn fix_prompt_lists_non_info_findings() {
        let ctx = ProjectContext { name: "demo", path: "/x", is_git_repo: false, commands: &[] };
        let findings = vec![
            Finding { source: "test".into(), severity: FindingSeverity::Fail, message: "2 tests failed".into() },
            Finding { source: "git".into(), severity: FindingSeverity::Info, message: "3 files changed".into() },
        ];
        let p = fix_prompt(&ctx, &task(), &[], &findings);
        assert!(p.contains("[FAIL] test: 2 tests failed"));
        assert!(!p.contains("3 files changed"));
    }
}
