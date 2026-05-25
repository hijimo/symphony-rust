use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::config::service_config::WorkspaceGcConfig;

use super::terminal_marker::{self, TerminalMarker};
use super::{read_workspace_metadata, WorkspaceError, WorkspaceManager};

const MAX_GC_ATTEMPTS: u32 = 3;
const POISON_RESET_HOURS: i64 = 24;
const STALE_FALLBACK_DAYS: i64 = 7;

#[derive(Debug, Clone, Copy, Default)]
pub struct WorkspaceGcCycleOptions {
    pub preview: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceGcCycleSummary {
    pub scanned: usize,
    pub deleted: usize,
    pub preview_deleted: usize,
    pub orphan_markers_cleaned: usize,
    pub skipped_locked: usize,
    pub skipped_grace: usize,
    pub skipped_hook_fail: usize,
    pub skipped_poison: usize,
    pub quarantine_cleaned: usize,
    pub stale_marked: usize,
    pub cycle_timeout_hit: bool,
}

pub struct WorkspaceGc {
    manager: WorkspaceManager,
    config: WorkspaceGcConfig,
}

impl WorkspaceGc {
    pub fn new(manager: WorkspaceManager, config: WorkspaceGcConfig) -> Self {
        Self { manager, config }
    }

    pub async fn run_cycle(
        &self,
        options: WorkspaceGcCycleOptions,
    ) -> Result<WorkspaceGcCycleSummary, WorkspaceError> {
        let cycle_start = Instant::now();
        let cycle_timeout = Duration::from_millis(self.config.gc_cycle_timeout_ms);
        let retention = chrono::Duration::milliseconds(self.config.gc_retention_ms as i64);
        let now = Utc::now();
        let mut summary = WorkspaceGcCycleSummary::default();
        let mut eligible = Vec::new();

        for (marker_path, mut marker) in terminal_marker::list(self.manager.root()).await? {
            summary.scanned += 1;
            let workspace_path = self.manager.workspace_path_for_key(&marker.workspace_key);
            if !workspace_path.exists() {
                if terminal_marker::remove_if_same_terminal_since(&marker_path, &marker).await? {
                    summary.orphan_markers_cleaned += 1;
                }
                continue;
            }

            if marker.gc_attempts >= MAX_GC_ATTEMPTS {
                if marker
                    .last_attempt_at
                    .is_some_and(|last| now - last > chrono::Duration::hours(POISON_RESET_HOURS))
                {
                    marker.gc_attempts = 0;
                    marker.last_attempt_at = None;
                    terminal_marker::write(self.manager.root(), &marker).await?;
                }
                summary.skipped_poison += 1;
                continue;
            }

            if marker.terminal_since + retention > now {
                summary.skipped_grace += 1;
                continue;
            }

            eligible.push(marker);
        }

        eligible.sort_by_key(|marker| marker.terminal_since);
        for marker in eligible.into_iter().take(self.config.gc_batch_size) {
            if cycle_start.elapsed() > cycle_timeout {
                summary.cycle_timeout_hit = true;
                break;
            }
            if options.preview {
                summary.preview_deleted += 1;
                continue;
            }
            if !self.delete_marked_workspace(&marker, &mut summary).await? {
                continue;
            }
        }

        self.clean_quarantine(&mut summary, now).await?;
        self.mark_stale_workspaces(&mut summary, now).await?;

        tracing::info!(
            scanned = summary.scanned,
            deleted = summary.deleted,
            preview_deleted = summary.preview_deleted,
            orphan_markers_cleaned = summary.orphan_markers_cleaned,
            skipped_locked = summary.skipped_locked,
            skipped_grace = summary.skipped_grace,
            skipped_hook_fail = summary.skipped_hook_fail,
            skipped_poison = summary.skipped_poison,
            quarantine_cleaned = summary.quarantine_cleaned,
            stale_marked = summary.stale_marked,
            cycle_timeout_hit = summary.cycle_timeout_hit,
            duration_ms = cycle_start.elapsed().as_millis() as u64,
            "workspace GC cycle complete"
        );

        Ok(summary)
    }

    async fn delete_marked_workspace(
        &self,
        marker: &TerminalMarker,
        summary: &mut WorkspaceGcCycleSummary,
    ) -> Result<bool, WorkspaceError> {
        let Some(_guard) = self
            .manager
            .try_acquire_issue_lock(&marker.issue_id_path_key)
            .await?
        else {
            summary.skipped_locked += 1;
            return Ok(false);
        };

        let workspace_path = self.manager.workspace_path_for_key(&marker.workspace_key);
        if let Some(ref script) = self.manager.hooks.before_remove {
            if let Err(e) = self
                .manager
                .run_hook("before_remove", script, &workspace_path)
                .await
            {
                let mut failed = marker.clone();
                failed.gc_attempts = failed.gc_attempts.saturating_add(1);
                failed.last_attempt_at = Some(Utc::now());
                terminal_marker::write(self.manager.root(), &failed).await?;
                summary.skipped_hook_fail += 1;
                tracing::warn!(error = %e, workspace_key = %marker.workspace_key, "workspace GC before_remove hook failed");
                return Ok(false);
            }
        }

        match tokio::fs::remove_dir_all(&workspace_path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(WorkspaceError::RemoveFailed { source: e }),
        }
        let marker_path = terminal_marker::path(self.manager.root(), &marker.issue_id_path_key);
        let _ = terminal_marker::remove_if_same_terminal_since(&marker_path, marker).await?;
        summary.deleted += 1;
        Ok(true)
    }

