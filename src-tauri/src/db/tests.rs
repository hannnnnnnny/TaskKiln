use super::*;
use crate::models::*;

pub(crate) fn setup() -> (Db, Project, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    let project = db.add_project(dir.path().to_str().unwrap()).unwrap();
    (db, project, dir)
}

pub(crate) fn new_task(db: &Db, project: &Project, title: &str, priority: i64) -> Task {
    db.create_task(&NewTask {
        project_id: project.id.clone(),
        title: title.into(),
        description: format!("do {title}"),
        acceptance_criteria: vec!["it works".into(), "  ".into()],
        priority,
    })
    .unwrap()
}

fn queued_titles(db: &Db) -> Vec<String> {
    db.tasks_with_status(&[TaskStatus::Queued]).unwrap().into_iter().map(|t| t.title).collect()
}

#[test]
fn migrations_set_latest_version() {
    let db = Db::open_in_memory().unwrap();
    let v: i64 = db.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, migrations::latest_version());
    // Running again is a no-op.
    migrations::run(db.conn()).unwrap();
}

#[test]
fn project_must_be_existing_directory_and_is_deduplicated() {
    let (db, project, dir) = setup();
    assert!(db.add_project("Z:/definitely/not/here").is_err());
    let again = db.add_project(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(again.id, project.id);
    assert!(project.path_exists);
}

#[test]
fn task_crud_round_trip() {
    let (db, project, _dir) = setup();
    let t = new_task(&db, &project, "A", 1);
    assert_eq!(t.status, TaskStatus::Queued);
    assert_eq!(t.acceptance_criteria, vec!["it works"], "blank criteria dropped");

    let updated = db
        .update_task(&t.id, &TaskUpdate {
            title: "A2".into(),
            description: "new".into(),
            acceptance_criteria: vec!["x".into(), "y".into()],
            priority: 2,
        })
        .unwrap();
    assert_eq!(updated.title, "A2");
    assert_eq!(updated.acceptance_criteria.len(), 2);

    db.delete_task(&t.id).unwrap();
    assert!(db.get_task(&t.id).is_err());
}

#[test]
fn task_input_is_validated() {
    let (db, project, _dir) = setup();
    let mut input = NewTask {
        project_id: project.id.clone(),
        title: "   ".into(),
        description: String::new(),
        acceptance_criteria: vec![],
        priority: 1,
    };
    assert!(db.create_task(&input).is_err(), "empty title");
    input.title = "ok".into();
    input.priority = 9;
    assert!(db.create_task(&input).is_err(), "priority out of range");
    input.priority = 1;
    input.project_id = "missing".into();
    assert!(db.create_task(&input).is_err(), "unknown project");
}

#[test]
fn queue_ordering_respects_priority_on_insert_and_manual_reorder() {
    let (db, project, _dir) = setup();
    new_task(&db, &project, "normal-1", 1);
    new_task(&db, &project, "normal-2", 1);
    new_task(&db, &project, "high", 2);
    new_task(&db, &project, "low", 0);
    assert_eq!(queued_titles(&db), ["high", "normal-1", "normal-2", "low"]);

    let mut ids: Vec<String> = db.tasks_with_status(&[TaskStatus::Queued]).unwrap().into_iter().map(|t| t.id).collect();
    ids.reverse();
    db.reorder_queue(&ids).unwrap();
    assert_eq!(queued_titles(&db), ["low", "normal-2", "normal-1", "high"]);
    assert_eq!(db.next_queued_task().unwrap().unwrap().title, "low");
}

#[test]
fn reorder_rejects_stale_id_sets() {
    let (db, project, _dir) = setup();
    let a = new_task(&db, &project, "a", 1);
    new_task(&db, &project, "b", 1);
    assert!(db.reorder_queue(&[a.id.clone()]).is_err());
    assert!(db.reorder_queue(&[a.id.clone(), "bogus".into()]).is_err());
}

#[test]
fn move_to_front_puts_task_first() {
    let (db, project, _dir) = setup();
    new_task(&db, &project, "a", 1);
    new_task(&db, &project, "b", 1);
    let c = new_task(&db, &project, "c", 1);
    db.move_to_front(&c.id).unwrap();
    assert_eq!(queued_titles(&db), ["c", "a", "b"]);
}

#[test]
fn transitions_are_enforced_and_timestamped() {
    let (db, project, _dir) = setup();
    let t = new_task(&db, &project, "a", 1);
    assert!(db.transition(&t.id, TaskStatus::Completed).is_err());
    let t = db.transition(&t.id, TaskStatus::Planning).unwrap();
    assert!(t.started_at.is_some());
    assert!(db.delete_task(&t.id).is_err(), "active tasks cannot be deleted");
    assert!(db.update_task(&t.id, &TaskUpdate {
        title: "x".into(), description: String::new(), acceptance_criteria: vec![], priority: 1
    }).is_err(), "active tasks cannot be edited");
    db.transition(&t.id, TaskStatus::Running).unwrap();
    db.transition(&t.id, TaskStatus::Validating).unwrap();
    let t = db.transition(&t.id, TaskStatus::Completed).unwrap();
    assert!(t.completed_at.is_some());
}

#[test]
fn attention_reason_is_recorded_and_cleared() {
    let (db, project, _dir) = setup();
    let t = new_task(&db, &project, "a", 1);
    db.transition(&t.id, TaskStatus::Planning).unwrap();
    db.transition(&t.id, TaskStatus::Running).unwrap();
    db.transition(&t.id, TaskStatus::Validating).unwrap();
    let t = db.require_attention(&t.id, AttentionReason::ValidationFail, "tests failed").unwrap();
    assert_eq!(t.status, TaskStatus::NeedsUser);
    assert_eq!(t.attention_reason, Some(AttentionReason::ValidationFail));
    let t = db.transition(&t.id, TaskStatus::Running).unwrap();
    assert_eq!(t.attention_reason, None);
}

#[test]
fn checkpoints_include_taskkiln_validation_step() {
    let (db, project, _dir) = setup();
    let t = new_task(&db, &project, "a", 1);
    let cps = db
        .replace_checkpoints(&t.id, &[("inspect".into(), 1.0), ("implement".into(), 3.0)])
        .unwrap();
    assert_eq!(cps.len(), 3);
    assert_eq!(cps[2].owner, "taskkiln");
    assert!(db.set_checkpoint_status(&t.id, 2, CheckpointStatus::Running).unwrap());
    assert!(!db.set_checkpoint_status(&t.id, 99, CheckpointStatus::Running).unwrap());
    let cps = db.list_checkpoints(&t.id).unwrap();
    assert_eq!(cps[1].status, CheckpointStatus::Running);
    assert!(cps[1].started_at.is_some());
}

#[test]
fn settings_round_trip_and_validate() {
    let db = Db::open_in_memory().unwrap();
    let mut s = db.get_settings().unwrap();
    assert_eq!(s, Settings::default());
    s.auto_run_next = false;
    s.check_lint = false;
    let saved = db.save_settings(&s).unwrap();
    assert!(!saved.auto_run_next && !saved.check_lint);
    s.permission_mode = "yolo".into();
    assert!(db.save_settings(&s).is_err());
}

#[test]
fn data_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let proj_dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("taskkiln.db");
    {
        let db = Db::open(&path).unwrap();
        let p = db.add_project(proj_dir.path().to_str().unwrap()).unwrap();
        new_task(&db, &p, "persisted", 1);
        db.add_event(None, "TASK_CREATED", "hello").unwrap();
    }
    let db = Db::open(&path).unwrap();
    assert_eq!(db.list_projects().unwrap().len(), 1);
    assert_eq!(queued_titles(&db), ["persisted"]);
    assert_eq!(db.list_events(None, 10).unwrap().len(), 1);
}

#[test]
fn validation_command_approval_survives_redetection() {
    let (db, project, _dir) = setup();
    let cmd = ValidationCommand {
        id: String::new(),
        project_id: project.id.clone(),
        kind: "test".into(),
        program: "npm".into(),
        args: vec!["run".into(), "test".into()],
        source: "package.json".into(),
        approved: false,
        enabled: true,
    };
    let stored = db.replace_validation_commands(&project.id, &[cmd.clone()]).unwrap();
    assert!(!stored[0].approved, "detected commands start unapproved");
    db.set_command_flags(&stored[0].id, true, true).unwrap();
    let again = db.replace_validation_commands(&project.id, &[cmd]).unwrap();
    assert!(again[0].approved);
}
