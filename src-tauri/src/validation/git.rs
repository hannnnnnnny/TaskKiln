//! Read-only git inspection used for change detection and review context.
//! TaskKiln never runs mutating git commands.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::process::{run_capture, SpawnSpec};

const GIT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_DIFF_BYTES: usize = 60_000;

/// path -> content hash ("deleted" for removed files) for every dirty file.
pub type Snapshot = BTreeMap<String, String>;

async fn git(root: &Path, args: &[&str], stdin: Option<String>) -> AppResult<String> {
    let mut a = vec!["-c".to_string(), "core.quotepath=false".to_string()];
    a.extend(args.iter().map(|s| s.to_string()));
    let mut spec = SpawnSpec::new("git", a, root);
    spec.stdin = stdin;
    let out = run_capture(&spec, GIT_TIMEOUT, 2_000_000).await?;
    if !out.outcome.success {
        return Err(AppError::Process(format!("git {} failed: {}", args.join(" "), out.stderr.trim())));
    }
    Ok(out.stdout)
}

/// Parse `git status --porcelain=v1` output into (status, path) pairs.
/// Renames report the new path.
pub fn parse_porcelain(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter(|l| l.len() > 3)
        .map(|l| {
            let status = l[..2].to_string();
            let path = l[3..].rsplit(" -> ").next().unwrap_or(&l[3..]);
            (status, path.trim_matches('"').to_string())
        })
        .collect()
}

pub async fn snapshot(root: &Path) -> AppResult<Snapshot> {
    let status = git(root, &["status", "--porcelain=v1", "--untracked-files=all"], None).await?;
    let entries = parse_porcelain(&status);
    let present: Vec<&String> = entries.iter().filter(|(_, p)| root.join(p).is_file()).map(|(_, p)| p).collect();
    let hashes = if present.is_empty() {
        String::new()
    } else {
        let list = present.iter().map(|p| p.as_str()).collect::<Vec<_>>().join("\n") + "\n";
        git(root, &["hash-object", "--stdin-paths"], Some(list)).await?
    };
    let mut hash_iter = hashes.lines();
    let mut snap = Snapshot::new();
    for (_, path) in &entries {
        let hash = if root.join(path).is_file() { hash_iter.next().unwrap_or("?").to_string() } else { "deleted".into() };
        snap.insert(path.clone(), hash);
    }
    Ok(snap)
}

/// Files whose state differs between the baseline and now. A file dirty in
/// both with identical content was pre-existing and is not attributed to the task.
pub fn changed_since(baseline: &Snapshot, now: &Snapshot) -> Vec<String> {
    let mut changed: Vec<String> = now
        .iter()
        .filter(|(p, h)| baseline.get(*p) != Some(*h))
        .map(|(p, _)| p.clone())
        .collect();
    // Dirty before, clean now: the task reverted it.
    changed.extend(baseline.keys().filter(|p| !now.contains_key(*p)).cloned());
    changed.sort();
    changed.dedup();
    changed
}

pub async fn has_head(root: &Path) -> bool {
    git(root, &["rev-parse", "--verify", "HEAD"], None).await.is_ok()
}

/// Diff stat and (truncated) diff of tracked changes for the given files.
pub async fn diff_for(root: &Path, files: &[String]) -> (String, String) {
    if files.is_empty() {
        return (String::new(), String::new());
    }
    let base: Vec<&str> = if has_head(root).await { vec!["diff", "HEAD"] } else { vec!["diff"] };
    let with_files = |extra: &[&'static str]| -> Vec<&str> {
        let mut a: Vec<&str> = base.clone();
        a.extend_from_slice(extra);
        a.push("--");
        a.extend(files.iter().map(String::as_str));
        a
    };
    let stat = git(root, &with_files(&["--stat"]), None).await.unwrap_or_default();
    let diff = git(root, &with_files(&[]), None).await.unwrap_or_default();
    (stat, crate::logging::truncate(&diff, MAX_DIFF_BYTES))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_including_renames_and_untracked() {
        let out = " M src/a.ts\n?? new file.txt\nR  old.rs -> new.rs\n D gone.md\n";
        let parsed = parse_porcelain(out);
        let paths: Vec<&str> = parsed.iter().map(|(_, p)| p.as_str()).collect();
        assert_eq!(paths, ["src/a.ts", "new file.txt", "new.rs", "gone.md"]);
    }

    #[test]
    fn attributes_only_new_changes_to_the_task() {
        let base: Snapshot = [("pre.txt".into(), "h1".into()), ("touched.txt".into(), "h2".into()), ("reverted.txt".into(), "h3".into())].into();
        let now: Snapshot = [("pre.txt".into(), "h1".into()), ("touched.txt".into(), "h9".into()), ("new.rs".into(), "h4".into())].into();
        assert_eq!(changed_since(&base, &now), ["new.rs", "reverted.txt", "touched.txt"]);
    }

    #[tokio::test]
    async fn snapshot_detects_real_changes_in_a_repo() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        git(root, &["init", "-q"], None).await.unwrap();
        std::fs::write(root.join("pre.txt"), "dirty before task").unwrap();
        let base = snapshot(root).await.unwrap();
        std::fs::write(root.join("made_by_task.txt"), "new").unwrap();
        let now = snapshot(root).await.unwrap();
        assert_eq!(changed_since(&base, &now), ["made_by_task.txt"]);
    }
}
