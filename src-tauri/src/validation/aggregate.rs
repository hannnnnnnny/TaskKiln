//! Turn every validation signal into one PASS / WARNING / FAIL verdict.
//!
//! Claude's self-report is one input among several; objective signals
//! (exit status, build/test commands, git changes) and the independent
//! review can each veto a PASS.

use crate::claude::plan::{CompletionReport, Review};
use crate::models::{CommandOutcome, Finding, FindingSeverity, ValidationStatus};

#[derive(Debug, Clone)]
pub enum ReviewState {
    Disabled,
    Unavailable(String),
    Done(Review),
}

#[derive(Debug, Clone)]
pub struct ValidationInput {
    /// `Some(reason)` when the Claude execution run itself failed.
    pub claude_failure: Option<String>,
    pub completion: Option<CompletionReport>,
    pub permission_denials: Vec<String>,
    pub is_git_repo: bool,
    pub changed_files: Vec<String>,
    pub commands: Vec<CommandOutcome>,
    /// Check kinds enabled in settings with no runnable command (e.g. "test").
    pub missing_checks: Vec<String>,
    /// Detected commands skipped because the user has not approved them.
    pub unapproved: Vec<String>,
    pub review: ReviewState,
}

fn finding(source: &str, severity: FindingSeverity, message: impl Into<String>) -> Finding {
    Finding { source: source.into(), severity, message: message.into() }
}

fn completion_findings(c: &Option<CompletionReport>, out: &mut Vec<Finding>) {
    use FindingSeverity::*;
    let Some(c) = c else {
        out.push(finding("claude", Warning, "Claude did not return a structured completion report"));
        return;
    };
    match c.status.as_str() {
        "completed" => out.push(finding("claude", Info, format!("Claude reports completion: {}", c.summary))),
        "blocked" => out.push(finding("claude", Fail, format!("Claude reports it is blocked: {}", c.summary))),
        other => out.push(finding("claude", Warning, format!("Claude reports status '{other}': {}", c.summary))),
    }
    if c.tests_passed == Some(false) {
        out.push(finding("claude", Warning, "Claude reports that tests did not pass"));
    }
    for issue in &c.remaining_issues {
        out.push(finding("claude", Warning, format!("Remaining issue: {issue}")));
    }
}

fn command_findings(commands: &[CommandOutcome], out: &mut Vec<Finding>) {
    for c in commands {
        let label = format!("{} {}", c.program, c.args.join(" "));
        let sev = match (c.success, c.kind.as_str()) {
            (true, _) => FindingSeverity::Info,
            (false, "lint") => FindingSeverity::Warning,
            (false, _) => FindingSeverity::Fail,
        };
        let msg = if c.success {
            format!("{label} passed")
        } else {
            format!("{label} failed (exit {})", c.exit_code.map_or("none".into(), |e| e.to_string()))
        };
        out.push(finding(&c.kind, sev, msg));
    }
}

fn review_findings(review: &ReviewState, out: &mut Vec<Finding>) {
    use FindingSeverity::*;
    match review {
        ReviewState::Disabled => out.push(finding("review", Info, "Claude acceptance review is disabled in settings")),
        ReviewState::Unavailable(why) => {
            out.push(finding("review", Warning, format!("Acceptance criteria review unavailable: {why}")))
        }
        ReviewState::Done(r) => {
            for c in &r.criteria {
                let sev = match c.verdict.as_str() {
                    "met" => Info,
                    "not_met" => Fail,
                    _ => Warning,
                };
                out.push(finding("criteria", sev, format!("{} — {}: {}", c.criterion, c.verdict.replace('_', " "), c.evidence)));
            }
            if r.overall == "fail" && !out.iter().any(|f| f.source == "criteria" && f.severity == Fail) {
                out.push(finding("review", Fail, format!("Reviewer verdict: fail — {}", r.summary)));
            } else if r.overall == "warning" && !out.iter().any(|f| f.source == "criteria" && f.severity != Info) {
                out.push(finding("review", Warning, format!("Reviewer verdict: warning — {}", r.summary)));
            }
        }
    }
}

pub fn aggregate(input: &ValidationInput) -> (ValidationStatus, Vec<Finding>, String) {
    use FindingSeverity::*;
    let mut f: Vec<Finding> = vec![];
    if let Some(reason) = &input.claude_failure {
        f.push(finding("claude", Fail, format!("Claude run failed: {reason}")));
    }
    completion_findings(&input.completion, &mut f);
    for d in &input.permission_denials {
        f.push(finding("permissions", Warning, format!("Claude was denied permission for: {d}")));
    }
    if !input.is_git_repo {
        f.push(finding("git", Info, "Not a git repository; changed-file detection unavailable"));
    } else if input.changed_files.is_empty() {
        f.push(finding("git", Warning, "No file changes were detected"));
    } else {
        f.push(finding("git", Info, format!("{} file(s) changed", input.changed_files.len())));
    }
    command_findings(&input.commands, &mut f);
    for kind in &input.missing_checks {
        let sev = if kind == "test" { Warning } else { Info };
        f.push(finding(kind, sev, format!("No {kind} command available for this project")));
    }
    for label in &input.unapproved {
        f.push(finding("commands", Warning, format!("Skipped unapproved validation command: {label}")));
    }
    review_findings(&input.review, &mut f);

    let status = if f.iter().any(|x| x.severity == Fail) {
        ValidationStatus::Fail
    } else if f.iter().any(|x| x.severity == Warning) {
        ValidationStatus::Warning
    } else {
        ValidationStatus::Pass
    };
    let summary = summarize(status, &f);
    (status, f, summary)
}

