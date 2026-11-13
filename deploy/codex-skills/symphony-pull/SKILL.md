---
name: symphony-pull
description:
  Merge the latest default branch into the current branch (merge, not rebase) and
  resolve conflicts; use before editing code and before hand-off.
---

# Symphony pull

1. Make sure the working tree is clean (commit with `symphony-commit` first).
2. `git config rerere.enabled true`
3. `git fetch origin`
4. If the work branch exists remotely: `git pull --ff-only origin "$(git branch --show-current)"`.
5. `git -c merge.conflictstyle=zdiff3 merge origin/<default branch>` (the default
   branch is given in the workflow prompt).
6. Resolve conflicts one file at a time, keeping the intent of both sides; run
   `git diff --check` to make sure no markers remain, then `git add` and
   `git commit`.
7. Record in the workpad: merge source, result (`clean` / `conflicts resolved`)
   and the resulting `HEAD` short SHA. A merge creates a new commit, so the
   validation must run again.
