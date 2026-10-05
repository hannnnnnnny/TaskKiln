//! One task's execution: PLANNING -> RUNNING -> TESTING -> VALIDATING.
//!
//! Every phase ends in a definite state: the next phase, PAUSED (user stop),
//! NEEDS_USER (failure), or a validation verdict.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::core::{self, RunMode, Settled};
use super::engine::Engine;
use super::progress;
use crate::claude::plan::{structured, CompletionReport, DraftCriteria, Plan, Review};
use crate::claude::prompts::{self, ProjectContext, ReviewInput};
use crate::claude::stream::{checkpoint_markers, MarkerKind, StreamEvent};
use crate::claude::{ClaudeOutcome, ClaudeRequest, ClaudeRunner, SessionMode};
use crate::db::{lock, new_id, now};
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::process::{CancelToken, Stream};
use crate::validation::aggregate::{aggregate, ReviewState, ValidationInput};
use crate::validation::{self, git};

const PLAN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const EXECUTE_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const REVIEW_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const DRAFT_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const READ_ONLY_TOOLS: &str = "Read Grep Glob";

/// Everything a pipeline phase needs, loaded once per run.
struct Ctx {
    task: Task,
    project: Project,
    settings: Settings,
    commands: Vec<ValidationCommand>,
    runner: ClaudeRunner,
}

impl Ctx {
    fn load(engine: &Engine, task_id: &str) -> AppResult<Self> {
        let runner = engine.runner()?;
        let db = lock(&engine.db)?;
        let task = db.get_task(task_id)?;
        let project = db.get_project(&task.project_id)?;
        if !project.path_exists {
            return Err(AppError::InvalidInput(format!("Project directory is missing: {}", project.path)));
        }
        Ok(Self {
            settings: db.get_settings()?,
            commands: db.list_validation_commands(&project.id)?,
            task,
            project,
            runner,
        })
    }

    fn id(&self) -> &str {
        &self.task.id
    }

    fn project_ctx(&self) -> ProjectContext<'_> {
        ProjectContext {
            name: &self.project.name,
            path: &self.project.path,
            is_git_repo: self.project.is_git_repo,
            commands: &self.commands,
        }
    }

    fn request(&self, prompt: String, session: SessionMode, schema: &'static str, tools: Option<&str>,
               permission_mode: Option<String>, timeout: Duration) -> ClaudeRequest {
        ClaudeRequest {
            cwd: self.project.path.clone().into(),
            prompt,
            session,
            json_schema: Some(schema),
            tools: tools.map(str::to_string),
            permission_mode,
            allowed_tools: if tools.is_some() { String::new() } else { self.settings.allowed_tools.clone() },
            disallowed_tools: self.settings.disallowed_tools.clone(),
            model: self.settings.model.clone(),
            timeout,
        }
    }
}

pub async fn run(engine: &Arc<Engine>, task_id: &str, mode: RunMode, cancel: &CancelToken) -> AppResult<Option<Settled>> {
    let ctx = match Ctx::load(engine, task_id) {
        Ok(c) => c,
        Err(e) => {
            flag_failure(engine, task_id, AttentionReason::ClaudeFailed, &e.to_string())?;
            return Ok(None);
        }
    };
    let mut exec: Option<ExecResult> = None;
    if mode == RunMode::Fresh && !plan(engine, &ctx, cancel).await? {
        return Ok(None);
    }
    if mode != RunMode::ValidateOnly {
        match execute(engine, &ctx, &mode, cancel).await? {
            Some(e) => exec = Some(e),
            None => return Ok(None),
        }
    }
    let Some(commands) = testing(engine, &ctx, cancel).await? else { return Ok(None) };
    validating(engine, &ctx, exec, commands, cancel).await
}

// ---------------------------------------------------------------- helpers

fn pause(engine: &Engine, task_id: &str, why: &str) -> AppResult<()> {
    {
        let db = lock(&engine.db)?;
        db.transition(task_id, TaskStatus::Paused)?;
        db.set_activity(task_id, None)?;
    }
    engine.event(Some(task_id), "TASK_PAUSED", why);
    engine.changed();
    Ok(())
}

