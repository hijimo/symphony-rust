-- Generic audit log (first user: project_repos changes and admin operations).
-- project_id intentionally has no FK so that records survive project deletion.
CREATE TABLE audit_logs (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_user_id  INTEGER REFERENCES users(id),
    project_id     INTEGER,
    object_type    TEXT    NOT NULL,
    object_id      TEXT,
    action         TEXT    NOT NULL,
    before_json    TEXT,
    after_json     TEXT,
    created_at     TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_audit_logs_project ON audit_logs(project_id, created_at);
CREATE INDEX idx_audit_logs_object ON audit_logs(object_type, object_id);
