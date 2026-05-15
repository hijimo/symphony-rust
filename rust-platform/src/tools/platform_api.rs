//! `platform_api` tool — controlled interface for agent-initiated platform operations.
//!
//! This module provides a structured, sanitized API that agents can use to interact
//! with the platform (GitHub/GitLab) without direct CLI access. All inputs are
//! recursively sanitized to prevent injection attacks via issue content.

use std::sync::Arc;

use serde_json::Value;

use crate::error::PlatformError;
use crate::platform::{CreatePrParams, IssueId, Platform};

/// Actions that the platform_api tool is allowed to execute.
const ALLOWED_ACTIONS: &[&str] = &[
    "create_branch",
    "create_pull_request",
    "add_comment",
    "get_issue",
    "list_pull_requests",
    "get_pull_request_status",
];

/// Maximum nesting depth for input sanitization.
/// Deeper structures are truncated to `Value::Null`.
const MAX_NESTING_DEPTH: usize = 3;

/// Maximum string length for input sanitization.
/// Longer strings are truncated (not rejected).
const MAX_STRING_LEN: usize = 10_000;

/// The platform_api tool provides controlled access to platform operations.
///
/// It validates the action against an allowlist, sanitizes all input parameters,
/// and routes to the appropriate Platform trait method.
pub struct PlatformApiTool {
    platform: Arc<dyn Platform>,
}

impl PlatformApiTool {
    /// Create a new PlatformApiTool backed by the given platform adapter.
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
    }

    /// Execute a platform API action with the given parameters.
    ///
    /// # Errors
    ///
    /// - `PlatformError::UnknownAction` if the action is not in the allowlist.
    /// - Any `PlatformError` from the underlying platform operation.
    pub async fn execute(&self, action: &str, params: Value) -> Result<Value, PlatformError> {
        if !ALLOWED_ACTIONS.contains(&action) {
            return Err(PlatformError::UnknownAction(action.to_string()));
        }

        let sanitized = sanitize_value(params, 0);

        match action {
            "get_issue" => {
                let issue_id = extract_issue_id(&sanitized)?;
                let issue = self.platform.fetch_issue(issue_id).await?;
                Ok(serde_json::to_value(issue).unwrap_or(Value::Null))
            }
            "add_comment" => {
                let issue_id = extract_issue_id(&sanitized)?;
                let body = extract_string(&sanitized, "body")?;
                let comment_id = self.platform.create_comment(issue_id, &body).await?;
                Ok(serde_json::json!({ "comment_id": comment_id.0 }))
            }
            "create_pull_request" => {
                let pr_params = extract_pr_params(&sanitized)?;
                let pr = self.platform.create_pull_request(pr_params).await?;
                Ok(serde_json::to_value(pr).unwrap_or(Value::Null))
            }
            "list_pull_requests" | "get_pull_request_status" | "create_branch" => {
                // Phase 1 skeleton: not yet implemented
                Err(PlatformError::UnknownAction(format!(
                    "{} (not yet implemented)",
                    action
                )))
            }
            _ => Err(PlatformError::UnknownAction(action.to_string())),
        }
    }
}

/// Recursively sanitize a JSON value:
/// - Strings are truncated to `MAX_STRING_LEN` characters.
/// - Nesting beyond `MAX_NESTING_DEPTH` is replaced with `Value::Null`.
/// - Arrays have null elements filtered out (after sanitization).
/// - Numbers, booleans, and null pass through unchanged.
pub fn sanitize_value(value: Value, depth: usize) -> Value {
    if depth > MAX_NESTING_DEPTH {
        tracing::warn!(depth, "Sanitize: max nesting depth exceeded, truncating");
        return Value::Null;
    }
    match value {
        Value::String(s) => Value::String(s.chars().take(MAX_STRING_LEN).collect()),
        Value::Number(_) | Value::Bool(_) | Value::Null => value,
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|v| sanitize_value(v, depth + 1))
                .filter(|v| !v.is_null())
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, sanitize_value(v, depth + 1)))
                .collect(),
        ),
    }
}

