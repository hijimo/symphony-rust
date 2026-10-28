//! Pure state rules: per-document transitions (§4.2), cascade (§4.3) and
//! issue-state derivation (§4.5). No I/O here so every rule is unit-testable.

use std::collections::BTreeMap;

use super::model::{DocStatus, DocType, FlowState, Track};
use crate::error::WebPlatformError;

/// Human action at a gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateAction {
    Approve,
    RequestChanges,
    Reject,
}

impl GateAction {
    pub fn as_str(self) -> &'static str {
        match self {
            GateAction::Approve => "approve",
            GateAction::RequestChanges => "request_changes",
            GateAction::Reject => "reject",
        }
    }

    pub fn parse(s: &str) -> Result<Self, WebPlatformError> {
        match s {
            "approve" => Ok(GateAction::Approve),
            "request_changes" => Ok(GateAction::RequestChanges),
            "reject" => Ok(GateAction::Reject),
            other => Err(WebPlatformError::BadRequest(format!(
                "unknown gate action: {other}"
            ))),
        }
    }
}

fn conflict(msg: impl Into<String>) -> WebPlatformError {
    WebPlatformError::Conflict(msg.into())
}

/// `draft | changes_requested | stale -> in_review` (submit for review).
pub fn submit(doc: DocType, status: DocStatus) -> Result<DocStatus, WebPlatformError> {
    match status {
        DocStatus::Draft | DocStatus::ChangesRequested | DocStatus::Stale => {
            Ok(DocStatus::InReview)
        }
        other => Err(conflict(format!(
            "{doc} 当前状态为 {}，不能提交审阅",
            other.as_str()
        ))),
    }
}

/// Apply a gate action to a document under review.
pub fn decide(
    doc: DocType,
    status: DocStatus,
    action: GateAction,
) -> Result<DocStatus, WebPlatformError> {
    if status != DocStatus::InReview {
        return Err(conflict(format!(
            "{doc} 不在审阅中（当前 {}），不能执行 {}",
            status.as_str(),
            action.as_str()
        )));
    }
    match action {
        GateAction::Approve => Ok(DocStatus::Approved),
        GateAction::RequestChanges => Ok(DocStatus::ChangesRequested),
        GateAction::Reject if doc == DocType::Intent => Ok(DocStatus::Rejected),
        GateAction::Reject => Err(WebPlatformError::BadRequest(
            "只有 intent 可以被拒绝".to_string(),
        )),
    }
}

/// Content of an approved/in-review document changed before merge (§4.2):
/// approval is void and the document goes back to review. Draft-like states
/// are unaffected. Merged documents are never compared (merged phase).
pub fn on_content_changed(status: DocStatus) -> DocStatus {
    match status {
        DocStatus::Approved => DocStatus::InReview,
        other => other,
    }
}

/// Downstream documents that become `stale` when `doc` is (re)approved (§4.3).
/// Only previously approved / in-review downstream documents are affected.
pub fn cascade_targets(doc: DocType, docs: &BTreeMap<DocType, DocStatus>) -> Vec<DocType> {
    doc.downstream()
        .iter()
        .copied()
        .filter(|d| matches!(docs.get(d), Some(DocStatus::Approved | DocStatus::InReview)))
        .collect()
}

/// Derive the issue flow state from document states (§4.5, review A4).
/// `docs` contains only documents that exist.
pub fn derive_flow_state(track: Track, docs: &BTreeMap<DocType, DocStatus>) -> FlowState {
    if docs.values().any(|s| *s == DocStatus::Rejected) {
        return FlowState::Rejected;
    }
    let required = track.doc_types();
    if required
        .iter()
        .all(|d| docs.get(d) == Some(&DocStatus::Merged))
    {
        return FlowState::Todo;
    }
    for d in required {
        let state = match docs.get(d) {
            None => return drafting_state(*d),
            Some(s) => *s,
        };
        match state {
            DocStatus::Approved | DocStatus::Merged => continue,
            DocStatus::InReview | DocStatus::Stale => return review_state(*d),
            DocStatus::Draft | DocStatus::ChangesRequested => return drafting_state(*d),
            DocStatus::Rejected => return FlowState::Rejected,
        }
    }
    // Every required document approved but not merged yet.
    FlowState::PlanApprovedPendingMerge
}

fn drafting_state(d: DocType) -> FlowState {
    match d {
        DocType::Intent => FlowState::Intent,
        DocType::Spec => FlowState::Spec,
        DocType::Plan => FlowState::Plan,
    }
}

fn review_state(d: DocType) -> FlowState {
    match d {
        DocType::Intent => FlowState::IntentReview,
        DocType::Spec => FlowState::SpecReview,
        DocType::Plan => FlowState::PlanReview,
    }
}

