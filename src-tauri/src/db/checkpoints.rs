use rusqlite::{params, Row};

use super::{new_id, now, Db};
use crate::error::AppResult;
use crate::models::{Checkpoint, CheckpointStatus};

pub const VALIDATION_CHECKPOINT_TITLE: &str = "TaskKiln validation";

fn row_to_checkpoint(r: &Row) -> rusqlite::Result<Checkpoint> {
    let status: String = r.get("status")?;
    Ok(Checkpoint {
        id: r.get("id")?,
        task_id: r.get("task_id")?,
        ordinal: r.get("ordinal")?,
        title: r.get("title")?,
        status: status.parse().unwrap_or(CheckpointStatus::Pending),
        weight: r.get("weight")?,
        owner: r.get("owner")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
    })
}

impl Db {
    /// Store Claude's plan as checkpoints 1..n, followed by the validation
    /// checkpoint that TaskKiln itself owns. Replaces any previous plan.
    pub fn replace_checkpoints(&self, task_id: &str, plan: &[(String, f64)]) -> AppResult<Vec<Checkpoint>> {
        let tx = self.conn().unchecked_transaction()?;
        tx.execute("DELETE FROM task_checkpoints WHERE task_id = ?1", [task_id])?;
        let insert =|ordinal: i64, title: &str, weight: f64, owner: &str| {
            tx.execute(
                "INSERT INTO task_checkpoints (id, task_id, ordinal, title, status, weight, owner)
                 VALUES (?1, ?2, ?3, ?4, 'PENDING', ?5, ?6)",
                params![new_id(), task_id, ordinal, title, weight, owner],
            )
        };
        for (i, (title, weight)) in plan.iter().enumerate() {
            insert(i as i64 + 1, title, *weight, "claude")?;
        }
        insert(plan.len() as i64 + 1, VALIDATION_CHECKPOINT_TITLE, 1.0, "taskkiln")?;
        tx.commit()?;
        self.list_checkpoints(task_id)
    }

    pub fn list_checkpoints(&self, task_id: &str) -> AppResult<Vec<Checkpoint>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT * FROM task_checkpoints WHERE task_id = ?1 ORDER BY ordinal")?;
        let rows = stmt.query_map([task_id], row_to_checkpoint)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn list_all_checkpoints(&self) -> AppResult<Vec<Checkpoint>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT * FROM task_checkpoints ORDER BY task_id, ordinal")?;
        let rows = stmt.query_map([], row_to_checkpoint)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Update one checkpoint's status. Returns false when no such ordinal exists
    /// (Claude referenced a checkpoint outside the plan), so callers can ignore it.
    pub fn set_checkpoint_status(&self, task_id: &str, ordinal: i64, status: CheckpointStatus) -> AppResult<bool> {
        let ts = now();
        let changed = self.conn().execute(
            "UPDATE task_checkpoints SET status = ?3,
                started_at = CASE WHEN ?3 = 'RUNNING' THEN COALESCE(started_at, ?4) ELSE started_at END,
                completed_at = CASE WHEN ?3 IN ('COMPLETED','FAILED') THEN ?4 ELSE NULL END
             WHERE task_id = ?1 AND ordinal = ?2",
            params![task_id, ordinal, status.as_str(), ts],
        )?;
        Ok(changed > 0)
    }

    /// Reset Claude-owned checkpoints that never finished (used before a fix round)
    /// and always reset the TaskKiln validation checkpoint.
    pub fn reset_unfinished_checkpoints(&self, task_id: &str) -> AppResult<()> {
        self.conn().execute(
            "UPDATE task_checkpoints SET status = 'PENDING', completed_at = NULL
             WHERE task_id = ?1 AND (owner = 'taskkiln' OR status IN ('RUNNING','FAILED'))",
            [task_id],
        )?;
        Ok(())
    }
}
