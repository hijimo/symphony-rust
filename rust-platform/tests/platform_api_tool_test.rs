//! Tests for the `platform_api` tool module.
//!
//! Verifies action validation, input sanitization, and correct routing
//! of tool calls to the underlying Platform trait methods.

use std::sync::Arc;

use serde_json::json;

use symphony_platform::error::PlatformError;
use symphony_platform::platform::{make_test_issue, IssueId, MemoryAdapter, Platform};
use symphony_platform::tools::PlatformApiTool;

/// Test: Unknown actions are rejected with UnknownAction error.
#[tokio::test]
async fn test_unknown_action_rejected() {
    let adapter = Arc::new(MemoryAdapter::new());
    let tool = PlatformApiTool::new(adapter as Arc<dyn Platform>);

    let result = tool.execute("delete_everything", json!({})).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        PlatformError::UnknownAction(action) => {
            assert_eq!(action, "delete_everything");
        }
        other => panic!("Expected UnknownAction, got: {:?}", other),
    }
}

/// Test: Actions not in the allowlist are rejected.
#[tokio::test]
async fn test_disallowed_actions_rejected() {
    let adapter = Arc::new(MemoryAdapter::new());
    let tool = PlatformApiTool::new(adapter as Arc<dyn Platform>);

    let disallowed = vec![
        "delete_issue",
        "admin_reset",
        "execute_shell",
        "DROP TABLE",
        "",
    ];

    for action in disallowed {
        let result = tool.execute(action, json!({})).await;
        assert!(result.is_err(), "Action '{}' should be rejected", action);
    }
}

/// Test: Sanitize truncates deeply nested structures.
///
/// Input with nesting deeper than MAX_NESTING_DEPTH (3) should have
/// deep values replaced with null (and filtered from arrays).
#[tokio::test]
async fn test_sanitize_truncates_deep_nesting() {
    use symphony_platform::tools::sanitize_value;

    // Create a deeply nested structure (depth 5)
    let deep = json!({
        "level1": {
            "level2": {
                "level3": {
                    "level4": {
                        "level5": "should be truncated"
                    }
                }
            }
        }
    });

    let sanitized = sanitize_value(deep, 0);

    // Level 1-3 should exist
    assert!(sanitized.get("level1").is_some());
    let l1 = sanitized.get("level1").unwrap();
    assert!(l1.get("level2").is_some());
    let l2 = l1.get("level2").unwrap();
    assert!(l2.get("level3").is_some());
    let l3 = l2.get("level3").unwrap();

    // Level 4 should be null (depth exceeded at depth > 3)
    let l4 = l3.get("level4");
    assert!(
        l4.is_none() || l4.unwrap().is_null(),
        "Level 4+ should be truncated to null"
    );
}

/// Test: Sanitize truncates long strings.
///
/// Strings longer than MAX_STRING_LEN (10,000) should be truncated.
#[tokio::test]
async fn test_sanitize_truncates_long_strings() {
    use symphony_platform::tools::sanitize_value;

    let long_string = "x".repeat(20_000);
    let input = json!({
        "body": long_string,
        "short": "hello"
    });

    let sanitized = sanitize_value(input, 0);

    let body = sanitized.get("body").unwrap().as_str().unwrap();
    assert_eq!(
        body.len(),
        10_000,
        "Long string should be truncated to MAX_STRING_LEN"
    );

    let short = sanitized.get("short").unwrap().as_str().unwrap();
    assert_eq!(short, "hello", "Short strings should be unchanged");
}

/// Test: get_issue routes correctly to Platform::fetch_issue.
#[tokio::test]
async fn test_get_issue_routes_correctly() {
    let adapter = Arc::new(MemoryAdapter::new());
    adapter
        .seed_issue(make_test_issue(42, "Test routing", Some("workflow::todo")))
        .await;

    let tool = PlatformApiTool::new(adapter.clone() as Arc<dyn Platform>);

    let result = tool
        .execute("get_issue", json!({ "issue_id": 42 }))
        .await
        .unwrap();

    // Should return serialized issue
    assert_eq!(
        result.get("title").unwrap().as_str().unwrap(),
        "Test routing"
    );
    assert_eq!(result.get("number").unwrap().as_u64().unwrap(), 42);

    // Verify the platform method was called
    assert_eq!(adapter.call_count("fetch_issue").await, 1);
}

