//! HTTP Server Extension — provides observability and operational control endpoints.
//!
//! Implements SPEC Section 13.7: optional HTTP interface with JSON REST API
//! and a human-readable dashboard.

pub mod api;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tokio::sync::mpsc;

use crate::server::api::{build_router, AppState, OrchestratorEvent, OrchestratorQuery};

/// Start the HTTP server on the given port, bound to loopback by default.
///
/// The server communicates with the orchestrator via:
/// - `query_tx`: request/reply channel for state queries
/// - `event_tx`: fire-and-forget channel for operational triggers (e.g. refresh)
pub async fn start_http_server(
    port: u16,
    query_tx: mpsc::Sender<OrchestratorQuery>,
    event_tx: mpsc::Sender<OrchestratorEvent>,
) -> std::io::Result<()> {
    let state = AppState {
        query_tx: Arc::new(query_tx),
        event_tx: Arc::new(event_tx),
    };

    let app: Router = build_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    tracing::info!(port, "HTTP server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Start the HTTP server with graceful shutdown support.
pub async fn start_http_server_with_shutdown(
    port: u16,
    query_tx: mpsc::Sender<OrchestratorQuery>,
    event_tx: mpsc::Sender<OrchestratorEvent>,
    shutdown_signal: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let state = AppState {
        query_tx: Arc::new(query_tx),
        event_tx: Arc::new(event_tx),
    };

    let app: Router = build_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    tracing::info!(port, "HTTP server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    Ok(())
}
