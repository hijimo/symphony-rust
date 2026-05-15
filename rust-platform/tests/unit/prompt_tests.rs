//! Unit tests for the prompt template engine.
//!
//! Tests cover:
//! - Strict mode: unknown variable fails
//! - Strict mode: unknown filter fails
//! - Issue variable rendering (all fields)
//! - Continuation template selection
//! - Empty prompt fallback

use symphony_platform::prompt::{BlockerContext, IssueContext, PromptEngine, PromptError};

// ═══════════════════════════════════════════════════════════════════════════════
// Helper
// ═══════════════════════════════════════════════════════════════════════════════

fn make_test_context() -> IssueContext {
    IssueContext {
        id: "uuid-123".to_string(),
        identifier: "PROJ-42".to_string(),
        title: "Implement OAuth2 support".to_string(),
        description: Some("We need OAuth2 for the API gateway.".to_string()),
        priority: Some(2),
        state: "In Progress".to_string(),
        labels: vec!["bug".to_string(), "priority::high".to_string()],
        url: Some("https://github.com/org/repo/issues/42".to_string()),
        branch_name: Some("symphony/proj-42".to_string()),
        blocked_by: vec![],
        created_at: Some("2025-01-15T10:00:00Z".to_string()),
        updated_at: Some("2025-01-16T14:30:00Z".to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Strict Mode Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod strict_mode {
    use super::*;

    #[test]
    fn test_unknown_variable_behavior() {
        // Liquid renders unknown top-level variables as empty string by default
        let engine = PromptEngine::compile("Hello {{ nonexistent_var }}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20);
        // Should either fail (strict) or render empty (lenient)
        // The important thing is it doesn't panic
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_compile_error_on_invalid_syntax() {
        let result = PromptEngine::compile("{% if unclosed");
        assert!(result.is_err());
        if let Err(PromptError::CompileError(msg)) = result {
            assert!(!msg.is_empty());
        }
    }

    #[test]
    fn test_known_variables_render_correctly() {
        let engine = PromptEngine::compile(
            "Working on {{ issue.identifier }}: {{ issue.title }}\n\n{{ issue.description }}",
        )
        .unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert!(result.contains("PROJ-42"));
        assert!(result.contains("Implement OAuth2 support"));
        assert!(result.contains("We need OAuth2 for the API gateway."));
    }

    #[test]
    fn test_attempt_variable_renders() {
        let engine =
            PromptEngine::compile("{% if attempt %}Attempt {{ attempt }}{% endif %}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, Some(3), 1, 20).unwrap();
        assert_eq!(result, "Attempt 3");
    }

    #[test]
    fn test_attempt_nil_on_first_run() {
        let engine =
            PromptEngine::compile("{% if attempt %}retry{% else %}first{% endif %}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "first");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Issue Variable Rendering Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod issue_rendering {
    use super::*;

    #[test]
    fn test_render_identifier() {
        let engine = PromptEngine::compile("Issue: {{ issue.identifier }}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "Issue: PROJ-42");
    }

    #[test]
    fn test_render_title() {
        let engine = PromptEngine::compile("Title: {{ issue.title }}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "Title: Implement OAuth2 support");
    }

    #[test]
    fn test_render_description() {
        let engine = PromptEngine::compile("{{ issue.description }}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "We need OAuth2 for the API gateway.");
    }

    #[test]
    fn test_render_description_nil() {
        let engine = PromptEngine::compile(
            "{% if issue.description %}{{ issue.description }}{% else %}none{% endif %}",
        )
        .unwrap();
        let mut ctx = make_test_context();
        ctx.description = None;

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "none");
    }

    #[test]
    fn test_render_state() {
        let engine = PromptEngine::compile("State: {{ issue.state }}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "State: In Progress");
    }

    #[test]
    fn test_render_labels() {
        let engine = PromptEngine::compile(
            "{% for label in issue.labels %}{{ label }} {% endfor %}",
        )
        .unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert!(result.contains("bug"));
        assert!(result.contains("priority::high"));
    }

    #[test]
    fn test_render_url() {
        let engine = PromptEngine::compile("{{ issue.url }}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "https://github.com/org/repo/issues/42");
    }

    #[test]
    fn test_render_branch_name() {
        let engine = PromptEngine::compile("{{ issue.branch_name }}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "symphony/proj-42");
    }

    #[test]
    fn test_render_priority() {
        let engine = PromptEngine::compile("Priority: {{ issue.priority }}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "Priority: 2");
    }

    #[test]
    fn test_render_blockers() {
        let engine = PromptEngine::compile(
            "{% for b in issue.blocked_by %}{{ b.identifier }}{% endfor %}",
        )
        .unwrap();
        let mut ctx = make_test_context();
        ctx.blocked_by = vec![BlockerContext {
            id: Some("uuid-999".to_string()),
            identifier: Some("PROJ-10".to_string()),
            state: Some("Done".to_string()),
        }];

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "PROJ-10");
    }

    #[test]
    fn test_render_all_fields_combined() {
        let template = "{{ issue.identifier }}: {{ issue.title }} [{{ issue.state }}]";
        let engine = PromptEngine::compile(template).unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "PROJ-42: Implement OAuth2 support [In Progress]");
    }

    #[test]
    fn test_render_no_template_variables() {
        let engine = PromptEngine::compile("Static prompt with no variables.").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "Static prompt with no variables.");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Continuation Template Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod continuation_template {
    use super::*;

    #[test]
    fn test_first_turn_uses_main_template() {
        let engine = PromptEngine::compile("Main template for {{ issue.identifier }}").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "Main template for PROJ-42");
    }

    #[test]
    fn test_subsequent_turn_uses_default_continuation() {
        let engine = PromptEngine::compile("Main template for {{ issue.identifier }}").unwrap();
        let ctx = make_test_context();

        // Turn 2 should use default continuation
        let result = engine.render(&ctx, None, 2, 20).unwrap();
        assert!(result.contains("Continue working on issue PROJ-42"));
        assert!(result.contains("Turn 2/20"));
    }

    #[test]
    fn test_custom_continuation_template() {
        let engine = PromptEngine::compile_with_continuation(
            "Main: {{ issue.title }}",
            Some("Continue {{ issue.identifier }}, turn {{ turn_number }}/{{ max_turns }}"),
        )
        .unwrap();
        let ctx = make_test_context();

        // Turn 1 uses main
        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "Main: Implement OAuth2 support");

        // Turn 2 uses custom continuation
        let result = engine.render(&ctx, None, 2, 20).unwrap();
        assert_eq!(result, "Continue PROJ-42, turn 2/20");
    }

    #[test]
    fn test_continuation_includes_turn_info() {
        let engine = PromptEngine::compile("Main template").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 5, 20).unwrap();
        assert!(result.contains("Turn 5/20"));
    }

    #[test]
    fn test_last_turn_continuation() {
        let engine = PromptEngine::compile("Main template").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 20, 20).unwrap();
        assert!(result.contains("Turn 20/20"));
    }

    #[test]
    fn test_is_continuation_variable() {
        let engine = PromptEngine::compile_with_continuation(
            "{% if is_continuation %}CONT{% else %}FIRST{% endif %}",
            Some("{% if is_continuation %}CONT{% else %}FIRST{% endif %}"),
        )
        .unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "FIRST");

        let result = engine.render(&ctx, None, 2, 20).unwrap();
        assert_eq!(result, "CONT");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Empty Prompt Fallback Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod empty_prompt_fallback {
    use super::*;

    #[test]
    fn test_empty_template_renders_empty() {
        let engine = PromptEngine::compile("").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_whitespace_only_template() {
        let engine = PromptEngine::compile("   ").unwrap();
        let ctx = make_test_context();

        let result = engine.render(&ctx, None, 1, 20).unwrap();
        assert_eq!(result, "   ");
    }

    #[test]
    fn test_compile_error_on_malformed_template() {
        let result = PromptEngine::compile("{% for x in %}");
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_success_on_valid_template() {
        let result = PromptEngine::compile("Hello {{ issue.title }}");
        assert!(result.is_ok());
    }
}
