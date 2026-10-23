# Ubuntu Codex GitLab Workflow Recovery Postmortem

## Summary

This incident initially looked like a GitLab Todo consumption problem: Symphony could see GitLab issues, but a `Todo` issue did not move to `In Progress`. The actual root cause was that Codex could not execute shell commands on Ubuntu because its sandbox failed during user namespace setup.

After fixing the Ubuntu AppArmor/user namespace setting, Codex could run shell commands and the GitLab issue successfully moved from `Todo` to `In Progress`. The investigation also exposed configuration and workflow design issues around workspace roots, platform CLI dependencies, and implicit reliance on the current directory being a valid Git repository.

## Symptoms

- Symphony backend and worker processes started.
- GitLab issues were visible to the orchestrator.
- A GitLab issue stayed in `Todo` instead of moving to `In Progress`.
- Installing and configuring `glab` alone did not immediately fix Todo consumption.
- Codex session logs showed shell commands failing before execution.
- Later, after the sandbox fix, one agent turn stalled for 300 seconds and was retried by the orchestrator.

## Timeline

1. Checked logs under the Symphony workspace and confirmed the worker could start.
2. Found the development workspace root was not aligned with the intended location.
3. Updated `dev.sh` so `SYMPHONY_WORKSPACE_ROOT` defaults to `$HOME/symphony-workspaces`.
4. Confirmed `glab` was missing, installed it, then upgraded to `glab 1.105.0` because the Ubuntu package version was too old for reliable workflow use.
5. Configured `glab` for the self-hosted GitLab instance:
   - Host: `gitlab.jushuitan-inc.com:8081`
   - API protocol: `http`
6. Confirmed `gh` was not installed.
7. Inspected Codex session logs and found:

   ```text
   bwrap: setting up uid map: Permission denied
   ```

8. Identified this as an Ubuntu AppArmor/unprivileged user namespace restriction affecting Codex shell sandboxing.
9. Applied and persisted the system fix.
10. Restarted backend, frontend, and project workers.
11. Verified the issue moved from `Todo` to `In Progress`.
12. Observed a later stall/retry event, separate from the sandbox issue.

## Root Cause

Codex could not start its shell sandbox because Ubuntu blocked the required user namespace setup. This caused every shell command inside the Codex agent session to fail, including commands such as:

```bash
glab issue update <iid> --label "In Progress" --unlabel "Todo"
```

The Rust orchestrator does not directly move GitLab issues from `Todo` to `In Progress`. It starts Codex with a prompt that instructs the agent to perform that transition. Therefore, if Codex cannot execute shell commands, the Todo label is never consumed.

## Effective Fix

The effective system-level fix was:

```bash
sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
printf 'kernel.apparmor_restrict_unprivileged_userns = 0\n' | sudo tee /etc/sysctl.d/99-codex-userns.conf
sudo sysctl --system
```

Expected verification state:

```text
kernel.apparmor_restrict_unprivileged_userns = 0
kernel.unprivileged_userns_clone = 1
user.max_user_namespaces = 63823
```

## Runtime Findings

- The active GitLab workflow template is:

  ```text
  web-platform/src/templates/workflow_gitlab.md
  ```

- The repository root `WORKFLOW.md` is not the runtime template used for web-created GitLab projects.
- The orchestrator fetches active issues and starts Codex agent workers.
- The `Todo -> In Progress` transition is prompt-driven through `glab`, not directly executed by Rust.
- After the system fix, `glab 1.105.0` was authenticated and usable against the self-hosted GitLab instance.
- `gh` was not installed, which would block GitHub workflow projects in the same way `glab` blocks GitLab workflow projects.

## Exposed Project Issues

### Workspace Root Configuration Is Confusing

There are multiple workspace root defaults or hardcoded assumptions:

- `dev.sh` provides a development default.
- `web-platform/src/config.rs` has its own fallback.
- `web-platform/src/handlers/project_workflow.rs` hardcodes `~/symphony-workspaces/{project.id}` for workflow preview/render contexts.
- Generated `WORKFLOW.md` files contain `workspace.root: <workspace_root>/<project_id>`.

This makes it unclear which source is authoritative. The project should treat `SYMPHONY_WORKSPACE_ROOT` as the web-platform entry point and derive all service workdirs, workflow rendering, previews, logs, and status paths from the same configured value.