/// Extract an `IssueId` from the `issue_id` field of a JSON object.
pub fn extract_issue_id(params: &Value) -> Result<IssueId, PlatformError> {
    params
        .get("issue_id")
        .and_then(|v| v.as_u64())
        .map(IssueId)
        .ok_or_else(|| PlatformError::Unprocessable("missing or invalid 'issue_id'".into()))
}

/// Extract a string field from a JSON object.
pub fn extract_string(params: &Value, key: &str) -> Result<String, PlatformError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| PlatformError::Unprocessable(format!("missing or invalid '{}'", key)))
}

/// Extract `CreatePrParams` from a JSON object.
pub fn extract_pr_params(params: &Value) -> Result<CreatePrParams, PlatformError> {
    Ok(CreatePrParams {
        title: extract_string(params, "title")?,
        body: extract_string(params, "body").unwrap_or_else(|_| String::new()),
        head: extract_string(params, "head")?,
        base: extract_string(params, "base")?,
        draft: params
            .get("draft")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_sanitize_truncates_long_strings() {
        let long_string = "a".repeat(20_000);
        let result = sanitize_value(Value::String(long_string), 0);
        if let Value::String(s) = result {
            assert_eq!(s.len(), MAX_STRING_LEN);
        } else {
            panic!("Expected string");
        }
    }

    #[test]
    fn test_sanitize_truncates_deep_nesting() {
        let deep = json!({"a": {"b": {"c": {"d": {"e": "too deep"}}}}});
        let result = sanitize_value(deep, 0);
        // At depth 3, the inner value should be Null
        let inner = result
            .get("a")
            .and_then(|v| v.get("b"))
            .and_then(|v| v.get("c"))
            .and_then(|v| v.get("d"));
        assert_eq!(inner, Some(&Value::Null));
    }

    #[test]
    fn test_sanitize_preserves_numbers_and_bools() {
        let input = json!({"count": 42, "active": true});
        let result = sanitize_value(input.clone(), 0);
        assert_eq!(result, input);
    }

    #[test]
    fn test_sanitize_filters_null_from_arrays() {
        // After sanitization at max depth, nulls are filtered from arrays
        let input = json!([1, null, "hello", null]);
        let result = sanitize_value(input, 0);
        assert_eq!(result, json!([1, "hello"]));
    }

    #[test]
    fn test_extract_issue_id_valid() {
        let params = json!({"issue_id": 42});
        let result = extract_issue_id(&params).unwrap();
        assert_eq!(result, IssueId(42));
    }

    #[test]
    fn test_extract_issue_id_missing() {
        let params = json!({"other": "value"});
        assert!(extract_issue_id(&params).is_err());
    }

    #[test]
    fn test_extract_issue_id_wrong_type() {
        let params = json!({"issue_id": "not a number"});
        assert!(extract_issue_id(&params).is_err());
    }

    #[test]
    fn test_extract_string_valid() {
        let params = json!({"body": "hello world"});
        let result = extract_string(&params, "body").unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_extract_string_missing() {
        let params = json!({"other": "value"});
        assert!(extract_string(&params, "body").is_err());
    }

    #[test]
    fn test_extract_pr_params_complete() {
        let params = json!({
            "title": "Fix bug",
            "body": "Description",
            "head": "feature-branch",
            "base": "main",
            "draft": true
        });
        let result = extract_pr_params(&params).unwrap();
        assert_eq!(result.title, "Fix bug");
        assert_eq!(result.body, "Description");
        assert_eq!(result.head, "feature-branch");
        assert_eq!(result.base, "main");
        assert!(result.draft);
    }

    #[test]
    fn test_extract_pr_params_minimal() {
        let params = json!({
            "title": "Fix bug",
            "head": "feature-branch",
            "base": "main"
        });
        let result = extract_pr_params(&params).unwrap();
        assert_eq!(result.title, "Fix bug");
        assert_eq!(result.body, ""); // defaults to empty
        assert!(!result.draft); // defaults to false
    }

    #[test]
    fn test_extract_pr_params_missing_required() {
        let params = json!({"title": "Fix bug"});
        assert!(extract_pr_params(&params).is_err()); // missing head
    }
}
