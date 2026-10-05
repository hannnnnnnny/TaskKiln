//! Tauri IPC commands. Thin wrappers: validate input, call the DB/engine,
//! and return user-presentable errors (`AppError` serializes to {kind, message}).

mod windows;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::claude::ClaudeStatus;
use crate::db::lock;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::scheduler::{pipeline, Engine, QueueInfo, UserAction};
use crate::validation::detect::detect_commands;

pub use windows::*;

pub struct AppState {
    pub engine: Arc<Engine>,
    pub db_path: String,
    /// Set when the on-disk database could not be opened and TaskKiln is
    /// running on a temporary in-memory database instead.
    pub db_error: Option<String>,
}

#[derive(Serialize)]
pub struct Snapshot {
    projects: Vec<Project>,
    tasks: Vec<Task>,
    checkpoints: HashMap<String, Vec<Checkpoint>>,
    validations: HashMap<String, ValidationResult>,
    commands: HashMap<String, Vec<ValidationCommand>>,
    queue: QueueInfo,
    settings: Settings,
    claude: ClaudeStatus,
    db_path: String,
    db_error: Option<String>,
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> AppResult<Snapshot> {
    let db = lock(&state.engine.db)?;
    let tasks = db.list_tasks()?;
    let mut checkpoints: HashMap<String, Vec<Checkpoint>> = HashMap::new();
    for cp in db.list_all_checkpoints()? {
        checkpoints.entry(cp.task_id.clone()).or_default().push(cp);
    }
    let mut validations = HashMap::new();
    for t in tasks.iter().filter(|t| t.validation_status.is_some()) {
        if let Some(v) = db.latest_validation(&t.id)? {
            validations.insert(t.id.clone(), v);
        }
    }
    let projects = db.list_projects()?;
    let mut commands = HashMap::new();
    for p in &projects {
        commands.insert(p.id.clone(), db.list_validation_commands(&p.id)?);
    }
    Ok(Snapshot {
        projects,
        tasks,
        checkpoints,
        validations,
        commands,
        queue: state.engine.queue_info(),
        settings: db.get_settings()?,
        claude: state.engine.claude_status(),
        db_path: state.db_path.clone(),
        db_error: state.db_error.clone(),
    })
}

#[tauri::command]
pub async fn refresh_claude(state: State<'_, AppState>) -> AppResult<ClaudeStatus> {
    state.engine.refresh_claude().await
}

// ---------------------------------------------------------------- projects

#[tauri::command]
pub fn add_project(state: State<'_, AppState>, path: String) -> AppResult<Project> {
    let db = lock(&state.engine.db)?;
    let project = db.add_project(&path)?;
    let detected = detect_commands(Path::new(&project.path));
    db.replace_validation_commands(&project.id, &detected)?;
    db.add_event(None, "PROJECT_ADDED", &project.path)?;
    drop(db);
    state.engine.changed();
    Ok(project)
}

#[tauri::command]
pub fn remove_project(state: State<'_, AppState>, id: String) -> AppResult<()> {
    lock(&state.engine.db)?.remove_project(&id)?;
    state.engine.changed();
    Ok(())
}

#[tauri::command]
pub fn redetect_commands(state: State<'_, AppState>, project_id: String) -> AppResult<Vec<ValidationCommand>> {
    let db = lock(&state.engine.db)?;
    let project = db.get_project(&project_id)?;
    if !project.path_exists {
        return Err(AppError::InvalidInput("Project directory no longer exists".into()));
    }
    let cmds = db.replace_validation_commands(&project.id, &detect_commands(Path::new(&project.path)))?;
    drop(db);
    state.engine.changed();
    Ok(cmds)
}

#[tauri::command]
pub fn set_command_flags(state: State<'_, AppState>, id: String, approved: bool, enabled: bool) -> AppResult<()> {
    let db = lock(&state.engine.db)?;
    db.set_command_flags(&id, approved, enabled)?;
    if approved {
        db.add_event(None, "COMMAND_APPROVED", &id)?;
    }
    drop(db);
    state.engine.changed();
    Ok(())
}

// ---------------------------------------------------------------- tasks

#[tauri::command]
pub fn create_task(state: State<'_, AppState>, input: NewTask) -> AppResult<Task> {
    let db = lock(&state.engine.db)?;
    let task = db.create_task(&input)?;
    db.add_event(Some(&task.id), "TASK_CREATED", &task.title)?;
    drop(db);
    state.engine.changed();
    Ok(task)
}

#[tauri::command]
pub fn update_task(state: State<'_, AppState>, id: String, input: TaskUpdate) -> AppResult<Task> {
    let task = lock(&state.engine.db)?.update_task(&id, &input)?;
    state.engine.changed();
    Ok(task)
}

#[tauri::command]
pub fn delete_task(state: State<'_, AppState>, id: String) -> AppResult<()> {
    if state.engine.queue_info().active_task_id.as_deref() == Some(&id) {
        return Err(AppError::Conflict("Stop the task before deleting it".into()));
    }
    lock(&state.engine.db)?.delete_task(&id)?;
    state.engine.changed();
    Ok(())
}

#[tauri::command]
pub fn reorder_queue(state: State<'_, AppState>, ids: Vec<String>) -> AppResult<()> {
    lock(&state.engine.db)?.reorder_queue(&ids)?;
    state.engine.changed();
    Ok(())
}

#[tauri::command]
pub fn move_to_front(state: State<'_, AppState>, id: String) -> AppResult<()> {
    lock(&state.engine.db)?.move_to_front(&id)?;
    state.engine.changed();
    Ok(())
}

#[tauri::command]
pub async fn draft_criteria(state: State<'_, AppState>, title: String, description: String) -> AppResult<Vec<String>> {
    if title.trim().is_empty() {
        return Err(AppError::InvalidInput("Enter a title first".into()));
    }
    pipeline::draft_criteria(&state.engine, title.trim(), description.trim()).await
}

// ---------------------------------------------------------------- execution

#[tauri::command]
pub fn start_queue(state: State<'_, AppState>) -> AppResult<()> {
    state.engine.start_queue()
}

#[tauri::command]
pub fn pause_queue(state: State<'_, AppState>) {
    state.engine.pause_queue();
}

#[tauri::command]
pub fn stop_task(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.engine.stop_active(&id)
}

#[tauri::command]
pub fn task_action(state: State<'_, AppState>, id: String, action: UserAction) -> AppResult<()> {
    state.engine.user_action(&id, action)
}

#[derive(Serialize)]
pub struct TaskLog {
    events: Vec<TaskEvent>,
    logs: Vec<LogLine>,
    validations: Vec<ValidationResult>,
}

#[tauri::command]
pub fn get_task_log(state: State<'_, AppState>, task_id: String) -> AppResult<TaskLog> {
    let db = lock(&state.engine.db)?;
    Ok(TaskLog {
        events: db.list_events(Some(&task_id), 1000)?,
        logs: db.list_logs(&task_id, 3000)?,
        validations: db.list_validations(&task_id)?,
    })
}

#[tauri::command]
pub fn get_recent_events(state: State<'_, AppState>) -> AppResult<Vec<TaskEvent>> {
    lock(&state.engine.db)?.list_events(None, 300)
}
