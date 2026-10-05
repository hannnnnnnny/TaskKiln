use std::path::Path;

use rusqlite::{params, OptionalExtension, Row};

use super::{new_id, now, Db};
use crate::error::{AppError, AppResult};
use crate::models::Project;

fn row_to_project(r: &Row) -> rusqlite::Result<Project> {
    let path: String = r.get("path")?;
    let p = Path::new(&path);
    Ok(Project {
        id: r.get("id")?,
        name: r.get("name")?,
        path_exists: p.is_dir(),
        is_git_repo: p.join(".git").exists(),
        path,
        created_at: r.get("created_at")?,
    })
}

impl Db {
    /// Register a project directory. The path must exist and be a directory;
    /// it is canonicalized so the same folder cannot be added twice.
    pub fn add_project(&self, path: &str) -> AppResult<Project> {
        let raw = Path::new(path.trim());
        if !raw.is_dir() {
            return Err(AppError::InvalidInput(format!(
                "Not a directory: {}",
                raw.display()
            )));
        }
        let canonical = dunce_canonical(raw)?;
        let name = Path::new(&canonical)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| canonical.clone());
        if let Some(existing) = self.find_project_by_path(&canonical)? {
            return Ok(existing);
        }
        let id = new_id();
        self.conn().execute(
            "INSERT INTO projects (id, name, path, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, name, canonical, now()],
        )?;
        self.get_project(&id)
    }

    pub fn get_project(&self, id: &str) -> AppResult<Project> {
        self.conn()
            .query_row("SELECT * FROM projects WHERE id = ?1", [id], row_to_project)
            .optional()?
            .ok_or_else(|| AppError::NotFound("Project not found".into()))
    }

    fn find_project_by_path(&self, path: &str) -> AppResult<Option<Project>> {
        Ok(self
            .conn()
            .query_row("SELECT * FROM projects WHERE path = ?1", [path], row_to_project)
            .optional()?)
    }

    pub fn list_projects(&self) -> AppResult<Vec<Project>> {
        let mut stmt = self.conn().prepare("SELECT * FROM projects ORDER BY created_at")?;
        let rows = stmt.query_map([], row_to_project)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Removes the project from TaskKiln only; files on disk are never touched.
    pub fn remove_project(&self, id: &str) -> AppResult<()> {
        let active: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM tasks WHERE project_id = ?1
             AND status IN ('PLANNING','RUNNING','TESTING','VALIDATING')",
            [id],
            |r| r.get(0),
        )?;
        if active > 0 {
            return Err(AppError::Conflict(
                "Stop the active task before removing its project".into(),
            ));
        }
        self.conn().execute("DELETE FROM projects WHERE id = ?1", [id])?;
        Ok(())
    }
}

/// Canonicalize without the Windows `\\?\` verbatim prefix, which confuses
/// child processes and users alike.
fn dunce_canonical(p: &Path) -> AppResult<String> {
    let c = std::fs::canonicalize(p)?;
    let s = c.to_string_lossy().to_string();
    Ok(s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s))
}
