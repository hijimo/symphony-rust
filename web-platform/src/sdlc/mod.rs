//! AI-native SDLC P1: document gates, workbench workspace and agent sessions.
//! Design: docs/方案/ai-native-sdlc-flow-design.md,
//!         docs/方案/ai-doc-workbench-design.md.

pub mod agent;
pub mod content;
pub mod kanban;
pub mod model;
pub mod platform;
pub mod service;
pub mod state;
pub mod store;
pub mod templates;
pub mod workspace;

use std::sync::Arc;

use crate::db::DbPool;

/// Shared state of the SDLC feature.
#[derive(Clone)]
pub struct SdlcState {
    pub store: store::SdlcStore,
    pub workspaces: workspace::WorkspaceService,
    pub agents: Arc<agent::AgentManager>,
    pub platforms: Arc<dyn platform::PlatformFactory>,
    /// Per-flow lock serializing state transitions and git operations.
    pub flow_locks: Arc<dashmap::DashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
    /// Per-flow change notifications for WebSocket subscribers.
    pub flow_events: Arc<dashmap::DashMap<i64, tokio::sync::broadcast::Sender<serde_json::Value>>>,
}

impl SdlcState {
    pub fn new(
        pool: DbPool,
        workspace_root: &str,
        agents: Arc<agent::AgentManager>,
        platforms: Arc<dyn platform::PlatformFactory>,
    ) -> Self {
        Self {
            store: store::SdlcStore::new(pool),
            workspaces: workspace::WorkspaceService::new(workspace_root),
            agents,
            platforms,
            flow_locks: Arc::new(dashmap::DashMap::new()),
            flow_events: Arc::new(dashmap::DashMap::new()),
        }
    }

    pub fn flow_lock(&self, flow_id: i64) -> Arc<tokio::sync::Mutex<()>> {
        self.flow_locks
            .entry(flow_id)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub fn flow_channel(&self, flow_id: i64) -> tokio::sync::broadcast::Sender<serde_json::Value> {
        self.flow_events
            .entry(flow_id)
            .or_insert_with(|| tokio::sync::broadcast::channel(256).0)
            .clone()
    }

    pub fn notify(&self, flow_id: i64, event: serde_json::Value) {
        let _ = self.flow_channel(flow_id).send(event);
    }
}
