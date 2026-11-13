//! Agent Module — Codex app-server integration and worker lifecycle.
//!
//! This module contains:
//! - `codex_client`: Manages the Codex app-server subprocess (spawn, turn, stop, kill)
//! - `runner`: Full worker lifecycle (workspace + prompt + turns + cleanup)
//!
//! SPEC reference: Section 10

pub mod change_docs;
pub mod codex_client;
pub mod plan_check;
pub mod post_run;
pub mod repo_instructions;
pub mod runner;
pub mod validation;

pub use crate::models::{CodexEventUpdate, TokenUsage};
pub use codex_client::{CodexClient, CodexError, TurnResult};
pub use runner::{AgentBlockerRef, AgentError, AgentIssue, AgentRunner, IssueStateRefresher};
