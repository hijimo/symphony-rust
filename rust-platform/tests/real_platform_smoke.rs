#![allow(
    unused_imports,
    unused_variables,
    dead_code,
    clippy::bind_instead_of_map,
    clippy::derivable_impls,
    clippy::manual_range_contains,
    clippy::needless_borrows_for_generic_args,
    clippy::ptr_arg,
    clippy::duplicated_attributes,
    clippy::approx_constant,
    clippy::bool_assert_comparison,
    clippy::len_zero,
    clippy::let_and_return
)]

//! Real Integration Tests — GitHub/Platform API smoke tests.
//!
//! These tests require a valid GITHUB_TOKEN environment variable and network
//! access to the GitHub API. They are marked with #[ignore] and should be run
//! explicitly with `cargo test --test real_platform_smoke -- --ignored`.
//!
//! Required environment:
//! - GITHUB_TOKEN: Valid GitHub token with repo access
//! - TEST_REPO_NAME: Repository in "owner/repo" format
//!
//! Run with: `cargo test --test real_platform_smoke -- --ignored`

use std::time::Duration;

/// Helper to get the GitHub token from environment.
fn get_github_token() -> Option<String> {
    dotenvy::dotenv().ok();
    std::env::var("GITHUB_TOKEN").ok()
}

/// Helper to get the test repo (owner/repo format).
fn get_test_repo() -> String {
    std::env::var("TEST_REPO_NAME").unwrap_or_else(|_| "anthropics/symphony".to_string())
}

/// Helper to split TEST_REPO_NAME into (owner, repo).
fn get_owner_and_repo() -> (String, String) {
    let full = get_test_repo();
    let parts: Vec<&str> = full.splitn(2, '/').collect();
    if parts.len() == 2 {
        (parts[0].to_string(), parts[1].to_string())
    } else {
        ("anthropics".to_string(), full)
    }
}

/// Helper to make a GitHub API request.
async fn github_api_request(token: &str, endpoint: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let url = format!("https://api.github.com{}", endpoint);

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "symphony-platform-test")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, body));
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

// ============================================================================
// Test: Fetch issues from a real GitHub repo
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_github_fetch_issues() {
    let token = get_github_token().expect("GITHUB_TOKEN required");
    let (owner, repo) = get_owner_and_repo();

    let endpoint = format!("/repos/{}/{}/issues?state=open&per_page=10", owner, repo);
    let result = github_api_request(&token, &endpoint).await;

    match result {
        Ok(data) => {
            let issues = data.as_array().expect("Expected array of issues");
            println!(
                "Fetched {} open issues from {}/{}",
                issues.len(),
                owner,
                repo
            );

            for issue in issues.iter().take(5) {
                let number = issue["number"].as_u64().unwrap_or(0);
                let title = issue["title"].as_str().unwrap_or("?");
                let labels: Vec<&str> = issue["labels"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|l| l["name"].as_str()).collect())
                    .unwrap_or_default();
                println!("  #{}: {} [labels: {:?}]", number, title, labels);
            }
        }
        Err(e) => panic!("GitHub API request failed: {}", e),
    }
}

