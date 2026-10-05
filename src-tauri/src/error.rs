use serde::Serialize;

/// Errors surfaced to the UI. Each variant carries a short, user-readable
/// message; raw library errors are reduced to their display text.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Db(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("Invalid state change: {0}")]
    InvalidTransition(String),
    #[error("{0}")]
    Conflict(String),
    #[error("Claude Code: {0}")]
    Claude(String),
    #[error("Process error: {0}")]
    Process(String),
    #[error("Filesystem error: {0}")]
    Io(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Db(format!("corrupt JSON column: {e}"))
    }
}

#[derive(Serialize)]
struct WireError<'a> {
    kind: &'a str,
    message: String,
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let kind = match self {
            AppError::Db(_) => "db",
            AppError::NotFound(_) => "not_found",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::InvalidTransition(_) => "invalid_transition",
            AppError::Conflict(_) => "conflict",
            AppError::Claude(_) => "claude",
            AppError::Process(_) => "process",
            AppError::Io(_) => "io",
        };
        WireError { kind, message: self.to_string() }.serialize(s)
    }
}
