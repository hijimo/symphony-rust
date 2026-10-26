//! Domain types of the AI-native SDLC document flow
//! (docs/方案/ai-native-sdlc-flow-design.md §4).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::error::WebPlatformError;

/// Root directory of change documents in the primary repository (§3, fixed).
pub const CHANGES_DIR: &str = "docs/changes";
/// Project-level template directory (§5.2).
pub const TEMPLATES_DIR: &str = "docs/changes/_templates";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum DocType {
    Intent,
    Spec,
    Plan,
}

impl DocType {
    pub const ALL: [DocType; 3] = [DocType::Intent, DocType::Spec, DocType::Plan];

    pub fn as_str(self) -> &'static str {
        match self {
            DocType::Intent => "intent",
            DocType::Spec => "spec",
            DocType::Plan => "plan",
        }
    }

    pub fn file_name(self) -> String {
        format!("{}.md", self.as_str())
    }

    pub fn gate(self) -> Gate {
        match self {
            DocType::Intent => Gate::G1,
            DocType::Spec => Gate::G2,
            DocType::Plan => Gate::G3,
        }
    }

    /// The next document drafted after this one is approved.
    pub fn next(self) -> Option<DocType> {
        match self {
            DocType::Intent => Some(DocType::Spec),
            DocType::Spec => Some(DocType::Plan),
            DocType::Plan => None,
        }
    }

    /// Documents downstream of this one (cascade targets, §4.3).
    pub fn downstream(self) -> &'static [DocType] {
        match self {
            DocType::Intent => &[DocType::Spec, DocType::Plan],
            DocType::Spec => &[DocType::Plan],
            DocType::Plan => &[],
        }
    }
}

impl fmt::Display for DocType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DocType {
    type Err = WebPlatformError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "intent" => Ok(DocType::Intent),
            "spec" => Ok(DocType::Spec),
            "plan" => Ok(DocType::Plan),
            other => Err(WebPlatformError::BadRequest(format!(
                "unknown document type: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gate {
    G1,
    G2,
    G3,
}

impl Gate {
    pub fn as_str(self) -> &'static str {
        match self {
            Gate::G1 => "G1",
            Gate::G2 => "G2",
            Gate::G3 => "G3",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocStatus {
    Draft,
    InReview,
    ChangesRequested,
    Approved,
    Stale,
    Rejected,
    Merged,
}

impl DocStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DocStatus::Draft => "draft",
            DocStatus::InReview => "in_review",
            DocStatus::ChangesRequested => "changes_requested",
            DocStatus::Approved => "approved",
            DocStatus::Stale => "stale",
            DocStatus::Rejected => "rejected",
            DocStatus::Merged => "merged",
        }
    }

    pub fn parse(s: &str) -> Result<Self, WebPlatformError> {
        Ok(match s {
            "draft" => DocStatus::Draft,
            "in_review" => DocStatus::InReview,
            "changes_requested" => DocStatus::ChangesRequested,
            "approved" => DocStatus::Approved,
            "stale" => DocStatus::Stale,
            "rejected" => DocStatus::Rejected,
            "merged" => DocStatus::Merged,
            other => {
                return Err(WebPlatformError::Internal(format!(
                    "unknown document status in db: {other}"
                )))
            }
        })
    }

    /// Editable by people and AI in the workbench (merged / rejected are frozen).
    pub fn is_editable(self) -> bool {
        !matches!(self, DocStatus::Merged | DocStatus::Rejected)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Track {
    Full,
    Plan,
}

impl Track {
    pub fn as_str(self) -> &'static str {
        match self {
            Track::Full => "full",
            Track::Plan => "plan",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Track::Full => "track:full",
            Track::Plan => "track:plan",
        }
    }

    pub fn parse(s: &str) -> Result<Self, WebPlatformError> {
        match s {
            "full" => Ok(Track::Full),
            "plan" => Ok(Track::Plan),
            other => Err(WebPlatformError::BadRequest(format!(
                "track must be 'full' or 'plan', got {other}"
            ))),
        }
    }

    /// Documents that exist on this track.
    pub fn doc_types(self) -> &'static [DocType] {
        match self {
            Track::Full => &[DocType::Intent, DocType::Spec, DocType::Plan],
            Track::Plan => &[DocType::Plan],
        }
    }
}

/// Issue flow state derived from document states (§4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowState {
    Intent,
    IntentReview,
    Spec,
    SpecReview,
    Plan,
    PlanReview,
    /// plan approved, docs MR not merged yet.
    PlanApprovedPendingMerge,
    /// All documents merged: implementation phase owns the issue.
    Todo,
    Rejected,
}

impl FlowState {
    /// The `doc:*` label shown on the platform (§11 decision 4). `Todo` and
    /// `Rejected` are not document labels.
    pub fn doc_label(self) -> Option<&'static str> {
        match self {
            FlowState::Intent => Some("doc:intent"),
            FlowState::IntentReview => Some("doc:intent-review"),
            FlowState::Spec => Some("doc:spec"),
            FlowState::SpecReview => Some("doc:spec-review"),
            FlowState::Plan => Some("doc:plan"),
            FlowState::PlanReview | FlowState::PlanApprovedPendingMerge => Some("doc:plan-review"),
            FlowState::Todo | FlowState::Rejected => None,
        }
    }
}

