//! The scheduler engine: owns queue state, launches task pipelines one at a
//! time, and advances the queue after a validated completion.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;

use super::core::{self, Next, RunMode, Settled, UserAction};
use super::pipeline;
use crate::claude::{ClaudeRunner, ClaudeStatus};
use crate::db::{lock, SharedDb};
use crate::error::{AppError, AppResult};
use crate::logging::sanitize;
use crate::models::{LogLine, Task, TaskEvent};
use crate::process::CancelToken;

/// Pause between a task completing and the next one starting, so the UI and
/// the user can register the transition (all DB writes are already committed).
const AUTO_NEXT_DELAY: Duration = Duration::from_millis(800);

/// Side effects the engine needs from its environment (Tauri in the app,
/// a recorder in tests).
pub trait Host: Send + Sync + 'static {
    fn changed(&self);
    fn log(&self, line: &LogLine);
    fn event(&self, event: &TaskEvent);
    fn notify(&self, title: &str, body: &str);
}

struct Active {
    task_id: String,
    cancel: CancelToken,
}

#[derive(Default)]
struct EngineState {
    queue_running: bool,
    active: Option<Active>,
    claude: ClaudeStatus,
    alert: Option<String>,
    queue_complete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueInfo {
    pub running: bool,
    pub active_task_id: Option<String>,
    pub alert: Option<String>,
    pub queue_complete: bool,
}

pub struct Engine {
    pub db: SharedDb,
    host: Arc<dyn Host>,
    state: Mutex<EngineState>,
}

impl Engine {
    pub fn new(db: SharedDb, host: Arc<dyn Host>) -> Arc<Self> {
        Arc::new(Self { db, host, state: Mutex::new(EngineState::default()) })
    }

    fn st(&self) -> MutexGuard<'_, EngineState> {
        // State holds no invariants a panicking thread could break mid-update.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn queue_info(&self) -> QueueInfo {
        let s = self.st();
        QueueInfo {
            running: s.queue_running,
            active_task_id: s.active.as_ref().map(|a| a.task_id.clone()),
            alert: s.alert.clone(),
            queue_complete: s.queue_complete,
        }
    }

    pub fn claude_status(&self) -> ClaudeStatus {
        self.st().claude.clone()
    }

    pub async fn refresh_claude(&self) -> AppResult<ClaudeStatus> {
        let custom = lock(&self.db)?.get_settings()?.claude_path;
        let status = crate::claude::detect::probe(&custom).await;
        self.st().claude = status.clone();
        self.host.changed();
        Ok(status)
    }

    pub(crate) fn runner(&self) -> AppResult<ClaudeRunner> {
        let s = self.claude_status();
        match (&s.path, s.usable()) {
            (Some(p), true) => Ok(ClaudeRunner::new(p, s.capabilities.clone())),
            (Some(_), false) => Err(AppError::Claude(format!(
                "installed CLI lacks required features: {}",
                s.capabilities.missing_required().join(", ")
            ))),
            (None, _) => Err(AppError::Claude("Claude Code CLI not found".into())),
        }
    }

    // ---- logging helpers used by the pipeline ----

    pub(crate) fn event(&self, task_id: Option<&str>, kind: &str, message: &str) {
        let msg = sanitize(message);
        if let Ok(db) = lock(&self.db) {
            if let Ok(e) = db.add_event(task_id, kind, &msg) {
                self.host.event(&e);
            }
        }
    }

    pub(crate) fn log(&self, task_id: &str, stream: &str, line: &str) {
        let line = sanitize(line);
        if line.trim().is_empty() {
            return;
        }
        if let Ok(db) = lock(&self.db) {
            if let Ok(l) = db.add_log(task_id, stream, &line) {
                self.host.log(&l);
            }
        }
    }

    pub(crate) fn changed(&self) {
        self.host.changed();
    }

    pub(crate) fn notify(&self, title: &str, body: &str) {
        let enabled = lock(&self.db).and_then(|db| db.get_settings()).map(|s| s.notifications).unwrap_or(true);
        if enabled {
            self.host.notify(title, &sanitize(body));
        }
    }

    // ---- queue control ----

    /// Run once at startup before anything can launch.
    pub fn recover(&self) -> AppResult<Vec<Task>> {
        let recovered = core::recover_interrupted(&*lock(&self.db)?)?;
        if !recovered.is_empty() {
            self.st().alert = Some(format!(
                "{} task(s) were interrupted when TaskKiln last closed. Review them before continuing.",
                recovered.len()
            ));
        }
        Ok(recovered)
    }

    pub fn start_queue(self: &Arc<Self>) -> AppResult<()> {
        self.runner()?;
        {
            let mut s = self.st();
            s.queue_running = true;
            s.alert = None;
            s.queue_complete = false;
        }
        self.event(None, "QUEUE_STARTED", "Queue started");
        self.tick();
        self.changed();
        Ok(())
    }

    /// Stop starting new tasks; the active task (if any) finishes normally.
    pub fn pause_queue(&self) {
        self.st().queue_running = false;
        self.event(None, "QUEUE_PAUSED", "Queue paused; the active task will finish normally");
        self.changed();
    }