fn flag_failure(engine: &Engine, task_id: &str, reason: AttentionReason, detail: &str) -> AppResult<()> {
    let detail = crate::logging::sanitize(detail);
    {
        let db = lock(&engine.db)?;
        let task = db.get_task(task_id)?;
        if task.status.can_transition_to(TaskStatus::NeedsUser) {
            db.require_attention(task_id, reason, &detail)?;
        }
        // Record the failure as a FAIL validation so history and fix prompts include it.
        db.insert_validation(&ValidationResult {
            id: new_id(),
            task_id: task_id.into(),
            status: ValidationStatus::Fail,
            summary: format!("FAIL: {detail}"),
            findings: vec![Finding { source: "claude".into(), severity: FindingSeverity::Fail, message: detail.clone() }],
            commands: vec![],
            changed_files: vec![],
            created_at: now(),
        })?;
        db.set_validation_status(task_id, Some(ValidationStatus::Fail))?;
    }
    engine.event(Some(task_id), "TASK_FAILED", &detail);
    let title = match reason {
        AttentionReason::ClaudeFailed | AttentionReason::PlanFailed => "Claude process failed",
        _ => "Task needs attention",
    };
    engine.notify(title, &detail);
    engine.changed();
    Ok(())
}

/// Tracks checkpoint markers in Claude's text and keeps progress honest.
#[derive(Default)]
struct Tracker {
    claude_checkpoints: i64,
    any_marker: bool,
    unreliable: bool,
    tool_calls_without_marker: u32,
}

impl Tracker {
    fn new(engine: &Engine, task_id: &str) -> Self {
        let n = lock(&engine.db)
            .and_then(|db| db.list_checkpoints(task_id))
            .map(|c| c.iter().filter(|c| c.owner == "claude").count() as i64)
            .unwrap_or(0);
        Self { claude_checkpoints: n, ..Default::default() }
    }

    fn on_text(&mut self, engine: &Engine, task_id: &str, text: &str) {
        for (kind, n) in checkpoint_markers(text) {
            if n < 1 || n > self.claude_checkpoints {
                continue;
            }
            self.any_marker = true;
            self.unreliable = false;
            self.tool_calls_without_marker = 0;
            let (status, ev) = match kind {
                MarkerKind::Start => (CheckpointStatus::Running, "CHECKPOINT_STARTED"),
                MarkerKind::Done => (CheckpointStatus::Completed, "CHECKPOINT_COMPLETED"),
                MarkerKind::Failed => (CheckpointStatus::Failed, "CHECKPOINT_FAILED"),
            };
            let title = update_checkpoint(engine, task_id, n, status);
            engine.event(Some(task_id), ev, &format!("{n}. {title}"));
        }
    }

    fn on_tool(&mut self, engine: &Engine, task_id: &str) {
        self.tool_calls_without_marker += 1;
        if !self.any_marker && !self.unreliable && self.tool_calls_without_marker >= progress::UNRELIABLE_AFTER_TOOL_CALLS {
            self.unreliable = true;
            if let Ok(db) = lock(&engine.db) {
                let _ = db.set_progress(task_id, None);
            }
            engine.event(Some(task_id), "PROGRESS_UNAVAILABLE", "Claude is not reporting checkpoints; progress cannot be determined");
        }
    }
}

/// Set a checkpoint's status and recompute task progress. Returns its title.
fn update_checkpoint(engine: &Engine, task_id: &str, ordinal: i64, status: CheckpointStatus) -> String {
    let Ok(db) = lock(&engine.db) else { return String::new() };
    let _ = db.set_checkpoint_status(task_id, ordinal, status);
    let cps = db.list_checkpoints(task_id).unwrap_or_default();
    let _ = db.set_progress(task_id, progress::compute(&cps));
    cps.into_iter().find(|c| c.ordinal == ordinal).map(|c| c.title).unwrap_or_default()
}

