//! End-to-end scheduler tests against a protocol-compatible fake Claude CLI
//! (tests/fixtures/fake_claude.cjs). Requires `node` and `git` on PATH;
//! skipped otherwise.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use taskkiln_lib::db::{lock, Db, SharedDb};
use taskkiln_lib::models::*;
use taskkiln_lib::scheduler::{Engine, Host, UserAction};

#[derive(Default)]
struct Recorder {
    notifications: Mutex<Vec<String>>,
    logs: Mutex<usize>,
}

impl Host for Recorder {
    fn changed(&self) {}
    fn log(&self, _line: &LogLine) {
        *self.logs.lock().unwrap() += 1;
    }
    fn event(&self, _e: &TaskEvent) {}
    fn notify(&self, title: &str, _body: &str) {
        self.notifications.lock().unwrap().push(title.to_string());
    }
}

fn tools_available() -> bool {
    which::which("node").is_ok() && which::which("git").is_ok()
}

/// Write a launcher for the fake CLI that TaskKiln can execute directly.
fn fake_cli(dir: &Path) -> PathBuf {
    let js = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_claude.cjs");
    if cfg!(windows) {
        let p = dir.join("claude.cmd");
        std::fs::write(&p, format!("@node \"{}\" %*\r\n", js.display())).unwrap();
        p
    } else {
        let p = dir.join("claude");
        std::fs::write(&p, format!("#!/bin/sh\nexec node \"{}\" \"$@\"\n", js.display())).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }
}

struct Fixture {
    engine: Arc<Engine>,
    db: SharedDb,
    host: Arc<Recorder>,
    project: Project,
    _dirs: Vec<tempfile::TempDir>,
}

async fn fixture() -> Fixture {
    let bin_dir = tempfile::tempdir().unwrap();
    let proj_dir = tempfile::tempdir().unwrap();
    std::process::Command::new("git").args(["init", "-q"]).current_dir(proj_dir.path()).status().unwrap();
    let db = Db::open_in_memory().unwrap();
    let mut settings = db.get_settings().unwrap();
    settings.claude_path = fake_cli(bin_dir.path()).to_string_lossy().to_string();
    // The empty fixture project has no build/test commands; don't warn about that here.
    settings.check_build = false;
    settings.check_test = false;
    settings.check_lint = false;
    db.save_settings(&settings).unwrap();
    let project = db.add_project(proj_dir.path().to_str().unwrap()).unwrap();
    let db = db.shared();
    let host = Arc::new(Recorder::default());
    let engine = Engine::new(db.clone(), host.clone());
    let status = engine.refresh_claude().await.unwrap();
    assert!(status.usable(), "fake CLI should be detected: {status:?}");
    Fixture { engine, db, host, project, _dirs: vec![bin_dir, proj_dir] }
}

fn add(f: &Fixture, title: &str, criteria: &[&str]) -> Task {
    lock(&f.db).unwrap().create_task(&NewTask {
        project_id: f.project.id.clone(),
        title: title.into(),
        description: "integration test".into(),
        acceptance_criteria: criteria.iter().map(|s| s.to_string()).collect(),
        priority: 1,
    }).unwrap()
}

