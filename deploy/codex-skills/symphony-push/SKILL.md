---
name: symphony-push
description:
  Push the current branch and create or update its merge/pull request on GitLab
  or GitHub; use when the workflow asks you to publish changes.
---

# Symphony push

Use the platform CLI that the workflow prompt names (`glab` for GitLab, `gh` for
GitHub). Do not run project-specific build commands from another repository.

1. The latest validation round must cover the commit you push (its `head`
   equals `HEAD`), see `App runtime validation (required)` in the workflow.
2. `git push -u origin HEAD`. If rejected as non-fast-forward, run
   `symphony-pull`, validate again, then push. Use `--force-with-lease` only if
   you rewrote your own unpushed history. Never `--force`.
3. If the push fails for auth or permission reasons, stop and report the exact
   error; do not change remotes or protocols.
4. Create or update the request for this branch:
   - GitLab: `glab mr view` / `glab mr create --title <t> --description <d> --label symphony`
     / `glab mr update <iid> --title <t> --description <d>`.
   - GitHub: `gh pr view` / `gh pr create --title <t> --body-file <f>` / `gh pr edit`.
   - If the branch is tied to a closed or merged request, create a new branch.
5. The description must reflect the whole branch: what changed, the validation
   table summary (unverified/skipped items included), screenshots when
   required, and the issue link required by the workflow (`Closes #<iid>` or the
   multi-repo trailer).
6. Reply with the request URL.
