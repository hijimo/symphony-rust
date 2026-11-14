-- Who may decide each document gate (docs/方案/ai-native-sdlc-p2-design.md §4 B6).
-- A gate without rows stays open to every project member.
CREATE TABLE gate_approvers (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    gate TEXT NOT NULL CHECK (gate IN ('G1', 'G2', 'G3')),
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (project_id, gate, user_id)
);
