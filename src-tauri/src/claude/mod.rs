//! Claude Code integration: detection, prompting, process runs, and parsing.
//! V1 talks to Claude Code directly; a future `AgentRunner` trait would wrap
//! `ClaudeRunner` alongside other agents.

pub mod detect;
pub mod plan;
pub mod prompts;
pub mod runner;
pub mod stream;

pub use detect::{Capabilities, ClaudeStatus};
pub use runner::{ClaudeOutcome, ClaudeRequest, ClaudeRunner, SessionMode};
