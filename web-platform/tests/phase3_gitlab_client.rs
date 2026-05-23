//! Phase 3 integration tests: GitLab client against real GitLab instance.
//!
//! These tests require the following environment:
//! - GITLAB_HOST=http://gitlab.jushuitan-inc.com:8081
//! - GITLAB_TOKEN=<your-gitlab-token>
//! - GITLAB_PROJECT=zimei10525/symphony_e2e_test_repo

use web_platform::models::kanban::CreateIssueRequest;
use web_platform::services::git_platform::{GitPlatformClient, ListIssuesOptions};
use web_platform::services::gitlab_client::GitLabClient;

const GITLAB_HOST: &str = "http://gitlab.jushuitan-inc.com:8081";
const GITLAB_PROJECT: &str = "zimei10525/symphony_e2e_test_repo";

fn gitlab_token() -> &'static str {
    static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TOKEN
        .get_or_init(|| {
            std::env::var("GITLAB_TOKEN")
                .expect("GITLAB_TOKEN must be set for Phase 3 GitLab client integration tests")
        })
        .as_str()
}

#[tokio::test]
async fn test_gitlab_list_issues() {
    let client = GitLabClient::new(GITLAB_HOST.to_string());

    let options = ListIssuesOptions {
        state: Some("opened".to_string()),
        limit: 10,
        ..Default::default()
    };

    let result = client
        .list_issues(gitlab_token(), GITLAB_PROJECT, &options)
        .await;
    match result {
        Ok((issues, total_count)) => {
            println!("Found {} issues (total: {})", issues.len(), total_count);
            assert!(issues.len() <= 10);
            for issue in &issues {
                println!("  #{}: {} [{}]", issue.iid, issue.title, issue.state);
                assert_eq!(issue.state, "opened");
            }
        }
        Err(e) => {
            panic!("Failed to list issues: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_gitlab_list_issues_with_label_filter() {
    let client = GitLabClient::new(GITLAB_HOST.to_string());

    let options = ListIssuesOptions {
        state: Some("opened".to_string()),
        labels: Some(vec!["bug".to_string()]),
        limit: 50,
        ..Default::default()
    };

    let result = client
        .list_issues(gitlab_token(), GITLAB_PROJECT, &options)
        .await;
    match result {
        Ok((issues, _total)) => {
            println!("Found {} issues with 'bug' label", issues.len());
            for issue in &issues {
                assert!(
                    issue.labels.contains(&"bug".to_string()),
                    "Issue #{} missing 'bug' label, has: {:?}",
                    issue.iid,
                    issue.labels
                );
            }
        }
        Err(e) => {
            panic!("Failed to list issues with label filter: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_gitlab_list_issues_exclude_label() {
    let client = GitLabClient::new(GITLAB_HOST.to_string());

    let options = ListIssuesOptions {
        state: Some("opened".to_string()),
        exclude_labels: Some(vec!["symphony-claimed".to_string()]),
        limit: 50,
        ..Default::default()
    };

    let result = client
        .list_issues(gitlab_token(), GITLAB_PROJECT, &options)
        .await;
    match result {
        Ok((issues, _total)) => {
            println!(
                "Found {} issues without 'symphony-claimed' label",
                issues.len()
            );
            for issue in &issues {
                assert!(
                    !issue.labels.contains(&"symphony-claimed".to_string()),
                    "Issue #{} should not have 'symphony-claimed' label",
                    issue.iid
                );
            }
        }
        Err(e) => {
            panic!("Failed to list issues with exclude filter: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_gitlab_create_and_get_issue() {
    let client = GitLabClient::new(GITLAB_HOST.to_string());

    // Create a test issue
    let req = CreateIssueRequest {
        title: format!("[E2E Test] Phase 3 test issue - {}", chrono::Utc::now().format("%Y%m%d%H%M%S")),
        description: Some("## 描述\n\n这是一个自动化测试创建的 Issue。\n\n## Acceptance Criteria\n\n- [ ] 测试通过\n\n## Validation\n\n- [ ] `cargo test`".to_string()),
        labels: vec!["test".to_string()],
        assignee: None,
    };

    let created = client
        .create_issue(gitlab_token(), GITLAB_PROJECT, &req)
        .await;
    match created {
        Ok(issue) => {
            println!("Created issue #{}: {}", issue.iid, issue.title);
            assert_eq!(issue.title, req.title);
            assert_eq!(issue.state, "opened");
            assert!(issue.labels.contains(&"test".to_string()));
            assert!(issue.web_url.contains("gitlab"));

            // Now fetch the same issue by iid
            let fetched = client
                .get_issue(gitlab_token(), GITLAB_PROJECT, issue.iid)
                .await;
            match fetched {
                Ok(fetched_issue) => {
                    assert_eq!(fetched_issue.iid, issue.iid);
                    assert_eq!(fetched_issue.title, issue.title);
                    println!("Successfully fetched issue #{}", fetched_issue.iid);
                }
                Err(e) => {
                    panic!("Failed to get issue #{}: {:?}", issue.iid, e);
                }
            }
        }
        Err(e) => {
            panic!("Failed to create issue: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_gitlab_get_issue_not_found() {
    let client = GitLabClient::new(GITLAB_HOST.to_string());

    let result = client
        .get_issue(gitlab_token(), GITLAB_PROJECT, 99999)
        .await;
    assert!(
        result.is_err(),
        "Should return error for non-existent issue"
    );
    if let Err(e) = result {
        println!("Expected error for non-existent issue: {:?}", e);
    }
}

#[tokio::test]
async fn test_gitlab_invalid_token() {
    let client = GitLabClient::new(GITLAB_HOST.to_string());

    let options = ListIssuesOptions {
        state: Some("opened".to_string()),
        limit: 10,
        ..Default::default()
    };

    let result = client
        .list_issues("invalid-token-xxx", GITLAB_PROJECT, &options)
        .await;
    assert!(result.is_err(), "Should return error for invalid token");
    if let Err(e) = result {
        println!("Expected error for invalid token: {:?}", e);
        // Should be a TokenInvalid error
        let err_str = format!("{:?}", e);
        assert!(
            err_str.contains("TokenInvalid") || err_str.contains("401") || err_str.contains("403"),
            "Error should indicate token is invalid: {}",
            err_str
        );
    }
}

#[tokio::test]
async fn test_gitlab_get_issue_merge_requests() {
    let client = GitLabClient::new(GITLAB_HOST.to_string());

    // First, find an issue that might have MRs
    let options = ListIssuesOptions {
        state: Some("all".to_string()),
        limit: 5,
        ..Default::default()
    };

    let (issues, _) = client
        .list_issues(gitlab_token(), GITLAB_PROJECT, &options)
        .await
        .expect("Failed to list issues");

    if issues.is_empty() {
        println!("No issues found, skipping MR test");
        return;
    }

    // Try to get MRs for the first issue
    let issue = &issues[0];
    let result = client
        .get_issue_merge_requests(gitlab_token(), GITLAB_PROJECT, issue.iid)
        .await;
    match result {
        Ok(mrs) => {
            println!("Issue #{} has {} related MRs", issue.iid, mrs.len());
            for mr in &mrs {
                println!("  MR !{}: {} [{}]", mr.iid, mr.title, mr.state);
            }
        }
        Err(e) => {
            panic!("Failed to get MRs for issue #{}: {:?}", issue.iid, e);
        }
    }
}

#[tokio::test]
async fn test_gitlab_search_issues() {
    let client = GitLabClient::new(GITLAB_HOST.to_string());

    let options = ListIssuesOptions {
        state: Some("all".to_string()),
        search: Some("test".to_string()),
        limit: 10,
        ..Default::default()
    };

    let result = client
        .list_issues(gitlab_token(), GITLAB_PROJECT, &options)
        .await;
    match result {
        Ok((issues, total)) => {
            println!(
                "Search 'test': found {} issues (total: {})",
                issues.len(),
                total
            );
        }
        Err(e) => {
            panic!("Failed to search issues: {:?}", e);
        }
    }
}
