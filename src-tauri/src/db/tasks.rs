use rusqlite::{params, OptionalExtension, Row};

use super::{json_vec, new_id, now, Db};
use crate::error::{AppError, AppResult};
use crate::models::{AttentionReason, NewTask, Task, TaskStatus, TaskUpdate, ValidationStatus};

const MAX_TITLE: usize = 200;
const MAX_DESCRIPTION: usize = 20_000;
const MAX_CRITERIA: usize = 50;
const MAX_CRITERION: usize = 1_000;

fn parse_opt<T: std::str::FromStr>(v: Option<String>) -> Option<T> {
    v.and_then(|s| s.parse().ok())
}

fn row_to_task(r: &Row) -> rusqlite::Result<Task> {
    let status: String = r.get("status")?;
    let criteria: String = r.get("acceptance_criteria")?;
    Ok(Task {
        id: r.get("id")?,
        project_id: r.get("project_id")?,
        title: r.get("title")?,
        description: r.get("description")?,
        acceptance_criteria: json_vec(criteria).unwrap_or_default(),
        priority: r.get("priority")?,
        queue_position: r.get("queue_position")?,
        status: status.parse().unwrap_or(TaskStatus::Failed),
        progress: r.get("progress")?,
        created_at: r.get("created_at")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
        claude_session_id: r.get("claude_session_id")?,
        validation_status: parse_opt::<ValidationStatus>(r.get("validation_status")?),
        attention_reason: parse_opt::<AttentionReason>(r.get("attention_reason")?),
        attention_detail: r.get("attention_detail")?,
        current_activity: r.get("current_activity")?,
        fix_attempts: r.get("fix_attempts")?,
    })
}

/// Validate and normalize user-supplied task fields.
fn clean_fields(title: &str, description: &str, criteria: &[String], priority: i64)
    -> AppResult<(String, String, Vec<String>)> {
    let title = title.trim().to_string();
    if title.is_empty() || title.chars().count() > MAX_TITLE {
        return Err(AppError::InvalidInput(format!("Title must be 1-{MAX_TITLE} characters")));
    }
    if description.len() > MAX_DESCRIPTION {
        return Err(AppError::InvalidInput("Description is too long".into()));
    }
    if !(0..=3).contains(&priority) {
        return Err(AppError::InvalidInput("Priority must be between 0 and 3".into()));
    }
    let criteria: Vec<String> = criteria
        .iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    if criteria.len() > MAX_CRITERIA || criteria.iter().any(|c| c.len() > MAX_CRITERION) {
        return Err(AppError::InvalidInput("Too many or too long acceptance criteria".into()));
    }
    Ok((title, description.trim().to_string(), criteria))
}

