CREATE TABLE idempotency_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    operation_id INTEGER REFERENCES merge_request_create_operations(id),
    response_status TEXT NOT NULL DEFAULT 'in_progress',
    http_status INTEGER NOT NULL DEFAULT 200,
    response_json TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK(response_status IN ('in_progress', 'succeeded', 'failed_final')),
    CHECK(idempotency_key <> ''),
    CHECK(request_hash <> '')
);

CREATE UNIQUE INDEX idx_idempotency_requests_key
ON idempotency_requests(project_id, user_id, idempotency_key);

CREATE INDEX idx_idempotency_requests_operation
ON idempotency_requests(operation_id);

CREATE TABLE merge_request_create_operations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    platform TEXT NOT NULL,
    project_path TEXT NOT NULL,
    source_project_path TEXT NOT NULL,
    business_key TEXT NOT NULL,
    business_key_json TEXT NOT NULL,
    source_branch TEXT NOT NULL,
    target_branch TEXT NOT NULL,
    purpose_type TEXT NOT NULL,
    purpose_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    platform_iid INTEGER,
    platform_node_id TEXT,
    web_url TEXT,
    last_error_code TEXT,
    last_error_message TEXT,
    lock_owner_request_id INTEGER REFERENCES idempotency_requests(id),
    locked_until TEXT NOT NULL,
    create_lease_token TEXT,
    create_lease_expires_at TEXT,
    creation_started_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK(status IN ('active', 'succeeded_open', 'succeeded_closed', 'failed_retryable', 'failed_final')),
    CHECK(business_key <> ''),
    CHECK(source_branch <> ''),
    CHECK(target_branch <> '')
);

CREATE UNIQUE INDEX idx_mr_create_active_business
ON merge_request_create_operations(project_id, business_key)
WHERE status IN ('active', 'succeeded_open', 'failed_retryable');

CREATE INDEX idx_mr_create_reconcile
ON merge_request_create_operations(status, locked_until, updated_at);
