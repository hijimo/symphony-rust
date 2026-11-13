-- AI-native SDLC P1 (docs/方案/ai-native-sdlc-flow-design.md §4.6).
-- One row per (project, issue): the document flow of an issue.
CREATE TABLE change_flows (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id        INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    issue_iid         INTEGER NOT NULL,
    track             TEXT    NOT NULL CHECK (track IN ('full','plan')),
    revision          INTEGER NOT NULL DEFAULT 1,
    branch            TEXT    NOT NULL,
    docs_mr_iid       INTEGER,
    docs_mr_url       TEXT,
    merge_commit_sha  TEXT,
    merge_error       TEXT,
    base_sha          TEXT,
    closed            INTEGER NOT NULL DEFAULT 0,
    created_by        INTEGER NOT NULL REFERENCES users(id),
    created_at        TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (project_id, issue_iid)
);

CREATE TABLE change_documents (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    flow_id           INTEGER NOT NULL REFERENCES change_flows(id) ON DELETE CASCADE,
    doc_type          TEXT    NOT NULL CHECK (doc_type IN ('intent','spec','plan')),
    status            TEXT    NOT NULL CHECK (status IN
                        ('draft','in_review','changes_requested','approved','stale','rejected','merged')),
    path              TEXT    NOT NULL,
    current_blob_sha  TEXT,
    template_ref      TEXT,
    org_config_sha    TEXT,
    base_sha          TEXT,
    draft_state       TEXT    NOT NULL DEFAULT 'idle' CHECK (draft_state IN ('idle','running','failed')),
    draft_failures    INTEGER NOT NULL DEFAULT 0,
    draft_error       TEXT,
    last_feedback     TEXT,
    updated_by        INTEGER REFERENCES users(id),
    updated_at        TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (flow_id, doc_type)
);

CREATE TABLE document_approvals (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id     INTEGER NOT NULL REFERENCES change_documents(id) ON DELETE CASCADE,
    gate            TEXT    NOT NULL CHECK (gate IN ('G1','G2','G3')),
    action          TEXT    NOT NULL CHECK (action IN ('approve','request_changes','reject')),
    actor           INTEGER NOT NULL REFERENCES users(id),
    comment         TEXT,
    blob_sha        TEXT    NOT NULL,
    commit_sha      TEXT,
    created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
    invalidated_at  TEXT
);
CREATE INDEX idx_document_approvals_doc ON document_approvals(document_id, created_at);

-- Agent sessions are keyed by (flow, stage) (workbench design §4.2).
CREATE TABLE agent_sessions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    flow_id     INTEGER NOT NULL REFERENCES change_flows(id) ON DELETE CASCADE,
    stage       TEXT    NOT NULL CHECK (stage IN ('intent','spec','plan')),
    thread_id   TEXT,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (flow_id, stage)
);