fn render_event(engine: &Engine, task_id: &str, ev: &StreamEvent, tracker: Option<&mut Tracker>) {
    match ev {
        StreamEvent::Init { session_id, model } => engine.log(
            task_id,
            "system",
            &format!("session {session_id} · model {}", model.as_deref().unwrap_or("default")),
        ),
        StreamEvent::Text(t) => {
            for line in t.lines() {
                engine.log(task_id, "claude", line);
            }
            if let Some(tr) = tracker {
                tr.on_text(engine, task_id, t);
            }
            engine.changed();
        }
        StreamEvent::ToolUse { summary, .. } => {
            engine.log(task_id, "tool", &format!("→ {summary}"));
            if let Ok(db) = lock(&engine.db) {
                let _ = db.set_activity(task_id, Some(summary));
            }
            if let Some(tr) = tracker {
                tr.on_tool(engine, task_id);
            }
            engine.changed();
        }
        StreamEvent::ToolResult { is_error: true, preview } => {
            engine.log(task_id, "tool", &format!("✗ {}", preview.lines().next().unwrap_or("")));
        }
        StreamEvent::ToolResult { .. } => {}
        StreamEvent::Result(r) => engine.log(
            task_id,
            "result",
            &format!(
                "{} · {} turn(s){}",
                if r.is_error { "ERROR" } else { "finished" },
                r.num_turns.unwrap_or(0),
                r.cost_usd.map(|c| format!(" · ${c:.2}")).unwrap_or_default()
            ),
        ),
    }
}

async fn run_claude(engine: &Engine, ctx: &Ctx, req: &ClaudeRequest, cancel: &CancelToken, track: bool)
    -> AppResult<ClaudeOutcome> {
    let id = ctx.id().to_string();
    engine.event(Some(&id), "CLAUDE_STARTED", &ctx.runner.spec(req).describe());
    let mut tracker = track.then(|| Tracker::new(engine, &id));
    let outcome = ctx
        .runner
        .run(
            req,
            cancel,
            |ev| render_event(engine, &id, ev, tracker.as_mut()),
            |stream, line| match stream {
                Stream::Stderr => engine.log(&id, "stderr", line),
                Stream::Stdout if !line.trim_start().starts_with('{') => engine.log(&id, "stdout", line),
                Stream::Stdout => {}
            },
        )
        .await?;
    engine.event(
        Some(&id),
        "CLAUDE_EXITED",
        &format!("exit code {}", outcome.process.exit_code.map_or("none".into(), |c| c.to_string())),
    );
    Ok(outcome)
}

// ---------------------------------------------------------------- phases

async fn capture_baseline(engine: &Engine, ctx: &Ctx) {
    if !ctx.project.is_git_repo {
        return;
    }
    let has = lock(&engine.db).ok().and_then(|db| db.git_baseline(ctx.id()).ok().flatten()).is_some();
    if has {
        return;
    }
    match git::snapshot(Path::new(&ctx.project.path)).await {
        Ok(snap) => {
            if let (Ok(db), Ok(json)) = (lock(&engine.db), serde_json::to_string(&snap)) {
                let _ = db.set_git_baseline(ctx.id(), &json);
            }
        }
        Err(e) => engine.event(Some(ctx.id()), "GIT_UNAVAILABLE", &e.to_string()),
    }
}

/// PLANNING. Returns false when the pipeline must stop.
async fn plan(engine: &Engine, ctx: &Ctx, cancel: &CancelToken) -> AppResult<bool> {
    let id = ctx.id();
    let session = new_id();
    {
        let db = lock(&engine.db)?;
        if db.get_task(id)?.status == TaskStatus::Queued {
            db.transition(id, TaskStatus::Planning)?;
        }
        db.set_session_id(id, &session)?;
        db.set_activity(id, Some("planning"))?;
    }
    engine.event(Some(id), "TASK_STARTED", &ctx.task.title);
    engine.changed();
    capture_baseline(engine, ctx).await;

    let prompt = prompts::plan_prompt(&ctx.project_ctx(), &ctx.task);
    let req = ctx.request(prompt, SessionMode::New(session), prompts::PLAN_SCHEMA, Some(READ_ONLY_TOOLS), None, PLAN_TIMEOUT);
    let out = run_claude(engine, ctx, &req, cancel, false).await?;
    if out.process.cancelled {
        pause(engine, id, "Stopped during planning")?;
        return Ok(false);
    }
    if !out.succeeded() {
        flag_failure(engine, id, AttentionReason::PlanFailed, &format!("Planning failed: {}", out.failure_reason()))?;
        return Ok(false);
    }
    let steps = out.result.as_ref().and_then(structured::<Plan>).map(|p| p.normalized()).unwrap_or_default();
    if steps.is_empty() {
        flag_failure(engine, id, AttentionReason::PlanFailed, "Claude did not return a usable checkpoint plan")?;
        return Ok(false);
    }
    {
        let db = lock(&engine.db)?;
        let cps = db.replace_checkpoints(id, &steps)?;
        db.set_progress(id, progress::compute(&cps))?;
        db.transition(id, TaskStatus::Running)?;
    }
    let titles: Vec<String> = steps.iter().enumerate().map(|(i, (t, w))| format!("{}. {t} (w{w})", i + 1)).collect();
    engine.event(Some(id), "PLAN_CREATED", &titles.join(" | "));
    engine.changed();
    Ok(true)
}

