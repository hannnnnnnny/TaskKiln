//! SQLite persistence. All access goes through [`Db`], which owns one
//! connection; callers share it behind a mutex (`SharedDb`).

mod checkpoints;
mod events;
pub mod migrations;
mod projects;
mod settings;
mod tasks;
mod validation;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub struct Db {
    conn: Connection,
}

pub type SharedDb = Arc<Mutex<Db>>;

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> AppResult<Self> {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
        )?;
        migrations::run(&conn)?;
        Ok(Self { conn })
    }

    pub fn shared(self) -> SharedDb {
        Arc::new(Mutex::new(self))
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// Lock the shared DB, turning a poisoned mutex into a regular error rather
/// than propagating a panic from another thread.
pub fn lock(db: &SharedDb) -> AppResult<MutexGuard<'_, Db>> {
    db.lock().map_err(|_| AppError::Db("database lock poisoned".into()))
}

fn json_vec(raw: String) -> AppResult<Vec<String>> {
    Ok(serde_json::from_str(&raw)?)
}

#[cfg(test)]
mod tests;
