use chrono::{Duration, Utc};
use dashmap::DashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::models::concurrency::{ConcurrencyEvent, ConcurrencyStatus, ProjectConcurrencyInfo};

#[derive(Debug, Clone)]
pub struct ProjectAgentState {
    pub active_agents: i64,
    pub queued_tasks: i64,
    pub project_name: String,
    pub max_agents: Option<i64>,
    pub service_status: String,
    pub last_updated: chrono::DateTime<Utc>,
}

pub struct ConcurrencyManager {
    pub global_max: AtomicI64,
    pub global_active: AtomicI64,
    projects: DashMap<i64, ProjectAgentState>,
    sse_tickets: DashMap<String, (i64, chrono::DateTime<Utc>)>,
    event_tx: broadcast::Sender<ConcurrencyEvent>,
    last_poll: std::sync::Mutex<chrono::DateTime<Utc>>,
}

impl ConcurrencyManager {
    pub fn new(global_max: i64) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            global_max: AtomicI64::new(global_max),
            global_active: AtomicI64::new(0),
            projects: DashMap::new(),
            sse_tickets: DashMap::new(),
            event_tx,
            last_poll: std::sync::Mutex::new(Utc::now()),
        }
    }

    pub fn get_event_sender(&self) -> broadcast::Sender<ConcurrencyEvent> {
        self.event_tx.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConcurrencyEvent> {
        self.event_tx.subscribe()
    }

    pub fn report_project_agents(
        &self,
        project_id: i64,
        project_name: &str,
        active: i64,
        queued: i64,
        service_status: &str,
    ) {
        let mut recalc = false;
        self.projects
            .entry(project_id)
            .and_modify(|state| {
                if state.active_agents != active {
                    recalc = true;
                }
                state.active_agents = active;
                state.queued_tasks = queued;
                state.project_name = project_name.to_string();
                state.service_status = service_status.to_string();
                state.last_updated = Utc::now();
            })
            .or_insert_with(|| {
                recalc = true;
                ProjectAgentState {
                    active_agents: active,
                    queued_tasks: queued,
                    project_name: project_name.to_string(),
                    max_agents: None,
                    service_status: service_status.to_string(),
                    last_updated: Utc::now(),
                }
            });

        if recalc {
            self.recalculate_global();
        }

        *self.last_poll.lock().unwrap() = Utc::now();
    }

    pub fn set_project_limit(&self, project_id: i64, max_agents: Option<i64>) {
        if let Some(mut entry) = self.projects.get_mut(&project_id) {
            entry.max_agents = max_agents;
        }
    }

    pub fn remove_project(&self, project_id: i64) {
        self.projects.remove(&project_id);
        self.recalculate_global();
    }

    fn recalculate_global(&self) {
        let total: i64 = self.projects.iter().map(|e| e.value().active_agents).sum();
        self.global_active.store(total, Ordering::Relaxed);
    }

    pub fn check_can_schedule(&self) -> Result<(), (i64, i64)> {
        let active = self.global_active.load(Ordering::Relaxed);
        let max = self.global_max.load(Ordering::Relaxed);
        if active >= max {
            Err((active, max))
        } else {
            Ok(())
        }
    }

    pub fn check_can_schedule_project(&self, project_id: i64) -> Result<(), (i64, i64)> {
        if let Some(state) = self.projects.get(&project_id) {
            if let Some(max) = state.max_agents {
                if state.active_agents >= max {
                    return Err((state.active_agents, max));
                }
            }
        }
        Ok(())
    }

    pub fn get_status(&self) -> ConcurrencyStatus {
        let global_max = self.global_max.load(Ordering::Relaxed);
        let global_active = self.global_active.load(Ordering::Relaxed);
        let utilization = if global_max > 0 {
            (global_active as f64 / global_max as f64) * 100.0
        } else {
            0.0
        };

        let projects: Vec<ProjectConcurrencyInfo> = self
            .projects
            .iter()
            .map(|entry| {
                let state = entry.value();
                ProjectConcurrencyInfo {
                    project_id: *entry.key(),
                    project_name: state.project_name.clone(),
                    active_agents: state.active_agents,
                    max_agents: state.max_agents,
                    queued_tasks: state.queued_tasks,
                    service_status: state.service_status.clone(),
                }
            })
            .collect();

        let freshness = {
            let last = self.last_poll.lock().unwrap();
            Utc::now().signed_duration_since(*last).num_seconds()
        };

        ConcurrencyStatus {
            global_max,
            global_active,
            utilization_percent: utilization,
            projects,
            data_freshness_seconds: freshness,
        }
    }

    pub fn get_project_status(&self, project_id: i64) -> Option<ProjectConcurrencyInfo> {
        self.projects.get(&project_id).map(|entry| {
            let state = entry.value();
            ProjectConcurrencyInfo {
                project_id,
                project_name: state.project_name.clone(),
                active_agents: state.active_agents,
                max_agents: state.max_agents,
                queued_tasks: state.queued_tasks,
                service_status: state.service_status.clone(),
            }
        })
    }

    pub fn update_global_max(&self, new_max: i64) -> i64 {
        self.global_max.swap(new_max, Ordering::Relaxed)
    }

    // SSE Ticket management
    pub fn generate_ticket(&self, user_id: i64) -> String {
        let ticket = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::seconds(30);
        self.sse_tickets
            .insert(ticket.clone(), (user_id, expires_at));
        ticket
    }

    pub fn validate_ticket(&self, ticket: &str) -> Option<i64> {
        if let Some((_, (user_id, expires_at))) = self.sse_tickets.remove(ticket) {
            if Utc::now() < expires_at {
                return Some(user_id);
            }
        }
        None
    }

    pub fn cleanup_expired_tickets(&self) {
        let now = Utc::now();
        self.sse_tickets
            .retain(|_, (_, expires_at)| *expires_at > now);
    }

    pub fn broadcast_event(&self, event: ConcurrencyEvent) {
        let _ = self.event_tx.send(event);
    }
}

impl Default for ConcurrencyManager {
    fn default() -> Self {
        Self::new(5)
    }
}