struct ExecResult {
    completion: Option<CompletionReport>,
    denials: Vec<String>,
}

fn latest_findings(engine: &Engine, task_id: &str) -> Vec<Finding> {
    lock(&engine.db)
        .ok()
        .and_then(|db| db.latest_validation(task_id).ok().flatten())
        .map(|v| v.findings)
        .unwrap_or_default()
}

fn execution_prompt(engine: &Engine, ctx: &Ctx, mode: &RunMode) -> AppResult<String> {
    let cps = lock(&engine.db)?.list_checkpoints(ctx.id())?;
    let pc = ctx.project_ctx();
    Ok(match mode {
        RunMode::Fix => prompts::fix_prompt(&pc, &ctx.task, &cps, &latest_findings(engine, ctx.id())),
        RunMode::Reconcile => prompts::reconcile_prompt(&pc, &ctx.task, &cps, &latest_findings(engine, ctx.id())),
        RunMode::Continue => prompts::resume_prompt(&pc, &ctx.task, &cps),
        _ => prompts::execute_prompt(&pc, &ctx.task, &cps),
    })
}

/// RUNNING. Returns None when the pipeline must stop.
async fn execute(engine: &Engine, ctx: &Ctx, mode: &RunMode, cancel: &CancelToken) -> AppResult<Option<ExecResult>> {
    let id = ctx.id();
    let prompt = execution_prompt(engine, ctx, mode)?;
    let session = lock(&engine.db)?.get_task(id)?.claude_session_id;
    let session = match session {
        Some(s) => SessionMode::Resume(s),
        None => new_session(engine, id)?,
    };
    let perm = Some(ctx.settings.permission_mode.clone());
    let mut req = ctx.request(prompt.clone(), session, prompts::COMPLETION_SCHEMA, None, perm, EXECUTE_TIMEOUT);
    engine.changed();
    let mut out = run_claude(engine, ctx, &req, cancel, true).await?;
    if out.resume_failed && !cancel.is_cancelled() {
        engine.event(Some(id), "SESSION_RESUME_FAILED", "Claude session could not be resumed; starting a new session with full task context");
        req.session = new_session(engine, id)?;
        req.prompt = format!("(TaskKiln note: the previous Claude session for this task could not be resumed.)\n\n{prompt}");
        out = run_claude(engine, ctx, &req, cancel, true).await?;
    }
    if out.process.cancelled {
        pause(engine, id, "Stopped by user while Claude was running")?;
        return Ok(None);
    }
    if !out.succeeded() {
        flag_failure(engine, id, AttentionReason::ClaudeFailed, &out.failure_reason())?;
        return Ok(None);
    }
    let result = out.result.unwrap_or_default();
    let completion: Option<CompletionReport> = structured(&result);
    if let Some(c) = &completion {
        engine.event(Some(id), "CLAUDE_COMPLETION", &serde_json::to_string(c).unwrap_or_default());
        apply_checkpoint_results(engine, id, c);
    }
    Ok(Some(ExecResult { completion, denials: result.permission_denials }))
}

fn new_session(engine: &Engine, task_id: &str) -> AppResult<SessionMode> {
    let s = new_id();
    lock(&engine.db)?.set_session_id(task_id, &s)?;
    Ok(SessionMode::New(s))
}

