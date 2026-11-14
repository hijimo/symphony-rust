-- Document-flow events for metrics (docs/方案/ai-native-sdlc-p2-design.md §4 B5).
-- kind: submitted / edited / cascade_stale / revision_started / revert / docs_merged
CREATE TABLE flow_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    flow_id INTEGER NOT NULL REFERENCES change_flows(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    doc_type TEXT,
    actor INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_flow_events_flow ON flow_events(flow_id, created_at);
