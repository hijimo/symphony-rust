//! External signals → intent (docs/方案/ai-native-sdlc-p3-design.md §3).
//!
//! Signals are pushed by monitoring sources, de-duplicated by fingerprint
//! and stored redacted. People triage them on the board (S3/S4).

pub mod sanitize;
pub mod sources;
pub mod store;

use serde::{Deserialize, Serialize};

pub use store::SignalStore;

/// Supported sources (`/api/ingest/{project_id}/{source}`).
pub const SOURCES: [&str; 3] = ["sentry", "alertmanager", "generic"];

/// Request body limit of the ingest endpoint.
pub const MAX_BODY_BYTES: usize = 256 * 1024;
/// Ingest requests per key and minute.
pub const PER_KEY_PER_MINUTE: u32 = 60;
/// New fingerprints are dropped while a project has this many untriaged signals.
pub const MAX_PENDING_PER_PROJECT: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Critical => "critical",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "critical" => Some(Severity::Critical),
            "warning" => Some(Severity::Warning),
            "info" => Some(Severity::Info),
            _ => None,
        }
    }
}

/// One signal parsed from a source payload, already redacted.
#[derive(Debug, Clone, PartialEq)]
pub struct IncomingSignal {
    pub fingerprint: String,
    pub title: String,
    /// `None` keeps the stored severity (default warning for new signals).
    pub severity: Option<Severity>,
    pub resolved: bool,
    pub evidence: serde_json::Value,
    pub external_url: Option<String>,
}
