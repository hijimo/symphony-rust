//! Real Integration Tests — Linear API smoke tests.
//!
//! These tests require a valid LINEAR_API_KEY environment variable and
//! network access to the Linear API. They are marked with #[ignore] and
//! should be run explicitly with `cargo test --test real_linear_smoke -- --ignored`.
//!
//! Required environment:
//! - LINEAR_API_KEY: Valid Linear API key with read access
//! - LINEAR_TEST_PROJECT (optional): Project slug to test against
//!
//! Run with: `cargo test --test real_linear_smoke -- --ignored`

use std::time::Duration;

/// Helper to get the Linear API key from environment.
fn get_linear_api_key() -> Option<String> {
    std::env::var("LINEAR_API_KEY").ok()
}

/// Helper to get the test project slug.
fn get_test_project() -> String {
    std::env::var("LINEAR_TEST_PROJECT").unwrap_or_else(|_| "symphony-test".to_string())
}

/// Helper to make a GraphQL request to Linear.
async fn linear_graphql_request(
    api_key: &str,
    query: &str,
    variables: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let mut body = serde_json::json!({"query": query});
    if let Some(vars) = variables {
        body["variables"] = vars;
    }

    let resp = client
        .post("https://api.linear.app/graphql")
        .header("Authorization", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "HTTP {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

// ============================================================================
// Test: Fetch candidate issues from real Linear project
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_linear_fetch_candidate_issues() {
    let api_key = get_linear_api_key().expect("LINEAR_API_KEY required for this test");
    let project = get_test_project();

    let query = r#"
        query($projectSlug: String!) {
            issues(
                filter: {
                    project: { slugId: { eq: $projectSlug } }
                    state: { type: { in: ["started", "unstarted"] } }
                }
                first: 50
            ) {
                nodes {
                    id
                    identifier
                    title
                    description
                    state { name type }
                    priority
                    labels { nodes { name } }
                    assignee { name }
                    branchName
                    createdAt
                    updatedAt
                }
                pageInfo { hasNextPage endCursor }
            }
        }
    "#;

    let variables = serde_json::json!({"projectSlug": project});
    let result = linear_graphql_request(&api_key, query, Some(variables)).await;

    match result {
        Ok(data) => {
            assert!(
                data["data"]["issues"]["nodes"].is_array(),
                "Expected issues.nodes array, got: {:?}",
                data
            );
            let nodes = data["data"]["issues"]["nodes"].as_array().unwrap();
            println!(
                "Fetched {} candidate issues from '{}'",
                nodes.len(),
                project
            );

            if let Some(first) = nodes.first() {
                assert!(first["id"].is_string());
                assert!(first["identifier"].is_string());
                assert!(first["title"].is_string());
                assert!(first["state"]["name"].is_string());
            }
        }
        Err(e) => panic!("Linear API request failed: {}", e),
    }
}

// ============================================================================
// Test: Fetch issue states by IDs
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_linear_fetch_issue_states_by_ids() {
    let api_key = get_linear_api_key().expect("LINEAR_API_KEY required for this test");

    let list_query = r#"
        query { issues(first: 3) { nodes { id identifier state { name type } } } }
    "#;

    let list_result = linear_graphql_request(&api_key, list_query, None)
        .await
        .expect("Failed to list issues");

    let nodes = list_result["data"]["issues"]["nodes"]
        .as_array()
        .expect("Expected issues array");

    if nodes.is_empty() {
        println!("SKIPPED: No issues found in Linear workspace");
        return;
    }

    let ids: Vec<&str> = nodes.iter().filter_map(|n| n["id"].as_str()).collect();

    let fetch_query = r#"
        query($ids: [String!]!) {
            issues(filter: { id: { in: $ids } }) {
                nodes { id identifier title state { name type } priority updatedAt }
            }
        }
    "#;

    let variables = serde_json::json!({"ids": ids});
    let result = linear_graphql_request(&api_key, fetch_query, Some(variables))
        .await
        .expect("Failed to fetch issues by ID");

    let fetched = result["data"]["issues"]["nodes"].as_array().unwrap();
    assert_eq!(fetched.len(), ids.len());

    for issue in fetched {
        assert!(issue["state"]["name"].is_string());
        println!(
            "  {}: state={} ({})",
            issue["identifier"], issue["state"]["name"], issue["state"]["type"]
        );
    }
}

// ============================================================================
// Test: Verify normalization (labels, priority, blockers)
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_linear_verify_normalization() {
    let api_key = get_linear_api_key().expect("LINEAR_API_KEY required for this test");

    let query = r#"
        query {
            issues(first: 10) {
                nodes {
                    id identifier title priority
                    labels { nodes { name } }
                    relations { nodes { type relatedIssue { id identifier } } }
                }
            }
        }
    "#;

    let result = linear_graphql_request(&api_key, query, None)
        .await
        .expect("Failed to fetch issues");

    let nodes = result["data"]["issues"]["nodes"].as_array().unwrap();

    if nodes.is_empty() {
        println!("SKIPPED: No issues found");
        return;
    }

    for issue in nodes {
        let identifier = issue["identifier"].as_str().unwrap_or("?");

        // Labels should be normalizable to lowercase
        if let Some(labels) = issue["labels"]["nodes"].as_array() {
            let normalized: Vec<String> = labels
                .iter()
                .filter_map(|l| l["name"].as_str())
                .map(|s| s.to_lowercase())
                .collect();
            println!("  {} labels: {:?}", identifier, normalized);
        }

        // Priority should be in range [0, 4]
        if let Some(priority) = issue["priority"].as_f64() {
            assert!(priority >= 0.0 && priority <= 4.0);
        }

        // Relations/blockers
        if let Some(relations) = issue["relations"]["nodes"].as_array() {
            let blockers: Vec<&str> = relations
                .iter()
                .filter(|r| r["type"].as_str() == Some("blocks"))
                .filter_map(|r| r["relatedIssue"]["identifier"].as_str())
                .collect();
            if !blockers.is_empty() {
                println!("  {} blocked by: {:?}", identifier, blockers);
            }
        }
    }
}

// ============================================================================
// Test: Pagination for projects with >50 issues
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_linear_pagination() {
    let api_key = get_linear_api_key().expect("LINEAR_API_KEY required for this test");

    let mut all_issues = Vec::new();
    let mut cursor: Option<String> = None;
    let page_size = 10;
    let max_pages = 5;

    for page in 0..max_pages {
        let query = if let Some(ref c) = cursor {
            format!(
                r#"query {{ issues(first: {}, after: "{}") {{ nodes {{ id identifier }} pageInfo {{ hasNextPage endCursor }} }} }}"#,
                page_size, c
            )
        } else {
            format!(
                r#"query {{ issues(first: {}) {{ nodes {{ id identifier }} pageInfo {{ hasNextPage endCursor }} }} }}"#,
                page_size
            )
        };

        let result = linear_graphql_request(&api_key, &query, None)
            .await
            .expect("Pagination request failed");

        let nodes = result["data"]["issues"]["nodes"].as_array().unwrap();
        let has_next = result["data"]["issues"]["pageInfo"]["hasNextPage"]
            .as_bool()
            .unwrap_or(false);

        println!(
            "  Page {}: {} issues, hasNextPage={}",
            page + 1,
            nodes.len(),
            has_next
        );
        all_issues.extend(nodes.iter().cloned());

        if !has_next {
            break;
        }
        cursor = result["data"]["issues"]["pageInfo"]["endCursor"]
            .as_str()
            .map(|s| s.to_string());
    }

    println!("Total fetched: {}", all_issues.len());

    // Verify no duplicates
    let ids: Vec<&str> = all_issues.iter().filter_map(|i| i["id"].as_str()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "Pagination returned duplicates");
}

// ============================================================================
// Test: API key validation
// ============================================================================

#[tokio::test]
#[ignore]
async fn real_linear_api_key_validation() {
    let api_key = get_linear_api_key().expect("LINEAR_API_KEY required for this test");

    let query = r#"query { viewer { id name email } }"#;
    let result = linear_graphql_request(&api_key, query, None)
        .await
        .expect("API key validation failed");

    assert!(result["data"]["viewer"]["id"].is_string());
    println!(
        "Authenticated as: {} ({})",
        result["data"]["viewer"]["name"].as_str().unwrap_or("?"),
        result["data"]["viewer"]["email"].as_str().unwrap_or("?")
    );
}
