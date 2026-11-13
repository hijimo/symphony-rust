//! PR/MR lookup by branch and PR comment upsert (P2 design §3 F2).
//!
//! Each platform adapter is exercised against wiremock; the memory adapter is
//! covered with the same behavioural cases.

use std::collections::HashMap;

use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use symphony_platform::config::platform::{IssueFilter, PlatformConfig, WorkflowConfig};
use symphony_platform::error::PlatformError;
use symphony_platform::platform::gitea::GiteaAdapter;
use symphony_platform::platform::github::GithubAdapter;
use symphony_platform::platform::gitlab::GitlabAdapter;
use symphony_platform::platform::{CommentMode, MemoryAdapter, Platform, PullRequestRef};

const MARKER: &str = "<!-- symphony:plan-check -->";

fn config(kind: &str, base_url: &str) -> PlatformConfig {
    let mut states = HashMap::new();
    states.insert("todo".to_string(), "Todo".to_string());
    PlatformConfig {
        kind: kind.to_string(),
        api_token: "unused".to_string(),
        base_url: base_url.to_string(),
        owner: "acme".to_string(),
        repo: "app".to_string(),
        project_id: Some("acme/app".to_string()),
        allow_custom_host: true,
        issue_filter: IssueFilter {
            labels: vec!["Todo".to_string()],
            assignee: None,
            milestone: None,
        },
        workflow: WorkflowConfig {
            states,
            active_states: vec!["todo".to_string()],
            terminal_states: vec!["done".to_string()],
        },
    }
}

fn pr_ref(slug: &str, number: u64) -> PullRequestRef {
    PullRequestRef {
        repo_slug: slug.to_string(),
        number,
        url: format!("https://example.com/{slug}/pull/{number}"),
    }
}

// =============================================================================
// GitHub
// =============================================================================

#[tokio::test]
async fn test_github_find_open_pull_request_filters_by_owner_and_branch() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/web/pulls"))
        .and(query_param("state", "open"))
        .and(query_param("head", "acme:symphony/issue-7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 900, "number": 12, "html_url": "https://github.com/acme/web/pull/12", "state": "open"}
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    let pr = adapter
        .find_open_pull_request("acme/web", "symphony/issue-7")
        .await
        .unwrap()
        .expect("open PR");
    assert_eq!(pr.repo_slug, "acme/web");
    assert_eq!(pr.number, 12);
    assert_eq!(pr.url, "https://github.com/acme/web/pull/12");
}

#[tokio::test]
async fn test_github_find_open_pull_request_returns_none_when_absent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    let pr = adapter
        .find_open_pull_request("acme/app", "feature/x")
        .await
        .unwrap();
    assert!(pr.is_none());
}

#[tokio::test]
async fn test_github_rejects_malformed_repo_slug() {
    let server = MockServer::start().await;
    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    for bad in ["acme", "acme/app/extra", "../etc/passwd", "acme/ap p", ""] {
        let err = adapter
            .find_open_pull_request(bad, "main")
            .await
            .expect_err(bad);
        assert!(
            matches!(err, PlatformError::Unprocessable(_)),
            "{bad}: {err:?}"
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_github_upsert_updates_existing_marker_comment() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/issues/12/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "body": "unrelated", "user": {"login": "a"}, "created_at": "2026-11-01T00:00:00Z"},
            {"id": 2, "body": format!("old\n{MARKER}"), "user": {"login": "bot"}, "created_at": "2026-11-02T00:00:00Z"}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/repos/acme/app/issues/comments/2"))
        .and(body_json(json!({"body": format!("new\n{MARKER}")})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 2})))
        .expect(1)
        .mount(&server)
        .await;

    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    let id = adapter
        .upsert_pull_request_comment(
            &pr_ref("acme/app", 12),
            MARKER,
            &format!("new\n{MARKER}"),
            CommentMode::Upsert,
        )
        .await
        .unwrap();
    assert_eq!(id.0, 2);
}