async fn wait_for(f: &Fixture, id: &str, status: TaskStatus) -> Task {
    let start = Instant::now();
    loop {
        let t = lock(&f.db).unwrap().get_task(id).unwrap();
        if t.status == status {
            return t;
        }
        assert!(start.elapsed() < Duration::from_secs(90), "timed out waiting for {status}, task is {}", t.status);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_runs_tasks_in_order_and_auto_advances() {
    if !tools_available() {
        eprintln!("skipping: node/git not available");
        return;
    }
    let f = fixture().await;
    let a = add(&f, "Task A", &["works"]);
    let b = add(&f, "Task B", &["works"]);
    // Reorder: B first.
    lock(&f.db).unwrap().move_to_front(&b.id).unwrap();
    f.engine.start_queue().unwrap();

    let b_done = wait_for(&f, &b.id, TaskStatus::Completed).await;
    let a_done = wait_for(&f, &a.id, TaskStatus::Completed).await;
    assert!(b_done.completed_at.as_ref().unwrap() <= a_done.started_at.as_ref().unwrap(),"B must finish before A starts");
    for t in [&a_done, &b_done] {
        assert_eq!(t.validation_status, Some(ValidationStatus::Pass));
        assert_eq!(t.progress, Some(1.0), "all checkpoints, including validation, completed");
    }
    let db = lock(&f.db).unwrap();
    let cps = db.list_checkpoints(&a.id).unwrap();
    assert!(cps.iter().all(|c| c.status == CheckpointStatus::Completed));
    let kinds: Vec<String> = db.list_events(Some(&a.id), 200).unwrap().into_iter().map(|e| e.kind).collect();
    for k in ["TASK_STARTED", "CLAUDE_STARTED", "PLAN_CREATED", "CHECKPOINT_STARTED", "CHECKPOINT_COMPLETED", "VALIDATION_STARTED", "VALIDATION_PASS", "TASK_COMPLETED"] {
        assert!(kinds.iter().any(|x| x == k), "missing event {k}: {kinds:?}");
    }
    let v = db.latest_validation(&a.id).unwrap().unwrap();
    assert_eq!(v.changed_files, vec!["Task_A.txt"]);
    drop(db);
    assert!(*f.host.logs.lock().unwrap() > 0, "Claude output was streamed");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let notes = f.host.notifications.lock().unwrap().clone();
    assert!(notes.iter().any(|n| n == "Task completed"), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("queue complete")), "{notes:?}");
    assert!(f.engine.queue_info().queue_complete);
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_validation_pauses_queue_and_fix_recovers() {
    if !tools_available() {
        eprintln!("skipping: node/git not available");
        return;
    }
    let f = fixture().await;
    let bad = add(&f, "Bad", &["MUST_FAIL"]);
    let next = add(&f, "Next", &["works"]);
    f.engine.start_queue().unwrap();

    let t = wait_for(&f, &bad.id, TaskStatus::NeedsUser).await;
    assert_eq!(t.attention_reason, Some(AttentionReason::ValidationFail));
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(lock(&f.db).unwrap().get_task(&next.id).unwrap().status, TaskStatus::Queued, "queue must not advance");
    assert!(!f.engine.queue_info().running);
    assert!(f.host.notifications.lock().unwrap().iter().any(|n| n == "Task needs attention"));

    f.engine.user_action(&bad.id, UserAction::Fix).unwrap();
    let fixed = wait_for(&f, &bad.id, TaskStatus::Completed).await;
    assert_eq!(fixed.fix_attempts, 1);
    assert_eq!(fixed.claude_session_id, t.claude_session_id, "fix reuses the Claude session");
    wait_for(&f, &next.id, TaskStatus::Completed).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ignore_and_continue_records_override() {
    if !tools_available() {
        eprintln!("skipping: node/git not available");
        return;
    }
    let f = fixture().await;
    let bad = add(&f, "Bad", &["MUST_FAIL"]);
    let next = add(&f, "Next", &["works"]);
    f.engine.start_queue().unwrap();
    wait_for(&f, &bad.id, TaskStatus::NeedsUser).await;
    f.engine.user_action(&bad.id, UserAction::Ignore).unwrap();
    let t = wait_for(&f, &bad.id, TaskStatus::Completed).await;
    assert_eq!(t.validation_status, Some(ValidationStatus::Overridden));
    wait_for(&f, &next.id, TaskStatus::Completed).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_active_task_pauses_it() {
    if !tools_available() {
        eprintln!("skipping: node/git not available");
        return;
    }
    let f = fixture().await;
    let a = add(&f, "A", &["works"]);
    f.engine.start_queue().unwrap();
    // Stop as soon as the task is active.
    let start = Instant::now();
    loop {
        if f.engine.queue_info().active_task_id.as_deref() == Some(&a.id) {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(30));
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    f.engine.stop_active(&a.id).unwrap();
    let start = Instant::now();
    loop {
        let t = lock(&f.db).unwrap().get_task(&a.id).unwrap();
        // The fake CLI is fast; the task may have finished before the stop landed.
        if matches!(t.status, TaskStatus::Paused | TaskStatus::Completed) && f.engine.queue_info().active_task_id.is_none() {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "stuck in {}", t.status);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!f.engine.queue_info().running);
}
