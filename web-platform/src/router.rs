use std::path::PathBuf;

use axum::{
    middleware,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Serialize;
use tower_http::services::{ServeDir, ServeFile};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::auth::middleware::{jwt_auth, require_admin};
use crate::handlers::{
    admin_config, admin_users, ai_generate, alerts, auth, concurrency, contributors, issue_mrs,
    issues, kanban, merge_requests, network_proxy, project_members, project_service,
    project_workflow, projects, token_validation, user_profile,
};
use crate::AppState;

#[derive(OpenApi)]
#[openapi(
    paths(
        auth::login,
        auth::change_password,
        user_profile::get_profile,
        user_profile::update_profile,
        user_profile::get_config,
        user_profile::update_config,
        admin_users::list_users,
        admin_users::create_user,
        admin_users::delete_user,
        admin_users::reset_password,
    ),
    components(schemas(
        auth::LoginRequest,
        auth::LoginResponse,
        auth::LoginUser,
        auth::ChangePasswordRequest,
        user_profile::UserProfile,
        user_profile::UpdateProfileRequest,
        user_profile::UserConfigResponse,
        user_profile::UpdateConfigRequest,
        admin_users::UserInfo,
        admin_users::CreateUserRequest,
        admin_users::ResetPasswordRequest,
    )),
    modifiers(&SecurityAddon),
    info(
        title = "Symphony Web Platform API",
        version = "1.0.0",
        description = "Symphony Web Management Platform API"
    )
)]
struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                utoipa::openapi::security::SecurityScheme::Http(
                    utoipa::openapi::security::Http::new(
                        utoipa::openapi::security::HttpAuthScheme::Bearer,
                    ),
                ),
            );
        }
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub fn create_router(state: AppState) -> Router {
    create_router_with_static_dir(state, None)
}

