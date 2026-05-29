use crate::git_url::Platform;

const GITHUB_TEMPLATE: &str = include_str!("workflow_github.md");
const GITLAB_TEMPLATE: &str = include_str!("workflow_gitlab.md");
const GITEA_TEMPLATE: &str = include_str!("workflow_gitea.md");
const TEST_GITHUB_TEMPLATE: &str = include_str!("workflow_test_github.md");
const TEST_GITLAB_TEMPLATE: &str = include_str!("workflow_test_gitlab.md");
const TEST_GITEA_TEMPLATE: &str = include_str!("workflow_test_gitea.md");

/// Context variables for rendering workflow templates.
pub struct WorkflowTemplateContext {
    pub platform: Platform,
    pub project_slug: String,
    pub platform_host: String,
    pub workspace_root: String,
    pub max_concurrent_agents: i64,
    pub default_branch: String,
    pub hooks_after_create: Option<String>,
    pub hooks_before_remove: Option<String>,
    pub codex_command: Option<String>,
    pub codex_approval_policy: Option<String>,
    pub codex_sandbox: Option<String>,
    pub testing_enabled: bool,
    pub testing_max_turns: Option<i64>,
}

/// Get the raw template content for a given platform.
pub fn get_default_template(platform: &Platform) -> &'static str {
    match platform {
        Platform::GitHub => GITHUB_TEMPLATE,
        Platform::GitLab => GITLAB_TEMPLATE,
        Platform::Gitea => GITEA_TEMPLATE,
    }
}

/// Get the raw test-engineer template content for a given platform.
pub fn get_test_template(platform: &Platform) -> &'static str {
    match platform {
        Platform::GitHub => TEST_GITHUB_TEMPLATE,
        Platform::GitLab => TEST_GITLAB_TEMPLATE,
        Platform::Gitea => TEST_GITEA_TEMPLATE,
    }
}

/// Render a workflow template with the given context variables.
///
/// Uses simple `{{variable}}` placeholder replacement (no external template engine needed).
pub fn render_template(ctx: &WorkflowTemplateContext) -> String {
    let template = get_default_template(&ctx.platform);
    render_template_string(template, ctx)
}

/// Render the test-engineer workflow template with the given context variables.
pub fn render_test_template(ctx: &WorkflowTemplateContext) -> String {
    let template = get_test_template(&ctx.platform);
    render_template_string(template, ctx)
}

/// Render a template string with context variables.
pub fn render_template_string(template: &str, ctx: &WorkflowTemplateContext) -> String {
    let platform_endpoint = match ctx.platform {
        Platform::GitLab => format!("{}/api/v4", ctx.platform_host.trim_end_matches('/')),
        Platform::GitHub => ctx.platform_host.clone(),
        Platform::Gitea => ctx.platform_host.clone(),
    };

    let hooks_section = build_hooks_section(&ctx.hooks_after_create, &ctx.hooks_before_remove);
    let codex_section = build_codex_section(
        &ctx.codex_command,
        &ctx.codex_approval_policy,
        &ctx.codex_sandbox,
    );

    let testing_max_turns = ctx.testing_max_turns.unwrap_or(12).to_string();
    let testing_workflow_labels = build_testing_workflow_labels(ctx.testing_enabled);
    let testing_status_map = build_testing_status_map(ctx.testing_enabled);
    let testing_gate_section = build_testing_gate_section(ctx.testing_enabled, &ctx.platform);
    let testing_fail_minor_section =
        build_testing_fail_minor_section(ctx.testing_enabled, &ctx.platform);

    template
        .replace("{{project_slug}}", &ctx.project_slug)
        .replace("{{platform_host}}", &ctx.platform_host)
        .replace("{{platform_endpoint}}", &platform_endpoint)
        .replace("{{workspace_root}}", &ctx.workspace_root)
        .replace(
            "{{max_concurrent_agents}}",
            &ctx.max_concurrent_agents.to_string(),
        )
        .replace("{{default_branch}}", &ctx.default_branch)
        .replace("{{hooks_section}}", &hooks_section)
        .replace("{{codex_section}}", &codex_section)
        .replace("{{testing_max_turns}}", &testing_max_turns)
        .replace("{{testing_workflow_labels}}", &testing_workflow_labels)
        .replace("{{testing_status_map}}", &testing_status_map)
        .replace("{{testing_gate_section}}", &testing_gate_section)
        .replace("{{testing_fail_minor_section}}", &testing_fail_minor_section)
}

fn build_hooks_section(after_create: &Option<String>, before_remove: &Option<String>) -> String {
    if after_create.is_none() && before_remove.is_none() {
        return String::new();
    }
    let mut s = String::from("hooks:\n");
    if let Some(ref ac) = after_create {
        s.push_str("  after_create: |\n");
        for line in ac.lines() {
            s.push_str("    ");
            s.push_str(line);
            s.push('\n');
        }
    }
    if let Some(ref br) = before_remove {
        s.push_str("  before_remove: |\n");
        for line in br.lines() {
            s.push_str("    ");
            s.push_str(line);
            s.push('\n');
        }
    }
    s
}

