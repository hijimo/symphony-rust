//! Shared E2E test harness module.
//!
//! This module provides test infrastructure for full lifecycle E2E tests:
//! - `FakeCodexProcess`: simulates the codex app-server subprocess
//! - `FakeLinearServer`: wiremock-based Linear GraphQL API simulator
//! - `TestOrchestrator`: wraps the Orchestrator with test controls
//!
//! Usage from test files:
//! ```rust
//! #[path = "e2e/harness/mod.rs"]
//! mod harness;
//! ```

pub mod fake_codex;
pub mod fake_linear;
pub mod test_orchestrator;

pub use fake_codex::{CodexBehavior, CodexEvent, FakeCodexProcess};
pub use fake_linear::{FakeLinearServer, LinearIssueBuilder};
pub use test_orchestrator::{TestOrchestrator, TestOrchestratorConfig, WorkerState};
