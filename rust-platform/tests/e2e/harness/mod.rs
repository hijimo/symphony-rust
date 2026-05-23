#![allow(
    unused_imports,
    unused_variables,
    dead_code,
    clippy::bind_instead_of_map,
    clippy::derivable_impls,
    clippy::manual_range_contains,
    clippy::needless_borrows_for_generic_args,
    clippy::ptr_arg,
    clippy::duplicated_attributes,
    clippy::approx_constant,
    clippy::bool_assert_comparison,
    clippy::len_zero,
    clippy::let_and_return
)]

//! Shared E2E test harness module.
//!
//! This module provides test infrastructure for full lifecycle E2E tests:
//! - `FakeCodexProcess`: simulates the codex app-server subprocess
//! - `FakeLinearServer`: wiremock-based Linear GraphQL API simulator
//! - `TestOrchestrator`: wraps the Orchestrator with test controls
//!
//! Usage from test files:
//! ```rust
//! #[allow(dead_code, unused_imports)]
//! #[path = "e2e/harness/mod.rs"]
//! mod harness;
//! ```

pub mod fake_codex;
pub mod fake_linear;
pub mod test_orchestrator;

pub use fake_codex::{
    CodexBehavior, CodexEvent, FakeCodexProcess, JsonRpcRequest, JsonRpcResponse, TurnScenario,
};
pub use fake_linear::{FakeLinearServer, LinearErrorMode, LinearIssueBuilder, StateChangeRecord};
pub use test_orchestrator::{TestOrchestrator, TestOrchestratorConfig, WorkerState};
