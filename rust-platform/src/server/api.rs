//! HTTP API routes and handlers for the Symphony HTTP Server Extension.
//!
//! Implements SPEC Section 13.7.2:
//! - GET /              -> Human-readable dashboard
//! - GET /api/v1/state  -> System state summary
//! - GET /api/v1/{id}   -> Issue-specific details
//! - POST /api/v1/refresh -> Trigger immediate poll+reconcile
//!
//! TODO(Phase 6): 操作员介入模式 API 端点
//! - GET  /api/v1/approvals/pending   -> 待审批列表
//! - POST /api/v1/approvals/:id/resolve -> 操作员审批/拒绝
//! - GET  /api/v1/inputs/pending      -> 待回答的用户输入请求
//! - POST /api/v1/inputs/:id/answer   -> 操作员提交回答
//!   参见 docs/migration-gap-analysis.md Phase 6。

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

/// Application state shared across all HTTP handlers.
#[derive(Clone)]
pub struct AppState {
    /// Channel for querying orchestrator state (request/reply pattern).
    pub query_tx: Arc<mpsc::Sender<OrchestratorQuery>>,
    /// Channel for sending operational events to the orchestrator.
    pub event_tx: Arc<mpsc::Sender<OrchestratorEvent>>,
}

/// Query messages sent from HTTP handlers to the orchestrator.
/// Each variant carries a oneshot reply channel for the response.
pub enum OrchestratorQuery {
    /// Request the full system state snapshot.
    GetState {
        reply: oneshot::Sender<StateResponse>,
    },
    /// Request details for a specific issue by identifier.
    GetIssue {
        identifier: String,
        reply: oneshot::Sender<Option<IssueDetailResponse>>,
    },
}

/// Events that HTTP handlers can send to the orchestrator.
pub enum OrchestratorEvent {
    /// Trigger an immediate poll + reconciliation cycle.
    ForceRefresh,
}

// --- Response types (SPEC Section 13.7.2) ---

/// Full system state response for GET /api/v1/state.
#[derive(Debug, Clone, Serialize)]
pub struct StateResponse {
    pub generated_at: String,
    pub counts: Counts,
    pub running: Vec<RunningRow>,
    pub retrying: Vec<RetryRow>,
    pub codex_totals: CodexTotalsJson,
    pub rate_limits: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Counts {
    pub running: usize,
    pub retrying: usize,
}

/// A single running session row in the state response.
#[derive(Debug, Clone, Serialize)]
pub struct RunningRow {
    pub issue_id: String,
    pub issue_identifier: String,
    pub state: String,
    pub session_id: String,
    pub turn_count: u32,
    pub last_event: Option<String>,
    pub last_message: Option<String>,
    pub started_at: String,
    pub last_event_at: Option<String>,
    pub tokens: TokensJson,
}

/// A single retry queue row in the state response.
#[derive(Debug, Clone, Serialize)]
pub struct RetryRow {
    pub issue_id: String,
    pub issue_identifier: String,
    pub attempt: u32,
    pub due_at: String,
    pub error: Option<String>,
}

/// Token counts in JSON-friendly format.
#[derive(Debug, Clone, Serialize)]
pub struct TokensJson {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Aggregate codex totals including runtime.
#[derive(Debug, Clone, Serialize)]
pub struct CodexTotalsJson {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub seconds_running: f64,
}

/// Issue-specific detail response for GET /api/v1/{identifier}.
#[derive(Debug, Clone, Serialize)]
pub struct IssueDetailResponse {
    pub issue_identifier: String,
    pub issue_id: String,
    pub status: String,
    pub running: Option<RunningRow>,
    pub retry: Option<RetryRow>,
    pub last_error: Option<String>,
}

/// Response for POST /api/v1/refresh.
#[derive(Debug, Clone, Serialize)]
pub struct RefreshResponse {
    pub queued: bool,
    pub coalesced: bool,
    pub requested_at: String,
    pub operations: Vec<String>,
}

/// Standard API error envelope.
#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiErrorDetail {
    pub code: String,
    pub message: String,
}

// --- Router construction ---

/// Build the axum router with all API routes.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard_handler))
        .route("/api/v1/state", get(get_state_handler))
        .route("/api/v1/refresh", post(refresh_handler))
        .route("/api/v1/{identifier}", get(get_issue_handler))
        .with_state(state)
}

