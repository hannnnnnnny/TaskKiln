use std::time::{Duration, Instant};

use super::*;

fn cwd() -> PathBuf {
    std::env::temp_dir()
}

fn long_running() -> SpawnSpec {
    if cfg!(windows) {
        SpawnSpec::new("ping", vec!["-n".into(), "30".into(), "127.0.0.1".into()], cwd())
    } else {
        SpawnSpec::new("sleep", vec!["30".into()], cwd())
    }
}

#[tokio::test]
async fn captures_stdout_and_exit_code() {
    let spec = SpawnSpec::new("git", vec!["--version".into()], cwd());
    let out = run_capture(&spec, Duration::from_secs(20), 10_000).await.unwrap();
    assert!(out.outcome.success);
    assert_eq!(out.outcome.exit_code, Some(0));
    assert!(out.stdout.starts_with("git version"));
}

#[tokio::test]
async fn writes_stdin_without_shell() {
    let mut spec = SpawnSpec::new("git", vec!["hash-object".into(), "--stdin".into()], cwd());
    spec.stdin = Some("hello\n".into());
    let out = run_capture(&spec, Duration::from_secs(20), 10_000).await.unwrap();
    assert_eq!(out.stdout.trim(), "ce013625030ba8dba906f756967f9e9ca394464a");
}

#[tokio::test]
async fn reports_nonzero_exit() {
    let spec = SpawnSpec::new("git", vec!["definitely-not-a-subcommand".into()], cwd());
    let out = run_capture(&spec, Duration::from_secs(20), 10_000).await.unwrap();
    assert!(!out.outcome.success);
    assert_ne!(out.outcome.exit_code, Some(0));
    assert!(!out.stderr.is_empty());
}

#[tokio::test]
async fn missing_program_is_an_error_not_a_panic() {
    let spec = SpawnSpec::new("taskkiln-no-such-binary", vec![], cwd());
    assert!(run_capture(&spec, Duration::from_secs(5), 100).await.is_err());
}

#[tokio::test]
async fn missing_cwd_is_rejected() {
    let spec = SpawnSpec::new("git", vec![], cwd().join("taskkiln-missing-dir-xyz"));
    assert!(run_capture(&spec, Duration::from_secs(5), 100).await.is_err());
}

#[tokio::test]
async fn cancellation_kills_the_process_promptly() {
    let token = CancelToken::new();
    let t2 = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        t2.cancel();
    });
    let start = Instant::now();
    let out = run_streaming(&long_running(), &token, None, |_, _| {}).await.unwrap();
    assert!(out.cancelled);
    assert!(!out.success);
    assert!(start.elapsed() < Duration::from_secs(10), "took {:?}", start.elapsed());
}

#[tokio::test]
async fn timeout_kills_the_process() {
    let start = Instant::now();
    let out = run_streaming(&long_running(), &CancelToken::new(), Some(Duration::from_millis(300)), |_, _| {})
        .await
        .unwrap();
    assert!(out.timed_out);
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn describe_quotes_arguments_for_audit_log() {
    let mut spec = SpawnSpec::new("claude", vec!["-p".into(), "two words".into()], cwd());
    spec.stdin = Some("secret prompt".into());
    let d = spec.describe();
    assert_eq!(d, "claude -p \"two words\" < [prompt via stdin]");
}
