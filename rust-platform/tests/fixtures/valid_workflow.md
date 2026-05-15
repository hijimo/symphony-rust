---
tracker:
  kind: linear
  project_slug: symphony-test
  api_key: $LINEAR_API_KEY
polling:
  interval_ms: 5000
concurrency:
  max_workers: 3
agent:
  max_retry_backoff_ms: 300000
  stall_timeout_ms: 120000
codex:
  command: codex app-server
hooks:
  timeout_ms: 60000
  after_create: "git init && git checkout -b {{issue.branch_name}}"
  before_run: "npm install"
  after_run: "npm test"
  before_remove: "git stash"
server:
  port: 8080
  bind: "127.0.0.1"
---
You are an expert software engineer working on issue #{{issue.number}}: {{issue.title}}.

## Context
{{issue.description}}

## Instructions
- Write clean, well-tested code
- Follow existing patterns in the codebase
- Create a PR when done

## Attempt
This is attempt {{attempt}} for this issue.
