//! GitLab Tracker Adapter — wraps the Platform trait to implement Tracker.
//!
//! Bridges the Platform adapter (which uses `IssueId(u64)` and `platform::Issue`)
//! to the Tracker trait (which uses `String` IDs and `TrackerIssue`).

use std::sync::Arc;

use async_trait::async_trait;

use crate::platform::{FetchOptions, IssueId, Platform};

use super::{BlockerRef, Tracker, TrackerError, TrackerIssue};

fn platform_error_to_tracker_error(e: crate::error::PlatformError) -> TrackerError {
    use crate::error::PlatformError;
    let status = match &e {
        PlatformError::HttpError(s) => *s,
        PlatformError::ServerError(s) => *s,
        PlatformError::RateLimited { .. } => 429,
        PlatformError::AuthExpired => 401,
        PlatformError::Forbidden(_) => 403,
        PlatformError::NotFound(_) => 404,
        PlatformError::Unprocessable(_) => 422,
        _ => 0,
    };
    TrackerError::ApiStatus {
        status,
        body: e.to_string(),
    }
}

fn normalize_tracker_state(state: &str) -> String {
    state.trim().to_lowercase().replace([' ', '-'], "_")
}

fn state_matches_any(state: &str, candidates: &[String]) -> bool {
    let normalized = normalize_tracker_state(state);
    candidates
        .iter()
        .any(|candidate| normalize_tracker_state(candidate) == normalized)
}

/// Wraps an existing Platform adapter to implement the Tracker trait.
pub struct GitlabTrackerAdapter {
    platform: Arc<dyn Platform>,
    active_states: Vec<String>,
    #[allow(dead_code)]
    terminal_states: Vec<String>,
}

impl GitlabTrackerAdapter {
    pub fn new(
        platform: Arc<dyn Platform>,
        active_states: Vec<String>,
        terminal_states: Vec<String>,
    ) -> Self {
        Self {
            platform,
            active_states,
            terminal_states,
        }
    }

    fn convert_issue(issue: &crate::platform::Issue) -> TrackerIssue {
        TrackerIssue {
            id: issue.id.0.to_string(),
            identifier: format!("#{}", issue.number),
            title: issue.title.clone(),
            description: issue.description.clone(),
            priority: issue.priority.map(|p| p as i32),
            state: issue.workflow_state.clone().unwrap_or_default(),
            branch_name: if issue.branch_name.is_empty() {
                None
            } else {
                Some(issue.branch_name.clone())
            },
            url: Some(issue.url.clone()),
            labels: issue.labels.iter().map(|l| l.to_lowercase()).collect(),
            blocked_by: issue
                .blocked_by
                .iter()
                .map(|id| BlockerRef {
                    id: Some(id.0.to_string()),
                    identifier: None,
                    state: None,
                })
                .collect(),
            created_at: issue.created_at,
            updated_at: issue.updated_at,
        }
    }
}

#[async_trait]
impl Tracker for GitlabTrackerAdapter {
    async fn fetch_candidate_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        let issues = self
            .platform
            .fetch_candidate_issues(FetchOptions::default())
            .await
            .map_err(platform_error_to_tracker_error)?;

        let candidates: Vec<TrackerIssue> = issues
            .iter()
            .filter(|i| {
                i.workflow_state
                    .as_ref()
                    .map(|s| state_matches_any(s, &self.active_states))
                    .unwrap_or(false)
            })
            .map(Self::convert_issue)
            .collect();

        Ok(candidates)
    }

    async fn fetch_issues_by_states(
        &self,
        states: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        let all_issues = self
            .platform
            .fetch_candidate_issues(FetchOptions::default())
            .await
            .map_err(platform_error_to_tracker_error)?;

        let filtered: Vec<TrackerIssue> = all_issues
            .iter()
            .filter(|i| {
                i.workflow_state
                    .as_ref()
                    .map(|s| state_matches_any(s, states))
                    .unwrap_or(false)
            })
            .map(Self::convert_issue)
            .collect();

        Ok(filtered)
    }

    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        let issue_ids: Vec<IssueId> = ids
            .iter()
            .filter_map(|s| s.parse::<u64>().ok().map(IssueId))
            .collect();

        if issue_ids.is_empty() {
            return Ok(Vec::new());
        }

        let issues = self
            .platform
            .fetch_issue_states_by_ids(&issue_ids)
            .await
            .map_err(platform_error_to_tracker_error)?;

        Ok(issues.iter().map(Self::convert_issue).collect())
    }

    async fn set_workflow_state(&self, issue_id: &str, state: &str) -> Result<(), TrackerError> {
        let id: u64 = issue_id.parse().map_err(|_| TrackerError::ApiStatus {
            status: 0,
            body: format!("invalid issue_id: {}", issue_id),
        })?;

        self.platform
            .set_workflow_state(IssueId(id), state)
            .await
            .map_err(platform_error_to_tracker_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_matching_treats_spaces_and_underscores_equivalently() {
        assert!(state_matches_any(
            "in_progress",
            &["In Progress".to_string()]
        ));
        assert!(state_matches_any(
            "In Progress",
            &["in_progress".to_string()]
        ));
    }
}
