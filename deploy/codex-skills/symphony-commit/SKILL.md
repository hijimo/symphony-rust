---
name: symphony-commit
description:
  Create a well-formed git commit in the current repository for a Symphony issue;
  use when the workflow asks you to commit your changes.
---

# Symphony commit

1. Inspect `git status`, `git diff` and `git diff --staged`.
2. Stage only the files that belong to this issue (`git add <paths>`). Never stage
   `.symphony/`, `.symphony-workspace.json`, `test-results/`, `playwright-report/`,
   env files or credentials.
3. Follow the repository's own commit conventions from its `AGENTS.md`
   (for example whether a `Co-authored-by` trailer is allowed). Otherwise use
   `<type>(<scope>): <summary>` (imperative, <= 72 chars) and a body with what
   changed, why, and how it was validated.
4. Write the message to a temp file and run `git commit -F <file>`.
5. Do not amend or rewrite commits that were already pushed.
