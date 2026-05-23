pub mod cleanup;
pub mod pid_verify;
pub mod spawn;
pub mod watcher;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::models::ServiceStatus;

/// State of a running Symphony child process.
#[derive(Debug, Clone)]
pub struct ProcessState {
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    pub status: ServiceStatus,
    pub restart_count: u32,
}

/// Manages Symphony child processes for all projects.
///
/// Uses a DashMap for lock-free reads of process state and per-project
/// mutexes to serialize start/stop operations on the same project.
#[derive(Clone)]
pub struct ProcessManager {
    /// project_id -> current process state
    pub processes: Arc<DashMap<i64, ProcessState>>,
    /// per-project mutex to serialize lifecycle operations
    pub locks: Arc<DashMap<i64, Arc<Mutex<()>>>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(DashMap::new()),
            locks: Arc::new(DashMap::new()),
        }
    }

    /// Get or create the per-project mutex.
    pub fn get_lock(&self, project_id: i64) -> Arc<Mutex<()>> {
        self.locks
            .entry(project_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Try to acquire the per-project lock with a timeout.
    /// Returns None if the lock could not be acquired within the timeout.
    pub async fn try_lock(
        &self,
        project_id: i64,
        timeout: std::time::Duration,
    ) -> Option<tokio::sync::OwnedMutexGuard<()>> {
        let lock = self.get_lock(project_id);
        match tokio::time::timeout(timeout, lock.lock_owned()).await {
            Ok(guard) => Some(guard),
            Err(_) => None,
        }
    }

    /// Get the current process state for a project.
    pub fn get_state(&self, project_id: i64) -> Option<ProcessState> {
        self.processes.get(&project_id).map(|r| r.clone())
    }

    /// Set the process state for a project.
    pub fn set_state(&self, project_id: i64, state: ProcessState) {
        self.processes.insert(project_id, state);
    }

    /// Remove the process state for a project.
    pub fn remove_state(&self, project_id: i64) {
        self.processes.remove(&project_id);
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}