/// Fill in checkpoint states Claude reported at the end but never marked live.
fn apply_checkpoint_results(engine: &Engine, task_id: &str, c: &CompletionReport) {
    let Ok(cps) = lock(&engine.db).and_then(|db| db.list_checkpoints(task_id)) else { return };
    for r in &c.checkpoint_results {
        let Some(cp) = cps.iter().find(|cp| cp.ordinal == r.index && cp.owner == "claude") else { continue };
        let status = match r.status.as_str() {
            "completed" => CheckpointStatus::Completed,
            "failed" => CheckpointStatus::Failed,
            _ => continue,
        };
        if cp.status != status {
            update_checkpoint(engine, task_id, r.index, status);
        }
    }
}

struct CommandsResult {
    outcomes: Vec<CommandOutcome>,
    missing: Vec<String>,
    unapproved: Vec<String>,
}

fn check_enabled(settings: &Settings, kind: &str) -> bool {
    match kind {
        "build" => settings.check_build,
        "test" => settings.check_test,
        "lint" => settings.check_lint,
        _ => false,
    }
}

/// TESTING: run approved build/test/lint commands. None when stopped.
async fn testing(engine: &Engine, ctx: &Ctx, cancel: &CancelToken) -> AppResult<Option<CommandsResult>> {
    let id = ctx.id();
    {
        let db = lock(&engine.db)?;
        if db.get_task(id)?.status != TaskStatus::Testing {
            db.transition(id, TaskStatus::Testing)?;
        }
    }
    engine.event(Some(id), "VALIDATION_STARTED", "Running build/test/lint checks");
    engine.changed();
    let mut res = CommandsResult { outcomes: vec![], missing: vec![], unapproved: vec![] };
    for kind in ["build", "test", "lint"] {
        if !check_enabled(&ctx.settings, kind) {
            continue;
        }
        let cmds: Vec<&ValidationCommand> = ctx.commands.iter().filter(|c| c.kind == kind && c.enabled).collect();
        if cmds.is_empty() {
            res.missing.push(kind.into());
        }
        for c in cmds {
            let label = format!("{} {}", c.program, c.args.join(" "));
            if !c.approved {
                res.unapproved.push(label);
                continue;
            }
            let _ = lock(&engine.db).map(|db| db.set_activity(id, Some(&format!("running {label}"))));
            engine.event(Some(id), "COMMAND_EXECUTED", &label);
            engine.changed();
            let outcome = validation::run_command(c, Path::new(&ctx.project.path), cancel, |_, l| engine.log(id, "validation", l)).await;
            if cancel.is_cancelled() {
                pause(engine, id, "Stopped by user during validation")?;
                return Ok(None);
            }
            engine.event(Some(id), "COMMAND_RESULT", &format!("{label}: {}", if outcome.success { "passed" } else { "FAILED" }));
            res.outcomes.push(outcome);
        }
    }
    Ok(Some(res))
}