    async fn clean_quarantine(
        &self,
        summary: &mut WorkspaceGcCycleSummary,
        now: DateTime<Utc>,
    ) -> Result<(), WorkspaceError> {
        let quarantine_root = self.manager.root().join(".symphony").join("quarantine");
        let mut roots = match tokio::fs::read_dir(&quarantine_root).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(WorkspaceError::Io { source: e }),
        };
        let retention = chrono::Duration::milliseconds(self.config.gc_retention_ms as i64);

        while let Some(root_entry) = roots
            .next_entry()
            .await
            .map_err(|e| WorkspaceError::Io { source: e })?
        {
            let mut entries = match tokio::fs::read_dir(root_entry.path()).await {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|e| WorkspaceError::Io { source: e })?
            {
                if summary.quarantine_cleaned >= self.config.gc_batch_size {
                    return Ok(());
                }
                let marker_path = entry.path().join(".quarantined_at");
                let Ok(raw) = tokio::fs::read_to_string(&marker_path).await else {
                    continue;
                };
                let Ok(marked_at) = DateTime::parse_from_rfc3339(raw.trim()) else {
                    continue;
                };
                if marked_at.with_timezone(&Utc) + retention > now {
                    continue;
                }
                tokio::fs::remove_dir_all(entry.path())
                    .await
                    .map_err(|e| WorkspaceError::RemoveFailed { source: e })?;
                summary.quarantine_cleaned += 1;
            }
        }

        Ok(())
    }

    async fn mark_stale_workspaces(
        &self,
        summary: &mut WorkspaceGcCycleSummary,
        now: DateTime<Utc>,
    ) -> Result<(), WorkspaceError> {
        let mut entries = match tokio::fs::read_dir(self.manager.root()).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(WorkspaceError::Io { source: e }),
        };
        let stale_before = now - chrono::Duration::days(STALE_FALLBACK_DAYS);

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| WorkspaceError::Io { source: e })?
        {
            if summary.stale_marked >= self.config.gc_batch_size {
                break;
            }
            let file_name = entry.file_name();
            let workspace_key = file_name.to_string_lossy();
            if !workspace_key.starts_with("i-") || !entry.path().is_dir() {
                continue;
            }

            let metadata_path = self.manager.metadata_path_for_key(&workspace_key);
            let Ok(metadata) = read_workspace_metadata(&metadata_path).await else {
                continue;
            };
            let marker_path =
                terminal_marker::path(self.manager.root(), &metadata.issue_id_path_key);
            if marker_path.exists()
                || self
                    .manager
                    .lock_sidecar_path_for_key(&metadata.issue_id_path_key)
                    .exists()
            {
                continue;
            }
            let Some(last_used_at) = metadata.last_used_at.or(metadata.initialized_at) else {
                continue;
            };
            if last_used_at > stale_before {
                continue;
            }
            if self
                .manager
                .try_acquire_issue_lock(&metadata.issue_id_path_key)
                .await?
                .is_none()
            {
                continue;
            }

            terminal_marker::write(
                self.manager.root(),
                &TerminalMarker {
                    issue_id: metadata.issue_id,
                    issue_id_path_key: metadata.issue_id_path_key,
                    workspace_key: metadata.workspace_key,
                    terminal_since: now,
                    state: "stale-fallback".to_string(),
                    gc_attempts: 0,
                    last_attempt_at: None,
                },
            )
            .await?;
            summary.stale_marked += 1;
        }

        Ok(())
    }
}

pub async fn run_workspace_gc_task(
    config_holder: std::sync::Arc<crate::config::watcher::ConfigHolder>,
    cancel: tokio_util::sync::CancellationToken,
) {
    loop {
        let snapshot = config_holder.load();
        let config = snapshot.service.workspace_gc.clone();
        if config.gc_interval_ms == 0 {
            tracing::info!("workspace GC disabled by gc_interval_ms=0");
            return;
        }
        let gc = WorkspaceGc::new(
            WorkspaceManager::new(
                snapshot.service.workspace_root.clone(),
                snapshot.service.hooks.clone(),
            ),
            config.clone(),
        );
        if let Err(e) = gc.run_cycle(WorkspaceGcCycleOptions::default()).await {
            tracing::warn!(error = %e, "workspace GC cycle failed");
        }

        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_millis(config.gc_interval_ms)) => {}
        }
    }
}
