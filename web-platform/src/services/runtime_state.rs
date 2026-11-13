//! Runtime state of rust-platform instances (P2 design §3 F1).
//!
//! Each Symphony child process is started with `--port <p>` and serves
//! `GET /api/v1/state` on `127.0.0.1:<p>`. This module polls those endpoints,
//! caches the last snapshot per (project, instance) and reports agent counts
//! to the `ConcurrencyManager`.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::concurrency::manager::ConcurrencyManager;
use crate::process_manager::{ProcessManager, ProcessState};

/// Poll interval for all running instances.
pub const POLL_INTERVAL: Duration = Duration::from_secs(10);
/// Per-request timeout; a slow instance must not delay the others for long.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Which Symphony process of a project a snapshot belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Instance {
    Dev,
    Test,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RuntimeTokens {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// A running agent session (subset of rust-platform `RunningRow`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunningIssue {
    pub issue_id: String,
    pub issue_identifier: String,
    pub state: String,
    pub turn_count: u32,
    pub started_at: String,
    pub last_event_at: Option<String>,
    #[serde(default)]
    pub tokens: RuntimeTokens,
}

/// An issue waiting for a retry (subset of rust-platform `RetryRow`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetryingIssue {
    pub issue_id: String,
    pub issue_identifier: String,
    pub attempt: u32,
    pub due_at: String,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StateDto {
    #[serde(default)]
    running: Vec<RunningIssue>,
    #[serde(default)]
    retrying: Vec<RetryingIssue>,
}

/// Last known state of one instance. `stale` means the latest poll failed and
/// the rows come from an earlier successful poll (or are empty).
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeSnapshot {
    pub fetched_at: DateTime<Utc>,
    pub stale: bool,
    pub running: Vec<RunningIssue>,
    pub retrying: Vec<RetryingIssue>,
}

/// Pick a free port on 127.0.0.1 for a child process.
///
/// The port is released before the child binds it, so another process can
/// take it in between. The child then only logs an HTTP server error and the
/// snapshot for that instance stays stale; the orchestrator keeps working.
pub fn allocate_local_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

pub struct RuntimeStateCache {
    client: reqwest::Client,
    entries: DashMap<(i64, Instance), RuntimeSnapshot>,
}