// --- Helper: query orchestrator with timeout ---

/// Send a query to the orchestrator and await the reply with a 5-second timeout.
///
/// Returns 503 Service Unavailable if the orchestrator is unreachable or times out.
async fn query_orchestrator<T>(
    query_tx: &mpsc::Sender<OrchestratorQuery>,
    build_query: impl FnOnce(oneshot::Sender<T>) -> OrchestratorQuery,
) -> Result<T, StatusCode> {
    let (reply_tx, reply_rx) = oneshot::channel();
    query_tx
        .send(build_query(reply_tx))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    tokio::time::timeout(Duration::from_secs(5), reply_rx)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)? // timeout
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE) // channel closed
}

// --- Handlers ---

/// GET / — Human-readable dashboard.
async fn dashboard_handler(State(state): State<AppState>) -> Response {
    let result = query_orchestrator(&state.query_tx, |reply| OrchestratorQuery::GetState {
        reply,
    })
    .await;

    match result {
        Ok(state_resp) => {
            let html = render_dashboard(&state_resp);
            Html(html).into_response()
        }
        Err(status) => {
            let html = "<html><body><h1>Symphony</h1><p>Orchestrator unavailable</p></body></html>";
            (status, Html(html.to_string())).into_response()
        }
    }
}

/// GET /api/v1/state — System state summary.
async fn get_state_handler(State(state): State<AppState>) -> Response {
    match query_orchestrator(&state.query_tx, |reply| OrchestratorQuery::GetState {
        reply,
    })
    .await
    {
        Ok(state_resp) => Json(state_resp).into_response(),
        Err(status) => (
            status,
            Json(ApiError {
                error: ApiErrorDetail {
                    code: "unavailable".into(),
                    message: "orchestrator unavailable or timed out".into(),
                },
            }),
        )
            .into_response(),
    }
}

/// GET /api/v1/{identifier} — Issue-specific details.
async fn get_issue_handler(
    State(state): State<AppState>,
    Path(identifier): Path<String>,
) -> Response {
    match query_orchestrator(&state.query_tx, |reply| OrchestratorQuery::GetIssue {
        identifier: identifier.clone(),
        reply,
    })
    .await
    {
        Ok(Some(detail)) => Json(detail).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: ApiErrorDetail {
                    code: "issue_not_found".into(),
                    message: format!("issue '{}' not found in current state", identifier),
                },
            }),
        )
            .into_response(),
        Err(status) => (
            status,
            Json(ApiError {
                error: ApiErrorDetail {
                    code: "unavailable".into(),
                    message: "orchestrator unavailable or timed out".into(),
                },
            }),
        )
            .into_response(),
    }
}

/// POST /api/v1/refresh — Trigger immediate poll + reconciliation.
async fn refresh_handler(State(state): State<AppState>) -> (StatusCode, Json<RefreshResponse>) {
    // Fire-and-forget: send ForceRefresh event to orchestrator
    let _ = state.event_tx.send(OrchestratorEvent::ForceRefresh).await;

    (
        StatusCode::ACCEPTED,
        Json(RefreshResponse {
            queued: true,
            coalesced: false,
            requested_at: Utc::now().to_rfc3339(),
            operations: vec!["poll".into(), "reconcile".into()],
        }),
    )
}

// --- Dashboard rendering ---

