//! Parsing Claude Code `--output-format stream-json` output.
//!
//! Each stdout line is one JSON message. We extract only what TaskKiln needs
//! and ignore unknown message types so newer CLI versions keep working.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Init { session_id: String, model: Option<String> },
    Text(String),
    ToolUse { name: String, summary: String },
    ToolResult { is_error: bool, preview: String },
    Result(RunResult),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunResult {
    pub is_error: bool,
    pub subtype: String,
    pub text: String,
    pub session_id: Option<String>,
    pub structured: Option<Value>,
    pub cost_usd: Option<f64>,
    pub num_turns: Option<i64>,
    pub permission_denials: Vec<String>,
}

/// Parse one stdout line. Returns `None` for non-JSON lines and an empty vec
/// for JSON messages TaskKiln does not care about.
pub fn parse_line(line: &str) -> Option<Vec<StreamEvent>> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let events = match v.get("type").and_then(Value::as_str) {
        Some("system") => parse_system(&v).into_iter().collect(),
        Some("assistant") => parse_assistant(&v),
        Some("user") => parse_tool_results(&v),
        Some("result") => vec![StreamEvent::Result(parse_result(&v))],
        _ => vec![],
    };
    Some(events)
}

fn parse_system(v: &Value) -> Option<StreamEvent> {
    if v.get("subtype").and_then(Value::as_str) != Some("init") {
        return None;
    }
    Some(StreamEvent::Init {
        session_id: v.get("session_id")?.as_str()?.to_string(),
        model: v.get("model").and_then(Value::as_str).map(str::to_string),
    })
}

fn content_blocks(v: &Value) -> &[Value] {
    v.pointer("/message/content").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn parse_assistant(v: &Value) -> Vec<StreamEvent> {
    content_blocks(v)
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") => block.get("text").and_then(Value::as_str).map(|t| StreamEvent::Text(t.to_string())),
            Some("tool_use") => {
                let name = block.get("name").and_then(Value::as_str).unwrap_or("tool").to_string();
                let summary = summarize_tool(&name, block.get("input").unwrap_or(&Value::Null));
                Some(StreamEvent::ToolUse { name, summary })
            }
            _ => None,
        })
        .collect()
}

fn parse_tool_results(v: &Value) -> Vec<StreamEvent> {
    content_blocks(v)
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
        .map(|b| StreamEvent::ToolResult {
            is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            preview: crate::logging::truncate(&tool_result_text(b.get("content")), 400),
        })
        .collect()
}

fn tool_result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_result(v: &Value) -> RunResult {
    let denials = v
        .get("permission_denials")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|d| {
                    let tool = d.get("tool_name").and_then(Value::as_str).unwrap_or("tool");
                    let input = d.get("tool_input").unwrap_or(&Value::Null);
                    format!("{tool}: {}", summarize_tool(tool, input))
                })
                .collect()
        })
        .unwrap_or_default();
    RunResult {
        is_error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false),
        subtype: v.get("subtype").and_then(Value::as_str).unwrap_or("").to_string(),
        text: v.get("result").and_then(Value::as_str).unwrap_or("").to_string(),
        session_id: v.get("session_id").and_then(Value::as_str).map(str::to_string),
        structured: v.get("structured_output").filter(|s| !s.is_null()).cloned(),
        cost_usd: v.get("total_cost_usd").and_then(Value::as_f64),
        num_turns: v.get("num_turns").and_then(Value::as_i64),
        permission_denials: denials,
    }
}

/// One-line description of a tool call for the CURRENT ACTIVITY display.
pub fn summarize_tool(name: &str, input: &Value) -> String {
    // The CLI's --json-schema mechanism surfaces as a tool call; describe it plainly.
    if name == "StructuredOutput" {
        return "writing structured report".into();
    }
    let field = |k: &str| input.get(k).and_then(Value::as_str);
    let detail = field("file_path")
        .or_else(|| field("command"))
        .or_else(|| field("pattern"))
        .or_else(|| field("path"))
        .or_else(|| field("url"))
        .or_else(|| field("description"))
        .unwrap_or("");
    let detail = detail.lines().next().unwrap_or("");
    let line = if detail.is_empty() { name.to_string() } else { format!("{name} {detail}") };
    crate::logging::sanitize(&crate::logging::truncate(&line, 160))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    Start,
    Done,
    Failed,
}

