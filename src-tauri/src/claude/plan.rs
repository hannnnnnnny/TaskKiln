//! Typed views of Claude's structured outputs (plan, completion, review).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::stream::{extract_json_object, RunResult};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct PlanStep {
    pub title: String,
    #[serde(default = "one")]
    pub weight: f64,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Plan {
    pub checkpoints: Vec<PlanStep>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CheckpointResult {
    pub index: i64,
    pub status: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CompletionReport {
    pub status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub tests_run: Vec<String>,
    #[serde(default)]
    pub tests_passed: Option<bool>,
    #[serde(default)]
    pub remaining_issues: Vec<String>,
    #[serde(default)]
    pub checkpoint_results: Vec<CheckpointResult>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CriterionVerdict {
    pub criterion: String,
    pub verdict: String,
    #[serde(default)]
    pub evidence: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Review {
    pub overall: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub criteria: Vec<CriterionVerdict>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DraftCriteria {
    pub criteria: Vec<String>,
}

/// Prefer the CLI's validated `structured_output`; fall back to the last JSON
/// object in the final message for CLI versions without `--json-schema`.
pub fn structured<T: for<'de> Deserialize<'de>>(result: &RunResult) -> Option<T> {
    let value: Value = result.structured.clone().or_else(|| extract_json_object(&result.text))?;
    serde_json::from_value(value).ok()
}

impl Plan {
    /// Clean titles, clamp weights, and drop empty steps so bad output can't
    /// produce zero-weight or absurd plans.
    pub fn normalized(&self) -> Vec<(String, f64)> {
        self.checkpoints
            .iter()
            .map(|s| (s.title.trim().chars().take(120).collect::<String>(), s.weight.clamp(1.0, 5.0)))
            .filter(|(t, _)| !t.is_empty())
            .take(12)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_plan_and_normalizes() {
        let r = RunResult {
            structured: Some(serde_json::json!({"checkpoints": [
                {"title": " inspect ", "weight": 0}, {"title": "", "weight": 2}, {"title": "implement", "weight": 9}
            ]})),
            ..Default::default()
        };
        let plan: Plan = structured(&r).unwrap();
        assert_eq!(plan.normalized(), vec![("inspect".to_string(), 1.0), ("implement".to_string(), 5.0)]);
    }

    #[test]
    fn falls_back_to_json_in_text() {
        let r = RunResult {
            text: "Done.\n{\"status\":\"completed\",\"summary\":\"ok\",\"tests_passed\":true}".into(),
            ..Default::default()
        };
        let c: CompletionReport = structured(&r).unwrap();
        assert_eq!(c.status, "completed");
        assert_eq!(c.tests_passed, Some(true));
        assert!(c.changed_files.is_empty());
    }

    #[test]
    fn missing_output_is_none() {
        assert!(structured::<Plan>(&RunResult::default()).is_none());
    }
}
