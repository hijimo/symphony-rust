use chrono::{Duration as ChronoDuration, Utc};
use symphony_platform::config::service_config::{HooksConfig, ServiceConfig, WorkspaceGcConfig};
use symphony_platform::config::workflow_loader::parse_workflow;
use symphony_platform::workspace::{
    TerminalMarker, WorkspaceGc, WorkspaceGcCycleOptions, WorkspaceManager,
};
use tempfile::TempDir;

#[tokio::test]
async fn workspace_gc_deletes_expired_terminal_marker_workspace() {
    let tmp = TempDir::new().unwrap();
    let manager = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());
    let lease = manager
        .prepare_issue_workspace("30", "#30", "run-1", "svc-1")
        .await
        .unwrap();
    drop(lease);

    manager
        .write_terminal_marker_at("30", "#30", "Done", Utc::now() - ChronoDuration::hours(2))
        .await
        .unwrap();

    let gc = WorkspaceGc::new(
        manager,
        WorkspaceGcConfig {
            gc_interval_ms: 0,
            gc_retention_ms: 60_000,
            gc_batch_size: 10,
            gc_cycle_timeout_ms: 120_000,
        },
    );

    let first = gc
        .run_cycle(WorkspaceGcCycleOptions::default())
        .await
        .unwrap();
    let second = gc
        .run_cycle(WorkspaceGcCycleOptions::default())
        .await
        .unwrap();

    assert_eq!(first.deleted, 1);
    assert_eq!(second.deleted, 0);
    assert!(!tmp.path().join("i-3330-_30").exists());
    assert!(!tmp
        .path()
        .join(".symphony/gc/terminal/i-3330.json")
        .exists());
}

#[tokio::test]
async fn workspace_gc_does_not_delete_unexpired_or_locked_workspaces() {
    let tmp = TempDir::new().unwrap();
    let manager = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());
    let unexpired = manager
        .prepare_issue_workspace("31", "#31", "run-1", "svc-1")
        .await
        .unwrap();
    drop(unexpired);
    let locked = manager
        .prepare_issue_workspace("32", "#32", "run-2", "svc-1")
        .await
        .unwrap();

    manager
        .write_terminal_marker_at("31", "#31", "Done", Utc::now())
        .await
        .unwrap();
    manager
        .write_terminal_marker_at("32", "#32", "Done", Utc::now() - ChronoDuration::hours(2))
        .await
        .unwrap();

    let gc = WorkspaceGc::new(
        manager,
        WorkspaceGcConfig {
            gc_interval_ms: 0,
            gc_retention_ms: 60_000,
            gc_batch_size: 10,
            gc_cycle_timeout_ms: 120_000,
        },
    );

    let summary = gc
        .run_cycle(WorkspaceGcCycleOptions::default())
        .await
        .unwrap();

    assert_eq!(summary.deleted, 0);
    assert_eq!(summary.skipped_grace, 1);
    assert_eq!(summary.skipped_locked, 1);
    assert!(tmp.path().join("i-3331-_31").exists());
    assert!(tmp.path().join("i-3332-_32").exists());
    drop(locked);
}

#[tokio::test]
async fn workspace_gc_cleans_orphan_marker_and_handles_empty_root() {
    let tmp = TempDir::new().unwrap();
    let manager = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());
    manager
        .write_terminal_marker_at("33", "#33", "Done", Utc::now() - ChronoDuration::hours(2))
        .await
        .unwrap();

    let gc = WorkspaceGc::new(
        manager,
        WorkspaceGcConfig {
            gc_interval_ms: 0,
            gc_retention_ms: 60_000,
            gc_batch_size: 10,
            gc_cycle_timeout_ms: 120_000,
        },
    );

    let summary = gc
        .run_cycle(WorkspaceGcCycleOptions::default())
        .await
        .unwrap();

    assert_eq!(summary.deleted, 0);
    assert_eq!(summary.orphan_markers_cleaned, 1);
}

#[tokio::test]
async fn workspace_gc_preview_does_not_delete() {
    let tmp = TempDir::new().unwrap();
    let manager = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());
    let lease = manager
        .prepare_issue_workspace("34", "#34", "run-1", "svc-1")
        .await
        .unwrap();
    let workspace_path = lease.workspace.path.clone();
    drop(lease);

    manager
        .write_terminal_marker_at("34", "#34", "Done", Utc::now() - ChronoDuration::hours(2))
        .await
        .unwrap();

    let gc = WorkspaceGc::new(
        manager,
        WorkspaceGcConfig {
            gc_interval_ms: 0,
            gc_retention_ms: 60_000,
            gc_batch_size: 10,
            gc_cycle_timeout_ms: 120_000,
        },
    );

    let summary = gc
        .run_cycle(WorkspaceGcCycleOptions { preview: true })
        .await
        .unwrap();

    assert_eq!(summary.preview_deleted, 1);
    assert_eq!(summary.deleted, 0);
    assert!(workspace_path.exists());
}

