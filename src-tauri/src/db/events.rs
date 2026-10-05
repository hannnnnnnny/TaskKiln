use rusqlite::{params, Row};

use super::{now, Db};
use crate::error::AppResult;
use crate::models::{LogLine, TaskEvent};

/// Cap per-task log rows so a chatty Claude run cannot grow the DB unbounded.
const MAX_LOG_ROWS_PER_TASK: i64 = 20_000;

fn row_to_event(r: &Row) -> rusqlite::Result<TaskEvent> {
    Ok(TaskEvent {
        id: r.get("id")?,
        task_id: r.get("task_id")?,
        kind: r.get("kind")?,
        message: r.get("message")?,
        created_at: r.get("created_at")?,
    })
}

fn row_to_log(r: &Row) -> rusqlite::Result<LogLine> {
    Ok(LogLine {
        id: r.get("id")?,
        task_id: r.get("task_id")?,
        stream: r.get("stream")?,
        line: r.get("line")?,
        created_at: r.get("created_at")?,
    })
}

impl Db {
    /// Messages must already be sanitized by the caller (see `logging::sanitize`).
    pub fn add_event(&self, task_id: Option<&str>, kind: &str, message: &str) -> AppResult<TaskEvent> {
        let ts = now();
        self.conn().execute(
            "INSERT INTO task_events (task_id, kind, message, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![task_id, kind, message, ts],
        )?;
        Ok(TaskEvent {
            id: self.conn().last_insert_rowid(),
            task_id: task_id.map(str::to_string),
            kind: kind.into(),
            message: message.into(),
            created_at: ts,
        })
    }

    pub fn list_events(&self, task_id: Option<&str>, limit: i64) -> AppResult<Vec<TaskEvent>> {
        let mut stmt = self.conn().prepare(
            "SELECT * FROM (SELECT * FROM task_events
               WHERE (?1 IS NULL OR task_id = ?1) ORDER BY id DESC LIMIT ?2)
             ORDER BY id",
        )?;
        let rows = stmt.query_map(params![task_id, limit], row_to_event)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn latest_event_of_kind(&self, task_id: &str, kind: &str) -> AppResult<Option<TaskEvent>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn()
            .query_row(
                "SELECT * FROM task_events WHERE task_id = ?1 AND kind = ?2 ORDER BY id DESC LIMIT 1",
                params![task_id, kind],
                row_to_event,
            )
            .optional()?)
    }

    pub fn add_log(&self, task_id: &str, stream: &str, line: &str) -> AppResult<LogLine> {
        let ts = now();
        self.conn().execute(
            "INSERT INTO task_logs (task_id, stream, line, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![task_id, stream, line, ts],
        )?;
        let id = self.conn().last_insert_rowid();
        if id % 500 == 0 {
            self.trim_logs(task_id)?;
        }
        Ok(LogLine { id, task_id: task_id.into(), stream: stream.into(), line: line.into(), created_at: ts })
    }

    fn trim_logs(&self, task_id: &str) -> AppResult<()> {
        self.conn().execute(
            "DELETE FROM task_logs WHERE task_id = ?1 AND id <= (
                SELECT id FROM task_logs WHERE task_id = ?1 ORDER BY id DESC LIMIT 1 OFFSET ?2)",
            params![task_id, MAX_LOG_ROWS_PER_TASK],
        )?;
        Ok(())
    }

    /// Most recent `limit` log lines for a task, oldest first.
    pub fn list_logs(&self, task_id: &str, limit: i64) -> AppResult<Vec<LogLine>> {
        let mut stmt = self.conn().prepare(
            "SELECT * FROM (SELECT * FROM task_logs WHERE task_id = ?1 ORDER BY id DESC LIMIT ?2)
             ORDER BY id",
        )?;
        let rows = stmt.query_map(params![task_id, limit], row_to_log)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}
