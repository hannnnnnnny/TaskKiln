//! Schema migrations, tracked with SQLite's `user_version` pragma.
//! Append new migrations to the end of [`MIGRATIONS`]; never edit old ones.

use rusqlite::Connection;

use crate::error::AppResult;

const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    r#"
    CREATE TABLE projects (
        id          TEXT PRIMARY KEY,
        name        TEXT NOT NULL,
        path        TEXT NOT NULL UNIQUE,
        created_at  TEXT NOT NULL
    );

    CREATE TABLE tasks (
        id                   TEXT PRIMARY KEY,
        project_id           TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        title                TEXT NOT NULL,
        description          TEXT NOT NULL DEFAULT '',
        acceptance_criteria  TEXT NOT NULL DEFAULT '[]',
        priority             INTEGER NOT NULL DEFAULT 0,
        queue_position       INTEGER NOT NULL DEFAULT 0,
        status               TEXT NOT NULL,
        progress             REAL,
        created_at           TEXT NOT NULL,
        started_at           TEXT,
        completed_at         TEXT,
        claude_session_id    TEXT,
        validation_status    TEXT,
        attention_reason     TEXT,
        attention_detail     TEXT,
        current_activity     TEXT,
        fix_attempts         INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_tasks_status ON tasks(status, queue_position);

    CREATE TABLE task_checkpoints (
        id            TEXT PRIMARY KEY,
        task_id       TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        ordinal       INTEGER NOT NULL,
        title         TEXT NOT NULL,
        status        TEXT NOT NULL,
        weight        REAL NOT NULL DEFAULT 1,
        owner         TEXT NOT NULL DEFAULT 'claude',
        started_at    TEXT,
        completed_at  TEXT
    );
    CREATE INDEX idx_checkpoints_task ON task_checkpoints(task_id, ordinal);

    CREATE TABLE task_events (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        task_id     TEXT REFERENCES tasks(id) ON DELETE CASCADE,
        kind        TEXT NOT NULL,
        message     TEXT NOT NULL,
        created_at  TEXT NOT NULL
    );
    CREATE INDEX idx_events_task ON task_events(task_id, id);

    CREATE TABLE task_logs (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        stream      TEXT NOT NULL,
        line        TEXT NOT NULL,
        created_at  TEXT NOT NULL
    );
    CREATE INDEX idx_logs_task ON task_logs(task_id, id);

    CREATE TABLE validation_results (
        id             TEXT PRIMARY KEY,
        task_id        TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        status         TEXT NOT NULL,
        summary        TEXT NOT NULL,
        findings       TEXT NOT NULL DEFAULT '[]',
        commands       TEXT NOT NULL DEFAULT '[]',
        changed_files  TEXT NOT NULL DEFAULT '[]',
        created_at     TEXT NOT NULL
    );
    CREATE INDEX idx_validation_task ON validation_results(task_id, created_at);

    CREATE TABLE validation_commands (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        kind        TEXT NOT NULL,
        program     TEXT NOT NULL,
        args        TEXT NOT NULL DEFAULT '[]',
        source      TEXT NOT NULL,
        approved    INTEGER NOT NULL DEFAULT 0,
        enabled     INTEGER NOT NULL DEFAULT 1
    );

    CREATE TABLE app_settings (
        key    TEXT PRIMARY KEY,
        value  TEXT NOT NULL
    );
    "#,
];

pub fn run(conn: &Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let version = idx as i64 + 1;
        if version <= current {
            continue;
        }
        // Each migration and its version bump commit atomically.
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

pub fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}
