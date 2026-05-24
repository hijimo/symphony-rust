//! Contract tests for the resume/recovery design.
//!
//! These tests intentionally encode the target behavior from
//! docs/方案/tmp/resume-recovery-design.md before the implementation exists.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use symphony_platform::agent::{AgentIssue, AgentRunner, IssueStateRefresher};
use symphony_platform::config::service_config::{CodexConfig, HooksConfig, ServiceConfig};
use symphony_platform::config::{ConfigHolder, EffectiveConfig};
use symphony_platform::models::RetryKind;
use symphony_platform::models::{Issue, OrchestratorEvent};
use symphony_platform::orchestrator::retry::{schedule_retry, RetrySchedule};
use symphony_platform::orchestrator::scheduler::DispatchConfig;
use symphony_platform::orchestrator::Orchestrator;
use symphony_platform::prompt::PromptEngine;
use symphony_platform::tracker::{Tracker, TrackerError, TrackerIssue};
use symphony_platform::workspace::WorkspaceManager;

fn make_issue(id: &str, identifier: &str, state: &str) -> Issue {
    Issue {
        id: id.to_string(),
        identifier: identifier.to_string(),
        title: format!("Issue {identifier}"),
        description: None,
        priority: Some(1),
        state: state.to_string(),
        branch_name: None,
        url: None,
        labels: vec![],
        blocked_by: vec![],
        created_at: None,
        updated_at: None,
    }
}

fn tracker_issue_from(issue: &Issue) -> TrackerIssue {
    TrackerIssue {
        id: issue.id.clone(),
        identifier: issue.identifier.clone(),
        title: issue.title.clone(),
        description: issue.description.clone(),
        priority: issue.priority,
        state: issue.state.clone(),
        branch_name: issue.branch_name.clone(),
        url: issue.url.clone(),
        labels: issue.labels.clone(),
        blocked_by: vec![],
        created_at: issue.created_at,
        updated_at: issue.updated_at,
    }
}

struct StaticRefreshTracker {
    refreshed: Vec<TrackerIssue>,
}

#[async_trait]
impl Tracker for StaticRefreshTracker {
    async fn fetch_candidate_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        Ok(vec![])
    }

    async fn fetch_issues_by_states(
        &self,
        _states: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        Ok(vec![])
    }

    async fn fetch_issue_states_by_ids(
        &self,
        _ids: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        Ok(self.refreshed.clone())
    }
}

struct ErrorRefreshTracker;

#[async_trait]
impl Tracker for ErrorRefreshTracker {
    async fn fetch_candidate_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        Ok(vec![])
    }

    async fn fetch_issues_by_states(
        &self,
        _states: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        Ok(vec![])
    }

    async fn fetch_issue_states_by_ids(
        &self,
        _ids: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        Err(TrackerError::UnknownPayload {
            detail: "temporary tracker outage".to_string(),
        })
    }
}

struct StaticIssueRefresher;

#[async_trait]
impl IssueStateRefresher for StaticIssueRefresher {
    async fn refresh_issue_state(&self, _issue_id: &str) -> Result<Option<AgentIssue>, String> {
        Ok(None)
    }

    fn is_terminal_state(&self, _state: &str) -> bool {
        false
    }

    fn is_active_state(&self, _state: &str) -> bool {
        true
    }
}

#[tokio::test]
async fn normal_exit_revalidates_tracker_state_before_continuation_retry() {
    let cancel = CancellationToken::new();
    let mut orchestrator = Orchestrator::new(
        DispatchConfig {
            poll_interval_ms: 60_000,
            ..DispatchConfig::default()
        },
        300_000,
        1_000,
        cancel.clone(),
    );

    let running_issue = make_issue("6", "#6", "Todo");
    let refreshed_issue = make_issue("6", "#6", "Human Review");
    orchestrator.set_tracker(Arc::new(StaticRefreshTracker {
        refreshed: vec![tracker_issue_from(&refreshed_issue)],
    }));
    orchestrator.register_running(
        running_issue,
        tokio::spawn(async {}),
        cancel.child_token(),
        None,
    );

    let tx = orchestrator.event_sender();
    let run_handle = tokio::spawn(async move {
        orchestrator.run().await;
    });

    tx.send(OrchestratorEvent::WorkerExitNormal {
        issue_id: "6".to_string(),
    })
    .await
    .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (reply, response) = oneshot::channel();
    tx.send(OrchestratorEvent::QueryState { reply })
        .await
        .unwrap();
    let state = response.await.unwrap();

    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), run_handle).await;

    assert_eq!(
        state.counts.retrying, 0,
        "normal exit must not schedule continuation retry after fresh tracker state is non-active"
    );
}