fn build_codex_section(
    command: &Option<String>,
    approval_policy: &Option<String>,
    sandbox: &Option<String>,
) -> String {
    let mut s = String::from("codex:\n");
    if let Some(ref cmd) = command {
        s.push_str(&format!("  command: {}\n", cmd));
    }
    if let Some(ref policy) = approval_policy {
        s.push_str(&format!("  approval_policy: {}\n", policy));
    }
    if let Some(ref sb) = sandbox {
        s.push_str(&format!("  thread_sandbox: {}\n", sb));
    }
    s.push_str("  read_timeout_ms: 30000\n");
    s
}

fn build_testing_workflow_labels(testing_enabled: bool) -> String {
    if testing_enabled {
        "    - Testing\n    - hotfix\n    - urgent\n    - docs-only".to_string()
    } else {
        String::new()
    }
}

fn build_testing_status_map(testing_enabled: bool) -> String {
    if testing_enabled {
        "- `Testing` -> test-engineer agent is running; do not modify.\n".to_string()
    } else {
        String::new()
    }
}

fn build_testing_gate_section(testing_enabled: bool, platform: &Platform) -> String {
    if !testing_enabled {
        return r#"12. Move the issue to `Human Review`.
    - Fallback: if blocked by missing required tools/auth per the blocked-access escape hatch, include a blocker brief in the workpad.
13. For `Todo` issues that already had a PR/MR attached at kickoff:
    - Ensure all existing feedback was reviewed and resolved.
    - Ensure branch was pushed with any required updates.
    - Then move to `Human Review`."#
            .to_string();
    }

    let transition_cmd = match platform {
        Platform::GitLab => r#"      ```bash
      glab issue update <iid> --label "Testing" --unlabel "In Progress"
      ```"#,
        Platform::GitHub => r#"      ```bash
      gh issue edit <number> --add-label "Testing" --remove-label "In Progress"
      ```"#,
        Platform::Gitea => r#"      ```bash
      transition_label <number> "In Progress" "Testing"
      ```"#,
    };

    format!(
        r#"12. Before state transition, add a `### Test Points` section to the workpad listing what changed:
    ```markdown
    ### Test Points（供 test agent 参考）

    - <describe each changed function/module and what needs testing>
    ```
13. Determine the target state using the **Testing Gate**:
    - If issue has any label in ["hotfix", "urgent", "docs-only"] → move to `Human Review`
    - If the diff only contains documentation/config files (no `.rs`/`.ts`/`.tsx`/`.js` changes) → move to `Human Review`
    - Otherwise → move to `Testing`:
{}
    - After moving to `Testing`, immediately end your turn.
    - Fallback to `Human Review` if blocked by missing required tools/auth per the blocked-access escape hatch.
14. For `Todo` issues that already had a PR/MR attached at kickoff:
    - Ensure all existing feedback was reviewed and resolved, including inline review/diff comments (code changes or explicit, justified pushback response).
    - Ensure branch was pushed with any required updates.
    - Then apply the Testing Gate (step 13)."#,
        transition_cmd
    )
}

