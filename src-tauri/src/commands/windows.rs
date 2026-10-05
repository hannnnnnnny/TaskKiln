//! Window, settings and OS-integration commands.

use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use super::AppState;
use crate::db::lock;
use crate::error::{AppError, AppResult};
use crate::models::Settings;

pub const MAIN_WINDOW: &str = "main";
pub const BAR_WINDOW: &str = "bar";

/// Apply window-related settings (always-on-top, status bar visibility).
pub fn apply_window_settings(app: &AppHandle, settings: &Settings) {
    if let Some(bar) = app.get_webview_window(BAR_WINDOW) {
        let _ = bar.set_always_on_top(settings.always_on_top);
        let _ = if settings.show_status_bar { bar.show() } else { bar.hide() };
    }
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    let previous_path = lock(&state.engine.db)?.get_settings()?.claude_path;
    let saved = lock(&state.engine.db)?.save_settings(&settings)?;
    apply_window_settings(&app, &saved);
    if saved.claude_path != previous_path {
        state.engine.refresh_claude().await?;
    }
    state.engine.changed();
    Ok(saved)
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> AppResult<()> {
    let w = app
        .get_webview_window(MAIN_WINDOW)
        .ok_or_else(|| AppError::NotFound("Main window is unavailable".into()))?;
    let _ = w.unminimize();
    w.show().map_err(|e| AppError::Io(e.to_string()))?;
    w.set_focus().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn open_project_folder(app: AppHandle, state: State<'_, AppState>, project_id: String) -> AppResult<()> {
    let project = lock(&state.engine.db)?.get_project(&project_id)?;
    if !project.path_exists {
        return Err(AppError::InvalidInput("Project directory no longer exists".into()));
    }
    app.opener()
        .open_path(&project.path, None::<&str>)
        .map_err(|e| AppError::Io(e.to_string()))
}

/// Quit after the user confirmed in the UI. Exiting drops every job object,
/// which terminates any Claude/validation process tree still running.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