### Platform CLIs Need First-Class Checks

GitLab projects depend on `glab`; GitHub projects depend on `gh`.

The startup/environment check should be platform-aware:

- GitLab: verify `glab --version` and `glab auth status`.
- GitHub: verify `gh --version` and `gh auth status`.
- Gitea: verify the configured workflow's required CLI or ensure it uses API-based operations.

The `gh` dependency has the same class of failure as `glab`: if the CLI is missing, unauthenticated, or cannot infer the repository, agent-driven issue state transitions will fail.

### Workflow Commands Implicitly Depend On Git Metadata

Codex is started with its current working directory set to the per-issue workspace:

```text
<workspace_root>/<project_id>/<issue_workspace>
```

In the observed workspace, `.git` existed as an empty directory and was not a valid Git repository. As a result, commands that infer the repository from `cwd` can fail:

```bash
git rev-parse --show-toplevel
glab issue view <iid>
gh issue view <number>
```

If the intended design is that project creation does not automatically clone code, workflows must not rely on `.git/config` to infer the repository. They should pass explicit repository/project information, or use API paths derived from `project_slug` or the issue URL.

### GitLab Template Compatibility Needs Hardening

The GitLab template currently includes commands such as:

```bash
glab issue view <iid> --json labels,title,description,web_url
```

In the tested environment, safer alternatives are:

```bash
glab issue view <iid> --repo <namespace/project> --output json
```

or:

```bash
glab api "projects/<encoded_project>/issues/<iid>"
```

The template should avoid relying on both a specific `glab --json` behavior and repository inference from the current directory.

### Stall/Retry Observability Is Limited

The worker log showed:

```text
stall detected, cancelling worker
agent run failed ... codex error: turn cancelled
scheduled retry
```

This is useful but not enough for quick diagnosis. The UI/API/logging should expose the current attempt, current turn age, stall timeout, latest tool call, retry reason, and next retry time.

## Recommended Improvements

1. Make `SYMPHONY_WORKSPACE_ROOT` the single authoritative workspace root input for web-platform.
2. Remove hardcoded `~/symphony-workspaces` from workflow preview/render logic.
3. Add startup diagnostics for:
   - `git --version`
   - `glab --version`
   - `glab auth status`
   - `gh --version`
   - `gh auth status`
   - `kernel.apparmor_restrict_unprivileged_userns`
   - `kernel.unprivileged_userns_clone`
   - `user.max_user_namespaces`
4. Add Ubuntu troubleshooting documentation for:

   ```text
   bwrap: setting up uid map: Permission denied
   ```

5. Update GitLab and GitHub workflow templates so issue/MR operations use explicit repository information.
6. Include platform/repository information in the prompt body, not only in YAML front matter.
7. Surface common Codex/session failures as first-class project service errors:
   - missing `glab`
   - missing `gh`
   - CLI auth failure
   - `fatal: not a git repository`
   - `Could not determine base repository`
   - `bwrap: setting up uid map: Permission denied`
   - incompatible CLI flags
8. Improve stall/retry visibility in the project service status API and UI.

## Operational Checklist

When GitLab/GitHub Todo issues are not being consumed on Ubuntu:

1. Check worker log:

   ```bash
   tail -n 200 <workspace_root>/<project_id>/symphony.log
   ```

2. Check latest Codex session for sandbox errors:

   ```bash
   rg "bwrap|Permission denied|not a git repository|Could not determine base repository|unknown flag" ~/.codex/sessions
   ```

3. Check Ubuntu namespace settings:

   ```bash
   cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns
   sysctl kernel.unprivileged_userns_clone user.max_user_namespaces
   ```

4. Check platform CLI:

   ```bash
   glab --version
   glab auth status
   gh --version
   gh auth status
   ```

5. Check whether the agent workspace is a valid Git repository:

   ```bash
   git rev-parse --show-toplevel
   ```

6. If the workspace is intentionally not cloned, ensure workflow commands use explicit repository/project arguments.

## Final Lesson

This was not just a missing `glab` installation. Running Symphony agent workflows on Ubuntu requires three independent prerequisites:

1. The operating system must allow Codex shell sandboxing.
2. The platform CLI required by the workflow must be installed and authenticated.
3. The workflow must not assume the agent workspace is a valid Git repository unless the project initialization process guarantees that.
