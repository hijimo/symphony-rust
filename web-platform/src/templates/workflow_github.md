---
tracker:
  kind: github
  project_slug: "{{project_slug}}"
  active_states:
    - Todo
    - In Progress
    - Merging
    - Rework
  terminal_states:
    - Closed
    - Cancelled
    - Canceled
    - Duplicate
    - Done
polling:
  interval_ms: 5000
workspace:
  root: "{{workspace_root}}"
agent:
  max_concurrent_agents: {{max_concurrent_agents}}
  max_turns: 20
{{hooks_section}}{{codex_section}}---

You are working on a GitHub issue `{{ issue.identifier }}`

{% if attempt %}
Continuation context:

- This is retry attempt #{{ attempt }} because the issue is still in an active state.
- Resume from the current workspace state instead of restarting from scratch.
- Do not repeat already-completed investigation or validation unless needed for new code changes.
- Do not end the turn while the issue remains in an active state unless you are blocked by missing required permissions/secrets.
  {% endif %}

Issue context:
Identifier: {{ issue.identifier }}
Title: {{ issue.title }}
Current status: {{ issue.state }}
Labels: {{ issue.labels }}
URL: {{ issue.url }}

Description:
{% if issue.description %}
{{ issue.description }}
{% else %}
No description provided.
{% endif %}

Instructions:

1. This is an unattended orchestration session. Never ask a human to perform follow-up actions.
2. Only stop early for a true blocker (missing required auth/permissions/secrets). If blocked, record it in the workpad and move the issue according to workflow.
3. Final message must report completed actions and blockers only. Do not include "next steps for user".

Work only in the provided repository copy. Do not touch any other path.

## Prerequisite: `gh` CLI is available and authenticated

The agent must have `gh` CLI in PATH with a valid `GITHUB_TOKEN` (scope: `repo`). Verify:
```bash
command -v gh >/dev/null || { echo "gh CLI not found"; exit 1; }
gh auth status || { echo "gh not authenticated"; exit 1; }
```
If `gh` is not available or not authenticated, stop and report the blocker.

## Default posture

- Start by determining the issue's current status (via labels), then follow the matching flow for that status.
- Start every task by opening the tracking workpad comment and bringing it up to date before doing new implementation work.
- Spend extra effort up front on planning and verification design before implementation.
- Reproduce first: always confirm the current behavior/issue signal before changing code so the fix target is explicit.
- Keep issue metadata current (labels, checklist, acceptance criteria, links).
- Treat a single persistent GitHub issue comment as the source of truth for progress.
- Use that single workpad comment for all progress and handoff notes; do not post separate "done"/summary comments.
- Treat any issue-authored `Validation`, `Test Plan`, or `Testing` section as non-negotiable acceptance input: mirror it in the workpad and execute it before considering the work complete.
- When meaningful out-of-scope improvements are discovered during execution,
  file a separate GitHub issue instead of expanding scope:
  ```bash
  gh issue create --title "<title>" --body "<description with acceptance criteria>" --label "Backlog"
  ```
  The follow-up issue must include a clear title, description, and acceptance criteria, be placed in
  `Backlog`, and reference the current issue.
- Move status (labels) only when the matching quality bar is met.
- Operate autonomously end-to-end unless blocked by missing requirements, secrets, or permissions.
- Use the blocked-access escape hatch only for true external blockers (missing required tools/auth) after exhausting documented fallbacks.

## Related skills

- `gh`: interact with GitHub (issues, PRs, API).
- `commit`: produce clean, logical commits during implementation.
- `push`: keep remote branch current and publish updates.
- `pull`: keep branch updated with latest `origin/{{default_branch}}` before handoff.
- `land`: when issue reaches `Merging`, explicitly open and follow `.codex/skills/land/SKILL.md`, which includes the `land` loop.

## Status management via labels

State is tracked via GitHub labels. To transition state, add the new label and remove the old one in a single command:

```bash
# Example: move from "Todo" to "In Progress"
gh issue edit <number> --add-label "In Progress" --remove-label "Todo"
```

Reading current state:
```bash
gh issue view <number> --json labels,title,body,url
```

## Status map

- `Backlog` -> out of scope for this workflow; do not modify.
- `Todo` -> queued; immediately transition to `In Progress` before active work.
  - Special case: if a PR is already attached, treat as feedback/rework loop (run full PR feedback sweep, address or explicitly push back, revalidate, return to `Human Review`).