impl Db {
    pub fn create_task(&self, input: &NewTask) -> AppResult<Task> {
        let (title, description, criteria) =
            clean_fields(&input.title, &input.description, &input.acceptance_criteria, input.priority)?;
        self.get_project(&input.project_id)?;
        let position = self.insert_position_for_priority(input.priority)?;
        let id = new_id();
        let tx = self.conn().unchecked_transaction()?;
        // Shift later tasks down to make room at `position`.
        tx.execute(
            "UPDATE tasks SET queue_position = queue_position + 1
             WHERE status = 'QUEUED' AND queue_position >= ?1",
            [position],
        )?;
        tx.execute(
            "INSERT INTO tasks (id, project_id, title, description, acceptance_criteria,
                priority, queue_position, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'QUEUED', ?8)",
            params![id, input.project_id, title, description,
                serde_json::to_string(&criteria)?, input.priority, position, now()],
        )?;
        tx.commit()?;
        self.get_task(&id)
    }

    /// New tasks go after every queued task of equal or higher priority, so
    /// priority shapes initial placement while manual reordering stays authoritative.
    fn insert_position_for_priority(&self, priority: i64) -> AppResult<i64> {
        let after: Option<i64> = self.conn().query_row(
            "SELECT MAX(queue_position) FROM tasks WHERE status = 'QUEUED' AND priority >= ?1",
            [priority],
            |r| r.get(0),
        )?;
        match after {
            Some(p) => Ok(p + 1),
            None => Ok(self
                .conn()
                .query_row(
                    "SELECT MIN(queue_position) FROM tasks WHERE status = 'QUEUED'",
                    [],
                    |r| r.get::<_, Option<i64>>(0),
                )?
                .unwrap_or(0)),
        }
    }

    pub fn update_task(&self, id: &str, input: &TaskUpdate) -> AppResult<Task> {
        let task = self.get_task(id)?;
        if task.status.is_active() {
            return Err(AppError::Conflict("A task cannot be edited while it is running".into()));
        }
        if task.status.is_terminal() {
            return Err(AppError::Conflict("Finished tasks cannot be edited".into()));
        }
        let (title, description, criteria) =
            clean_fields(&input.title, &input.description, &input.acceptance_criteria, input.priority)?;
        self.conn().execute(
            "UPDATE tasks SET title = ?2, description = ?3, acceptance_criteria = ?4, priority = ?5
             WHERE id = ?1",
            params![id, title, description, serde_json::to_string(&criteria)?, input.priority],
        )?;
        self.get_task(id)
    }

    pub fn delete_task(&self, id: &str) -> AppResult<()> {
        let task = self.get_task(id)?;
        if task.status.is_active() {
            return Err(AppError::Conflict("Stop the task before deleting it".into()));
        }
        self.conn().execute("DELETE FROM tasks WHERE id = ?1", [id])?;
        self.compact_queue()
    }

    pub fn get_task(&self, id: &str) -> AppResult<Task> {
        self.conn()
            .query_row("SELECT * FROM tasks WHERE id = ?1", [id], row_to_task)
            .optional()?
            .ok_or_else(|| AppError::NotFound("Task not found".into()))
    }

    pub fn list_tasks(&self) -> AppResult<Vec<Task>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT * FROM tasks ORDER BY queue_position, created_at")?;
        let rows = stmt.query_map([], row_to_task)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn tasks_with_status(&self, statuses: &[TaskStatus]) -> AppResult<Vec<Task>> {
        Ok(self
            .list_tasks()?
            .into_iter()
            .filter(|t| statuses.contains(&t.status))
            .collect())
    }

    /// The task the scheduler should start next: first in queue order.
    pub fn next_queued_task(&self) -> AppResult<Option<Task>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT * FROM tasks WHERE status = 'QUEUED'
                 ORDER BY queue_position, created_at LIMIT 1",
                [],
                row_to_task,
            )
            .optional()?)
    }

    /// Apply a full new ordering of the queued tasks. `ordered_ids` must contain
    /// exactly the currently queued task ids, so a stale UI cannot drop tasks.
    pub fn reorder_queue(&self, ordered_ids: &[String]) -> AppResult<()> {
        let queued = self.tasks_with_status(&[TaskStatus::Queued])?;
        let mut current: Vec<&str> = queued.iter().map(|t| t.id.as_str()).collect();
        let mut requested: Vec<&str> = ordered_ids.iter().map(String::as_str).collect();
        current.sort_unstable();
        requested.sort_unstable();
        if current != requested {
            return Err(AppError::Conflict("Queue changed; refresh and try again".into()));
        }
        let tx = self.conn().unchecked_transaction()?;
        for (pos, id) in ordered_ids.iter().enumerate() {
            tx.execute("UPDATE tasks SET queue_position = ?2 WHERE id = ?1", params![id, pos as i64])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn move_to_front(&self, id: &str) -> AppResult<()> {
        let task = self.get_task(id)?;
        if task.status != TaskStatus::Queued {
            return Err(AppError::Conflict("Only queued tasks can be moved".into()));
        }
        let mut ids: Vec<String> = self
            .tasks_with_status(&[TaskStatus::Queued])?
            .into_iter()
            .map(|t| t.id)
            .filter(|t| t != id)
            .collect();
        ids.insert(0, id.to_string());
        self.reorder_queue(&ids)
    }

    /// Renumber queued tasks 0..n while preserving their order.
    fn compact_queue(&self) -> AppResult<()> {
        let ids: Vec<String> = self
            .tasks_with_status(&[TaskStatus::Queued])?
            .into_iter()
            .map(|t| t.id)
            .collect();
        self.reorder_queue(&ids)
    }

    /// The only way task status changes. Rejects transitions the state machine
    /// does not allow and maintains started/completed timestamps.
    pub fn transition(&self, id: &str, next: TaskStatus) -> AppResult<Task> {
        let task = self.get_task(id)?;
        if !task.status.can_transition_to(next) {
            return Err(AppError::InvalidTransition(format!("{} -> {}", task.status, next)));
        }
        let ts = now();
        let started = if task.started_at.is_none() && next.is_active() { Some(ts.clone()) } else { None };
        let finished = matches!(next, TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled);
        self.conn().execute(
            "UPDATE tasks SET status = ?2,
                started_at = COALESCE(started_at, ?3),
                completed_at = CASE WHEN ?4 THEN ?5 ELSE NULL END,
                attention_reason = CASE WHEN ?2 = 'NEEDS_USER' THEN attention_reason ELSE NULL END,
                attention_detail = CASE WHEN ?2 = 'NEEDS_USER' THEN attention_detail ELSE NULL END
             WHERE id = ?1",
            params![id, next.as_str(), started, finished, ts],
        )?;
        if next == TaskStatus::Queued {
            self.place_at_end_of_queue(id)?;
        }
        self.get_task(id)
    }

    /// Move a task to NEEDS_USER and record why, in one step.
    pub fn require_attention(&self, id: &str, reason: AttentionReason, detail: &str) -> AppResult<Task> {
        self.transition(id, TaskStatus::NeedsUser)?;
        self.conn().execute(
            "UPDATE tasks SET attention_reason = ?2, attention_detail = ?3, current_activity = NULL
             WHERE id = ?1",
            params![id, reason.as_str(), detail],
        )?;
        self.get_task(id)
    }

    fn place_at_end_of_queue(&self, id: &str) -> AppResult<()> {
        let max: Option<i64> = self.conn().query_row(
            "SELECT MAX(queue_position) FROM tasks WHERE status = 'QUEUED' AND id != ?1",
            [id],
            |r| r.get(0),
        )?;
        self.conn().execute(
            "UPDATE tasks SET queue_position = ?2 WHERE id = ?1",
            params![id, max.map_or(0, |m| m + 1)],
        )?;
        Ok(())
    }

    pub fn set_session_id(&self, id: &str, session: &str) -> AppResult<()> {
        self.conn()
            .execute("UPDATE tasks SET claude_session_id = ?2 WHERE id = ?1", params![id, session])?;
        Ok(())
    }

    pub fn set_progress(&self, id: &str, progress: Option<f64>) -> AppResult<()> {
        self.conn()
            .execute("UPDATE tasks SET progress = ?2 WHERE id = ?1", params![id, progress])?;
        Ok(())
    }

    pub fn set_activity(&self, id: &str, activity: Option<&str>) -> AppResult<()> {
        self.conn()
            .execute("UPDATE tasks SET current_activity = ?2 WHERE id = ?1", params![id, activity])?;
        Ok(())
    }

    pub fn set_validation_status(&self, id: &str, status: Option<ValidationStatus>) -> AppResult<()> {
        self.conn().execute(
            "UPDATE tasks SET validation_status = ?2 WHERE id = ?1",
            params![id, status.map(|s| s.as_str())],
        )?;
        Ok(())
    }

    pub fn set_git_baseline(&self, id: &str, baseline: &str) -> AppResult<()> {
        self.conn()
            .execute("UPDATE tasks SET git_baseline = ?2 WHERE id = ?1", params![id, baseline])?;
        Ok(())
    }

    pub fn git_baseline(&self, id: &str) -> AppResult<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT git_baseline FROM tasks WHERE id = ?1", [id], |r| r.get(0))
            .optional()?
            .flatten())
    }

    pub fn increment_fix_attempts(&self, id: &str) -> AppResult<()> {
        self.conn()
            .execute("UPDATE tasks SET fix_attempts = fix_attempts + 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Replace acceptance criteria while a task waits on the user (ADJUST REQUIREMENT).
    pub fn set_criteria(&self, id: &str, criteria: &[String]) -> AppResult<Task> {
        let task = self.get_task(id)?;
        let (_, _, criteria) = clean_fields(&task.title, &task.description, criteria, task.priority)?;
        self.conn().execute(
            "UPDATE tasks SET acceptance_criteria = ?2 WHERE id = ?1",
            params![id, serde_json::to_string(&criteria)?],
        )?;
        self.get_task(id)
    }
}