// ============================================================================
// Test: Verify label-based state detection
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_github_label_based_state_detection() {
    let token = get_github_token().expect("GITHUB_TOKEN required");
    let (owner, repo) = get_owner_and_repo();

    let endpoint = format!("/repos/{}/{}/issues?state=open&per_page=50", owner, repo);
    let result = github_api_request(&token, &endpoint)
        .await
        .expect("Failed to fetch issues");

    let issues = result.as_array().expect("Expected array");

    let workflow_prefix = "workflow::";
    let mut issues_with_state = 0;
    let mut state_distribution: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for issue in issues {
        if let Some(labels) = issue["labels"].as_array() {
            for label in labels {
                if let Some(name) = label["name"].as_str() {
                    if name.starts_with(workflow_prefix) {
                        issues_with_state += 1;
                        let state = name.strip_prefix(workflow_prefix).unwrap_or(name);
                        *state_distribution.entry(state.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    println!(
        "Issues with workflow labels: {}/{}",
        issues_with_state,
        issues.len()
    );
    println!("State distribution: {:?}", state_distribution);
}

// ============================================================================
// Test: Verify issue normalization
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_github_issue_normalization() {
    let token = get_github_token().expect("GITHUB_TOKEN required");
    let (owner, repo) = get_owner_and_repo();

    let endpoint = format!("/repos/{}/{}/issues?state=open&per_page=5", owner, repo);
    let result = github_api_request(&token, &endpoint)
        .await
        .expect("Failed to fetch issues");

    let issues = result.as_array().expect("Expected array");

    if issues.is_empty() {
        println!("SKIPPED: No open issues found in {}/{}", owner, repo);
        return;
    }

    for issue in issues {
        // Verify required fields
        assert!(issue["id"].is_number(), "Issue should have numeric id");
        assert!(issue["number"].is_number(), "Issue should have number");
        assert!(issue["title"].is_string(), "Issue should have title");
        assert!(issue["html_url"].is_string(), "Issue should have html_url");

        let id = issue["number"].as_u64().unwrap();
        let title = issue["title"].as_str().unwrap();
        let url = issue["html_url"].as_str().unwrap();
        let assignee = issue["assignee"]["login"].as_str();
        let labels: Vec<String> = issue["labels"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l["name"].as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let workflow_state = labels.iter().find(|l| l.starts_with("workflow::")).cloned();

        let branch_name = format!(
            "symphony/{}-{}",
            id,
            title
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .take(40)
                .collect::<String>()
                .trim_end_matches('-')
        );

        println!(
            "  #{}: title={}, state={:?}, branch={}",
            id, title, workflow_state, branch_name
        );

        assert!(!title.is_empty());
        assert!(url.starts_with("https://"));
        assert!(branch_name.starts_with("symphony/"));
    }
}

// ============================================================================
// Test: GitHub API authentication validation
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_github_token_validation() {
    let token = get_github_token().expect("GITHUB_TOKEN required");

    let result = github_api_request(&token, "/user").await;

    match result {
        Ok(user) => {
            println!(
                "Authenticated as: {} ({})",
                user["login"].as_str().unwrap_or("?"),
                user["name"].as_str().unwrap_or("?")
            );
            assert!(user["login"].is_string());
        }
        Err(e) => panic!("Token validation failed: {}", e),
    }
}

// ============================================================================
// Test: GitHub rate limit awareness
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_github_rate_limit_check() {
    let token = get_github_token().expect("GITHUB_TOKEN required");

    let result = github_api_request(&token, "/rate_limit").await;

    match result {
        Ok(data) => {
            let core = &data["resources"]["core"];
            let remaining = core["remaining"].as_u64().unwrap_or(0);
            let limit = core["limit"].as_u64().unwrap_or(0);
            println!("Rate limit: {}/{}", remaining, limit);
            assert!(
                remaining > 10,
                "Rate limit too low: {}/{}",
                remaining,
                limit
            );
        }
        Err(e) => panic!("Rate limit check failed: {}", e),
    }
}

// ============================================================================
// Test: Fetch repository labels
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_github_fetch_labels() {
    let token = get_github_token().expect("GITHUB_TOKEN required");
    let (owner, repo) = get_owner_and_repo();

    let endpoint = format!("/repos/{}/{}/labels?per_page=100", owner, repo);
    let result = github_api_request(&token, &endpoint).await;

    match result {
        Ok(data) => {
            let labels = data.as_array().expect("Expected array of labels");
            println!("Repository has {} labels", labels.len());

            let workflow_labels: Vec<&str> = labels
                .iter()
                .filter_map(|l| l["name"].as_str())
                .filter(|name| name.starts_with("workflow::"))
                .collect();
            println!("  Workflow labels: {:?}", workflow_labels);
        }
        Err(e) => println!("Note: Could not fetch labels: {}", e),
    }
}
