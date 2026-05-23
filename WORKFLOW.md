---
tracker:
  kind: github
  project_slug: hijimo/symphony-rust
  active_states:
    - Todo
    - In Progress
    - Rework
  terminal_states:
    - Done
polling:
  interval_ms: 5000
agent:
  max_concurrent_agents: 1
  max_turns: 20
---

You are working on issue `{{ issue.identifier }}`.

{% if attempt %}
Continuation context:

- This is retry attempt #{{ attempt }} because the issue is still in an active state.
- Resume from the current workspace state instead of restarting from scratch.
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

## Status map

- Todo -> queued
- In Progress -> active work
- Rework -> requested changes
- Human Review -> waiting on review
- Merging -> approved
- Done -> terminal

## Codex Workpad

```text
<hostname>:<abs-path>@<short-sha>
```

### Plan

- [ ] 1. Reproduce and plan
- [ ] 2. Implement
- [ ] 3. Validate

### Acceptance Criteria

- [ ] Issue requirements are satisfied

### Validation

- [ ] Required checks pass

### Notes

- Workpad notes go here.

### Confusions

- Only include when something was unclear.