impl Default for RuntimeStateCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeStateCache {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            // Loopback only: never route through HTTP(S)_PROXY.
            .no_proxy()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client");
        Self {
            client,
            entries: DashMap::new(),
        }
    }

    async fn fetch(&self, port: u16) -> Result<StateDto, String> {
        let url = format!("http://127.0.0.1:{port}/api/v1/state");
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("status {}", resp.status()));
        }
        resp.json::<StateDto>().await.map_err(|e| e.to_string())
    }

    /// Poll one instance. On failure the previous rows are kept and marked stale.
    pub async fn refresh(&self, project_id: i64, instance: Instance, port: u16) {
        match self.fetch(port).await {
            Ok(dto) => {
                self.entries.insert(
                    (project_id, instance),
                    RuntimeSnapshot {
                        fetched_at: Utc::now(),
                        stale: false,
                        running: dto.running,
                        retrying: dto.retrying,
                    },
                );
            }
            Err(error) => {
                tracing::debug!(project_id, ?instance, port, %error, "runtime state poll failed");
                self.entries
                    .entry((project_id, instance))
                    .and_modify(|s| s.stale = true)
                    .or_insert_with(|| RuntimeSnapshot {
                        fetched_at: Utc::now(),
                        stale: true,
                        running: Vec::new(),
                        retrying: Vec::new(),
                    });
            }
        }
    }

    pub fn get(&self, project_id: i64, instance: Instance) -> Option<RuntimeSnapshot> {
        self.entries.get(&(project_id, instance)).map(|e| e.clone())
    }

    /// Running sessions of a project across its dev and test instances.
    pub fn running_issues(&self, project_id: i64) -> Vec<(Instance, RunningIssue)> {
        [Instance::Dev, Instance::Test]
            .into_iter()
            .filter_map(|i| self.get(project_id, i).map(|s| (i, s)))
            .flat_map(|(i, s)| s.running.into_iter().map(move |r| (i, r)))
            .collect()
    }

    /// All cached snapshots (used by alerting).
    pub fn snapshots(&self) -> Vec<((i64, Instance), RuntimeSnapshot)> {
        self.entries
            .iter()
            .map(|e| (*e.key(), e.value().clone()))
            .collect()
    }

    /// Poll every active instance that has an API port, drop entries for
    /// instances that are gone, and report per-project agent counts.
    pub async fn poll_once(
        &self,
        pm: &ProcessManager,
        cm: &ConcurrencyManager,
        project_name: impl Fn(i64) -> String,
    ) {
        let targets = live_targets(pm);
        let live: HashSet<(i64, Instance)> = targets.iter().map(|(k, _)| *k).collect();
        let stale_projects: HashSet<i64> = self
            .entries
            .iter()
            .map(|e| e.key().0)
            .filter(|id| !live.iter().any(|(p, _)| p == id))
            .collect();
        self.entries.retain(|k, _| live.contains(k));
        for id in stale_projects {
            cm.remove_project(id);
        }

        futures::future::join_all(
            targets
                .iter()
                .map(|((id, inst), port)| self.refresh(*id, *inst, *port)),
        )
        .await;

        let projects: HashSet<i64> = live.iter().map(|(p, _)| *p).collect();
        for id in projects {
            let snaps: Vec<RuntimeSnapshot> = [Instance::Dev, Instance::Test]
                .into_iter()
                .filter_map(|i| self.get(id, i))
                .collect();
            // Do not overwrite counts with zeros when nothing could be read.
            if snaps.iter().all(|s| s.stale) {
                continue;
            }
            let active = snaps.iter().map(|s| s.running.len() as i64).sum();
            let queued = snaps.iter().map(|s| s.retrying.len() as i64).sum();
            cm.report_project_agents(id, &project_name(id), active, queued, "running");
        }
    }
}

fn live_targets(pm: &ProcessManager) -> Vec<((i64, Instance), u16)> {
    let pick = |map: &DashMap<i64, ProcessState>, inst: Instance| {
        map.iter()
            .filter(|e| ProcessManager::is_active_process(e.value()))
            .filter_map(|e| e.value().api_port.map(|p| ((*e.key(), inst), p)))
            .collect::<Vec<_>>()
    };
    let mut out = pick(&pm.processes, Instance::Dev);
    out.extend(pick(&pm.test_processes, Instance::Test));
    out
}

impl RuntimeStateCache {
    /// One poll tick: resolve names of live projects with `lookup` (unknown
    /// names fall back to `project <id>`), then `poll_once`.
    pub async fn poll_with_lookup<F, Fut>(
        &self,
        pm: &ProcessManager,
        cm: &ConcurrencyManager,
        lookup: F,
    ) where
        F: Fn(i64) -> Fut,
        Fut: std::future::Future<Output = Option<String>>,
    {
        let mut names = std::collections::HashMap::new();
        let mut asked = HashSet::new();
        for ((id, _), _) in live_targets(pm) {
            if asked.insert(id) {
                if let Some(name) = lookup(id).await {
                    names.insert(id, name);
                }
            }
        }
        self.poll_once(pm, cm, |id| {
            names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| format!("project {id}"))
        })
        .await;
    }
}

/// Background poller. Project names come from the database once per tick.
pub fn spawn_poller(
    cache: Arc<RuntimeStateCache>,
    pm: ProcessManager,
    cm: Arc<ConcurrencyManager>,
    repo: crate::repository::SqliteRepository,
) {
    use crate::repository::ProjectRepository;
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(POLL_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            cache
                .poll_with_lookup(&pm, &cm, |id| {
                    let repo = repo.clone();
                    async move { repo.get_project(id).await.ok().flatten().map(|p| p.name) }
                })
                .await;
        }
    });
}
