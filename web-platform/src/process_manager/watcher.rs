use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::models::{Project, ServiceLifecycleUpdate, ServiceStatus, ServiceStatusUpdate};
use crate::process_manager::{pid_verify, spawn, ProcessManager, ProcessState};
use crate::repository::{ProjectRepository, SqliteRepository};

const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(10);
const MAX_RESTART_ATTEMPTS: u32 = 3;

fn next_service_identity(project: &Project) -> (i64, String) {
    let generation = project.service_generation + 1;
    (
        generation,
        format!("svc-{}-{}-{}", project.id, generation, Uuid::new_v4()),
    )
}

fn service_workdir(workspace_root: &str, project_id: i64) -> String {
    std::path::PathBuf::from(workspace_root)
        .join(project_id.to_string())
        .to_string_lossy()
        .to_string()
}

fn service_cmdline_hash(symphony_bin: &str, workdir: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(symphony_bin.as_bytes());
    hasher.update([0]);
    hasher.update(b"WORKFLOW.md");
    hasher.update([0]);
    hasher.update(workdir.as_bytes());
    hex::encode(hasher.finalize())
}

fn web_instance_id() -> String {
    std::env::var("SYMPHONY_WEB_INSTANCE_ID")
        .unwrap_or_else(|_| format!("web-{}", std::process::id()))
}

#[cfg(unix)]
fn process_group_id(pid: u32) -> Option<i64> {
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    (pgid >= 0).then_some(pgid as i64)
}

#[cfg(not(unix))]
fn process_group_id(_pid: u32) -> Option<i64> {
    None
}

#[cfg(unix)]
fn process_session_id(pid: u32) -> Option<i64> {
    let sid = unsafe { libc::getsid(pid as libc::pid_t) };
    (sid >= 0).then_some(sid as i64)
}

#[cfg(not(unix))]
fn process_session_id(_pid: u32) -> Option<i64> {
    None
}

fn build_lifecycle_update(
    project: &Project,
    symphony_bin: &str,
    workspace_root: &str,
    pid: u32,
    service_generation: i64,
    service_instance_id: String,
    proxy_config_version: String,
) -> ServiceLifecycleUpdate {
    let web_instance_id = web_instance_id();
    let workdir = service_workdir(workspace_root, project.id);
    ServiceLifecycleUpdate {
        web_instance_id: web_instance_id.clone(),
        lifecycle_op_id: Uuid::new_v4().to_string(),
        service_owner_web_instance_id: web_instance_id,
        service_generation,
        service_instance_id,
        service_pgid: process_group_id(pid),
        service_session_id: process_session_id(pid),
        service_cmdline_hash: service_cmdline_hash(symphony_bin, &workdir),
        service_workdir: workdir,
        last_lifecycle_op: "restart".to_string(),
        service_proxy_config_version: proxy_config_version,
    }
}

pub fn spawn_watcher(
    project_id: i64,
    process_manager: ProcessManager,
    repo: SqliteRepository,
    encryption_key: [u8; 32],
    symphony_bin: String,
    workspace_root: String,
) {
    tokio::spawn(async move {
        watch_process(
            project_id,
            process_manager,
            repo,
            encryption_key,
            symphony_bin,
            workspace_root,
        )
        .await;
    });
}