/// Render a simple server-side HTML dashboard from the state response.
fn render_dashboard(state: &StateResponse) -> String {
    let running_rows: String = state
        .running
        .iter()
        .map(|r| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                r.issue_identifier,
                r.state,
                r.session_id,
                r.turn_count,
                r.last_event.as_deref().unwrap_or("-"),
                r.started_at,
            )
        })
        .collect();

    let retry_rows: String = state
        .retrying
        .iter()
        .map(|r| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                r.issue_identifier,
                r.attempt,
                r.due_at,
                r.error.as_deref().unwrap_or("-"),
            )
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Symphony Dashboard</title>
    <meta charset="utf-8">
    <style>
        body {{ font-family: system-ui, sans-serif; margin: 2rem; background: #f8f9fa; }}
        h1 {{ color: #333; }}
        .stats {{ display: flex; gap: 2rem; margin: 1rem 0; }}
        .stat {{ background: white; padding: 1rem 2rem; border-radius: 8px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
        .stat-value {{ font-size: 2rem; font-weight: bold; color: #2563eb; }}
        .stat-label {{ color: #666; font-size: 0.9rem; }}
        table {{ border-collapse: collapse; width: 100%; background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
        th, td {{ padding: 0.75rem 1rem; text-align: left; border-bottom: 1px solid #eee; }}
        th {{ background: #f1f5f9; font-weight: 600; }}
        .section {{ margin: 2rem 0; }}
    </style>
</head>
<body>
    <h1>Symphony Dashboard</h1>
    <p>Generated at: {generated_at}</p>

    <div class="stats">
        <div class="stat">
            <div class="stat-value">{running_count}</div>
            <div class="stat-label">Running</div>
        </div>
        <div class="stat">
            <div class="stat-value">{retrying_count}</div>
            <div class="stat-label">Retrying</div>
        </div>
        <div class="stat">
            <div class="stat-value">{total_tokens}</div>
            <div class="stat-label">Total Tokens</div>
        </div>
        <div class="stat">
            <div class="stat-value">{seconds_running:.1}s</div>
            <div class="stat-label">Runtime</div>
        </div>
    </div>

    <div class="section">
        <h2>Running Sessions</h2>
        <table>
            <thead><tr><th>Issue</th><th>State</th><th>Session</th><th>Turns</th><th>Last Event</th><th>Started</th></tr></thead>
            <tbody>{running_rows}</tbody>
        </table>
    </div>

    <div class="section">
        <h2>Retry Queue</h2>
        <table>
            <thead><tr><th>Issue</th><th>Attempt</th><th>Due At</th><th>Error</th></tr></thead>
            <tbody>{retry_rows}</tbody>
        </table>
    </div>
</body>
</html>"#,
        generated_at = state.generated_at,
        running_count = state.counts.running,
        retrying_count = state.counts.retrying,
        total_tokens = state.codex_totals.total_tokens,
        seconds_running = state.codex_totals.seconds_running,
        running_rows = running_rows,
        retry_rows = retry_rows,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_state() -> StateResponse {
        StateResponse {
            generated_at: "2024-01-15T10:00:00Z".into(),
            counts: Counts {
                running: 1,
                retrying: 0,
            },
            running: vec![RunningRow {
                issue_id: "uuid-1".into(),
                issue_identifier: "PROJ-42".into(),
                state: "In Progress".into(),
                session_id: "thread-1-turn-1".into(),
                turn_count: 3,
                last_event: Some("notification".into()),
                last_message: Some("Working on tests".into()),
                started_at: "2024-01-15T09:50:00Z".into(),
                last_event_at: Some("2024-01-15T09:59:00Z".into()),
                tokens: TokensJson {
                    input_tokens: 1200,
                    output_tokens: 800,
                    total_tokens: 2000,
                },
            }],
            retrying: vec![],
            codex_totals: CodexTotalsJson {
                input_tokens: 5000,
                output_tokens: 2400,
                total_tokens: 7400,
                seconds_running: 1834.2,
            },
            rate_limits: None,
        }
    }

    #[test]
    fn test_render_dashboard_produces_html() {
        let state = make_test_state();
        let html = render_dashboard(&state);
        assert!(html.contains("Symphony Dashboard"));
        assert!(html.contains("PROJ-42"));
        assert!(html.contains("In Progress"));
    }

    #[test]
    fn test_refresh_response_serialization() {
        let resp = RefreshResponse {
            queued: true,
            coalesced: false,
            requested_at: "2024-01-15T10:00:00Z".into(),
            operations: vec!["poll".into(), "reconcile".into()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["queued"], true);
        assert_eq!(json["operations"][0], "poll");
    }
}