/// Upstream documents must be approved before a downstream gate can approve.
pub fn upstream_approved(track: Track, doc: DocType, docs: &BTreeMap<DocType, DocStatus>) -> bool {
    track
        .doc_types()
        .iter()
        .take_while(|d| **d != doc)
        .all(|d| matches!(docs.get(d), Some(DocStatus::Approved | DocStatus::Merged)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(DocType, DocStatus)]) -> BTreeMap<DocType, DocStatus> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn submit_rules() {
        assert_eq!(
            submit(DocType::Spec, DocStatus::Draft).unwrap(),
            DocStatus::InReview
        );
        assert_eq!(
            submit(DocType::Spec, DocStatus::Stale).unwrap(),
            DocStatus::InReview
        );
        assert_eq!(
            submit(DocType::Spec, DocStatus::ChangesRequested).unwrap(),
            DocStatus::InReview
        );
        assert!(submit(DocType::Spec, DocStatus::Approved).is_err());
        assert!(submit(DocType::Spec, DocStatus::Merged).is_err());
    }

    #[test]
    fn decide_rules() {
        use GateAction::*;
        assert_eq!(
            decide(DocType::Plan, DocStatus::InReview, Approve).unwrap(),
            DocStatus::Approved
        );
        assert_eq!(
            decide(DocType::Plan, DocStatus::InReview, RequestChanges).unwrap(),
            DocStatus::ChangesRequested
        );
        assert_eq!(
            decide(DocType::Intent, DocStatus::InReview, Reject).unwrap(),
            DocStatus::Rejected
        );
        assert!(decide(DocType::Spec, DocStatus::InReview, Reject).is_err());
        assert!(decide(DocType::Spec, DocStatus::Draft, Approve).is_err());
    }

    #[test]
    fn content_change_voids_approval_only() {
        assert_eq!(on_content_changed(DocStatus::Approved), DocStatus::InReview);
        assert_eq!(on_content_changed(DocStatus::Draft), DocStatus::Draft);
        assert_eq!(on_content_changed(DocStatus::Merged), DocStatus::Merged);
    }

    #[test]
    fn cascade_only_hits_reviewed_downstream() {
        let docs = m(&[
            (DocType::Intent, DocStatus::Approved),
            (DocType::Spec, DocStatus::Approved),
            (DocType::Plan, DocStatus::Draft),
        ]);
        assert_eq!(cascade_targets(DocType::Intent, &docs), vec![DocType::Spec]);
        let docs = m(&[
            (DocType::Intent, DocStatus::Approved),
            (DocType::Spec, DocStatus::Approved),
            (DocType::Plan, DocStatus::InReview),
        ]);
        assert_eq!(
            cascade_targets(DocType::Intent, &docs),
            vec![DocType::Spec, DocType::Plan]
        );
        assert!(cascade_targets(DocType::Plan, &docs).is_empty());
    }

    #[test]
    fn derive_full_track() {
        use DocStatus::*;
        use DocType::*;
        let t = Track::Full;
        assert_eq!(
            derive_flow_state(t, &m(&[(Intent, Draft)])),
            FlowState::Intent
        );
        assert_eq!(
            derive_flow_state(t, &m(&[(Intent, InReview)])),
            FlowState::IntentReview
        );
        assert_eq!(
            derive_flow_state(t, &m(&[(Intent, Approved)])),
            FlowState::Spec
        );
        assert_eq!(
            derive_flow_state(t, &m(&[(Intent, Approved), (Spec, ChangesRequested)])),
            FlowState::Spec
        );
        assert_eq!(
            derive_flow_state(t, &m(&[(Intent, Approved), (Spec, InReview)])),
            FlowState::SpecReview
        );
        assert_eq!(
            derive_flow_state(t, &m(&[(Intent, Approved), (Spec, Approved)])),
            FlowState::Plan
        );
        assert_eq!(
            derive_flow_state(
                t,
                &m(&[(Intent, Approved), (Spec, Approved), (Plan, InReview)])
            ),
            FlowState::PlanReview
        );
        // A4: approved but not merged stays in plan review.
        assert_eq!(
            derive_flow_state(
                t,
                &m(&[(Intent, Approved), (Spec, Approved), (Plan, Approved)])
            ),
            FlowState::PlanApprovedPendingMerge
        );
        assert_eq!(
            derive_flow_state(t, &m(&[(Intent, Merged), (Spec, Merged), (Plan, Merged)])),
            FlowState::Todo
        );
        // Stale downstream returns to its review.
        assert_eq!(
            derive_flow_state(t, &m(&[(Intent, Approved), (Spec, Stale), (Plan, Stale)])),
            FlowState::SpecReview
        );
        // Upstream back in review wins.
        assert_eq!(
            derive_flow_state(t, &m(&[(Intent, InReview), (Spec, Approved)])),
            FlowState::IntentReview
        );
        assert_eq!(
            derive_flow_state(t, &m(&[(Intent, Rejected)])),
            FlowState::Rejected
        );
    }

    #[test]
    fn derive_plan_track() {
        use DocStatus::*;
        let t = Track::Plan;
        assert_eq!(derive_flow_state(t, &m(&[])), FlowState::Plan);
        assert_eq!(
            derive_flow_state(t, &m(&[(DocType::Plan, Draft)])),
            FlowState::Plan
        );
        assert_eq!(
            derive_flow_state(t, &m(&[(DocType::Plan, Approved)])),
            FlowState::PlanApprovedPendingMerge
        );
        assert_eq!(
            derive_flow_state(t, &m(&[(DocType::Plan, Merged)])),
            FlowState::Todo
        );
    }

    #[test]
    fn upstream_rule() {
        use DocStatus::*;
        use DocType::*;
        assert!(upstream_approved(Track::Full, Intent, &m(&[])));
        assert!(!upstream_approved(
            Track::Full,
            Spec,
            &m(&[(Intent, InReview)])
        ));
        assert!(upstream_approved(
            Track::Full,
            Plan,
            &m(&[(Intent, Approved), (Spec, Approved)])
        ));
        assert!(upstream_approved(Track::Plan, Plan, &m(&[])));
    }

    #[test]
    fn gate_action_parse() {
        assert_eq!(GateAction::parse("approve").unwrap(), GateAction::Approve);
        assert!(GateAction::parse("merge").is_err());
    }
}