#[tokio::test]
async fn test_github_upsert_creates_comment_and_appends_missing_marker() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/issues/12/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/app/issues/12/comments"))
        .and(body_json(json!({"body": format!("report\n\n{MARKER}")})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 77, "body": "x", "user": {"login": "bot"}, "created_at": "2026-11-02T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    let id = adapter
        .upsert_pull_request_comment(
            &pr_ref("acme/app", 12),
            MARKER,
            "report",
            CommentMode::Upsert,
        )
        .await
        .unwrap();
    assert_eq!(id.0, 77);
}

#[tokio::test]
async fn test_github_append_always_creates_new_comment() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/app/issues/12/comments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 78, "body": "x", "user": {"login": "bot"}, "created_at": "2026-11-02T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    adapter
        .upsert_pull_request_comment(
            &pr_ref("acme/app", 12),
            MARKER,
            &format!("round 2\n{MARKER}"),
            CommentMode::Append,
        )
        .await
        .unwrap();
    // No GET on comments: Append must not look for an existing comment.
    let gets = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == "GET")
        .count();
    assert_eq!(gets, 0);
}

#[tokio::test]
async fn test_github_list_pull_request_comments() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/issues/12/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 5, "body": "hello", "user": {"login": "rev"}, "created_at": "2026-11-02T00:00:00Z"}
        ])))
        .mount(&server)
        .await;

    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    let comments = adapter
        .list_pull_request_comments(&pr_ref("acme/app", 12))
        .await
        .unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].body, "hello");
    assert_eq!(comments[0].author, "rev");
}

// =============================================================================
// GitLab
// =============================================================================

#[tokio::test]
async fn test_gitlab_error_status_maps_to_platform_error_with_truncated_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/projects/acme%2Fapp/merge_requests"))
        .respond_with(ResponseTemplate::new(403).set_body_string("x".repeat(2000)))
        .mount(&server)
        .await;

    let adapter = GitlabAdapter::new_with_token(config("gitlab", &server.uri()), "t").unwrap();
    let err = adapter
        .find_open_pull_request("acme/app", "feat")
        .await
        .expect_err("403 must fail");
    match err {
        PlatformError::Forbidden(body) => assert!(body.len() <= 500, "body not truncated"),
        other => panic!("expected Forbidden, got {other:?}"),
    }
}

#[tokio::test]
async fn test_gitlab_find_open_merge_request_by_source_branch() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/projects/group%2Fsub%2Fbackend/merge_requests"))
        .and(query_param("state", "opened"))
        .and(query_param("source_branch", "symphony/app-7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 3001, "iid": 4, "web_url": "https://gitlab.example.com/group/sub/backend/-/merge_requests/4", "state": "opened"}
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let adapter = GitlabAdapter::new_with_token(config("gitlab", &server.uri()), "t").unwrap();
    let mr = adapter
        .find_open_pull_request("group/sub/backend", "symphony/app-7")
        .await
        .unwrap()
        .expect("open MR");
    assert_eq!(mr.repo_slug, "group/sub/backend");
    assert_eq!(mr.number, 4);
}

#[tokio::test]
async fn test_gitlab_upsert_updates_mr_note_and_skips_system_notes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/projects/acme%2Fapp/merge_requests/4/notes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 10, "body": format!("system mention {MARKER}"), "author": {"username": "root"}, "created_at": "2026-11-01T00:00:00Z", "system": true},
            {"id": 11, "body": format!("old {MARKER}"), "author": {"username": "bot"}, "created_at": "2026-11-02T00:00:00Z", "system": false}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/projects/acme%2Fapp/merge_requests/4/notes/11"))
        .and(body_json(json!({"body": format!("new {MARKER}")})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 11})))
        .expect(1)
        .mount(&server)
        .await;

    let adapter = GitlabAdapter::new_with_token(config("gitlab", &server.uri()), "t").unwrap();
    let id = adapter
        .upsert_pull_request_comment(
            &pr_ref("acme/app", 4),
            MARKER,
            &format!("new {MARKER}"),
            CommentMode::Upsert,
        )
        .await
        .unwrap();
    assert_eq!(id.0, 11);
}

