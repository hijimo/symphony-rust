//! Orphan related-MR scanner (multi-repo design §7.2).
//!
//! If a human merges the primary MR (which `Closes #iid`) before the
//! secondary MRs, the issue closes and Symphony stops tracking the rest.
//! This task scans issues closed since a persisted watermark and raises an
//! `orphan_related_mr` alert for every still-open MR carrying the
//! `Symphony-Issue:` trailer.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::models::{InsertAlertHistory, PlatformIssue, PlatformMergeRequest};
use crate::services::multi_repo_mrs::orphan_mrs;

pub const ALERT_RULE_ID: &str = "orphan_related_mr";
pub const SCAN_INTERVAL: Duration = Duration::from_secs(600);

pub fn watermark_key(project_id: i64) -> String {
    format!("orphan_mr_scanner.watermark.{project_id}")
}

/// Remote calls the scanner needs (faked in tests).
#[async_trait]
pub trait OrphanScanSource: Send + Sync {
    /// Recently closed issues of the primary project.
    async fn closed_issues(&self, project_id: i64) -> Result<Vec<PlatformIssue>, String>;
    /// Related MRs of one issue (any project).
    async fn related_mrs(
        &self,
        project_id: i64,
        iid: u64,
    ) -> Result<Vec<PlatformMergeRequest>, String>;
}

/// Outcome of scanning one project.
#[derive(Debug, Default, PartialEq)]
pub struct ScanResult {
    pub alerts: Vec<InsertAlertHistory>,
    /// New watermark (latest `closed_at` seen), `None` = unchanged.
    pub watermark: Option<String>,
}

/// Scan one project. `watermark` is the RFC3339 `closed_at` already processed.
pub async fn scan_project(
    source: &dyn OrphanScanSource,
    project_id: i64,
    project_name: &str,
    primary_slug: &str,
    watermark: Option<&str>,
    now: DateTime<Utc>,
) -> Result<ScanResult, String> {
    let since = watermark.and_then(|w| DateTime::parse_from_rfc3339(w).ok());
    let mut issues = source.closed_issues(project_id).await?;
    issues.retain(|i| match (&i.closed_at, since) {
        (Some(c), Some(since)) => DateTime::parse_from_rfc3339(c)
            .map(|c| c > since)
            .unwrap_or(false),
        (Some(_), None) => true,
        (None, _) => false,
    });
    issues.sort_by(|a, b| a.closed_at.cmp(&b.closed_at));

    let mut result = ScanResult::default();
    for issue in &issues {
        let mrs = source.related_mrs(project_id, issue.iid).await?;
        for mr in orphan_mrs(&mrs, primary_slug, issue.iid) {
            let repo = mr.target_project_path.clone().unwrap_or_default();
            result.alerts.push(InsertAlertHistory {
                rule_id: ALERT_RULE_ID.to_string(),
                severity: "warning".to_string(),
                project_id: Some(project_id),
                title: format!(
                    "{project_name}: issue #{} closed with open related MR",
                    issue.iid
                ),
                message: format!(
                    "Issue {primary_slug}#{} was closed but {repo}!{} ({}) is still open. \
                     Merge or close it manually.",
                    issue.iid, mr.iid, mr.web_url
                ),
                context_json: Some(
                    serde_json::json!({
                        "issue_iid": issue.iid,
                        "mr_iid": mr.iid,
                        "mr_project": repo,
                        "mr_url": mr.web_url,
                    })
                    .to_string(),
                ),
                fired_at: now.to_rfc3339(),
            });
        }
        result.watermark = issue.closed_at.clone();
    }
    Ok(result)
}

/// Background loop wired from `main.rs`.
pub async fn run(state: crate::AppState) {
    use crate::repository::{
        AlertRepository, ProjectRepoRepository, ProjectRepository, SystemConfigRepository,
    };
    let mut ticker = tokio::time::interval(SCAN_INTERVAL);
    loop {
        ticker.tick().await;
        let Ok(project_ids) = state.repo.list_projects_with_repos().await else {
            continue;
        };
        let configs = state.repo.list_system_configs().await.unwrap_or_default();
        for project_id in project_ids {
            let Ok(Some(project)) = state.repo.get_project(project_id).await else {
                continue;
            };
            let source = live::LiveSource {
                state: state.clone(),
                project: project.clone(),
            };
            let key = watermark_key(project_id);
            let watermark = configs
                .iter()
                .find(|c| c.key == key)
                .map(|c| c.value.clone());
            let slug = format!("{}/{}", project.namespace, project.repo_name);
            match scan_project(
                &source,
                project_id,
                &project.name,
                &slug,
                watermark.as_deref(),
                Utc::now(),
            )
            .await
            {
                Ok(result) => {
                    for alert in &result.alerts {
                        if let Err(e) = state.repo.insert_alert_history(alert).await {
                            tracing::warn!(project_id, error = %e, "failed to persist orphan MR alert");
                        }
                        if let Some(manager) = &state.alert_manager {
                            let event = crate::notification::AlertEvent {
                                id: uuid::Uuid::new_v4().to_string(),
                                rule_id: alert.rule_id.clone(),
                                severity: crate::notification::Severity::Warning,
                                project_id: Some(project_id),
                                project_name: Some(project.name.clone()),
                                title: alert.title.clone(),
                                message: alert.message.clone(),
                                context: Default::default(),
                                fired_at: Utc::now(),
                            };
                            let _ = manager.dispatcher.dispatch(&event).await;
                        }
                    }
                    if let Some(w) = result.watermark {
                        if let Err(e) = state.repo.upsert_system_config(&key, &w).await {
                            tracing::warn!(project_id, error = %e, "failed to persist orphan MR watermark");
                        }
                    }
                }
                Err(e) => tracing::warn!(project_id, error = %e, "orphan MR scan failed"),
            }
        }
    }
}