    /// Kill the active Claude/validation process tree. The task becomes PAUSED.
    pub fn stop_active(&self, task_id: &str) -> AppResult<()> {
        let mut s = self.st();
        match &s.active {
            Some(a) if a.task_id == task_id => {
                a.cancel.cancel();
                s.queue_running = false;
                Ok(())
            }
            _ => Err(AppError::Conflict("That task is not running".into())),
        }
    }

    pub fn user_action(self: &Arc<Self>, task_id: &str, action: UserAction) -> AppResult<()> {
        if action.resumes_queue() || matches!(action, UserAction::StopQueue) {
            // Launching or stopping requires that nothing else is mid-flight.
            if self.st().active.as_ref().is_some_and(|a| a.task_id != task_id) {
                return Err(AppError::Conflict("Another task is running; wait for it or stop it first".into()));
            }
        }
        let mode = core::apply_user_action(&*lock(&self.db)?, task_id, &action)?;
        if matches!(action, UserAction::StopQueue) {
            self.st().queue_running = false;
        } else if action.resumes_queue() {
            let mut s = self.st();
            s.queue_running = true;
            s.alert = None;
        }
        match mode {
            Some(m) => {
                let task = lock(&self.db)?.get_task(task_id)?;
                self.launch(task, m);
            }
            None => self.tick(),
        }
        self.changed();
        Ok(())
    }

    /// If idle and the queue is running, start the next task (or finish the queue).
    pub fn tick(self: &Arc<Self>) {
        if self.st().active.is_some() {
            return;
        }
        let running = self.st().queue_running;
        let next = match lock(&self.db).and_then(|db| core::choose_next(&db, running)) {
            Ok(n) => n,
            Err(e) => return self.halt(&format!("Could not read the queue: {e}")),
        };
        match next {
            Next::Hold => {}
            Next::Start(task) => self.start_task(task),
            Next::QueueComplete => self.finish_queue(),
        }
    }

    fn finish_queue(&self) {
        {
            let mut s = self.st();
            s.queue_running = false;
            s.queue_complete = true;
        }
        self.event(None, "QUEUE_COMPLETE", "All queued tasks are done");
        self.notify("TaskKiln: queue complete", "All queued tasks have finished.");
        self.changed();
    }

    /// Pause the queue with a visible alert (setup problems, DB errors).
    fn halt(&self, message: &str) {
        {
            let mut s = self.st();
            s.queue_running = false;
            s.alert = Some(message.to_string());
        }
        self.event(None, "QUEUE_HALTED", message);
        self.changed();
    }

    fn start_task(self: &Arc<Self>, task: Task) {
        let project = match lock(&self.db).and_then(|db| db.get_project(&task.project_id)) {
            Ok(p) => p,
            Err(e) => return self.halt(&format!("Task '{}': {e}", task.title)),
        };
        if !Path::new(&project.path).is_dir() {
            return self.halt(&format!(
                "Project directory for '{}' no longer exists: {}",
                task.title, project.path
            ));
        }
        if let Err(e) = self.runner() {
            return self.halt(&e.to_string());
        }
        self.launch(task, RunMode::Fresh);
    }

    /// Spawn the pipeline for `task`. Exactly one task may be active.
    fn launch(self: &Arc<Self>, task: Task, mode: RunMode) {
        let cancel = CancelToken::new();
        {
            let mut s = self.st();
            if s.active.is_some() {
                return;
            }
            s.active = Some(Active { task_id: task.id.clone(), cancel: cancel.clone() });
            s.queue_complete = false;
        }
        let engine = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let id = task.id.clone();
            let settled = pipeline::run(&engine, &id, mode, &cancel).await;
            engine.st().active = None;
            engine.after_pipeline(&task, settled).await;
        });
        self.changed();
    }

    async fn after_pipeline(self: &Arc<Self>, task: &Task, settled: AppResult<Option<Settled>>) {
        match settled {
            Ok(Some(Settled::Completed)) => {
                self.notify("Task completed", &format!("{} passed validation.", task.title));
                let auto = lock(&self.db).and_then(|db| db.get_settings()).map(|s| s.auto_run_next).unwrap_or(true);
                if !auto {
                    self.st().queue_running = false;
                }
                self.changed();
                tokio::time::sleep(AUTO_NEXT_DELAY).await;
                self.tick();
            }
            Ok(Some(Settled::NeedsUser)) => {
                self.st().queue_running = false;
                self.notify("Task needs attention", &format!("{}: validation did not pass.", task.title));
            }
            Ok(None) => {
                // Stopped, or failed before validation; the pipeline set the state.
                self.st().queue_running = false;
            }
            Err(e) => {
                self.st().queue_running = false;
                self.event(Some(&task.id), "TASK_ERROR", &e.to_string());
                // Never leave a task looking active after its pipeline died.
                if let Ok(db) = lock(&self.db) {
                    if db.get_task(&task.id).is_ok_and(|t| t.status.is_active()) {
                        let detail = format!("TaskKiln hit an internal error: {e}");
                        let _ = db.require_attention(&task.id, crate::models::AttentionReason::ClaudeFailed, &detail);
                    }
                }
                self.st().alert = Some(format!("Task '{}' hit an internal error: {e}", task.title));
            }
        }
        self.changed();
    }
}