#[tokio::test]
async fn normal_exit_revalidation_uses_normalized_active_state_matching() {
    let cancel = CancellationToken::new();
    let mut orchestrator = Orchestrator::new(
        DispatchConfig {
            poll_interval_ms: 60_000,
            active_states: vec!["Todo".to_string(), "In Progress".to_string()],
            ..DispatchConfig::default()
        },
        300_000,
        1_000,
        cancel.clone(),
    );

    let running_issue = make_issue("6", "#6", "Todo");
    let refreshed_issue = make_issue("6", "#6", "todo");
    orchestrator.set_tracker(Arc::new(StaticRefreshTracker {
        refreshed: vec![tracker_issue_from(&refreshed_issue)],
    }));
    orchestrator.register_running(
        running_issue,
        tokio::spawn(async {}),
        cancel.child_token(),
        None,
    );

    let tx = orchestrator.event_sender();
    let run_handle = tokio::spawn(async move {
        orchestrator.run().await;
    });

    tx.send(OrchestratorEvent::WorkerExitNormal {
        issue_id: "6".to_string(),
    })
    .await
    .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (reply, response) = oneshot::channel();
    tx.send(OrchestratorEvent::QueryState { reply })
        .await
        .unwrap();
    let state = response.await.unwrap();

    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), run_handle).await;

    assert_eq!(
        state.counts.retrying, 1,
        "lowercase fresh active state must still schedule continuation retry"
    );
}

#[tokio::test]
async fn retry_fired_releases_claim_when_fresh_state_is_non_active() {
    let cancel = CancellationToken::new();
    let mut orchestrator = Orchestrator::new(
        DispatchConfig {
            poll_interval_ms: 60_000,
            ..DispatchConfig::default()
        },
        300_000,
        1_000,
        cancel.clone(),
    );

    let refreshed_issue = make_issue("6", "#6", "Human Review");
    orchestrator.set_tracker(Arc::new(StaticRefreshTracker {
        refreshed: vec![tracker_issue_from(&refreshed_issue)],
    }));
    let event_sender = orchestrator.event_sender();
    schedule_retry(
        &mut orchestrator.state,
        RetrySchedule::new(
            "6",
            "#6",
            2,
            RetryKind::Failure,
            60_000,
            Some("previous failure".to_string()),
        ),
        &event_sender,
    );

    let tx = orchestrator.event_sender();
    let run_handle = tokio::spawn(async move {
        orchestrator.run().await;
    });
    tx.send(OrchestratorEvent::RetryFired {
        issue_id: "6".to_string(),
    })
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (reply, response) = oneshot::channel();
    tx.send(OrchestratorEvent::QueryState { reply })
        .await
        .unwrap();
    let state = response.await.unwrap();

    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), run_handle).await;

    assert_eq!(state.counts.retrying, 0);
    assert_eq!(state.counts.running, 0);
}

#[tokio::test]
async fn retry_fired_revalidation_error_preserves_original_attempt() {
    let cancel = CancellationToken::new();
    let mut orchestrator = Orchestrator::new(
        DispatchConfig {
            poll_interval_ms: 60_000,
            ..DispatchConfig::default()
        },
        300_000,
        1_000,
        cancel.clone(),
    );
    orchestrator.set_tracker(Arc::new(ErrorRefreshTracker));
    let event_sender = orchestrator.event_sender();
    schedule_retry(
        &mut orchestrator.state,
        RetrySchedule::new(
            "6",
            "#6",
            2,
            RetryKind::Failure,
            60_000,
            Some("previous failure".to_string()),
        ),
        &event_sender,
    );

    let tx = orchestrator.event_sender();
    let run_handle = tokio::spawn(async move {
        orchestrator.run().await;
    });
    tx.send(OrchestratorEvent::RetryFired {
        issue_id: "6".to_string(),
    })
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (reply, response) = oneshot::channel();
    tx.send(OrchestratorEvent::QueryState { reply })
        .await
        .unwrap();
    let state = response.await.unwrap();

    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), run_handle).await;

    assert_eq!(state.counts.retrying, 1);
    assert_eq!(
        state.retrying[0].attempt, 2,
        "tracker revalidation delay must not increment prompt retry attempt"
    );
}