async fn watch_process(
    project_id: i64,
    process_manager: ProcessManager,
    repo: SqliteRepository,
    encryption_key: [u8; 32],
    symphony_bin: String,
    workspace_root: String,
) {
    loop {
        sleep(HEALTH_CHECK_INTERVAL).await;

        let state = match process_manager.get_state(project_id) {
            Some(s) => s,
            None => break,
        };

        if state.status != ServiceStatus::Running {
            break;
        }

        if pid_verify::verify_pid(state.pid, state.started_at.timestamp()) {
            continue;
        }

        let log_tail = read_log_tail(project_id, &workspace_root);
        warn!(
            project_id,
            pid = state.pid,
            log_tail = %log_tail,
            "Process died unexpectedly, attempting recovery"
        );

        let project = match repo.get_project(project_id).await {
            Ok(Some(p)) => p,
            _ => {
                error!(project_id, "Failed to load project for crash recovery");
                break;
            }
        };

        if !project.auto_restart || state.restart_count >= MAX_RESTART_ATTEMPTS {
            info!(
                project_id,
                restart_count = state.restart_count,
                "Max restart attempts reached or auto-restart disabled, marking as failed"
            );

            let status_update = ServiceStatusUpdate {
                status: ServiceStatus::Failed,
                pid: None,
                error_message: Some(format!(
                    "Process crashed after {} restart attempts. Last output:\n{}",
                    state.restart_count, log_tail
                )),
            };
            let _ = repo.update_service_status(project_id, &status_update).await;

            process_manager.set_state(
                project_id,
                ProcessState {
                    pid: 0,
                    started_at: state.started_at,
                    status: ServiceStatus::Failed,
                    restart_count: state.restart_count,
                },
            );
            break;
        }

        let delay = match state.restart_count {
            0 => Duration::from_secs(5),
            1 => Duration::from_secs(15),
            _ => Duration::from_secs(60),
        };

        info!(
            project_id,
            restart_count = state.restart_count + 1,
            delay_secs = delay.as_secs(),
            "Scheduling auto-restart"
        );

        let status_update = ServiceStatusUpdate {
            status: ServiceStatus::Error,
            pid: None,
            error_message: Some("Process crashed, auto-restarting...".to_string()),
        };
        let _ = repo.update_service_status(project_id, &status_update).await;

        process_manager.set_state(
            project_id,
            ProcessState {
                pid: 0,
                started_at: state.started_at,
                status: ServiceStatus::Error,
                restart_count: state.restart_count + 1,
            },
        );

        sleep(delay).await;

        let (service_generation, service_instance_id) = next_service_identity(&project);

        match spawn::spawn_symphony(
            &project,
            &repo,
            &encryption_key,
            &symphony_bin,
            &workspace_root,
            &service_instance_id,
        )
        .await
        {
            Ok(result) => {
                let now = chrono::Utc::now();
                let new_pid = result.pid;
                let proxy_config_version = result.proxy_config_version.clone();

                let lifecycle_update = build_lifecycle_update(
                    &project,
                    &symphony_bin,
                    &workspace_root,
                    new_pid,
                    service_generation,
                    service_instance_id,
                    proxy_config_version,
                );
                let _ = repo
                    .update_service_lifecycle(project_id, &lifecycle_update)
                    .await;

                let status_update = ServiceStatusUpdate {
                    status: ServiceStatus::Running,
                    pid: Some(new_pid as i64),
                    error_message: None,
                };
                let _ = repo.update_service_status(project_id, &status_update).await;

                process_manager.set_state(
                    project_id,
                    ProcessState {
                        pid: new_pid,
                        started_at: now,
                        status: ServiceStatus::Running,
                        restart_count: state.restart_count + 1,
                    },
                );

                info!(project_id, pid = new_pid, "Process restarted successfully");
            }
            Err(e) => {
                error!(project_id, error = %e, "Failed to restart process");

                let status_update = ServiceStatusUpdate {
                    status: ServiceStatus::Failed,
                    pid: None,
                    error_message: Some(format!("Restart failed: {}", e)),
                };
                let _ = repo.update_service_status(project_id, &status_update).await;

                process_manager.set_state(
                    project_id,
                    ProcessState {
                        pid: 0,
                        started_at: state.started_at,
                        status: ServiceStatus::Failed,
                        restart_count: state.restart_count + 1,
                    },
                );
                break;
            }
        }
    }
}

fn read_log_tail(project_id: i64, workspace_root: &str) -> String {
    let log_path = std::path::PathBuf::from(workspace_root)
        .join(project_id.to_string())
        .join("symphony.log");

    match std::fs::read_to_string(&log_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(20);
            lines[start..].join("\n")
        }
        Err(_) => "(no log file available)".to_string(),
    }
}
