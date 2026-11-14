-- AI review of PRs before human review (docs/方案/ai-native-sdlc-p2-design.md §4 B1).
-- Off by default; block = Important findings send the issue back to In Progress.
ALTER TABLE projects ADD COLUMN ai_review_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE projects ADD COLUMN ai_review_block INTEGER NOT NULL DEFAULT 0;
