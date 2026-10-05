//! Status enums and the task state machine.
//!
//! Every status change in TaskKiln goes through [`TaskStatus::can_transition_to`],
//! so impossible transitions (e.g. QUEUED -> COMPLETED) are rejected centrally.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Queued,
    Planning,
    Running,
    Testing,
    Validating,
    NeedsUser,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub const ALL: [TaskStatus; 10] = [
        TaskStatus::Queued,
        TaskStatus::Planning,
        TaskStatus::Running,
        TaskStatus::Testing,
        TaskStatus::Validating,
        TaskStatus::NeedsUser,
        TaskStatus::Paused,
        TaskStatus::Completed,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Queued => "QUEUED",
            TaskStatus::Planning => "PLANNING",
            TaskStatus::Running => "RUNNING",
            TaskStatus::Testing => "TESTING",
            TaskStatus::Validating => "VALIDATING",
            TaskStatus::NeedsUser => "NEEDS_USER",
            TaskStatus::Paused => "PAUSED",
            TaskStatus::Completed => "COMPLETED",
            TaskStatus::Failed => "FAILED",
            TaskStatus::Cancelled => "CANCELLED",
        }
    }

    /// States in which a Claude process or validation step may be live.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            TaskStatus::Planning | TaskStatus::Running | TaskStatus::Testing | TaskStatus::Validating
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, TaskStatus::Completed | TaskStatus::Cancelled)
    }

    /// The allowed transition table. Keep this the single source of truth.
    pub fn can_transition_to(self, next: TaskStatus) -> bool {
        use TaskStatus::*;
        match self {
            Queued => matches!(next, Planning | Running | Cancelled),
            // Running is allowed from Queued only for tasks that already have a plan
            // (e.g. returned to the queue after an interruption).
            Planning => matches!(next, Running | NeedsUser | Failed | Paused),
            Running => matches!(next, Testing | Validating | NeedsUser | Failed | Paused),
            Testing => matches!(next, Validating | NeedsUser | Failed | Paused),
            Validating => matches!(next, Completed | NeedsUser | Failed | Paused),
            // Planning is reachable again only to resume a task interrupted before it had a plan.
            NeedsUser => matches!(
                next,
                Planning | Running | Testing | Validating | Completed | Failed | Queued | Paused | Cancelled
            ),
            Paused => matches!(next, Queued | Planning | Running | Testing | Failed | Cancelled),
            Failed => matches!(next, Queued | Cancelled),
            Completed | Cancelled => false,
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TaskStatus::ALL
            .iter()
            .copied()
            .find(|st| st.as_str() == s)
            .ok_or_else(|| format!("unknown task status: {s}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckpointStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl CheckpointStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CheckpointStatus::Pending => "PENDING",
            CheckpointStatus::Running => "RUNNING",
            CheckpointStatus::Completed => "COMPLETED",
            CheckpointStatus::Failed => "FAILED",
        }
    }
}

impl FromStr for CheckpointStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PENDING" => Ok(Self::Pending),
            "RUNNING" => Ok(Self::Running),
            "COMPLETED" => Ok(Self::Completed),
            "FAILED" => Ok(Self::Failed),
            _ => Err(format!("unknown checkpoint status: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValidationStatus {
    Pass,
    Warning,
    Fail,
    /// The user chose IGNORE & CONTINUE on a WARNING/FAIL ("completed with warning").
    Overridden,
}

impl ValidationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ValidationStatus::Pass => "PASS",
            ValidationStatus::Warning => "WARNING",
            ValidationStatus::Fail => "FAIL",
            ValidationStatus::Overridden => "OVERRIDDEN",
        }
    }
}

impl FromStr for ValidationStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PASS" => Ok(Self::Pass),
            "WARNING" => Ok(Self::Warning),
            "FAIL" => Ok(Self::Fail),
            "OVERRIDDEN" => Ok(Self::Overridden),
            _ => Err(format!("unknown validation status: {s}")),
        }
    }
}

/// Why a task is waiting on the user. Drives which actions the UI offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttentionReason {
    ValidationWarning,
    ValidationFail,
    /// TaskKiln exited while the task was active; real state is unknown.
    Interrupted,
    ClaudeFailed,
    PlanFailed,
}

impl AttentionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            AttentionReason::ValidationWarning => "VALIDATION_WARNING",
            AttentionReason::ValidationFail => "VALIDATION_FAIL",
            AttentionReason::Interrupted => "INTERRUPTED",
            AttentionReason::ClaudeFailed => "CLAUDE_FAILED",
            AttentionReason::PlanFailed => "PLAN_FAILED",
        }
    }
}

impl FromStr for AttentionReason {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "VALIDATION_WARNING" => Ok(Self::ValidationWarning),
            "VALIDATION_FAIL" => Ok(Self::ValidationFail),
            "INTERRUPTED" => Ok(Self::Interrupted),
            "CLAUDE_FAILED" => Ok(Self::ClaudeFailed),
            "PLAN_FAILED" => Ok(Self::PlanFailed),
            _ => Err(format!("unknown attention reason: {s}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TaskStatus::*;

    #[test]
    fn happy_path_is_allowed() {
        let path = [Queued, Planning, Running, Testing, Validating, Completed];
        for w in path.windows(2) {
            assert!(w[0].can_transition_to(w[1]), "{} -> {}", w[0], w[1]);
        }
    }

    #[test]
    fn failure_and_fix_path_is_allowed() {
        let path = [Running, Validating, NeedsUser, Running, Testing, Validating, Completed];
        for w in path.windows(2) {
            assert!(w[0].can_transition_to(w[1]), "{} -> {}", w[0], w[1]);
        }
        assert!(NeedsUser.can_transition_to(Completed), "ignore & continue");
        assert!(NeedsUser.can_transition_to(Paused), "stop queue");
    }

    #[test]
    fn impossible_transitions_are_rejected() {
        assert!(!Queued.can_transition_to(Completed));
        assert!(!Queued.can_transition_to(Validating));
        assert!(!Planning.can_transition_to(Completed));
        assert!(!Running.can_transition_to(Completed), "must validate first");
        assert!(!Completed.can_transition_to(Queued));
        assert!(!Cancelled.can_transition_to(Running));
        for s in TaskStatus::ALL {
            assert!(!s.can_transition_to(s), "{s} self-loop");
        }
    }

    #[test]
    fn status_round_trips_through_strings() {
        for s in TaskStatus::ALL {
            assert_eq!(s.as_str().parse::<TaskStatus>().unwrap(), s);
        }
        assert!("BOGUS".parse::<TaskStatus>().is_err());
    }
}
