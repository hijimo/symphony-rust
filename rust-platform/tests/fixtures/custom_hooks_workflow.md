---
tracker:
  kind: linear
  project_slug: hooks-test
polling:
  interval_ms: 10000
hooks:
  timeout_ms: 30000
  after_create: |
    git init
    git remote add origin https://github.com/test/repo.git
    git fetch origin main
    git checkout -b {{issue.branch_name}} origin/main
  before_run: |
    npm ci
    npm run lint -- --fix
  after_run: |
    npm test
    npm run build
  before_remove: |
    git stash --include-untracked
    echo "Workspace preserved for debugging"
---
You are working on {{issue.title}}.

Use the hooks defined in this workflow to manage the workspace lifecycle.
Attempt number: {{attempt}}.