mod live {
    use super::*;
    use crate::handlers::network_proxy::load_effective_proxy_config;
    use crate::models::Project;
    use crate::services::git_platform::{create_platform_client_with_proxy, ListIssuesOptions};

    pub struct LiveSource {
        pub state: crate::AppState,
        pub project: Project,
    }

    impl LiveSource {
        async fn token(&self) -> Result<String, String> {
            use crate::repository::UserConfigRepository;
            let owner = self.project.created_by.ok_or("project has no owner")?;
            let cfg = self
                .state
                .repo
                .get_config(owner)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("owner has no token")?;
            let enc = cfg.gitlab_token.ok_or("owner has no gitlab token")?;
            crate::crypto::decrypt(&enc, &self.state.encryption_key).map_err(|e| e.to_string())
        }

        async fn client(
            &self,
        ) -> Result<Box<dyn crate::services::git_platform::GitPlatformClient>, String> {
            let proxy = load_effective_proxy_config(&self.state.repo, &self.state.encryption_key)
                .await
                .map_err(|e| e.to_string())?;
            create_platform_client_with_proxy(
                &self.project.platform,
                self.project.platform_host.as_deref(),
                Some(&proxy),
            )
            .map_err(|e| e.to_string())
        }
    }

    #[async_trait]
    impl OrphanScanSource for LiveSource {
        async fn closed_issues(&self, _project_id: i64) -> Result<Vec<PlatformIssue>, String> {
            let token = self.token().await?;
            let slug = format!("{}/{}", self.project.namespace, self.project.repo_name);
            let opts = ListIssuesOptions {
                state: Some("closed".to_string()),
                limit: 100,
                ..Default::default()
            };
            self.client()
                .await?
                .list_issues(&token, &slug, &opts)
                .await
                .map(|(i, _)| i)
                .map_err(|e| e.to_string())
        }

        async fn related_mrs(
            &self,
            _project_id: i64,
            iid: u64,
        ) -> Result<Vec<PlatformMergeRequest>, String> {
            let token = self.token().await?;
            let slug = format!("{}/{}", self.project.namespace, self.project.repo_name);
            self.client()
                .await?
                .get_issue_merge_requests(&token, &slug, iid)
                .await
                .map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Fake {
        issues: Vec<PlatformIssue>,
        mrs: HashMap<u64, Vec<PlatformMergeRequest>>,
    }

    #[async_trait]
    impl OrphanScanSource for Fake {
        async fn closed_issues(&self, _: i64) -> Result<Vec<PlatformIssue>, String> {
            Ok(self.issues.clone())
        }
        async fn related_mrs(&self, _: i64, iid: u64) -> Result<Vec<PlatformMergeRequest>, String> {
            Ok(self.mrs.get(&iid).cloned().unwrap_or_default())
        }
    }

    fn issue(iid: u64, closed_at: &str) -> PlatformIssue {
        serde_json::from_value(serde_json::json!({
            "iid": iid, "title": "t", "description": null, "state": "closed",
            "labels": [], "author": {"username": "u", "display_name": null, "avatar_url": null},
            "assignees": [], "milestone": null, "created_at": "x", "updated_at": "x",
            "closed_at": closed_at, "web_url": "w", "comment_count": null
        }))
        .expect("PlatformIssue fixture matches the model")
    }

    fn mr(iid: u64, state: &str, desc: &str) -> PlatformMergeRequest {
        serde_json::from_value(serde_json::json!({
            "iid": iid, "platform_node_id": null, "title": "m", "description": desc, "state": state,
            "author": {"username": "u", "display_name": null, "avatar_url": null},
            "source_project_path": "JstAi/jxy", "target_project_path": "JstAi/jxy",
            "source_branch": "b", "target_branch": "pre", "ci_status": null, "ci_web_url": null,
            "review_status": null, "reviewers": [], "merge_status": null, "related_issue_iids": [],
            "additions": null, "deletions": null, "changed_files": null,
            "created_at": "x", "updated_at": "x", "merged_at": null, "web_url": "http://h/JstAi/jxy/-/merge_requests/123"
        }))
        .expect("PlatformMergeRequest fixture matches the model")
    }

    #[tokio::test]
    async fn alerts_on_open_trailer_mr_and_advances_watermark() {
        let trailer = "Symphony-Issue: zimei10525/jst-ai-studio#32";
        let fake = Fake {
            issues: vec![
                issue(31, "2026-10-20T00:00:00Z"),
                issue(32, "2026-10-21T00:00:00Z"),
            ],
            mrs: HashMap::from([
                (
                    32,
                    vec![mr(123, "opened", trailer), mr(124, "merged", trailer)],
                ),
                (31, vec![mr(9, "opened", "no trailer")]),
            ]),
        };
        let r = scan_project(
            &fake,
            2,
            "jst-ai-studio",
            "zimei10525/jst-ai-studio",
            None,
            Utc::now(),
        )
        .await
        .unwrap();
        assert_eq!(r.alerts.len(), 1);
        assert_eq!(r.alerts[0].rule_id, ALERT_RULE_ID);
        assert!(r.alerts[0].message.contains("JstAi/jxy!123"));
        assert_eq!(r.watermark.as_deref(), Some("2026-10-21T00:00:00Z"));

        // Already-processed issues are skipped on the next run.
        let again = scan_project(
            &fake,
            2,
            "p",
            "zimei10525/jst-ai-studio",
            Some("2026-10-21T00:00:00Z"),
            Utc::now(),
        )
        .await
        .unwrap();
        assert!(again.alerts.is_empty());
        assert_eq!(again.watermark, None);
    }
}
