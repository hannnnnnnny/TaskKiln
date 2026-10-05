use super::core::*;
use crate::db::tests::{new_task, setup};
use crate::db::{new_id, now};
use crate::models::*;

fn validation(task_id: &str, status: ValidationStatus) -> ValidationResult {
    ValidationResult {
        id: new_id(),
        task_id: task_id.into(),
        status,
        summary: format!("{}: test", status.as_str()),
        findings: vec![Finding { source: "test".into(), severity: FindingSeverity::Fail, message: "2 failed".into() }],
        commands: vec![],
        changed_files: vec![],
        created_at: now(),
    }
}

/// Drive a task through planning/running/testing/validating as the pipeline would.
fn run_to_validating(db: &crate::db::Db, id: &str) {
    db.transition(id, TaskStatus::Planning).unwrap();
    db.replace_checkpoints(id, &[("inspect".into(), 1.0), ("build".into(), 2.0)]).unwrap();
    db.set_session_id(id, "session-1").unwrap();
    db.transition(id, TaskStatus::Running).unwrap();
    db.transition(id, TaskStatus::Testing).unwrap();
    db.transition(id, TaskStatus::Validating).unwrap();
}

#[test]
fn held_queue_never_starts_tasks() {
    let (db, p, _d) = setup();
    new_task(&db, &p, "a", 1);
    assert_eq!(choose_next(&db, false).unwrap(), Next::Hold);
}

#[test]
fn empty_running_queue_is_complete() {
    let (db, _p, _d) = setup();
    assert_eq!(choose_next(&db, true).unwrap(), Next::QueueComplete);
}

#[test]
fn pass_completes_and_auto_next_picks_following_task() {
    let (db, p, _d) = setup();
    let a = new_task(&db, &p, "A", 1);
    new_task(&db, &p, "B", 1);
    run_to_validating(&db, &a.id);
    assert_eq!(settle_validation(&db, &validation(&a.id, ValidationStatus::Pass)).unwrap(), Settled::Completed);
    let a = db.get_task(&a.id).unwrap();
    assert_eq!(a.status, TaskStatus::Completed);
    assert_eq!(a.validation_status, Some(ValidationStatus::Pass));
    // The TaskKiln validation checkpoint completes on PASS.
    let cps = db.list_checkpoints(&a.id).unwrap();
    assert_eq!(cps.last().unwrap().status, CheckpointStatus::Completed);
    match choose_next(&db, true).unwrap() {
        Next::Start(t) => assert_eq!(t.title, "B"),
        other => panic!("expected B, got {other:?}"),
    }
}

#[test]
fn reordered_queue_is_respected_by_auto_next() {
    let (db, p, _d) = setup();
    let a = new_task(&db, &p, "A", 1);
    new_task(&db, &p, "B", 1);
    let c = new_task(&db, &p, "C", 1);
    run_to_validating(&db, &a.id);
    // Moving C to the front while A runs does not affect A, only what comes next.
    db.move_to_front(&c.id).unwrap();
    assert_eq!(db.get_task(&a.id).unwrap().status, TaskStatus::Validating);
    settle_validation(&db, &validation(&a.id, ValidationStatus::Pass)).unwrap();
    let Next::Start(t) = choose_next(&db, true).unwrap() else { panic!() };
    assert_eq!(t.title, "C");
}

#[test]
fn failed_validation_requires_user_and_records_reason() {
    let (db, p, _d) = setup();
    let a = new_task(&db, &p, "A", 1);
    run_to_validating(&db, &a.id);
    assert_eq!(settle_validation(&db, &validation(&a.id, ValidationStatus::Fail)).unwrap(), Settled::NeedsUser);
    let a = db.get_task(&a.id).unwrap();
    assert_eq!(a.status, TaskStatus::NeedsUser);
    assert_eq!(a.attention_reason, Some(AttentionReason::ValidationFail));
    assert_eq!(db.list_events(Some(&a.id), 50).unwrap().iter().filter(|e| e.kind == "VALIDATION_FAIL").count(), 1);

    let (db, p, _d) = setup();
    let b = new_task(&db, &p, "B", 1);
    run_to_validating(&db, &b.id);
    assert_eq!(settle_validation(&db, &validation(&b.id, ValidationStatus::Warning)).unwrap(), Settled::NeedsUser);
    assert_eq!(db.get_task(&b.id).unwrap().attention_reason, Some(AttentionReason::ValidationWarning));
}

#[test]
fn ignore_overrides_and_lets_queue_continue() {
    let (db, p, _d) = setup();
    let a = new_task(&db, &p, "A", 1);
    new_task(&db, &p, "B", 1);
    run_to_validating(&db, &a.id);
    settle_validation(&db, &validation(&a.id, ValidationStatus::Warning)).unwrap();
    assert!(UserAction::Ignore.resumes_queue());
    assert_eq!(apply_user_action(&db, &a.id, &UserAction::Ignore).unwrap(), None);
    let a = db.get_task(&a.id).unwrap();
    assert_eq!(a.status, TaskStatus::Completed);
    assert_eq!(a.validation_status, Some(ValidationStatus::Overridden));
    let Next::Start(t) = choose_next(&db, true).unwrap() else { panic!() };
    assert_eq!(t.title, "B");
}