#[tokio::test]
async fn retry_fired_revalidation_uses_normalized_active_state_matching() {
    let cancel = CancellationToken::new();
    let mut orchestrator = Orchestrator::new(
        DispatchConfig {
            poll_interval_ms: 60_000,
            max_concurrent_agents: 0,
            active_states: vec!["Todo".to_string(), "In Progress".to_string()],
            ..DispatchConfig::default()
        },
        300_000,
        1_000,
        cancel.clone(),
    );

    let refreshed_issue = make_issue("6", "#6", "todo");
    orchestrator.set_tracker(Arc::new(StaticRefreshTracker {
        refreshed: vec![tracker_issue_from(&refreshed_issue)],
    }));
    let event_sender = orchestrator.event_sender();
    schedule_retry(
        &mut orchestrator.state,
        RetrySchedule::new(
            "6",
            "#6",
            2,
            RetryKind::Failure,
            60_000,
            Some("previous failure".to_string()),
        ),
        &event_sender,
    );

    let tx = orchestrator.event_sender();
    let run_handle = tokio::spawn(async move {
        orchestrator.run().await;
    });
    tx.send(OrchestratorEvent::RetryFired {
        issue_id: "6".to_string(),
    })
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (reply, response) = oneshot::channel();
    tx.send(OrchestratorEvent::QueryState { reply })
        .await
        .unwrap();
    let state = response.await.unwrap();

    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), run_handle).await;

    assert_eq!(
        state.counts.retrying, 1,
        "lowercase fresh active state must preserve retry instead of releasing the claim"
    );
    assert_eq!(state.retrying[0].attempt, 2);
}

#[tokio::test]
async fn agent_runner_uses_issue_id_keyed_workspace_contract() {
    let dir = TempDir::new().unwrap();
    let workspace_mgr = Arc::new(WorkspaceManager::new(
        dir.path().to_path_buf(),
        HooksConfig::default(),
    ));
    let service = ServiceConfig {
        workspace_root: dir.path().to_path_buf(),
        max_turns: 1,
        codex: CodexConfig {
            command: "exit 42".to_string(),
            read_timeout_ms: 50,
            turn_timeout_ms: 200,
            ..CodexConfig::default()
        },
        ..ServiceConfig::default()
    };
    let config_holder = Arc::new(ConfigHolder::new(
        EffectiveConfig {
            service,
            prompt_template: "{{ issue.title }}".to_string(),
            loaded_at: Utc::now(),
        },
        dir.path().join("WORKFLOW.md"),
    ));
    let prompt_engine = Arc::new(PromptEngine::compile("{{ issue.title }}").unwrap());
    let (event_tx, _event_rx) = tokio::sync::mpsc::channel(8);
    let runner = AgentRunner::new(
        workspace_mgr,
        config_holder,
        prompt_engine,
        event_tx,
        CancellationToken::new(),
    );

    let issue = AgentIssue {
        id: "6".to_string(),
        identifier: "#6".to_string(),
        title: "Issue #6".to_string(),
        description: None,
        priority: None,
        state: "Todo".to_string(),
        labels: vec![],
        url: None,
        branch_name: None,
        blocked_by: vec![],
        created_at: None,
        updated_at: None,
    };

    let _ = runner
        .run_attempt(issue, None, Arc::new(StaticIssueRefresher))
        .await;

    assert!(
        dir.path().join("i-36-_6/.symphony-workspace.json").exists(),
        "runner must use issue-id keyed workspace metadata on its real run path"
    );
    assert!(
        dir.path().join(".symphony/locks/issues/i-36.lock").exists(),
        "runner must create the canonical issue lock on its real run path"
    );
}

