//! Agent Module — Codex app-server integration and worker lifecycle.
//!
//! This module contains:
//! - `codex_client`: Manages the Codex app-server subprocess (spawn, turn, stop, kill)
//! - `runner`: Full worker lifecycle (workspace + prompt + turns + cleanup)
//!
//! SPEC reference: Section 10

pub mod codex_client;
pub mod runner;

pub use codex_client::{CodexClient, CodexError, CodexEventUpdate, TokenUsage, TurnResult};
pub use runner::{AgentBlockerRef, AgentError, AgentIssue, AgentRunner, IssueStateRefresher};
