-- Labels that allow track:fast (docs/方案/ai-native-sdlc-p2-design.md §5 A1).
-- Comma-separated, matched case-insensitively.
ALTER TABLE projects ADD COLUMN fast_track_labels TEXT NOT NULL DEFAULT 'hotfix, docs-only';
