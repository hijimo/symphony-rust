---
name: symphony-land
description:
  Merge an approved merge/pull request once it is mergeable; use when the issue is
  in `Merging` and the workflow tells you to land it.
---

# Symphony land

Follow the `Merging` instructions of the workflow first (blocked label, draft
state, re-validation of baseline steps). Then:

1. Make sure the working tree is clean and the branch is pushed.
2. Check mergeability:
   - GitLab: `glab mr view <iid> --output json` (`detailed_merge_status`,
     `has_conflicts`, `draft`).
   - GitHub: `gh pr view --json mergeable,mergeStateStatus,isDraft`.
3. Conflicts: run `symphony-pull`, re-run the baseline validation steps, push
   with `symphony-push`, then re-check.
4. Wait for pipelines/checks if the project has them (`glab ci status` /
   `gh pr checks --watch`). A failing pipeline caused by this change must be
   fixed; items the human accepted in the workpad are not re-fixed.
5. Do not merge while actionable review comments are unanswered.
6. Merge:
   - GitLab: `glab mr merge <iid> --squash --yes --auto-merge=false`.
   - GitHub: `gh pr merge --squash` (without `--auto`).
   - Gitea: `gitea_api POST "${REPO_PATH}/pulls/<index>/merge" -d '{"Do":"squash"}'`.
   If mergeability is unknown, wait and check again; never enable auto-merge.
7. Report the merged request URL.
