use tracing::{info, warn};

use crate::models::{ServiceStatus, ServiceStatusUpdate};
use crate::process_manager::pid_verify;
use crate::repository::{ProjectRepository, SqliteRepository};

/// Perform startup cleanup: check all projects marked as "running" in the DB
/// and verify their PIDs. If a PID is invalid (process no longer exists or
/// doesn't match), mark the project as stopped.
///
/// This handles the case where web-platform was restarted but child processes
/// from a previous run are either dead or orphaned.
pub async fn startup_cleanup(repo: &SqliteRepository) {
    info!("Running startup cleanup for orphan processes...");

    // Find all projects that are marked as running/starting
    let running_projects = match repo
        .list_projects_for_user(0, true, 1, 1000, None, Some("running"), None)
        .await
    {
        Ok((projects, _)) => projects,
        Err(e) => {
            warn!("Failed to query running projects during cleanup: {}", e);
            return;
        }
    };

    let starting_projects = match repo
        .list_projects_for_user(0, true, 1, 1000, None, Some("starting"), None)
        .await
    {
        Ok((projects, _)) => projects,
        Err(e) => {
            warn!("Failed to query starting projects during cleanup: {}", e);
            Vec::new()
        }
    };

    let all_stale: Vec<_> = running_projects
        .into_iter()
        .chain(starting_projects)
        .collect();

    if all_stale.is_empty() {
        info!("No stale processes found during startup cleanup");
        return;
    }

    info!(
        count = all_stale.len(),
        "Found projects with stale running/starting status"
    );

    for project in all_stale {
        let pid = match project.service_pid {
            Some(pid) if pid > 0 => pid as u32,
            _ => {
                // No PID recorded but status is running - mark as stopped
                warn!(
                    project_id = project.id,
                    "Project marked as running but has no PID, marking as stopped"
                );
                let status = ServiceStatusUpdate {
                    status: ServiceStatus::Stopped,
                    pid: None,
                    error_message: None,
                };
                let _ = repo.update_service_status(project.id, &status).await;
                continue;
            }
        };

        let start_time = project
            .last_started_at
            .map(|dt| dt.and_utc().timestamp())
            .unwrap_or(0);

        if pid_verify::verify_pid(pid, start_time) {
            // Process is still alive and valid - send SIGTERM to clean up
            // orphan from previous web-platform instance
            info!(
                project_id = project.id,
                pid, "Found orphan process, sending SIGTERM"
            );
            pid_verify::send_sigterm(pid);

            // Give it a moment to exit, then mark as stopped
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            if !pid_verify::verify_pid(pid, start_time) {
                info!(project_id = project.id, "Orphan process terminated");
            } else {
                warn!(
                    project_id = project.id,
                    pid, "Orphan process did not exit after SIGTERM, sending SIGKILL"
                );
                pid_verify::send_sigkill(pid);
            }
        }

        // Mark as stopped regardless
        let status = ServiceStatusUpdate {
            status: ServiceStatus::Stopped,
            pid: None,
            error_message: None,
        };
        let _ = repo.update_service_status(project.id, &status).await;
    }

    info!("Startup cleanup complete");
}