fn summarize(status: ValidationStatus, findings: &[Finding]) -> String {
    let count = |s: FindingSeverity| findings.iter().filter(|f| f.severity == s).count();
    let headline = findings
        .iter()
        .find(|f| f.severity == FindingSeverity::Fail)
        .or_else(|| findings.iter().find(|f| f.severity == FindingSeverity::Warning))
        .map(|f| f.message.clone())
        .unwrap_or_else(|| "All checks passed".into());
    format!(
        "{}: {} failure(s), {} warning(s). {}",
        status.as_str(),
        count(FindingSeverity::Fail),
        count(FindingSeverity::Warning),
        crate::logging::truncate(&headline, 300)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::plan::CriterionVerdict;

    fn report(status: &str) -> CompletionReport {
        CompletionReport {
            status: status.into(), summary: "did it".into(), changed_files: vec![], tests_run: vec![],
            tests_passed: Some(true), remaining_issues: vec![], checkpoint_results: vec![],
        }
    }

    fn cmd(kind: &str, success: bool) -> CommandOutcome {
        CommandOutcome {
            kind: kind.into(), program: "npm".into(), args: vec!["run".into(), kind.into()],
            exit_code: Some(if success { 0 } else { 1 }), success, duration_ms: 10, output_tail: String::new(),
        }
    }

    fn review(verdicts: &[&str], overall: &str) -> ReviewState {
        ReviewState::Done(Review {
            overall: overall.into(),
            summary: "s".into(),
            criteria: verdicts.iter().enumerate().map(|(i, v)| CriterionVerdict {
                criterion: format!("c{i}"), verdict: v.to_string(), evidence: "e".into(),
            }).collect(),
        })
    }

    fn passing() -> ValidationInput {
        ValidationInput {
            claude_failure: None,
            completion: Some(report("completed")),
            permission_denials: vec![],
            is_git_repo: true,
            changed_files: vec!["src/a.ts".into()],
            commands: vec![cmd("build", true), cmd("test", true), cmd("lint", true)],
            missing_checks: vec![],
            unapproved: vec![],
            review: review(&["met", "met"], "pass"),
        }
    }

    #[test]
    fn all_good_is_pass() {
        let (s, _, summary) = aggregate(&passing());
        assert_eq!(s, ValidationStatus::Pass);
        assert!(summary.starts_with("PASS"));
    }

    #[test]
    fn failing_tests_or_build_fail() {
        let mut i = passing();
        i.commands[1] = cmd("test", false);
        assert_eq!(aggregate(&i).0, ValidationStatus::Fail);
        let mut i = passing();
        i.commands[0] = cmd("build", false);
        assert_eq!(aggregate(&i).0, ValidationStatus::Fail);
    }

    #[test]
    fn lint_failure_is_only_a_warning() {
        let mut i = passing();
        i.commands[2] = cmd("lint", false);
        assert_eq!(aggregate(&i).0, ValidationStatus::Warning);
    }

    #[test]
    fn unmet_criterion_fails_even_if_claude_says_done() {
        let mut i = passing();
        i.review = review(&["met", "not_met"], "fail");
        let (s, f, _) = aggregate(&i);
        assert_eq!(s, ValidationStatus::Fail);
        assert!(f.iter().any(|x| x.source == "criteria" && x.severity == FindingSeverity::Fail));
    }

    #[test]
    fn partial_or_uncertain_is_warning() {
        let mut i = passing();
        i.review = review(&["met", "cannot_determine"], "warning");
        assert_eq!(aggregate(&i).0, ValidationStatus::Warning);
        let mut i = passing();
        i.review = ReviewState::Unavailable("timeout".into());
        assert_eq!(aggregate(&i).0, ValidationStatus::Warning);
        let mut i = passing();
        i.completion = Some(report("partial"));
        assert_eq!(aggregate(&i).0, ValidationStatus::Warning);
    }

    #[test]
    fn claude_crash_or_block_fails() {
        let mut i = passing();
        i.claude_failure = Some("exit 1".into());
        assert_eq!(aggregate(&i).0, ValidationStatus::Fail);
        let mut i = passing();
        i.completion = Some(report("blocked"));
        assert_eq!(aggregate(&i).0, ValidationStatus::Fail);
    }

    #[test]
    fn missing_tests_and_no_changes_warn() {
        let mut i = passing();
        i.commands.retain(|c| c.kind != "test");
        i.missing_checks = vec!["test".into()];
        assert_eq!(aggregate(&i).0, ValidationStatus::Warning);
        let mut i = passing();
        i.changed_files.clear();
        assert_eq!(aggregate(&i).0, ValidationStatus::Warning);
        let mut i = passing();
        i.missing_checks = vec!["lint".into()];
        assert_eq!(aggregate(&i).0, ValidationStatus::Pass, "missing lint is informational");
    }

    #[test]
    fn permission_denials_and_unapproved_commands_warn() {
        let mut i = passing();
        i.permission_denials = vec!["Bash: git push".into()];
        assert_eq!(aggregate(&i).0, ValidationStatus::Warning);
        let mut i = passing();
        i.unapproved = vec!["npm run test".into()];
        assert_eq!(aggregate(&i).0, ValidationStatus::Warning);
    }
}
