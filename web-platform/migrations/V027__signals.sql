-- External signals waiting for triage (docs/方案/ai-native-sdlc-p3-design.md §3 S1).
CREATE TABLE signal_ingest_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source TEXT NOT NULL CHECK (source IN ('sentry', 'alertmanager', 'generic')),
    name TEXT NOT NULL,
    -- sha256 of the key; the plain key is shown once.
    key_hash TEXT NOT NULL,
    -- Sentry signs with the key as HMAC secret: kept encrypted for that source only.
    secret_enc TEXT,
    created_by INTEGER REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT,
    revoked_at TEXT
);
CREATE INDEX idx_signal_keys_project ON signal_ingest_keys(project_id, source);

CREATE TABLE signals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    title TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (severity IN ('critical', 'warning', 'info')),
    status TEXT NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'intent', 'ignored')),
    resolved INTEGER NOT NULL DEFAULT 0,
    first_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    count INTEGER NOT NULL DEFAULT 1,
    evidence_json TEXT NOT NULL DEFAULT '{}',
    external_url TEXT,
    flow_id INTEGER REFERENCES change_flows(id) ON DELETE SET NULL,
    issue_iid INTEGER,
    ignored_until TEXT,
    triaged_by INTEGER REFERENCES users(id),
    triaged_at TEXT,
    UNIQUE (project_id, source, fingerprint)
);
CREATE INDEX idx_signals_project_status ON signals(project_id, status, last_seen_at);