- `In Progress` -> implementation actively underway.
- `Human Review` -> PR is attached and validated; waiting on human approval.
- `Merging` -> approved by human; execute the `land` skill flow (do not call `gh pr merge` directly).
- `Rework` -> reviewer requested changes; planning + implementation required.
- `Done` -> terminal state; no further action required.

## Step 0: Determine current issue state and route

1. Fetch the issue by explicit issue number:
   ```bash
   gh issue view <number> --json labels,title,body,url
   ```
2. Read the current state from labels.
3. Route to the matching flow:
   - `Backlog` -> do not modify issue content/state; stop and wait for human to move it to `Todo`.
   - `Todo` -> immediately move to `In Progress`, then ensure bootstrap workpad comment exists (create if missing), then start execution flow.
     - If PR is already attached, start by reviewing all open PR comments and deciding required changes vs explicit pushback responses.
   - `In Progress` -> continue execution flow from current workpad comment.
   - `Human Review` -> wait and poll for decision/review updates.
   - `Merging` -> on entry, open and follow `.codex/skills/land/SKILL.md`; do not call `gh pr merge` directly.
   - `Rework` -> run rework flow.
   - `Done` -> do nothing and shut down.
4. Check whether a PR already exists for the current branch and whether it is closed/merged.
   - If a branch PR exists and is `CLOSED` or `MERGED`, treat prior branch work as non-reusable for this run.
   - Create a fresh branch from `origin/{{default_branch}}` and restart execution flow as a new attempt.
5. For `Todo` issues, do startup sequencing in this exact order:
   - Move to `In Progress`:
     ```bash
     gh issue edit <number> --add-label "In Progress" --remove-label "Todo"
     ```
   - Find/create `## Codex Workpad` bootstrap comment
   - Only then begin analysis/planning/implementation work.
6. Add a short comment if state and issue content are inconsistent, then proceed with the safest flow.

## Step 1: Start/continue execution (Todo or In Progress)

1. Find or create a single persistent workpad comment for the issue:
    - Search existing comments for a marker header: `## Codex Workpad`.
    - Query comments:
      ```bash
      gh api "repos/:owner/:repo/issues/<number>/comments" --paginate
      ```
    - If found, reuse that comment; do not create a new workpad comment.
    - If not found, create one:
      ```bash
      gh issue comment <number> -b "## Codex Workpad\n\n..."
      ```
    - Persist the workpad comment ID and only write progress updates to that ID.
    - To update the workpad:
      ```bash
      gh api PATCH "repos/:owner/:repo/issues/comments/<comment_id>" -f body="<updated content>"
      ```
2. If arriving from `Todo`, do not delay on additional status transitions: the issue should already be `In Progress` before this step begins.
3. Immediately reconcile the workpad before new edits:
    - Check off items that are already done.
    - Expand/fix the plan so it is comprehensive for current scope.
    - Ensure `Acceptance Criteria` and `Validation` are current and still make sense for the task.
4. Start work by writing/updating a hierarchical plan in the workpad comment.
5. Ensure the workpad includes a compact environment stamp at the top as a code fence line:
    - Format: `<host>:<abs-workdir>@<short-sha>`
    - Do not include metadata already inferable from issue fields (`issue ID`, `status`, `branch`, `PR link`).
6. Add explicit acceptance criteria and TODOs in checklist form in the same comment.
7. Run a principal-style self-review of the plan and refine it in the comment.
8. Before implementing, capture a concrete reproduction signal and record it in the workpad `Notes` section.
9. Run the `pull` skill to sync with latest `origin/{{default_branch}}` before any code edits, then record the pull/sync result in the workpad `Notes`.
10. Compact context and proceed to execution.

## PR feedback sweep protocol (required)

When an issue has an attached PR, run this protocol before moving to `Human Review`:

1. Identify the PR number from issue links/description.
2. Gather feedback from all channels:
   - Top-level PR comments:
     ```bash
     gh pr view <pr_number> --comments
     ```
   - Inline review comments:
     ```bash
     gh api "repos/:owner/:repo/pulls/<pr_number>/comments" --paginate
     ```
   - Review summaries/states:
     ```bash
     gh pr view <pr_number> --json reviews
     ```
