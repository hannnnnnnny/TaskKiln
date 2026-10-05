//! Task scheduling: deterministic decisions (`core`), progress math
//! (`progress`), the long-lived `Engine`, and per-task `pipeline`s.

pub mod core;
pub mod engine;
pub mod pipeline;
pub mod progress;

pub use self::core::{RunMode, UserAction};
pub use engine::{Engine, Host, QueueInfo};

#[cfg(test)]
mod tests;
