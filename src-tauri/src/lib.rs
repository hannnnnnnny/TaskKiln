pub mod claude;
pub mod commands;
pub mod db;
pub mod error;
pub mod logging;
pub mod models;
pub mod notifications;
pub mod process;
pub mod scheduler;
pub mod validation;

use std::sync::Arc;

use tauri::{Emitter, Manager, WindowEvent};

use commands::{AppState, MAIN_WINDOW};
use db::Db;
use scheduler::Engine;

/// Open the on-disk database, or fall back to an in-memory one so the UI can
/// still start and show a clear "database error" state instead of crashing.
fn open_db(app: &tauri::App) -> (Db, String, Option<String>) {
    let path = app.path().app_data_dir().map(|d| d.join("taskkiln.db"));
    let attempt = path
        .as_ref()
        .map_err(|e| e.to_string())
        .and_then(|p| Db::open(p).map_err(|e| e.to_string()));
    let shown = path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    match attempt {
        Ok(db) => (db, shown, None),
        Err(e) => {
            let mem = Db::open_in_memory().expect("in-memory SQLite is always available");
            (mem, shown, Some(format!("Could not open the TaskKiln database ({e}). Changes will not be saved this session.")))
        }
    }
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let (db, db_path, db_error) = open_db(app);
    let settings = db.get_settings().unwrap_or_default();
    let host = Arc::new(notifications::TauriHost { app: app.handle().clone() });
    let engine = Engine::new(db.shared(), host);
    if let Err(e) = engine.recover() {
        eprintln!("[taskkiln] recovery failed: {e}");
    }
    commands::apply_window_settings(app.handle(), &settings);
    let probe = Arc::clone(&engine);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = probe.refresh_claude().await {
            eprintln!("[taskkiln] claude detection failed: {e}");
        }
    });
    app.manage(AppState { engine, db_path, db_error });
    Ok(())
}

/// Closing the main window quits TaskKiln. While a task is active the close
/// is intercepted and the UI asks for confirmation first.
fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        let app = window.app_handle();
        let busy = app
            .try_state::<AppState>()
            .is_some_and(|s| s.engine.queue_info().active_task_id.is_some());
        api.prevent_close();
        if busy {
            let _ = window.emit("tk://confirm-quit", ());
        } else {
            app.exit(0);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(setup)
        .on_window_event(on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::refresh_claude,
            commands::add_project,
            commands::remove_project,
            commands::redetect_commands,
            commands::set_command_flags,
            commands::create_task,
            commands::update_task,
            commands::delete_task,
            commands::reorder_queue,
            commands::move_to_front,
            commands::draft_criteria,
            commands::start_queue,
            commands::pause_queue,
            commands::stop_task,
            commands::task_action,
            commands::get_task_log,
            commands::get_recent_events,
            commands::save_settings,
            commands::show_main_window,
            commands::open_project_folder,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running TaskKiln");
}
