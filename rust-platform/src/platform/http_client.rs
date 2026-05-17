//! Shared HTTP client for platform API calls.
//!
//! Provides authenticated requests, automatic pagination, label management,
//! and token resolution from environment variables.
//!
//! Depends on:
//! - `crate::error::PlatformError` (src/error.rs — another agent)
//! - `crate::config::PlatformConfig` (src/config/ — another agent)
//! - `crate::config::Label` (src/config/ — another agent)

use std::collections::HashSet;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::de::DeserializeOwned;

use crate::config::{Label, PlatformConfig};
use crate::error::PlatformError;

/// Maximum number of pages to fetch during automatic pagination.
/// Safety limit to prevent runaway requests (10 pages * 100 items = 1000 max).
const MAX_PAGES: u32 = 10;

/// Number of items requested per page.
const PER_PAGE: u32 = 100;

/// Shared HTTP client that handles authentication, pagination, and label operations
/// for both GitHub and GitLab platforms.
#[derive(Debug, Clone)]
pub struct HttpClient {
    client: reqwest::Client,
    base_url: String,
    config: PlatformConfig,
}

impl HttpClient {
    /// Constructs a new `HttpClient` from the given platform configuration.
    ///
    /// Resolves the API token from the environment variable referenced in config,
    /// builds a `reqwest::Client` with appropriate auth headers and timeouts.
    ///
    /// # Errors
    ///
    /// Returns `PlatformError::InvalidToken` if the token header value is invalid.
    /// Returns `PlatformError::Network` if the client builder fails.
    pub fn new(config: PlatformConfig) -> Result<Self, PlatformError> {
        let client = build_client(&config)?;
        let base_url = config.base_url.trim_end_matches('/').to_string();
        Ok(Self {
            client,
            base_url,
            config,
        })
    }

    /// Constructs a client from an already-resolved token value.
    ///
    /// This is used by the WORKFLOW.md runtime path, where ServiceConfig has
    /// already resolved `$VAR` references during startup validation.
    pub fn new_with_resolved_token(
        config: PlatformConfig,
        token: &str,
    ) -> Result<Self, PlatformError> {
        let client = build_client_with_token(&config, token)?;
        let base_url = config.base_url.trim_end_matches('/').to_string();
        Ok(Self {
            client,
            base_url,
            config,
        })
    }

    /// Returns a reference to the underlying platform configuration.
    pub fn config(&self) -> &PlatformConfig {
        &self.config
    }

    /// Returns a reference to the underlying reqwest client.
    pub fn inner(&self) -> &reqwest::Client {
        &self.client
    }

    /// Returns the base URL (without trailing slash).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Fetches all pages of a paginated API endpoint and deserializes each page
    /// as a `Vec<T>`, concatenating all results.
    ///
    /// Pagination detection:
    /// - GitHub: follows `Link` header with `rel="next"`
    /// - GitLab: follows `x-next-page` header when non-empty
    ///
    /// Stops after `MAX_PAGES` (10) pages and logs a warning if truncated.
    ///
    /// # Arguments
    ///
    /// * `path` — API path (e.g., `/repos/owner/repo/labels`)
    /// * `params` — Additional query parameters (page/per_page are added automatically)
    pub async fn get_all_pages<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<Vec<T>, PlatformError> {
        let mut all_items: Vec<T> = Vec::new();
        let mut page = 1u32;

        loop {
            if page > MAX_PAGES {
                tracing::warn!(
                    path,
                    max_pages = MAX_PAGES,
                    "Reached max page limit, results may be truncated"
                );
                break;
            }

            let page_str = page.to_string();
            let per_page_str = PER_PAGE.to_string();

            let mut query: Vec<(&str, &str)> = params.to_vec();
            query.push(("page", &page_str));
            query.push(("per_page", &per_page_str));

            let url = format!("{}{}", self.base_url, path);
            let response = self
                .client
                .get(&url)
                .query(&query)
                .send()
                .await
                .map_err(|e| {
                    if e.is_timeout() {
                        PlatformError::Timeout
                    } else if e.is_connect() {
                        PlatformError::ConnectionRefused
                    } else {
                        PlatformError::Network(e)
                    }
                })?;

            let status = response.status();
            if !status.is_success() {
                let status_code = status.as_u16();
                let body = response.text().await.unwrap_or_default();
                return Err(PlatformError::from_status(status_code, &body));
            }

            let has_next = has_next_page(&response);
            let items: Vec<T> = response.json().await.map_err(|e| {
                tracing::error!(path, page, error = %e, "Failed to deserialize page response");
                PlatformError::Network(e)
            })?;

            let item_count = items.len();
            all_items.extend(items);

            // If we got fewer items than per_page, there's no next page regardless of headers
            if !has_next || item_count < PER_PAGE as usize {
                break;
            }
            page += 1;
        }

        Ok(all_items)
    }

