//! Deterministic scheduler decisions on top of the database. No processes,
//! no async: everything here is directly unit-testable.

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::models::{
    AttentionReason, CheckpointStatus, Task, TaskStatus, ValidationResult, ValidationStatus,
};

use super::progress;

/// How a task's pipeline should run when launched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunMode {
    /// Plan, implement, validate.
    Fresh,
    /// Continue an interrupted/stopped run in the same Claude session.
    Continue,
    /// Ask Claude to fix validation findings.
    Fix,
    /// Requirements changed; reconcile the implementation.
    Reconcile,
    /// Skip Claude; rerun TESTING + VALIDATING only.
    ValidateOnly,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    Start(Task),
    QueueComplete,
    Hold,
}

/// What to do once no task is active.
pub fn choose_next(db: &Db, queue_running: bool) -> AppResult<Next> {
    if !queue_running {
        return Ok(Next::Hold);
    }
    Ok(match db.next_queued_task()? {
        Some(t) => Next::Start(t),
        None => Next::QueueComplete,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    Completed,
    NeedsUser,
}

/// Persist a validation result and move the task to COMPLETED (PASS) or
/// NEEDS_USER (WARNING / FAIL). The queue must not advance on NeedsUser.
pub fn settle_validation(db: &Db, result: &ValidationResult) -> AppResult<Settled> {
    let id = result.task_id.as_str();
    db.insert_validation(result)?;
    db.set_validation_status(id, Some(result.status))?;
    let validation_ordinal = db
        .list_checkpoints(id)?
        .iter()
        .find(|c| c.owner == "taskkiln")
        .map(|c| c.ordinal);
    let passed = result.status == ValidationStatus::Pass;
    if let Some(ord) = validation_ordinal {
        let st = if passed { CheckpointStatus::Completed } else { CheckpointStatus::Failed };
        db.set_checkpoint_status(id, ord, st)?;
    }
    db.set_progress(id, progress::compute(&db.list_checkpoints(id)?))?;
    db.set_activity(id, None)?;
    if passed {
        db.transition(id, TaskStatus::Completed)?;
        db.add_event(Some(id), "VALIDATION_PASS", &result.summary)?;
        db.add_event(Some(id), "TASK_COMPLETED", "Validation passed")?;
        return Ok(Settled::Completed);
    }
    let (reason, kind) = match result.status {
        ValidationStatus::Fail => (AttentionReason::ValidationFail, "VALIDATION_FAIL"),
        _ => (AttentionReason::ValidationWarning, "VALIDATION_WARNING"),
    };
    db.require_attention(id, reason, &result.summary)?;
    db.add_event(Some(id), kind, &result.summary)?;
    Ok(Settled::NeedsUser)
}

/// Decisions the user can make about a task. Serialized from the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserAction {
    /// ASK CLAUDE TO FIX
    Fix,
    /// ADJUST REQUIREMENT with new acceptance criteria.
    Adjust { criteria: Vec<String> },
    /// IGNORE & CONTINUE: record the override and complete the task.
    Ignore,
    /// STOP QUEUE while the task awaits the user.
    StopQueue,
    /// Recovery: continue an interrupted/stopped task.
    Resume,
    /// Recovery: rerun validation only.
    RetryValidation,
    MarkFailed,
    ReturnToQueue,
    Cancel,
}

impl UserAction {
    /// Whether choosing this action means the user wants the queue to keep going.
    pub fn resumes_queue(&self) -> bool {
        matches!(self, Self::Fix | Self::Adjust { .. } | Self::Ignore | Self::Resume | Self::RetryValidation)
    }
}

fn require_status(task: &Task, allowed: &[TaskStatus], action: &str) -> AppResult<()> {
    if allowed.contains(&task.status) {
        Ok(())
    } else {
        Err(AppError::Conflict(format!("Cannot {action} a task that is {}", task.status)))
    }
}

fn is_validation_issue(task: &Task) -> bool {
    matches!(
        task.attention_reason,
        Some(AttentionReason::ValidationFail | AttentionReason::ValidationWarning | AttentionReason::ClaudeFailed)
    )
}

/// Apply a user decision to the task state. Returns the pipeline to launch, if any.
pub fn apply_user_action(db: &Db, task_id: &str, action: &UserAction) -> AppResult<Option<RunMode>> {
    use TaskStatus::*;
    let task = db.get_task(task_id)?;
    match action {
        UserAction::Fix | UserAction::Adjust { .. } => {
            require_status(&task, &[NeedsUser], "fix")?;
            if !is_validation_issue(&task) {
                return Err(AppError::Conflict("This task has no validation findings to fix".into()));
            }
            if let UserAction::Adjust { criteria } = action {
                db.set_criteria(task_id, criteria)?;
                db.add_event(Some(task_id), "REQUIREMENT_ADJUSTED", &criteria.join(" | "))?;
            }
            db.increment_fix_attempts(task_id)?;
            db.reset_unfinished_checkpoints(task_id)?;
            db.transition(task_id, Running)?;
            Ok(Some(if matches!(action, UserAction::Fix) { RunMode::Fix } else { RunMode::Reconcile }))
        }
        UserAction::Ignore => {
            require_status(&task, &[NeedsUser], "override")?;
            if !matches!(task.attention_reason, Some(AttentionReason::ValidationFail | AttentionReason::ValidationWarning)) {
                return Err(AppError::Conflict("Only validation results can be overridden".into()));
            }
            db.transition(task_id, Completed)?;
            db.set_validation_status(task_id, Some(ValidationStatus::Overridden))?;
            db.add_event(Some(task_id), "VALIDATION_OVERRIDDEN", "User chose IGNORE & CONTINUE")?;
            db.add_event(Some(task_id), "TASK_COMPLETED", "Completed with warning (validation overridden)")?;
            Ok(None)
        }
        UserAction::StopQueue => {
            require_status(&task, &[NeedsUser], "stop")?;
            db.transition(task_id, Paused)?;
            db.add_event(Some(task_id), "TASK_PAUSED", "Queue stopped by user")?;
            Ok(None)
        }
        UserAction::Resume => resume(db, &task),
        UserAction::RetryValidation => {
            require_status(&task, &[NeedsUser, Paused], "retry validation for")?;
            db.reset_unfinished_checkpoints(task_id)?;
            db.transition(task_id, Testing)?;
            Ok(Some(RunMode::ValidateOnly))
        }
        UserAction::MarkFailed => {
            require_status(&task, &[NeedsUser, Paused], "mark failed")?;
            db.transition(task_id, Failed)?;
            db.add_event(Some(task_id), "TASK_FAILED", "Marked failed by user")?;
            Ok(None)
        }
        UserAction::ReturnToQueue => {
            require_status(&task, &[NeedsUser, Paused, Failed], "requeue")?;
            db.transition(task_id, Queued)?;
            db.move_to_front(task_id)?;
            db.add_event(Some(task_id), "TASK_REQUEUED", "Returned to the front of the queue")?;
            Ok(None)
        }
        UserAction::Cancel => {
            require_status(&task, &[Queued, NeedsUser, Paused, Failed], "cancel")?;
            db.transition(task_id, Cancelled)?;
            db.add_event(Some(task_id), "TASK_CANCELLED", "Cancelled by user")?;
            Ok(None)
        }
    }
}

fn resume(db: &Db, task: &Task) -> AppResult<Option<RunMode>> {
    require_status(task, &[TaskStatus::NeedsUser, TaskStatus::Paused], "resume")?;
    let has_plan = db.list_checkpoints(&task.id)?.iter().any(|c| c.owner == "claude");
    db.add_event(Some(&task.id), "TASK_RESUMED", "Resumed by user")?;
    if has_plan && task.claude_session_id.is_some() {
        db.reset_unfinished_checkpoints(&task.id)?;
        db.transition(&task.id, TaskStatus::Running)?;
        Ok(Some(RunMode::Continue))
    } else {
        db.transition(&task.id, TaskStatus::Planning)?;
        Ok(Some(RunMode::Fresh))
    }
}

/// Startup recovery: any task left in an active state lost its process when
/// TaskKiln exited. Never assume it finished; flag it for the user instead.
pub fn recover_interrupted(db: &Db) -> AppResult<Vec<Task>> {
    let stuck = db.tasks_with_status(&[TaskStatus::Planning, TaskStatus::Running, TaskStatus::Testing, TaskStatus::Validating])?;
    let mut out = vec![];
    for t in stuck {
        let detail = format!(
            "TaskKiln closed while this task was {}. Its real state is unknown: Claude may have made partial changes.",
            t.status
        );
        let updated = db.require_attention(&t.id, AttentionReason::Interrupted, &detail)?;
        db.add_event(Some(&t.id), "TASK_INTERRUPTED", &detail)?;
        out.push(updated);
    }
    Ok(out)
}