async fn changed_files(engine: &Engine, ctx: &Ctx) -> (bool, Vec<String>) {
    if !ctx.project.is_git_repo {
        return (false, vec![]);
    }
    let root = Path::new(&ctx.project.path);
    let baseline: git::Snapshot = lock(&engine.db)
        .ok()
        .and_then(|db| db.git_baseline(ctx.id()).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    match git::snapshot(root).await {
        Ok(now_snap) => (true, git::changed_since(&baseline, &now_snap)),
        Err(e) => {
            engine.event(Some(ctx.id()), "GIT_UNAVAILABLE", &e.to_string());
            (false, vec![])
        }
    }
}

fn command_summary(outcomes: &[CommandOutcome]) -> String {
    if outcomes.is_empty() {
        return "(no validation commands were run)".into();
    }
    outcomes
        .iter()
        .map(|o| {
            let tail: String = o.output_tail.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
            format!("$ {} {} -> {}\n{}", o.program, o.args.join(" "), if o.success { "passed" } else { "FAILED" }, tail)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

async fn review(engine: &Engine, ctx: &Ctx, input: &ReviewInput<'_>, cancel: &CancelToken) -> AppResult<Option<ReviewState>> {
    if !ctx.settings.claude_review {
        return Ok(Some(ReviewState::Disabled));
    }
    let _ = lock(&engine.db).map(|db| db.set_activity(ctx.id(), Some("Claude reviewing acceptance criteria")));
    engine.changed();
    let task = lock(&engine.db)?.get_task(ctx.id())?;
    let prompt = prompts::review_prompt(&ctx.project_ctx(), &task, input);
    let req = ctx.request(prompt, SessionMode::Ephemeral, prompts::REVIEW_SCHEMA, Some(READ_ONLY_TOOLS), None, REVIEW_TIMEOUT);
    let out = run_claude(engine, ctx, &req, cancel, false).await?;
    if out.process.cancelled {
        return Ok(None);
    }
    if !out.succeeded() {
        return Ok(Some(ReviewState::Unavailable(out.failure_reason())));
    }
    Ok(Some(match out.result.as_ref().and_then(structured::<Review>) {
        Some(r) => ReviewState::Done(r),
        None => ReviewState::Unavailable("reviewer returned no structured verdict".into()),
    }))
}

fn stored_completion(engine: &Engine, task_id: &str) -> Option<CompletionReport> {
    let ev = lock(&engine.db).ok()?.latest_event_of_kind(task_id, "CLAUDE_COMPLETION").ok()??;
    serde_json::from_str(&ev.message).ok()
}

/// VALIDATING: change detection, criteria review, verdict, settle.
async fn validating(engine: &Engine, ctx: &Ctx, exec: Option<ExecResult>, cmds: CommandsResult, cancel: &CancelToken)
    -> AppResult<Option<Settled>> {
    let id = ctx.id();
    lock(&engine.db)?.transition(id, TaskStatus::Validating)?;
    engine.changed();
    let (is_git, changed) = changed_files(engine, ctx).await;
    let (stat, diff) = git::diff_for(Path::new(&ctx.project.path), &changed).await;
    let (completion, denials) = match exec {
        Some(e) => (e.completion, e.denials),
        None => (stored_completion(engine, id), vec![]),
    };
    let summary_text = completion.as_ref().map(|c| c.summary.clone()).unwrap_or_default();
    let cmd_summary = command_summary(&cmds.outcomes);
    let input = ReviewInput { diff_stat: &stat, diff: &diff, changed_files: &changed, command_summary: &cmd_summary, claude_summary: &summary_text };
    let Some(review_state) = review(engine, ctx, &input, cancel).await? else {
        pause(engine, id, "Stopped by user during review")?;
        return Ok(None);
    };
    let vin = ValidationInput {
        claude_failure: None,
        completion,
        permission_denials: denials,
        is_git_repo: is_git,
        changed_files: changed.clone(),
        commands: cmds.outcomes.clone(),
        missing_checks: cmds.missing,
        unapproved: cmds.unapproved,
        review: review_state,
    };
    let (status, findings, summary) = aggregate(&vin);
    let result = ValidationResult {
        id: new_id(), task_id: id.into(), status, summary, findings,
        commands: cmds.outcomes, changed_files: changed, created_at: now(),
    };
    let settled = core::settle_validation(&*lock(&engine.db)?, &result)?;
    engine.event(Some(id), "VALIDATION_RESULT", &result.summary);
    engine.changed();
    Ok(Some(settled))
}

/// Ask Claude (no tools, no session) to draft acceptance criteria for review
/// by the user before the task is queued.
pub async fn draft_criteria(engine: &Engine, title: &str, description: &str) -> AppResult<Vec<String>> {
    let runner = engine.runner()?;
    let settings = lock(&engine.db)?.get_settings()?;
    let req = ClaudeRequest {
        cwd: std::env::temp_dir(),
        prompt: prompts::criteria_prompt(title, description),
        session: SessionMode::Ephemeral,
        json_schema: Some(prompts::CRITERIA_SCHEMA),
        tools: Some(String::new()),
        permission_mode: None,
        allowed_tools: String::new(),
        disallowed_tools: String::new(),
        model: settings.model,
        timeout: DRAFT_TIMEOUT,
    };
    let out = runner.run(&req, &CancelToken::new(), |_| {}, |_, _| {}).await?;
    if !out.succeeded() {
        return Err(AppError::Claude(out.failure_reason()));
    }
    out.result
        .as_ref()
        .and_then(structured::<DraftCriteria>)
        .map(|d| d.criteria.into_iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect())
        .ok_or_else(|| AppError::Claude("Claude did not return criteria".into()))
}
