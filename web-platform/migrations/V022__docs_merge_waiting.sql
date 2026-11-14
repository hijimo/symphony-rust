-- Docs MR merge waits for CI (docs/方案/ai-native-sdlc-p2-design.md §5 A3).
ALTER TABLE change_flows ADD COLUMN merge_waiting_since TEXT;
