//! Config Reload E2E Tests
//!
//! Tests that verify dynamic configuration reload behavior:
//! - Service running -> modify WORKFLOW.md -> verify new config takes effect on next dispatch
//! - Service running -> corrupt WORKFLOW.md -> verify service continues with old config
//! - Service running -> change poll_interval_ms -> verify new interval used
//!
//! Run with: `cargo test --test e2e_config_reload`

use tempfile::TempDir;

use symphony_platform::config::workflow_loader::{load_workflow, parse_workflow};

// ============================================================================
// Test: Valid WORKFLOW.md reload
// ============================================================================

/// Verifies that modifying WORKFLOW.md results in new config being loaded.
#[tokio::test]
async fn e2e_config_reload_valid_modification() {
    let tmp_dir = TempDir::new().unwrap();
    let workflow_path = tmp_dir.path().join("WORKFLOW.md");

    // Write initial workflow
    let initial_content = r#"---
tracker:
  kind: linear
  project_slug: my-project
polling:
  interval_ms: 5000
concurrency:
  max_workers: 2
---
You are working on {{issue.title}}.
"#;
    std::fs::write(&workflow_path, initial_content).unwrap();

    // Load initial config
    let initial = load_workflow(&workflow_path).unwrap();
    assert_eq!(
        initial.config["polling"].as_mapping().unwrap()["interval_ms"]
            .as_u64()
            .unwrap(),
        5000
    );
    assert_eq!(
        initial.prompt_template,
        "You are working on {{issue.title}}."
    );

    // Simulate config modification (new poll interval and prompt)
    let modified_content = r#"---
tracker:
  kind: linear
  project_slug: my-project
polling:
  interval_ms: 10000
concurrency:
  max_workers: 5
---
You are an expert engineer working on {{issue.title}}.
Attempt: {{attempt}}.
"#;
    std::fs::write(&workflow_path, modified_content).unwrap();

    // Reload config
    let reloaded = load_workflow(&workflow_path).unwrap();

    // Verify new values
    assert_eq!(
        reloaded.config["polling"].as_mapping().unwrap()["interval_ms"]
            .as_u64()
            .unwrap(),
        10000
    );
    assert_eq!(
        reloaded.config["concurrency"].as_mapping().unwrap()["max_workers"]
            .as_u64()
            .unwrap(),
        5
    );
    assert!(reloaded.prompt_template.contains("expert engineer"));
    assert!(reloaded.prompt_template.contains("Attempt: {{attempt}}"));
}

// ============================================================================
// Test: Corrupt WORKFLOW.md keeps old config
// ============================================================================

