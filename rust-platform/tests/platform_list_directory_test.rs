//! Listing a directory on a branch (P2 design §3 F3).

use std::collections::HashMap;

use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use symphony_platform::config::platform::{IssueFilter, PlatformConfig, WorkflowConfig};
use symphony_platform::error::PlatformError;
use symphony_platform::platform::gitea::GiteaAdapter;
use symphony_platform::platform::github::GithubAdapter;
use symphony_platform::platform::gitlab::GitlabAdapter;
use symphony_platform::platform::{MemoryAdapter, Platform, RepoFile};

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

fn names(files: &[RepoFile]) -> Vec<&str> {
    files.iter().map(|f| f.name.as_str()).collect()
}

fn contents_listing() -> serde_json::Value {
    json!([
        {"name": "intent.md", "path": "docs/changes/7/intent.md", "sha": "aaa111", "type": "file"},
        {"name": "assets", "path": "docs/changes/7/assets", "sha": "ddd444", "type": "dir"},
        {"name": "plan.md", "path": "docs/changes/7/plan.md", "sha": "ccc333", "type": "file"},
        {"name": "spec.md", "path": "docs/changes/7/spec.md", "sha": "bbb222", "type": "file"}
    ])
}

// =============================================================================
// GitHub
// =============================================================================

#[tokio::test]
async fn test_github_list_directory_returns_sorted_files_only() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/docs/changes/7"))
        .and(query_param("ref", "main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(contents_listing()))
        .expect(1)
        .mount(&server)
        .await;

    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    let files = adapter
        .list_directory("acme/app", "docs/changes/7", "main")
        .await
        .unwrap()
        .expect("directory exists");
    assert_eq!(names(&files), vec!["intent.md", "plan.md", "spec.md"]);
    assert_eq!(files[1].path, "docs/changes/7/plan.md");
    assert_eq!(files[1].blob_sha, "ccc333");
}

#[tokio::test]
async fn test_github_list_directory_missing_returns_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/docs/changes/8"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .mount(&server)
        .await;

    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    let files = adapter
        .list_directory("acme/app", "docs/changes/8", "main")
        .await
        .unwrap();
    assert!(files.is_none());
}

#[tokio::test]
async fn test_github_list_directory_on_a_file_returns_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/README.md"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(
            {"name": "README.md", "path": "README.md", "sha": "eee", "type": "file"}
        )))
        .mount(&server)
        .await;

    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    assert!(adapter
        .list_directory("acme/app", "README.md", "main")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_list_directory_rejects_unsafe_paths_without_requests() {
    let server = MockServer::start().await;
    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    for bad in [
        "",
        "/etc",
        "docs/../..",
        "docs//x",
        "docs/x?ref=evil",
        "docs/x#y",
    ] {
        let err = adapter
            .list_directory("acme/app", bad, "main")
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
async fn test_github_list_directory_server_error_propagates() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/docs"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let adapter = GithubAdapter::new_with_token(config("github", &server.uri()), "t").unwrap();
    let err = adapter
        .list_directory("acme/app", "docs", "main")
        .await
        .expect_err("500 must fail");
    assert!(matches!(err, PlatformError::ServerError(500)), "{err:?}");
}

// =============================================================================
// GitLab
// =============================================================================

#[tokio::test]
async fn test_gitlab_list_directory_uses_repository_tree() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/projects/group%2Fbackend/repository/tree"))
        .and(query_param("path", "docs/changes/7"))
        .and(query_param("ref", "develop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": "t1", "name": "assets", "type": "tree", "path": "docs/changes/7/assets", "mode": "040000"},
            {"id": "b2", "name": "spec.md", "type": "blob", "path": "docs/changes/7/spec.md", "mode": "100644"},
            {"id": "b1", "name": "plan.md", "type": "blob", "path": "docs/changes/7/plan.md", "mode": "100644"}
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let adapter = GitlabAdapter::new_with_token(config("gitlab", &server.uri()), "t").unwrap();
    let files = adapter
        .list_directory("group/backend", "docs/changes/7", "develop")
        .await
        .unwrap()
        .expect("directory exists");
    assert_eq!(names(&files), vec!["plan.md", "spec.md"]);
    assert_eq!(files[0].blob_sha, "b1");
}

#[tokio::test]
async fn test_gitlab_list_directory_missing_or_empty_returns_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/projects/acme%2Fapp/repository/tree"))
        .and(query_param("path", "docs/changes/8"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({"message": "404 Tree Not Found"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/projects/acme%2Fapp/repository/tree"))
        .and(query_param("path", "docs/changes/9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let adapter = GitlabAdapter::new_with_token(config("gitlab", &server.uri()), "t").unwrap();
    for p in ["docs/changes/8", "docs/changes/9"] {
        assert!(adapter
            .list_directory("acme/app", p, "main")
            .await
            .unwrap()
            .is_none());
    }
}

// =============================================================================
// Gitea
// =============================================================================

#[tokio::test]
async fn test_gitea_list_directory_uses_contents_api() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/contents/docs/changes/7"))
        .and(query_param("ref", "main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(contents_listing()))
        .mount(&server)
        .await;

    let adapter = GiteaAdapter::new_with_token(config("gitea", &server.uri()), "t").unwrap();
    let files = adapter
        .list_directory("acme/app", "docs/changes/7", "main")
        .await
        .unwrap()
        .expect("directory exists");
    assert_eq!(names(&files), vec!["intent.md", "plan.md", "spec.md"]);
}

// =============================================================================
// Memory
// =============================================================================

#[tokio::test]
async fn test_memory_list_directory_returns_direct_children_only() {
    let adapter = MemoryAdapter::new();
    adapter
        .seed_repo_file("acme/app", "main", "docs/changes/7/plan.md", "p1")
        .await;
    adapter
        .seed_repo_file("acme/app", "main", "docs/changes/7/intent.md", "i1")
        .await;
    adapter
        .seed_repo_file("acme/app", "main", "docs/changes/7/assets/a.png", "x")
        .await;
    adapter
        .seed_repo_file("acme/app", "dev", "docs/changes/7/spec.md", "s1")
        .await;

    let files = adapter
        .list_directory("acme/app", "docs/changes/7", "main")
        .await
        .unwrap()
        .expect("seeded");
    assert_eq!(names(&files), vec!["intent.md", "plan.md"]);
    assert!(adapter
        .list_directory("acme/app", "docs/changes/8", "main")
        .await
        .unwrap()
        .is_none());
}