fn build_testing_fail_minor_section(testing_enabled: bool, platform: &Platform) -> String {
    if !testing_enabled {
        return String::new();
    }

    let transition_cmd = match platform {
        Platform::GitLab => r#"   ```bash
   glab issue update <iid> --label "Testing" --unlabel "In Progress"
   ```"#,
        Platform::GitHub => r#"   ```bash
   gh issue edit <number> --add-label "Testing" --remove-label "In Progress"
   ```"#,
        Platform::Gitea => r#"   ```bash
   transition_label <number> "In Progress" "Testing"
   ```"#,
    };

    format!(
        r#"## Step 3.5: Handling test-engineer feedback (FAIL-MINOR)

When the issue is in `In Progress` and the latest issue comment contains a `## Test Report` with `FAIL-MINOR`:

1. This means the test-engineer agent found minor gaps (missing test coverage only, ≤2 items).
2. Do NOT perform a full Rework reset.
3. Read the latest Test Report comment (find the one with the highest `test-report-version` number).
4. Address only the specific failures listed in the report:
   - Add missing tests
   - Fix the identified coverage gaps
5. Update the workpad `### Test Points` section with what was fixed.
6. Re-run validation, push changes.
7. Move back to `Testing`:
{}
8. Immediately end your turn after the state transition.

"#,
        transition_cmd
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_github_template() {
        let ctx = WorkflowTemplateContext {
            platform: Platform::GitHub,
            project_slug: "owner/my-repo".to_string(),
            platform_host: "https://github.com".to_string(),
            workspace_root: "~/symphony-workspaces/1".to_string(),
            max_concurrent_agents: 3,
            default_branch: "main".to_string(),
            hooks_after_create: None,
            hooks_before_remove: None,
            codex_command: None,
            codex_approval_policy: None,
            codex_sandbox: None,
            testing_enabled: false,
            testing_max_turns: None,
        };
        let rendered = render_template(&ctx);
        assert!(rendered.contains("kind: github"));
        assert!(rendered.contains("project_slug: \"owner/my-repo\""));
        assert!(rendered.contains("max_concurrent_agents: 3"));
        assert!(rendered.contains("root: \"~/symphony-workspaces/1\""));
        assert!(rendered.contains("origin/main"));
        assert!(!rendered.contains("hooks:"));
        assert!(rendered.contains("codex:\n  read_timeout_ms: 30000"));
        assert!(!rendered.contains("Testing Gate"));
        assert!(!rendered.contains("test-engineer agent is running"));
    }

    #[test]
    fn test_render_github_template_testing_enabled() {
        let ctx = WorkflowTemplateContext {
            platform: Platform::GitHub,
            project_slug: "owner/my-repo".to_string(),
            platform_host: "https://github.com".to_string(),
            workspace_root: "~/symphony-workspaces/1".to_string(),
            max_concurrent_agents: 3,
            default_branch: "main".to_string(),
            hooks_after_create: None,
            hooks_before_remove: None,
            codex_command: None,
            codex_approval_policy: None,
            codex_sandbox: None,
            testing_enabled: true,
            testing_max_turns: None,
        };
        let rendered = render_template(&ctx);
        assert!(rendered.contains("Testing Gate"));
        assert!(rendered.contains("- Testing"));
        assert!(rendered.contains("- hotfix"));
        assert!(rendered.contains("- urgent"));
        assert!(rendered.contains("- docs-only"));
        assert!(rendered.contains("Step 3.5"));
    }

    #[test]
    fn test_render_gitlab_template() {
        let ctx = WorkflowTemplateContext {
            platform: Platform::GitLab,
            project_slug: "group/sub/project".to_string(),
            platform_host: "https://gitlab.example.com".to_string(),
            workspace_root: "~/symphony-workspaces/5".to_string(),
            max_concurrent_agents: 2,
            default_branch: "develop".to_string(),
            hooks_after_create: Some(
                "git clone --depth 1 https://github.com/openai/symphony .".to_string(),
            ),
            hooks_before_remove: None,
            codex_command: Some(
                "codex --config shell_environment_policy.inherit=all app-server".to_string(),
            ),
            codex_approval_policy: Some("never".to_string()),
            codex_sandbox: Some("workspace-write".to_string()),
            testing_enabled: false,
            testing_max_turns: None,
        };
        let rendered = render_template(&ctx);
        assert!(rendered.contains("kind: gitlab"));
        assert!(rendered.contains("project_slug: \"group/sub/project\""));
        assert!(rendered.contains("endpoint: \"https://gitlab.example.com/api/v4\""));
        assert!(rendered.contains("max_concurrent_agents: 2"));
        assert!(rendered.contains("origin/develop"));
        assert!(rendered.contains("hooks:\n  after_create:"));
        assert!(rendered.contains("codex:\n  command:"));
        assert!(!rendered.contains("Testing Gate"));
    }

    #[test]
    fn test_render_test_github_template() {
        let ctx = WorkflowTemplateContext {
            platform: Platform::GitHub,
            project_slug: "owner/my-repo".to_string(),
            platform_host: "https://github.com".to_string(),
            workspace_root: "~/symphony-workspaces/1".to_string(),
            max_concurrent_agents: 2,
            default_branch: "main".to_string(),
            hooks_after_create: None,
            hooks_before_remove: None,
            codex_command: None,
            codex_approval_policy: None,
            codex_sandbox: None,
            testing_enabled: true,
            testing_max_turns: Some(12),
        };
        let rendered = render_test_template(&ctx);
        assert!(rendered.contains("kind: github"));
        assert!(rendered.contains("active_states:\n    - Testing"));
        assert!(rendered.contains("max_turns: 12"));
        assert!(rendered.contains("test-engineer agent"));
        assert!(rendered.contains("FAIL-MINOR"));
        assert!(rendered.contains("FAIL-MAJOR"));
    }

    #[test]
    fn test_render_test_gitlab_template() {
        let ctx = WorkflowTemplateContext {
            platform: Platform::GitLab,
            project_slug: "group/project".to_string(),
            platform_host: "https://gitlab.example.com".to_string(),
            workspace_root: "~/symphony-workspaces/2".to_string(),
            max_concurrent_agents: 2,
            default_branch: "main".to_string(),
            hooks_after_create: None,
            hooks_before_remove: None,
            codex_command: None,
            codex_approval_policy: None,
            codex_sandbox: None,
            testing_enabled: true,
            testing_max_turns: Some(10),
        };
        let rendered = render_test_template(&ctx);
        assert!(rendered.contains("kind: gitlab"));
        assert!(rendered.contains("active_states:\n    - Testing"));
        assert!(rendered.contains("max_turns: 10"));
        assert!(rendered.contains("test-engineer agent"));
    }

    #[test]
    fn test_get_default_template_github() {
        let template = get_default_template(&Platform::GitHub);
        assert!(template.contains("github"));
        assert!(!template.is_empty());
    }

    #[test]
    fn test_get_default_template_gitlab() {
        let template = get_default_template(&Platform::GitLab);
        assert!(template.contains("gitlab"));
        assert!(!template.is_empty());
    }
}
