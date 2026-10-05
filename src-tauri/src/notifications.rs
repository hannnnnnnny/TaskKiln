//! Tauri-backed implementation of the engine's `Host`: UI events and native
//! desktop notifications.

use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::models::{LogLine, TaskEvent};
use crate::scheduler::Host;

pub const EVT_CHANGED: &str = "tk://changed";
pub const EVT_LOG: &str = "tk://log";
pub const EVT_EVENT: &str = "tk://event";

pub struct TauriHost {
    pub app: AppHandle,
}

impl Host for TauriHost {
    fn changed(&self) {
        // Emission only fails when no window is listening, which is harmless.
        let _ = self.app.emit(EVT_CHANGED, ());
    }

    fn log(&self, line: &LogLine) {
        let _ = self.app.emit(EVT_LOG, line);
    }

    fn event(&self, event: &TaskEvent) {
        let _ = self.app.emit(EVT_EVENT, event);
    }

    fn notify(&self, title: &str, body: &str) {
        if let Err(e) = self.app.notification().builder().title(title).body(body).show() {
            eprintln!("[taskkiln] notification failed: {e}");
        }
    }
}