3. Treat every actionable reviewer comment (human or bot), including inline review comments, as blocking until one of these is true:
   - code/test/docs updated to address it, or
   - explicit, justified pushback reply is posted on that thread.
4. Update the workpad plan/checklist to include each feedback item and its resolution status.
5. Re-run validation after feedback-driven changes and push updates.
6. Repeat this sweep until there are no outstanding actionable comments.

## Step 2: Execution phase (Todo -> In Progress -> Human Review)

1. Determine current repo state (`branch`, `git status`, `HEAD`) and verify the kickoff `pull` sync result is already recorded in the workpad before implementation continues.
2. If current issue state is `Todo`, move it to `In Progress`; otherwise leave the current state unchanged.
3. Load the existing workpad comment and treat it as the active execution checklist.
4. Implement against the hierarchical TODOs and keep the comment current:
    - Check off completed items.
    - Add newly discovered items in the appropriate section.
    - Keep parent/child structure intact as scope evolves.
    - Update the workpad immediately after each meaningful milestone.
5. Run validation/tests required for the scope.
6. Re-check all acceptance criteria and close any gaps.
7. Before every `git push` attempt, run the required validation for your scope and confirm it passes.
8. Create PR and link to the issue:
    ```bash
    gh pr create --title "<title>" --body "Closes #<number>" --label "symphony"
    ```
9. Merge latest `origin/{{default_branch}}` into branch, resolve conflicts, and rerun checks.
10. Update the workpad comment with final checklist status and validation notes.
11. Before moving to `Human Review`, poll PR feedback and checks:
    - Run the full PR feedback sweep protocol.
    - Confirm PR checks are passing (green) after the latest changes.
    - Repeat this check-address-verify loop until no outstanding comments remain and checks are fully passing.
12. Only then move issue to `Human Review`:
    ```bash
    gh issue edit <number> --add-label "Human Review" --remove-label "In Progress"
    ```

## Step 3: Human Review and merge handling

1. When the issue is in `Human Review`, do not code or change issue content.
2. Poll for updates as needed, including PR review comments from humans and bots.
3. If review feedback requires changes, move the issue to `Rework`:
   ```bash
   gh issue edit <number> --add-label "Rework" --remove-label "Human Review"
   ```
   Then follow the rework flow.
4. If approved, human moves the issue to `Merging`.
5. When the issue is in `Merging`, open and follow `.codex/skills/land/SKILL.md`, then run the `land` skill in a loop until the PR is merged. Do not call `gh pr merge` directly.
6. After merge is complete, move the issue to `Done`:
   ```bash
   gh issue edit <number> --add-label "Done" --remove-label "Merging"
   ```

## Step 3: Rework handling

1. Treat `Rework` as a full approach reset, not incremental patching.
2. Re-read the full issue body and all human comments; explicitly identify what will be done differently this attempt.
3. Close the existing PR tied to the issue:
   ```bash
   gh pr close <pr_number>
   ```
4. Remove the existing `## Codex Workpad` comment from the issue:
   ```bash
   gh api DELETE "repos/:owner/:repo/issues/comments/<comment_id>"
   ```
5. Create a fresh branch from `origin/{{default_branch}}`.
6. Start over from the normal kickoff flow:
   - Move to `In Progress` if not already:
     ```bash
     gh issue edit <number> --add-label "In Progress" --remove-label "Rework"
     ```
   - Create a new bootstrap `## Codex Workpad` comment.
   - Build a fresh plan/checklist and execute end-to-end.

## Guardrails

- If the branch PR is already closed/merged, do not reuse that branch or prior implementation state for continuation.
- For closed/merged branch PRs, create a new branch from `origin/{{default_branch}}` and restart from reproduction/planning as if starting fresh.
- If issue state is `Backlog`, do not modify it; wait for human to move to `Todo`.
- Do not edit the issue body/description for planning or progress tracking.
- Use exactly one persistent workpad comment (`## Codex Workpad`) per issue.
- Do not move to `Human Review` unless the completion bar is satisfied.
- In `Human Review`, do not make changes; wait and poll.
- If state is terminal (`Done`), do nothing and shut down.
- Avoid calling GitHub API in tight loops. GitHub rate limit: 5000 req/hour (authenticated).
