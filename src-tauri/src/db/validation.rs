use rusqlite::{params, OptionalExtension, Row};

use super::{json_vec, new_id, Db};
use crate::error::{AppError, AppResult};
use crate::models::{ValidationCommand, ValidationResult, ValidationStatus};

fn row_to_result(r: &Row) -> rusqlite::Result<ValidationResult> {
    let status: String = r.get("status")?;
    let findings: String = r.get("findings")?;
    let commands: String = r.get("commands")?;
    let changed: String = r.get("changed_files")?;
    Ok(ValidationResult {
        id: r.get("id")?,
        task_id: r.get("task_id")?,
        status: status.parse().unwrap_or(ValidationStatus::Warning),
        summary: r.get("summary")?,
        findings: serde_json::from_str(&findings).unwrap_or_default(),
        commands: serde_json::from_str(&commands).unwrap_or_default(),
        changed_files: json_vec(changed).unwrap_or_default(),
        created_at: r.get("created_at")?,
    })
}

fn row_to_command(r: &Row) -> rusqlite::Result<ValidationCommand> {
    let args: String = r.get("args")?;
    Ok(ValidationCommand {
        id: r.get("id")?,
        project_id: r.get("project_id")?,
        kind: r.get("kind")?,
        program: r.get("program")?,
        args: json_vec(args).unwrap_or_default(),
        source: r.get("source")?,
        approved: r.get("approved")?,
        enabled: r.get("enabled")?,
    })
}

impl Db {
    pub fn insert_validation(&self, v: &ValidationResult) -> AppResult<()> {
        self.conn().execute(
            "INSERT INTO validation_results
                (id, task_id, status, summary, findings, commands, changed_files, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                v.id, v.task_id, v.status.as_str(), v.summary,
                serde_json::to_string(&v.findings)?,
                serde_json::to_string(&v.commands)?,
                serde_json::to_string(&v.changed_files)?,
                v.created_at
            ],
        )?;
        Ok(())
    }

    pub fn latest_validation(&self, task_id: &str) -> AppResult<Option<ValidationResult>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT * FROM validation_results WHERE task_id = ?1
                 ORDER BY created_at DESC, rowid DESC LIMIT 1",
                [task_id],
                row_to_result,
            )
            .optional()?)
    }

    pub fn list_validations(&self, task_id: &str) -> AppResult<Vec<ValidationResult>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT * FROM validation_results WHERE task_id = ?1 ORDER BY created_at, rowid")?;
        let rows = stmt.query_map([task_id], row_to_result)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn list_validation_commands(&self, project_id: &str) -> AppResult<Vec<ValidationCommand>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT * FROM validation_commands WHERE project_id = ?1 ORDER BY kind, rowid")?;
        let rows = stmt.query_map([project_id], row_to_command)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Replace detected commands, keeping the user's approval for any command
    /// whose program and args are unchanged (re-detection must not silently
    /// approve something new, nor revoke what was already confirmed).
    pub fn replace_validation_commands(&self, project_id: &str, detected: &[ValidationCommand])
        -> AppResult<Vec<ValidationCommand>> {
        let previous = self.list_validation_commands(project_id)?;
        let tx = self.conn().unchecked_transaction()?;
        tx.execute("DELETE FROM validation_commands WHERE project_id = ?1", [project_id])?;
        for c in detected {
            let prior = previous.iter().find(|p| p.program == c.program && p.args == c.args && p.kind == c.kind);
            tx.execute(
                "INSERT INTO validation_commands (id, project_id, kind, program, args, source, approved, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    new_id(), project_id, c.kind, c.program, serde_json::to_string(&c.args)?, c.source,
                    prior.map(|p| p.approved).unwrap_or(false),
                    prior.map(|p| p.enabled).unwrap_or(true)
                ],
            )?;
        }
        tx.commit()?;
        self.list_validation_commands(project_id)
    }

    pub fn set_command_flags(&self, id: &str, approved: bool, enabled: bool) -> AppResult<()> {
        let n = self.conn().execute(
            "UPDATE validation_commands SET approved = ?2, enabled = ?3 WHERE id = ?1",
            params![id, approved, enabled],
        )?;
        if n == 0 {
            return Err(AppError::NotFound("Validation command not found".into()));
        }
        Ok(())
    }
}
