//! Checkpoint-weighted progress. Never derived from time or token counts.

use crate::models::{Checkpoint, CheckpointStatus};

/// completed weight / total weight, or `None` when there is no plan.
pub fn compute(checkpoints: &[Checkpoint]) -> Option<f64> {
    let total: f64 = checkpoints.iter().map(|c| c.weight.max(0.0)).sum();
    if checkpoints.is_empty() || total <= 0.0 {
        return None;
    }
    let done: f64 = checkpoints
        .iter()
        .filter(|c| c.status == CheckpointStatus::Completed)
        .map(|c| c.weight.max(0.0))
        .sum();
    Some((done / total).clamp(0.0, 1.0))
}

/// Without any checkpoint marker after this many tool calls, we stop
/// claiming a percentage and report progress as unavailable.
pub const UNRELIABLE_AFTER_TOOL_CALLS: u32 = 15;

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(weight: f64, status: CheckpointStatus) -> Checkpoint {
        Checkpoint {
            id: String::new(), task_id: String::new(), ordinal: 0, title: String::new(),
            status, weight, owner: "claude".into(), started_at: None, completed_at: None,
        }
    }

    #[test]
    fn weighted_completion() {
        use CheckpointStatus::*;
        let cps = [cp(1.0, Completed), cp(3.0, Running), cp(1.0, Pending), cp(1.0, Completed)];
        assert_eq!(compute(&cps), Some(2.0 / 6.0));
    }

    #[test]
    fn running_and_failed_do_not_count() {
        use CheckpointStatus::*;
        assert_eq!(compute(&[cp(2.0, Running), cp(2.0, Failed)]), Some(0.0));
    }

    #[test]
    fn no_plan_means_unknown() {
        assert_eq!(compute(&[]), None);
        assert_eq!(compute(&[cp(0.0, CheckpointStatus::Completed)]), None);
    }

    #[test]
    fn all_done_is_one() {
        use CheckpointStatus::*;
        assert_eq!(compute(&[cp(1.0, Completed), cp(4.0, Completed)]), Some(1.0));
    }
}