/// All `doc:*` labels written by Symphony.
pub const DOC_LABELS: [&str; 6] = [
    "doc:intent",
    "doc:intent-review",
    "doc:spec",
    "doc:spec-review",
    "doc:plan",
    "doc:plan-review",
];

/// Path of a document inside the primary repository checkout.
pub fn doc_repo_path(issue_iid: i64, doc: DocType) -> String {
    format!("{CHANGES_DIR}/{issue_iid}/{}", doc.file_name())
}

/// Writable directory of an issue inside the primary repository checkout.
pub fn issue_changes_dir(issue_iid: i64) -> String {
    format!("{CHANGES_DIR}/{issue_iid}")
}

/// Docs branch name (§3): `symphony/docs-<iid>`, `-r<n>` from revision 2.
pub fn docs_branch(issue_iid: i64, revision: i64) -> String {
    if revision <= 1 {
        format!("symphony/docs-{issue_iid}")
    } else {
        format!("symphony/docs-{issue_iid}-r{revision}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_type_round_trip_and_gates() {
        for d in DocType::ALL {
            assert_eq!(d.as_str().parse::<DocType>().unwrap(), d);
        }
        assert_eq!(DocType::Intent.gate(), Gate::G1);
        assert_eq!(DocType::Plan.gate(), Gate::G3);
        assert!("nope".parse::<DocType>().is_err());
    }

    #[test]
    fn downstream_and_next() {
        assert_eq!(DocType::Intent.next(), Some(DocType::Spec));
        assert_eq!(DocType::Plan.next(), None);
        assert_eq!(
            DocType::Intent.downstream(),
            &[DocType::Spec, DocType::Plan]
        );
        assert!(DocType::Plan.downstream().is_empty());
    }

    #[test]
    fn status_round_trip() {
        for s in [
            DocStatus::Draft,
            DocStatus::InReview,
            DocStatus::ChangesRequested,
            DocStatus::Approved,
            DocStatus::Stale,
            DocStatus::Rejected,
            DocStatus::Merged,
        ] {
            assert_eq!(DocStatus::parse(s.as_str()).unwrap(), s);
        }
        assert!(!DocStatus::Merged.is_editable());
        assert!(DocStatus::Stale.is_editable());
    }

    #[test]
    fn paths_and_branches() {
        assert_eq!(
            doc_repo_path(123, DocType::Spec),
            "docs/changes/123/spec.md"
        );
        assert_eq!(docs_branch(123, 1), "symphony/docs-123");
        assert_eq!(docs_branch(123, 2), "symphony/docs-123-r2");
    }

    #[test]
    fn track_docs() {
        assert_eq!(Track::Plan.doc_types(), &[DocType::Plan]);
        assert_eq!(Track::Full.doc_types().len(), 3);
        assert!(Track::parse("fast").is_err());
    }

    #[test]
    fn flow_state_labels() {
        assert_eq!(
            FlowState::PlanApprovedPendingMerge.doc_label(),
            Some("doc:plan-review")
        );
        assert_eq!(FlowState::Todo.doc_label(), None);
    }
}