    /// Lists all labels in the repository/project.
    ///
    /// - GitHub: `GET /repos/:owner/:repo/labels`
    /// - GitLab: `GET /projects/:id/labels`
    pub async fn list_labels(&self) -> Result<Vec<Label>, PlatformError> {
        let path = match self.config.kind.as_str() {
            "github" => format!("/repos/{}/{}/labels", self.config.owner, self.config.repo),
            "gitlab" => {
                let project_id = self.resolve_project_id();
                format!("/projects/{}/labels", project_id)
            }
            other => {
                return Err(PlatformError::Unprocessable(format!(
                    "unknown platform kind: {}",
                    other
                )))
            }
        };
        self.get_all_pages(&path, &[]).await
    }

    /// Creates a label in the project (used by GitLab adapter during startup).
    ///
    /// GitLab: `POST /projects/:id/labels` with `name` and `color` fields.
    ///
    /// # Arguments
    ///
    /// * `name` — Label name (e.g., "workflow::todo")
    /// * `color` — Hex color code (e.g., "#428BCA")
    pub async fn create_label(&self, name: &str, color: &str) -> Result<(), PlatformError> {
        let project_id = self.resolve_project_id();
        let path = format!("/projects/{}/labels", project_id);
        let url = format!("{}{}", self.base_url, path);
        let body = serde_json::json!({ "name": name, "color": color });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    PlatformError::Timeout
                } else {
                    PlatformError::Network(e)
                }
            })?;

        if !response.status().is_success() {
            let status_code = response.status().as_u16();
            let body_text = response.text().await.unwrap_or_default();
            return Err(PlatformError::from_status(status_code, &body_text));
        }

        Ok(())
    }

    /// Checks that all workflow labels exist in the repository/project.
    ///
    /// Behavior differs by platform:
    /// - **GitHub**: Logs a warning listing missing labels (user must create them manually).
    /// - **GitLab**: Automatically creates missing labels with a default color.
    ///
    /// Called once during adapter startup after credential validation.
    pub async fn ensure_workflow_labels(&self) -> Result<(), PlatformError> {
        let required_labels: Vec<&str> = self
            .config
            .workflow
            .states
            .values()
            .map(|s| s.as_str())
            .collect();

        let existing = self.list_labels().await?;
        let existing_names: HashSet<&str> = existing.iter().map(|l| l.name.as_str()).collect();

        let missing: Vec<&str> = required_labels
            .iter()
            .filter(|l| !existing_names.contains(*l))
            .copied()
            .collect();

        if missing.is_empty() {
            tracing::info!("All workflow labels verified present");
            return Ok(());
        }

        match self.config.kind.as_str() {
            "github" => {
                tracing::warn!(
                    missing = ?missing,
                    "Workflow labels not found in repository. Please create them manually."
                );
            }
            "gitlab" => {
                for label_name in &missing {
                    tracing::info!(label = label_name, "Auto-creating missing workflow label");
                    self.create_label(label_name, "#428BCA").await?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Resolves the GitLab project ID from config.
    /// Falls back to 0 if not set (should be caught by config validation).
    fn resolve_project_id(&self) -> u64 {
        self.config.project_id.unwrap_or(0)
    }
}

/// Builds a `reqwest::Client` with platform-specific authentication headers and timeouts.
///
/// - GitHub: `Authorization: Bearer <token>`
/// - GitLab: `PRIVATE-TOKEN: <token>`
///
/// Timeouts:
/// - Request timeout: 30 seconds
/// - Connection timeout: 5 seconds
fn build_client(config: &PlatformConfig) -> Result<reqwest::Client, PlatformError> {
    let token = resolve_token(&config.api_token)?;
    build_client_with_token(config, &token)
}

fn build_client_with_token(
    config: &PlatformConfig,
    token: &str,
) -> Result<reqwest::Client, PlatformError> {
    let mut headers = HeaderMap::new();

    match config.kind.as_str() {
        "github" => {
            let auth_value = format!("Bearer {}", token);
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&auth_value).map_err(|_| PlatformError::InvalidToken)?,
            );
            // GitHub recommends setting Accept header for API versioning
            headers.insert(
                reqwest::header::ACCEPT,
                HeaderValue::from_static("application/vnd.github+json"),
            );
        }
        "gitlab" => {
            headers.insert(
                HeaderName::from_static("private-token"),
                HeaderValue::from_str(&token).map_err(|_| PlatformError::InvalidToken)?,
            );
        }
        _ => {
            // Unknown kind — config validation should catch this before we get here,
            // but we handle it gracefully.
        }
    }

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .user_agent("symphony-platform/0.1")
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(PlatformError::Network)?;

    Ok(client)
}

/// Resolves a token reference (e.g., `$GITHUB_TOKEN`) to its actual value
/// by reading the named environment variable.
///
/// # Errors
///
/// - Returns `PlatformError::InvalidToken` if the reference doesn't start with `$`.
/// - Returns `PlatformError::InvalidToken` if the environment variable is unset or empty.
fn resolve_token(token_ref: &str) -> Result<String, PlatformError> {
    if !token_ref.starts_with('$') {
        return Err(PlatformError::InvalidToken);
    }
    let var_name = &token_ref[1..];
    match std::env::var(var_name) {
        Ok(val) if val.is_empty() => {
            tracing::error!(var = var_name, "Environment variable is set but empty");
            Err(PlatformError::InvalidToken)
        }
        Err(_) => {
            tracing::error!(var = var_name, "Environment variable is not set");
            Err(PlatformError::InvalidToken)
        }
        Ok(val) => Ok(val),
    }
}

/// Checks response headers to determine if there's a next page.
///
/// - GitHub: `Link` header containing `rel="next"`
/// - GitLab: `x-next-page` header with a non-empty value
fn has_next_page(response: &reqwest::Response) -> bool {
    // GitHub: Link header with rel="next"
    if let Some(link) = response.headers().get("link") {
        if let Ok(link_str) = link.to_str() {
            if link_str.contains("rel=\"next\"") {
                return true;
            }
        }
    }
    // GitLab: x-next-page header non-empty
    if let Some(next_page) = response.headers().get("x-next-page") {
        if let Ok(val) = next_page.to_str() {
            if !val.is_empty() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_token_rejects_literal() {
        let result = resolve_token("ghp_abc123");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_token_missing_env_var() {
        let result = resolve_token("$SYMPHONY_TEST_NONEXISTENT_VAR_XYZ");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_token_success() {
        // Use a variable that's very likely to exist
        std::env::set_var("SYMPHONY_TEST_TOKEN_ABC", "test-value-123");
        let result = resolve_token("$SYMPHONY_TEST_TOKEN_ABC");
        assert_eq!(result.unwrap(), "test-value-123");
        std::env::remove_var("SYMPHONY_TEST_TOKEN_ABC");
    }

    #[test]
    fn test_resolve_token_empty_value() {
        std::env::set_var("SYMPHONY_TEST_EMPTY_TOKEN", "");
        let result = resolve_token("$SYMPHONY_TEST_EMPTY_TOKEN");
        assert!(result.is_err());
        std::env::remove_var("SYMPHONY_TEST_EMPTY_TOKEN");
    }
}