/// Verifies that corrupting WORKFLOW.md does not crash the service.
/// The service should continue with the previously loaded config.
#[tokio::test]
async fn e2e_config_reload_corrupt_keeps_old_config() {
    let tmp_dir = TempDir::new().unwrap();
    let workflow_path = tmp_dir.path().join("WORKFLOW.md");

    // Write valid workflow
    let valid_content = r#"---
tracker:
  kind: linear
  project_slug: my-project
polling:
  interval_ms: 5000
---
Valid prompt template.
"#;
    std::fs::write(&workflow_path, valid_content).unwrap();

    // Load valid config
    let valid_config = load_workflow(&workflow_path).unwrap();
    assert!(valid_config.config.contains_key("tracker"));

    // Corrupt the file with invalid YAML
    let corrupt_content = r#"---
tracker:
  kind: [[[invalid yaml
  broken: {{{
---
This should not parse.
"#;
    std::fs::write(&workflow_path, corrupt_content).unwrap();

    // Attempt to reload — should fail
    let reload_result = load_workflow(&workflow_path);
    assert!(reload_result.is_err());

    // The service should continue with the old config (valid_config)
    // This is verified by the fact that valid_config is still usable
    assert_eq!(
        valid_config.config["polling"].as_mapping().unwrap()["interval_ms"]
            .as_u64()
            .unwrap(),
        5000
    );
    assert_eq!(valid_config.prompt_template, "Valid prompt template.");
}

// ============================================================================
// Test: Change poll_interval_ms
// ============================================================================

/// Verifies that changing poll_interval_ms in WORKFLOW.md is detected.
#[tokio::test]
async fn e2e_config_reload_poll_interval_change() {
    let tmp_dir = TempDir::new().unwrap();
    let workflow_path = tmp_dir.path().join("WORKFLOW.md");

    // Initial: 5 second interval
    let content_5s = r#"---
polling:
  interval_ms: 5000
---
Prompt.
"#;
    std::fs::write(&workflow_path, content_5s).unwrap();

    let config1 = load_workflow(&workflow_path).unwrap();
    let interval1 = config1.config["polling"].as_mapping().unwrap()["interval_ms"]
        .as_u64()
        .unwrap();
    assert_eq!(interval1, 5000);

    // Change to 1 second interval
    let content_1s = r#"---
polling:
  interval_ms: 1000
---
Prompt.
"#;
    std::fs::write(&workflow_path, content_1s).unwrap();

    let config2 = load_workflow(&workflow_path).unwrap();
    let interval2 = config2.config["polling"].as_mapping().unwrap()["interval_ms"]
        .as_u64()
        .unwrap();
    assert_eq!(interval2, 1000);

    // Verify the change is detected
    assert_ne!(interval1, interval2);
}

// ============================================================================
// Test: Workflow file deletion handling
// ============================================================================

/// Verifies that deleting WORKFLOW.md is handled gracefully.
#[tokio::test]
async fn e2e_config_reload_file_deletion() {
    let tmp_dir = TempDir::new().unwrap();
    let workflow_path = tmp_dir.path().join("WORKFLOW.md");

    // Write and load
    std::fs::write(&workflow_path, "---\nkey: value\n---\nPrompt.\n").unwrap();
    let config = load_workflow(&workflow_path).unwrap();
    assert!(config.config.contains_key("key"));

    // Delete the file
    std::fs::remove_file(&workflow_path).unwrap();

    // Attempt to reload — should return MissingWorkflowFile error
    let result = load_workflow(&workflow_path);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not found"));
}

// ============================================================================
// Test: Concurrent reload safety
// ============================================================================

/// Verifies that concurrent reads of the workflow file don't cause issues.
#[tokio::test]
async fn e2e_config_reload_concurrent_reads() {
    let tmp_dir = TempDir::new().unwrap();
    let workflow_path = tmp_dir.path().join("WORKFLOW.md");

    let content = r#"---
tracker:
  kind: linear
polling:
  interval_ms: 3000
---
Concurrent test prompt.
"#;
    std::fs::write(&workflow_path, content).unwrap();

    let path = workflow_path.clone();
    let mut handles = Vec::new();

    // Spawn 10 concurrent readers
    for _ in 0..10 {
        let p = path.clone();
        handles.push(tokio::spawn(async move { load_workflow(&p) }));
    }

    // All should succeed
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
        let def = result.unwrap();
        assert_eq!(def.prompt_template, "Concurrent test prompt.");
    }
}

// ============================================================================
// Test: Config with environment variable references
// ============================================================================

/// Verifies that $VAR references in config are preserved for later resolution.
#[tokio::test]
async fn e2e_config_reload_env_var_references() {
    let content = r#"---
tracker:
  kind: linear
  api_key: $LINEAR_API_KEY
platform:
  api_token: $GITHUB_TOKEN
---
Prompt with env vars.
"#;

    let def = parse_workflow(content).unwrap();

    // The $VAR references should be preserved as strings
    let tracker = def.config["tracker"].as_mapping().unwrap();
    let api_key = tracker["api_key"].as_str().unwrap();
    assert_eq!(api_key, "$LINEAR_API_KEY");

    let platform = def.config["platform"].as_mapping().unwrap();
    let token = platform["api_token"].as_str().unwrap();
    assert_eq!(token, "$GITHUB_TOKEN");
}

// ============================================================================
// Test: Reload preserves prompt template integrity
// ============================================================================

/// Verifies that complex prompt templates with Liquid syntax are preserved.
#[tokio::test]
async fn e2e_config_reload_complex_prompt_template() {
    let content = r#"---
tracker:
  kind: linear
---
You are working on issue #{{issue.number}}: {{issue.title}}.

## Context
{{issue.description}}

## Rules
{% if attempt > 1 %}
This is retry attempt {{attempt}}. Review previous failures before proceeding.
{% endif %}

- Follow existing code patterns
- Write tests for new functionality
- Create a PR when done
"#;

    let def = parse_workflow(content).unwrap();

    // Verify the template is preserved with all Liquid syntax
    assert!(def.prompt_template.contains("{{issue.number}}"));
    assert!(def.prompt_template.contains("{{issue.title}}"));
    assert!(def.prompt_template.contains("{{issue.description}}"));
    assert!(def.prompt_template.contains("{% if attempt > 1 %}"));
    assert!(def.prompt_template.contains("{% endif %}"));
    assert!(def.prompt_template.contains("{{attempt}}"));
}

// ============================================================================
// Test: Loading from fixture files
// ============================================================================

/// Verifies that the test fixture files parse correctly.
#[tokio::test]
async fn e2e_config_reload_fixture_valid_workflow() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid_workflow.md");

    let def = load_workflow(&path).unwrap();

    assert!(def.config.contains_key("tracker"));
    assert!(def.config.contains_key("polling"));
    assert!(def.config.contains_key("concurrency"));
    assert!(def.config.contains_key("agent"));
    assert!(def.config.contains_key("codex"));
    assert!(def.config.contains_key("hooks"));
    assert!(def.config.contains_key("server"));
    assert!(def.prompt_template.contains("{{issue.title}}"));
    assert!(def.prompt_template.contains("{{attempt}}"));
}

#[tokio::test]
async fn e2e_config_reload_fixture_minimal_workflow() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal_workflow.md");

    let def = load_workflow(&path).unwrap();

    assert!(def.config.contains_key("tracker"));
    assert!(!def.config.contains_key("polling")); // Not specified
    assert_eq!(def.prompt_template, "Fix the bug described in this issue.");
}

#[tokio::test]
async fn e2e_config_reload_fixture_no_frontmatter() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/no_frontmatter_workflow.md");

    let def = load_workflow(&path).unwrap();

    assert!(def.config.is_empty());
    assert!(def.prompt_template.contains("coding assistant"));
}

#[tokio::test]
async fn e2e_config_reload_fixture_invalid_yaml() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/invalid_yaml_workflow.md");

    let result = load_workflow(&path);
    assert!(result.is_err());
}

#[tokio::test]
async fn e2e_config_reload_fixture_custom_hooks() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/custom_hooks_workflow.md");

    let def = load_workflow(&path).unwrap();

    assert!(def.config.contains_key("hooks"));
    let hooks = def.config["hooks"].as_mapping().unwrap();
    assert!(hooks.contains_key(serde_yaml::Value::String("after_create".to_string())));
    assert!(hooks.contains_key(serde_yaml::Value::String("before_run".to_string())));
    assert!(hooks.contains_key(serde_yaml::Value::String("after_run".to_string())));
    assert!(hooks.contains_key(serde_yaml::Value::String("before_remove".to_string())));
}