#[test]
fn fix_and_adjust_restart_claude_in_the_same_task() {
    let (db, p, _d) = setup();
    let a = new_task(&db, &p, "A", 1);
    run_to_validating(&db, &a.id);
    settle_validation(&db, &validation(&a.id, ValidationStatus::Fail)).unwrap();
    assert_eq!(apply_user_action(&db, &a.id, &UserAction::Fix).unwrap(), Some(RunMode::Fix));
    let t = db.get_task(&a.id).unwrap();
    assert_eq!(t.status, TaskStatus::Running);
    assert_eq!(t.fix_attempts, 1);
    assert_eq!(t.claude_session_id.as_deref(), Some("session-1"), "same session is reused");

    db.transition(&a.id, TaskStatus::Validating).unwrap();
    settle_validation(&db, &validation(&a.id, ValidationStatus::Fail)).unwrap();
    let action = UserAction::Adjust { criteria: vec!["new rule".into()] };
    assert_eq!(apply_user_action(&db, &a.id, &action).unwrap(), Some(RunMode::Reconcile));
    assert_eq!(db.get_task(&a.id).unwrap().acceptance_criteria, vec!["new rule"]);
}

#[test]
fn stop_queue_pauses_task() {
    let (db, p, _d) = setup();
    let a = new_task(&db, &p, "A", 1);
    run_to_validating(&db, &a.id);
    settle_validation(&db, &validation(&a.id, ValidationStatus::Fail)).unwrap();
    apply_user_action(&db, &a.id, &UserAction::StopQueue).unwrap();
    assert_eq!(db.get_task(&a.id).unwrap().status, TaskStatus::Paused);
}

#[test]
fn actions_are_rejected_in_wrong_states() {
    let (db, p, _d) = setup();
    let a = new_task(&db, &p, "A", 1);
    assert!(apply_user_action(&db, &a.id, &UserAction::Fix).is_err());
    assert!(apply_user_action(&db, &a.id, &UserAction::Ignore).is_err());
    assert!(apply_user_action(&db, &a.id, &UserAction::Resume).is_err());
    // Interrupted tasks cannot be "ignored" into completion.
    run_to_validating(&db, &a.id);
    recover_interrupted(&db).unwrap();
    assert!(apply_user_action(&db, &a.id, &UserAction::Ignore).is_err());
}

#[test]
fn startup_recovery_flags_active_tasks_without_completing_them() {
    let (db, p, _d) = setup();
    let a = new_task(&db, &p, "A", 1);
    let b = new_task(&db, &p, "B", 1);
    let q = new_task(&db, &p, "Q", 1);
    db.transition(&a.id, TaskStatus::Planning).unwrap();
    run_to_validating(&db, &b.id);
    let recovered = recover_interrupted(&db).unwrap();
    assert_eq!(recovered.len(), 2);
    for id in [&a.id, &b.id] {
        let t = db.get_task(id).unwrap();
        assert_eq!(t.status, TaskStatus::NeedsUser);
        assert_eq!(t.attention_reason, Some(AttentionReason::Interrupted));
        assert!(t.completed_at.is_none());
    }
    assert_eq!(db.get_task(&q.id).unwrap().status, TaskStatus::Queued);
    // Recovery options
    assert_eq!(apply_user_action(&db, &a.id, &UserAction::Resume).unwrap(), Some(RunMode::Fresh), "no plan yet");
    assert_eq!(apply_user_action(&db, &b.id, &UserAction::RetryValidation).unwrap(), Some(RunMode::ValidateOnly));
}

#[test]
fn recovery_resume_with_plan_continues_session_and_requeue_goes_first() {
    let (db, p, _d) = setup();
    let a = new_task(&db, &p, "A", 1);
    let other = new_task(&db, &p, "other", 1);
    run_to_validating(&db, &a.id);
    recover_interrupted(&db).unwrap();
    assert_eq!(apply_user_action(&db, &a.id, &UserAction::Resume).unwrap(), Some(RunMode::Continue));

    let (db2, p2, _d2) = setup();
    let x = new_task(&db2, &p2, "X", 1);
    new_task(&db2, &p2, "Y", 1);
    run_to_validating(&db2, &x.id);
    recover_interrupted(&db2).unwrap();
    apply_user_action(&db2, &x.id, &UserAction::ReturnToQueue).unwrap();
    assert_eq!(db2.next_queued_task().unwrap().unwrap().title, "X");
    let _ = other;
}

#[test]
fn mark_failed_then_retry() {
    let (db, p, _d) = setup();
    let a = new_task(&db, &p, "A", 1);
    run_to_validating(&db, &a.id);
    recover_interrupted(&db).unwrap();
    apply_user_action(&db, &a.id, &UserAction::MarkFailed).unwrap();
    assert_eq!(db.get_task(&a.id).unwrap().status, TaskStatus::Failed);
    apply_user_action(&db, &a.id, &UserAction::ReturnToQueue).unwrap();
    assert_eq!(db.get_task(&a.id).unwrap().status, TaskStatus::Queued);
}
