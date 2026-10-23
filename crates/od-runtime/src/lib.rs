//! Coding-agent runtimes (spec 2.3, 5.4).
//!
//! Each agent is a [`RuntimeDef`] value: binary, argv shape, stream format,
//! prompt delivery, session handling. One [`RunEngine`] interprets them:
//! detection, spawn as a process-group leader, stream parsing into
//! [`od_contracts::AgentEvent`], steering, cancel escalation, idle timeout,
//! retry, resume fallback and the tool-loop guard.

pub mod artifact;
pub mod byok;
pub mod critique;
pub mod def;
pub mod detect;
pub mod engine;
pub mod env;
pub mod guard;
pub mod parse;
pub mod process;
pub mod stage;
pub mod tokens;

pub use def::{PermissionMode, RuntimeDef};
pub use engine::{EngineConfig, LaunchSpec, RunEngine, RunOutcome, SteerError};
pub use tokens::ToolTokens;