pub fn create_router_with_static_dir(state: AppState, static_dir: Option<PathBuf>) -> Router {
    let public_routes = Router::new()
        .route("/health", get(health_check))
        .route("/api/auth/login", post(auth::login));

    let user_routes = Router::new()
        .route("/api/auth/password", put(auth::change_password))
        .route("/api/user/profile", get(user_profile::get_profile))
        .route("/api/user/profile", put(user_profile::update_profile))
        .route("/api/user/config", get(user_profile::get_config))
        .route("/api/user/config", put(user_profile::update_config))
        .layer(middleware::from_fn_with_state(state.clone(), jwt_auth));

    let admin_routes = Router::new()
        .route("/api/admin/users", get(admin_users::list_users))
        .route("/api/admin/users", post(admin_users::create_user))
        .route("/api/admin/users/{id}", delete(admin_users::delete_user))
        .route(
            "/api/admin/users/{id}/reset-password",
            put(admin_users::reset_password),
        )
        // Phase 4: Concurrency control (admin)
        .route(
            "/api/admin/concurrency",
            get(concurrency::get_global_concurrency),
        )
        .route(
            "/api/admin/concurrency/config",
            put(concurrency::update_concurrency_config),
        )
        .route(
            "/api/admin/concurrency/events/ticket",
            post(concurrency::create_sse_ticket),
        )
        // Phase 5: Alert & Notification
        .route("/api/admin/alerts", get(alerts::list_alert_history))
        .route("/api/admin/alerts/rules", get(alerts::get_alert_rules))
        .route("/api/admin/alerts/rules", put(alerts::update_alert_rules))
        .route(
            "/api/admin/alerts/channels",
            get(alerts::get_alert_channels),
        )
        .route(
            "/api/admin/alerts/channels",
            put(alerts::update_alert_channels),
        )
        .route("/api/admin/alerts/test", post(alerts::test_notification))
        // Phase 6: System config
        .route("/api/admin/config", get(admin_config::get_system_config))
        .route("/api/admin/config", put(admin_config::update_system_config))
        .route("/api/admin/stats", get(admin_config::get_system_stats))
        .route(
            "/api/admin/network-proxy",
            get(network_proxy::get_network_proxy),
        )
        .route(
            "/api/admin/network-proxy",
            put(network_proxy::update_network_proxy),
        )
        .route(
            "/api/admin/network-proxy/effective",
            get(network_proxy::get_effective_network_proxy),
        )
        .route(
            "/api/admin/network-proxy/test",
            post(network_proxy::test_network_proxy),
        )
        .layer(middleware::from_fn(require_admin))
        .layer(middleware::from_fn_with_state(state.clone(), jwt_auth));

    // Project routes (require authentication)
    let project_routes = Router::new()
        // Project CRUD
        .route("/api/projects", get(projects::list_projects))
        .route("/api/projects", post(projects::create_project))
        .route("/api/projects/{id}", get(projects::get_project))
        .route("/api/projects/{id}", put(projects::update_project))
        .route("/api/projects/{id}", delete(projects::delete_project))
        // Service control
        .route(
            "/api/projects/{id}/start",
            post(project_service::start_service),
        )
        .route(
            "/api/projects/{id}/stop",
            post(project_service::stop_service),
        )
        .route(
            "/api/projects/{id}/restart",
            post(project_service::restart_service),
        )
        .route(
            "/api/projects/{id}/status",
            get(project_service::get_service_status),
        )
        .route(
            "/api/projects/{id}/diagnostics",
            get(project_service::get_diagnostics),
        )
        // Members
        .route(
            "/api/projects/{id}/members",
            get(project_members::list_members),
        )
        .route(
            "/api/projects/{id}/members",
            post(project_members::add_member),
        )
        .route(
            "/api/projects/{id}/members/{userId}",
            put(project_members::update_member_role),
        )
        .route(
            "/api/projects/{id}/members/{userId}",
            delete(project_members::remove_member),
        )
        .route(
            "/api/projects/{id}/members/sync",
            post(project_members::sync_members),
        )
        // Workflow
        .route(
            "/api/projects/{id}/workflow",
            get(project_workflow::get_workflow),
        )
        .route(
            "/api/projects/{id}/workflow",
            put(project_workflow::update_workflow),
        )
        .route(
            "/api/projects/{id}/workflow/reset",
            post(project_workflow::reset_workflow),
        )
        // Phase 3: Kanban & Issues
        .route("/api/projects/{id}/kanban", get(kanban::get_kanban))
        .route("/api/projects/{id}/issues", post(issues::create_issue))
        .route(
            "/api/projects/{id}/issues/ai-generate",
            post(ai_generate::generate_issue),
        )
        .route("/api/projects/{id}/issues/{iid}", get(issues::get_issue))
        .route(
            "/api/projects/{id}/issues/{iid}/mrs",
            get(issue_mrs::get_issue_mrs),
        )
        .route(
            "/api/projects/{id}/mrs",
            post(merge_requests::create_merge_request),
        )
        .route(
            "/api/projects/{id}/mrs/{iid}",
            get(merge_requests::get_merge_request),
        )
        // Phase 4: Contributors & Project Concurrency
        .route(
            "/api/projects/{id}/contributors",
            get(contributors::get_contributors),
        )
        .route(
            "/api/projects/{id}/concurrency",
            get(concurrency::get_project_concurrency),
        )
        .route(
            "/api/projects/{id}/concurrency",
            put(concurrency::update_project_concurrency),
        )
        .layer(middleware::from_fn_with_state(state.clone(), jwt_auth));

    // Phase 4: Token validation (user route)
    let token_validation_routes = Router::new()
        .route(
            "/api/user/config/validate-token",
            post(token_validation::validate_token),
        )
        .layer(middleware::from_fn_with_state(state.clone(), jwt_auth));

    // Phase 4: SSE endpoint (uses ticket auth, no JWT middleware)
    let sse_routes = Router::new().route(
        "/api/admin/concurrency/events",
        get(concurrency::concurrency_events_sse),
    );

    let router = Router::new()
        .merge(public_routes)
        .merge(user_routes)
        .merge(admin_routes)
        .merge(project_routes)
        .merge(token_validation_routes)
        .merge(sse_routes)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .with_state(state);

    if let Some(static_dir) = static_dir {
        let index_file = static_dir.join("index.html");
        router.fallback_service(
            ServeDir::new(static_dir).not_found_service(ServeFile::new(index_file)),
        )
    } else {
        router
    }
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}