#[tokio::test]
async fn test_gitlab_append_creates_mr_note() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/projects/acme%2Fapp/merge_requests/4/notes"))
        .and(body_json(json!({"body": format!("r1 {MARKER}")})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 12, "body": "x", "author": {"username": "bot"}, "created_at": "2026-11-02T00:00:00Z", "system": false
        })))
        .expect(1)
        .mount(&server)
        .await;

    let adapter = GitlabAdapter::new_with_token(config("gitlab", &server.uri()), "t").unwrap();
    let id = adapter
        .upsert_pull_request_comment(
            &pr_ref("acme/app", 4),
            MARKER,
            &format!("r1 {MARKER}"),
            CommentMode::Append,
        )
        .await
        .unwrap();
    assert_eq!(id.0, 12);
}

// =============================================================================
// Gitea
// =============================================================================

#[tokio::test]
async fn test_gitea_find_open_pull_request_matches_head_ref() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/pulls"))
        .and(query_param("state", "open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "number": 2, "html_url": "https://gitea.example.com/acme/app/pulls/2", "state": "open", "head": {"ref": "other"}},
            {"id": 3, "number": 4, "html_url": "https://gitea.example.com/acme/app/pulls/4", "state": "open", "head": {"ref": "symphony/issue-9"}}
        ])))
        .mount(&server)
        .await;

    let adapter = GiteaAdapter::new_with_token(config("gitea", &server.uri()), "t").unwrap();
    let pr = adapter
        .find_open_pull_request("acme/app", "symphony/issue-9")
        .await
        .unwrap()
        .expect("open PR");
    assert_eq!(pr.number, 4);
}

#[tokio::test]
async fn test_gitea_upsert_updates_existing_comment() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/issues/4/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 31, "body": format!("old {MARKER}"), "user": {"login": "bot"}, "created_at": "2026-11-02T00:00:00Z"}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/repos/acme/app/issues/comments/31"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 31})))
        .expect(1)
        .mount(&server)
        .await;

    let adapter = GiteaAdapter::new_with_token(config("gitea", &server.uri()), "t").unwrap();
    let id = adapter
        .upsert_pull_request_comment(
            &pr_ref("acme/app", 4),
            MARKER,
            &format!("new {MARKER}"),
            CommentMode::Upsert,
        )
        .await
        .unwrap();
    assert_eq!(id.0, 31);
}

// =============================================================================
// Memory adapter
// =============================================================================

#[tokio::test]
async fn test_memory_find_open_pull_request_by_repo_and_branch() {
    let adapter = MemoryAdapter::new();
    adapter.seed_open_pull_request("acme/web", "feat", 5).await;

    let pr = adapter
        .find_open_pull_request("acme/web", "feat")
        .await
        .unwrap()
        .expect("seeded PR");
    assert_eq!(pr.number, 5);
    assert!(adapter
        .find_open_pull_request("acme/app", "feat")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_memory_upsert_and_append_semantics() {
    let adapter = MemoryAdapter::new();
    adapter.seed_open_pull_request("acme/app", "feat", 5).await;
    let pr = pr_ref("acme/app", 5);

    let a = adapter
        .upsert_pull_request_comment(&pr, MARKER, "v1", CommentMode::Upsert)
        .await
        .unwrap();
    let b = adapter
        .upsert_pull_request_comment(&pr, MARKER, "v2", CommentMode::Upsert)
        .await
        .unwrap();
    assert_eq!(a, b, "upsert reuses the marker comment");

    adapter
        .upsert_pull_request_comment(&pr, MARKER, "r1", CommentMode::Append)
        .await
        .unwrap();
    let comments = adapter.list_pull_request_comments(&pr).await.unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].body, format!("v2\n\n{MARKER}"));
    assert!(comments[1].body.ends_with(MARKER));
}