/// Extract `TASKKILN_CHECKPOINT_<START|DONE|FAILED> <n>` markers that the
/// execution prompt asks Claude to print on their own lines.
pub fn checkpoint_markers(text: &str) -> Vec<(MarkerKind, i64)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?m)^[\s>*`]*TASKKILN_CHECKPOINT_(START|DONE|FAILED)[\s:#]+(\d{1,3})\b").expect("static regex")
    });
    re.captures_iter(text)
        .filter_map(|c| {
            let kind = match &c[1] {
                "START" => MarkerKind::Start,
                "DONE" => MarkerKind::Done,
                _ => MarkerKind::Failed,
            };
            c[2].parse().ok().map(|n| (kind, n))
        })
        .collect()
}

/// Fallback when `structured_output` is absent: take the last JSON object in
/// the final text (optionally inside a ```json fence).
pub fn extract_json_object(text: &str) -> Option<Value> {
    let bytes: Vec<(usize, char)> = text.char_indices().collect();
    for (i, (start, ch)) in bytes.iter().enumerate().rev() {
        if *ch != '{' {
            continue;
        }
        let mut depth = 0i32;
        for (pos, c) in &bytes[i..] {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(v) = serde_json::from_str::<Value>(&text[*start..=*pos]) {
                            if v.is_object() && v.as_object().is_some_and(|o| o.len() > 1) {
                                return Some(v);
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init_text_tool_and_result() {
        let init = r#"{"type":"system","subtype":"init","session_id":"abc","model":"claude-x"}"#;
        assert_eq!(parse_line(init).unwrap(), vec![StreamEvent::Init { session_id: "abc".into(), model: Some("claude-x".into()) }]);

        let asst = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"},{"type":"tool_use","name":"Edit","input":{"file_path":"src/a.ts"}}]}}"#;
        let ev = parse_line(asst).unwrap();
        assert_eq!(ev[0], StreamEvent::Text("hi".into()));
        assert_eq!(ev[1], StreamEvent::ToolUse { name: "Edit".into(), summary: "Edit src/a.ts".into() });

        let result = r#"{"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"abc","total_cost_usd":0.12,"num_turns":4,"structured_output":{"status":"completed"},"permission_denials":[{"tool_name":"Bash","tool_input":{"command":"git push"}}]}"#;
        let StreamEvent::Result(r) = &parse_line(result).unwrap()[0] else { panic!() };
        assert!(!r.is_error);
        assert_eq!(r.structured.as_ref().unwrap()["status"], "completed");
        assert_eq!(r.permission_denials, vec!["Bash: Bash git push"]);
    }

    #[test]
    fn ignores_unknown_and_non_json_lines() {
        assert_eq!(parse_line("not json"), None);
        assert_eq!(parse_line(r#"{"type":"system","subtype":"hook_started"}"#), Some(vec![]));
        assert_eq!(parse_line(r#"{"type":"stream_event"}"#), Some(vec![]));
    }

    #[test]
    fn tool_results_flag_errors() {
        let l = r#"{"type":"user","message":{"content":[{"type":"tool_result","is_error":true,"content":[{"type":"text","text":"3 failed"}]}]}}"#;
        assert_eq!(parse_line(l).unwrap(), vec![StreamEvent::ToolResult { is_error: true, preview: "3 failed".into() }]);
    }

    #[test]
    fn finds_checkpoint_markers() {
        let text = "Starting.\nTASKKILN_CHECKPOINT_START 1\nwork\n  TASKKILN_CHECKPOINT_DONE 1\n`TASKKILN_CHECKPOINT_START 2`\nmention TASKKILN_CHECKPOINT_DONE 9 inline\nTASKKILN_CHECKPOINT_FAILED: 2";
        assert_eq!(
            checkpoint_markers(text),
            vec![(MarkerKind::Start, 1), (MarkerKind::Done, 1), (MarkerKind::Start, 2), (MarkerKind::Failed, 2)]
        );
    }

    #[test]
    fn extracts_trailing_json() {
        let text = "Summary below\n```json\n{\"status\": \"completed\", \"summary\": \"ok {braces}\"}\n```";
        assert_eq!(extract_json_object(text).unwrap()["status"], "completed");
        assert!(extract_json_object("no json {x}").is_none());
    }

    #[test]
    fn summaries_are_sanitized() {
        let input = serde_json::json!({"command": "curl -H 'Authorization: Bearer abcdefghijklmnop123'"});
        assert!(summarize_tool("Bash", &input).contains("REDACTED"));
    }
}