#[tokio::test]
async fn workspace_key_uses_issue_id_path_key_not_identifier_only() {
    let tmp = TempDir::new().unwrap();
    let manager = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

    let lease = manager
        .prepare_issue_workspace("6", "#6", "run-1", "svc-1")
        .await
        .unwrap();

    assert_eq!(
        lease.workspace.workspace_key, "i-36-_6",
        "issue workspace key must include lowercase-hex issue_id_path_key plus sanitized identifier"
    );
}

#[tokio::test]
async fn issue_id_keyed_workspace_cleanup_removes_canonical_workspace() {
    let tmp = TempDir::new().unwrap();
    let manager = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());

    let lease = manager
        .prepare_issue_workspace("6", "#6", "run-1", "svc-1")
        .await
        .unwrap();
    let workspace_path = lease.workspace.path.clone();
    let lock_path = lease.lock_path.clone();
    drop(lease);

    manager.remove_issue_workspace("6", "#6").await;

    assert!(
        !workspace_path.exists(),
        "terminal cleanup must remove issue-id keyed workspace directories"
    );
    assert!(
        lock_path.exists(),
        "canonical lock file must survive workspace cleanup"
    );
}

#[tokio::test]
async fn nonempty_workspace_without_ready_metadata_is_not_reused() {
    let tmp = TempDir::new().unwrap();
    let legacy_key = "i-36-_6";
    let workspace_path = tmp.path().join(legacy_key);
    tokio::fs::create_dir_all(&workspace_path).await.unwrap();
    tokio::fs::write(
        workspace_path.join("partial.txt"),
        "left by interrupted init",
    )
    .await
    .unwrap();

    let manager = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());
    let result = manager
        .prepare_issue_workspace("6", "#6", "run-1", "svc-1")
        .await;

    assert!(
        result.is_err(),
        "non-empty workspace without ready metadata must enter manual diagnostic, not Codex"
    );
}

#[tokio::test]
async fn after_create_failure_quarantines_partial_workspace_before_retry() {
    let tmp = TempDir::new().unwrap();
    let manager = WorkspaceManager::new(
        tmp.path().to_path_buf(),
        HooksConfig {
            after_create: Some("echo partial > partial.txt; exit 42".to_string()),
            timeout_ms: 5_000,
            ..HooksConfig::default()
        },
    );

    let result = manager
        .prepare_issue_workspace("6", "#6", "run-1", "svc-1")
        .await;
    assert!(result.is_err());

    let quarantine_root = tmp.path().join(".symphony").join("quarantine").join("i-36");
    assert!(
        quarantine_root.exists(),
        "failed after_create with partial files must be quarantined before retry"
    );
}

#[tokio::test]
async fn concurrent_issue_workspace_prepare_is_serialized_by_lock_guard() {
    let dir = TempDir::new().unwrap();
    let mgr = Arc::new(WorkspaceManager::new(
        dir.path().to_path_buf(),
        HooksConfig {
            after_create: Some("sleep 0.25; echo ready > marker.txt".to_string()),
            ..HooksConfig::default()
        },
    ));

    let first_mgr = mgr.clone();
    let first = tokio::spawn(async move {
        first_mgr
            .prepare_issue_workspace("6", "#6", "run-1", "svc-1")
            .await
            .unwrap()
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let second_mgr = mgr.clone();
    let second = tokio::spawn(async move {
        second_mgr
            .prepare_issue_workspace("6", "#6", "run-2", "svc-2")
            .await
            .unwrap()
    });

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(80), second)
            .await
            .is_err(),
        "second prepare must wait while first issue lock guard is alive"
    );

    let first_lease = first.await.unwrap();
    drop(first_lease);

    let second_lease = mgr
        .prepare_issue_workspace("6", "#6", "run-2", "svc-2")
        .await
        .unwrap();
    assert_eq!(second_lease.workspace.workspace_key, "i-36-_6");
}
