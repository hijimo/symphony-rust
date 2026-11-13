//! Polling rust-platform `/api/v1/state` (P2 design §3 F1).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::{routing::get, Json, Router};
use chrono::Utc;
use serde_json::json;
use tokio::net::TcpListener;

use web_platform::concurrency::manager::ConcurrencyManager;
use web_platform::models::ServiceStatus;
use web_platform::process_manager::{ProcessManager, ProcessState};
use web_platform::services::runtime_state::{allocate_local_port, Instance, RuntimeStateCache};

fn state_body() -> serde_json::Value {
    json!({
        "generated_at": "2026-11-13T08:00:00Z",
        "counts": {"running": 1, "retrying": 1},
        "running": [{
            "issue_id": "42",
            "issue_identifier": "#42",
            "state": "In Progress",
            "session_id": "s1",
            "turn_count": 3,
            "last_event": "turn/completed",
            "last_message": "hi",
            "started_at": "2026-11-13T07:50:00Z",
            "last_event_at": "2026-11-13T07:59:00Z",
            "tokens": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}
        }],
        "retrying": [{
            "issue_id": "43", "issue_identifier": "#43", "attempt": 2,
            "due_at": "2026-11-13T08:01:00Z", "error": "boom"
        }],
        "codex_totals": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 0, "seconds_running": 0.0},
        "rate_limits": null
    })
}

/// Fake rust-platform HTTP server. `healthy=false` makes it answer 500.
async fn fake_server(healthy: Arc<AtomicBool>) -> u16 {
    let app = Router::new().route(
        "/api/v1/state",
        get(move || {
            let healthy = healthy.clone();
            async move {
                if healthy.load(Ordering::SeqCst) {
                    Ok(Json(state_body()))
                } else {
                    Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

fn running_state(port: Option<u16>) -> ProcessState {
    ProcessState {
        pid: 1234,
        started_at: Utc::now(),
        status: ServiceStatus::Running,
        restart_count: 0,
        api_port: port,
    }
}

#[test]
fn test_allocate_local_port_returns_bindable_port() {
    let port = allocate_local_port().expect("port");
    assert!(port > 0);
    std::net::TcpListener::bind(("127.0.0.1", port)).expect("port is free after allocation");
}

#[tokio::test]
async fn test_refresh_parses_running_and_retrying_rows() {
    let port = fake_server(Arc::new(AtomicBool::new(true))).await;
    let cache = RuntimeStateCache::new();
    cache.refresh(7, Instance::Dev, port).await;

    let snap = cache.get(7, Instance::Dev).expect("snapshot");
    assert!(!snap.stale);
    assert_eq!(snap.running.len(), 1);
    assert_eq!(snap.running[0].issue_identifier, "#42");
    assert_eq!(snap.running[0].state, "In Progress");
    assert_eq!(snap.running[0].tokens.total_tokens, 15);
    assert_eq!(snap.retrying.len(), 1);
    assert_eq!(snap.retrying[0].attempt, 2);
}

#[tokio::test]
async fn test_failed_refresh_keeps_last_snapshot_and_marks_stale() {
    let healthy = Arc::new(AtomicBool::new(true));
    let port = fake_server(healthy.clone()).await;
    let cache = RuntimeStateCache::new();
    cache.refresh(7, Instance::Dev, port).await;

    healthy.store(false, Ordering::SeqCst);
    cache.refresh(7, Instance::Dev, port).await;

    let snap = cache.get(7, Instance::Dev).expect("kept");
    assert!(snap.stale);
    assert_eq!(snap.running.len(), 1, "last good data retained");
}

#[tokio::test]
async fn test_unreachable_port_without_history_stores_nothing() {
    let port = allocate_local_port().unwrap(); // nothing listens here
    let cache = RuntimeStateCache::new();
    cache.refresh(9, Instance::Test, port).await;
    let snap = cache.get(9, Instance::Test).expect("stale placeholder");
    assert!(snap.stale);
    assert!(snap.running.is_empty());
}

#[tokio::test]
async fn test_poll_once_reports_agents_and_drops_stopped_instances() {
    let port = fake_server(Arc::new(AtomicBool::new(true))).await;
    let pm = ProcessManager::new();
    let cm = ConcurrencyManager::new(5);
    let cache = RuntimeStateCache::new();

    pm.set_state(1, running_state(Some(port)));
    pm.set_test_state(1, running_state(Some(port)));
    pm.set_state(2, running_state(None)); // started before F1: no port, skipped

    cache
        .poll_once(&pm, &cm, |id| format!("project-{id}"))
        .await;

    assert!(cache.get(1, Instance::Dev).is_some());
    assert!(cache.get(1, Instance::Test).is_some());
    assert!(cache.get(2, Instance::Dev).is_none());
    let info = cm.get_project_status(1).expect("reported");
    assert_eq!(info.active_agents, 2, "dev + test running rows");
    assert_eq!(info.queued_tasks, 2);
    assert_eq!(info.project_name, "project-1");
    assert_eq!(cache.running_issues(1).len(), 2);

    pm.remove_test_state(1);
    pm.remove_state(1);
    cache
        .poll_once(&pm, &cm, |id| format!("project-{id}"))
        .await;
    assert!(cache.get(1, Instance::Dev).is_none());
    assert!(cache.get(1, Instance::Test).is_none());
    assert!(
        cm.get_project_status(1).is_none(),
        "stopped project removed"
    );
}

#[tokio::test]
async fn test_all_stale_project_keeps_previous_counts() {
    let healthy = Arc::new(AtomicBool::new(true));
    let port = fake_server(healthy.clone()).await;
    let pm = ProcessManager::new();
    let cm = ConcurrencyManager::new(5);
    let cache = RuntimeStateCache::new();
    pm.set_state(3, running_state(Some(port)));

    cache.poll_once(&pm, &cm, |_| "p".to_string()).await;
    assert_eq!(cm.get_project_status(3).unwrap().active_agents, 1);

    healthy.store(false, Ordering::SeqCst);
    cache.poll_once(&pm, &cm, |_| "p".to_string()).await;
    assert_eq!(
        cm.get_project_status(3).unwrap().active_agents,
        1,
        "a failed poll must not report zero agents"
    );
    let snaps = cache.snapshots();
    assert_eq!(snaps.len(), 1);
    assert_eq!(snaps[0].0, (3, Instance::Dev));
    assert!(snaps[0].1.stale);
}

#[tokio::test]
async fn test_poll_with_lookup_resolves_names_once_and_falls_back() {
    let port = fake_server(Arc::new(AtomicBool::new(true))).await;
    let pm = ProcessManager::new();
    let cm = ConcurrencyManager::new(5);
    let cache = RuntimeStateCache::new();
    pm.set_state(4, running_state(Some(port)));
    pm.set_test_state(4, running_state(Some(port)));
    pm.set_state(5, running_state(Some(port)));

    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let c = calls.clone();
    cache
        .poll_with_lookup(&pm, &cm, move |id| {
            c.lock().unwrap().push(id);
            async move { (id == 4).then(|| "Claims".to_string()) }
        })
        .await;

    let mut seen = calls.lock().unwrap().clone();
    seen.sort();
    assert_eq!(seen, vec![4, 5], "one lookup per project");
    assert_eq!(cm.get_project_status(4).unwrap().project_name, "Claims");
    assert_eq!(cm.get_project_status(5).unwrap().project_name, "project 5");
}
