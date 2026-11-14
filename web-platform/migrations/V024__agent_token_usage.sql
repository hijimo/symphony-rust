-- Token usage of document drafting sessions, one row per
-- `thread/tokenUsage/updated` (`tokenUsage.last`), docs/方案/ai-native-sdlc-p2-design.md §5 A6.
CREATE TABLE agent_token_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    flow_id INTEGER NOT NULL REFERENCES change_flows(id) ON DELETE CASCADE,
    doc_type TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT,
    input INTEGER NOT NULL DEFAULT 0,
    cached_input INTEGER NOT NULL DEFAULT 0,
    output INTEGER NOT NULL DEFAULT 0,
    reasoning INTEGER NOT NULL DEFAULT 0,
    total INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE INDEX idx_agent_token_usage_flow ON agent_token_usage(flow_id, doc_type);
CREATE INDEX idx_agent_token_usage_created ON agent_token_usage(created_at);
