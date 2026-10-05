use rusqlite::params;

use super::Db;
use crate::error::{AppError, AppResult};
use crate::models::Settings;

const PERMISSION_MODES: &[&str] = &["acceptEdits", "auto", "bypassPermissions", "dontAsk"];

impl Db {
    /// Settings are stored as one JSON value per key, then overlaid on the
    /// defaults, so new settings added in later versions get sane values.
    pub fn get_settings(&self) -> AppResult<Settings> {
        let mut stmt = self.conn().prepare("SELECT key, value FROM app_settings")?;
        let mut merged = serde_json::to_value(Settings::default())?;
        let obj = merged.as_object_mut().expect("Settings serializes to an object");
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (key, raw) = row?;
            if let (Some(slot), Ok(v)) = (obj.get_mut(&key), serde_json::from_str(&raw)) {
                *slot = v;
            }
        }
        // A stored value of the wrong type falls back to defaults rather than failing startup.
        Ok(serde_json::from_value(merged).unwrap_or_default())
    }

    pub fn save_settings(&self, settings: &Settings) -> AppResult<Settings> {
        validate(settings)?;
        let value = serde_json::to_value(settings)?;
        let tx = self.conn().unchecked_transaction()?;
        for (key, v) in value.as_object().expect("Settings serializes to an object") {
            tx.execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, v.to_string()],
            )?;
        }
        tx.commit()?;
        self.get_settings()
    }
}

fn validate(s: &Settings) -> AppResult<()> {
    if !PERMISSION_MODES.contains(&s.permission_mode.as_str()) {
        return Err(AppError::InvalidInput("Unsupported permission mode".into()));
    }
    let is_clean = |v: &str| !v.chars().any(|c| c == '\0' || c == '\n' || c == '\r');
    if !is_clean(&s.claude_path) || !is_clean(&s.model) {
        return Err(AppError::InvalidInput("Paths and model names must be a single line".into()));
    }
    if !s.claude_path.is_empty() && !std::path::Path::new(&s.claude_path).is_file() {
        return Err(AppError::InvalidInput("Custom Claude CLI path does not point to a file".into()));
    }
    if s.model.len() > 100 || !s.model.chars().all(|c| c.is_ascii_alphanumeric() || "-._[]".contains(c)) {
        return Err(AppError::InvalidInput("Model name contains unsupported characters".into()));
    }
    Ok(())
}