/// Test: add_comment routes correctly to Platform::create_comment.
#[tokio::test]
async fn test_add_comment_routes_correctly() {
    let adapter = Arc::new(MemoryAdapter::new());
    adapter
        .seed_issue(make_test_issue(
            10,
            "Comment target",
            Some("workflow::todo"),
        ))
        .await;

    let tool = PlatformApiTool::new(adapter.clone() as Arc<dyn Platform>);

    let result = tool
        .execute(
            "add_comment",
            json!({
                "issue_id": 10,
                "body": "Hello from the tool!"
            }),
        )
        .await
        .unwrap();

    // Should return comment_id
    assert!(result.get("comment_id").is_some());
    let comment_id = result.get("comment_id").unwrap().as_u64().unwrap();
    assert!(comment_id > 0);

    // Verify the comment was actually created
    let comments = adapter.list_comments(IssueId(10)).await.unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].body, "Hello from the tool!");

    // Verify call count
    assert_eq!(adapter.call_count("create_comment").await, 1);
}

/// Test: get_issue with missing issue_id returns Unprocessable error.
#[tokio::test]
async fn test_get_issue_missing_id() {
    let adapter = Arc::new(MemoryAdapter::new());
    let tool = PlatformApiTool::new(adapter as Arc<dyn Platform>);

    let result = tool.execute("get_issue", json!({})).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        PlatformError::Unprocessable(msg) => {
            assert!(msg.contains("issue_id"));
        }
        other => panic!("Expected Unprocessable, got: {:?}", other),
    }
}

/// Test: add_comment with missing body returns Unprocessable error.
#[tokio::test]
async fn test_add_comment_missing_body() {
    let adapter = Arc::new(MemoryAdapter::new());
    adapter.seed_issue(make_test_issue(1, "Test", None)).await;
    let tool = PlatformApiTool::new(adapter as Arc<dyn Platform>);

    let result = tool.execute("add_comment", json!({ "issue_id": 1 })).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        PlatformError::Unprocessable(msg) => {
            assert!(msg.contains("body"));
        }
        other => panic!("Expected Unprocessable, got: {:?}", other),
    }
}

/// Test: create_pull_request routes correctly.
#[tokio::test]
async fn test_create_pull_request_routes_correctly() {
    let adapter = Arc::new(MemoryAdapter::new());
    let tool = PlatformApiTool::new(adapter.clone() as Arc<dyn Platform>);

    let result = tool
        .execute(
            "create_pull_request",
            json!({
                "title": "feat: add new feature",
                "body": "This PR adds...",
                "head": "feature-branch",
                "base": "main",
                "draft": true
            }),
        )
        .await
        .unwrap();

    // Should return PR info
    assert!(result.get("number").is_some());
    assert!(result.get("url").is_some());
    assert_eq!(result.get("state").unwrap().as_str().unwrap(), "draft");

    assert_eq!(adapter.call_count("create_pull_request").await, 1);
}

/// Test: Sanitization preserves numbers and booleans.
#[tokio::test]
async fn test_sanitize_preserves_primitives() {
    use symphony_platform::tools::sanitize_value;

    let input = json!({
        "count": 42,
        "enabled": true,
        "ratio": 3.125,
        "nothing": null
    });

    let sanitized = sanitize_value(input, 0);

    assert_eq!(sanitized.get("count").unwrap().as_u64().unwrap(), 42);
    assert!(sanitized.get("enabled").unwrap().as_bool().unwrap());
    assert_eq!(sanitized.get("ratio").unwrap().as_f64().unwrap(), 3.125);
    // null is preserved in objects (only filtered from arrays)
    assert!(sanitized.get("nothing").unwrap().is_null());
}

/// Test: Sanitization filters null from arrays.
#[tokio::test]
async fn test_sanitize_filters_null_from_arrays() {
    use symphony_platform::tools::sanitize_value;

    // Create array with elements that will become null at depth limit
    let input = json!({
        "items": [
            "keep",
            {"nested": {"deep": {"too_deep": "gone"}}},
            "also_keep"
        ]
    });

    let sanitized = sanitize_value(input, 0);
    let items = sanitized.get("items").unwrap().as_array().unwrap();

    // "keep" and "also_keep" should survive
    assert!(items.iter().any(|v| v.as_str() == Some("keep")));
    assert!(items.iter().any(|v| v.as_str() == Some("also_keep")));
}