#[tokio::test]
async fn workspace_gc_skips_poison_marker_until_reset_window() {
    let tmp = TempDir::new().unwrap();
    let manager = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());
    let lease = manager
        .prepare_issue_workspace("35", "#35", "run-1", "svc-1")
        .await
        .unwrap();
    drop(lease);
    let marker = TerminalMarker {
        issue_id: "35".to_string(),
        issue_id_path_key: WorkspaceManager::issue_id_path_key("35"),
        workspace_key: "i-3335-_35".to_string(),
        terminal_since: Utc::now() - ChronoDuration::hours(2),
        state: "Done".to_string(),
        gc_attempts: 3,
        last_attempt_at: Some(Utc::now() - ChronoDuration::hours(1)),
    };
    manager.write_terminal_marker(marker).await.unwrap();

    let gc = WorkspaceGc::new(
        manager,
        WorkspaceGcConfig {
            gc_interval_ms: 0,
            gc_retention_ms: 60_000,
            gc_batch_size: 10,
            gc_cycle_timeout_ms: 120_000,
        },
    );

    let summary = gc
        .run_cycle(WorkspaceGcCycleOptions::default())
        .await
        .unwrap();

    assert_eq!(summary.deleted, 0);
    assert_eq!(summary.skipped_poison, 1);
}

#[tokio::test]
async fn workspace_gc_marks_stale_workspace_only_without_sidecar() {
    let tmp = TempDir::new().unwrap();
    let manager = WorkspaceManager::new(tmp.path().to_path_buf(), HooksConfig::default());
    let lease = manager
        .prepare_issue_workspace("36", "#36", "run-1", "svc-1")
        .await
        .unwrap();
    drop(lease);

    manager.delete_lock_sidecar("36").await.unwrap();
    manager
        .rewrite_workspace_last_used_at("36", "#36", Utc::now() - ChronoDuration::days(8))
        .await
        .unwrap();

    let gc = WorkspaceGc::new(
        manager,
        WorkspaceGcConfig {
            gc_interval_ms: 0,
            gc_retention_ms: 60_000,
            gc_batch_size: 10,
            gc_cycle_timeout_ms: 120_000,
        },
    );

    let summary = gc
        .run_cycle(WorkspaceGcCycleOptions::default())
        .await
        .unwrap();

    assert_eq!(summary.stale_marked, 1);
}

#[tokio::test]
async fn run_hook_drains_large_stderr_without_timing_out() {
    let tmp = TempDir::new().unwrap();
    let manager = WorkspaceManager::new(
        tmp.path().to_path_buf(),
        HooksConfig {
            timeout_ms: 1_000,
            ..HooksConfig::default()
        },
    );

    manager
        .run_hook(
            "large_stderr",
            "for i in $(seq 1 20000); do echo stderr-line-$i >&2; done",
            tmp.path(),
        )
        .await
        .unwrap();
}

#[test]
fn workspace_gc_config_defaults_and_workflow_values() {
    let defaults = WorkspaceGcConfig::default();
    assert_eq!(defaults.gc_interval_ms, 300_000);
    assert_eq!(defaults.gc_retention_ms, 3_600_000);
    assert_eq!(defaults.gc_batch_size, 10);
    assert_eq!(defaults.gc_cycle_timeout_ms, 120_000);

    let workflow = parse_workflow(
        r#"---
tracker:
  kind: github
  api_key: token
  project_slug: owner/repo
workspace:
  root: ./workspaces
  gc_interval_ms: 1000
  gc_retention_ms: 2000
  gc_batch_size: 3
  gc_cycle_timeout_ms: 4000
---
Prompt.
"#,
    )
    .unwrap();

    let config = ServiceConfig::from_workflow(&workflow, std::path::Path::new("/tmp")).unwrap();
    assert_eq!(config.workspace_gc.gc_interval_ms, 1_000);
    assert_eq!(config.workspace_gc.gc_retention_ms, 2_000);
    assert_eq!(config.workspace_gc.gc_batch_size, 3);
    assert_eq!(config.workspace_gc.gc_cycle_timeout_ms, 4_000);
}
