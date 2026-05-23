-- 用户表
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    role TEXT NOT NULL DEFAULT 'user',
    deleted_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_users_deleted_at ON users(deleted_at);

-- 用户配置表
CREATE TABLE user_configs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL UNIQUE REFERENCES users(id),
    gitlab_token TEXT,
    gitlab_host TEXT,
    github_token TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 项目表
CREATE TABLE projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    git_url TEXT NOT NULL UNIQUE,
    platform TEXT NOT NULL,
    platform_host TEXT,
    namespace TEXT NOT NULL,
    repo_name TEXT NOT NULL,
    default_branch TEXT DEFAULT 'main',
    workflow_template TEXT NOT NULL DEFAULT 'default',
    workflow_content TEXT,
    service_status TEXT NOT NULL DEFAULT 'stopped',
    service_pid INTEGER,
    created_by INTEGER REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_projects_service_status ON projects(service_status);
CREATE INDEX idx_projects_platform ON projects(platform);

-- 项目成员表
CREATE TABLE project_members (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id),
    role TEXT NOT NULL DEFAULT 'member',
    synced_from TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project_id, user_id)
);

CREATE INDEX idx_project_members_user ON project_members(user_id);
CREATE INDEX idx_project_members_project ON project_members(project_id);

-- 系统配置表
CREATE TABLE system_configs (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    description TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO system_configs (key, value, description) VALUES
('max_concurrent_codex', '5', '全局最大 Codex 并行数'),
('kanban_pending_limit', '50', '看板待处理 Issue 显示数量'),
('kanban_done_days', '7', '看板已完成 Issue 回溯天数');

-- Token 黑名单表
CREATE TABLE token_blacklist (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id),
    invalidated_at TEXT NOT NULL DEFAULT (datetime('now')),
    reason TEXT
);

CREATE INDEX idx_token_blacklist_user ON token_blacklist(user_id);

-- 告警历史表
CREATE TABLE alert_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id TEXT NOT NULL,
    severity TEXT NOT NULL,
    project_id INTEGER REFERENCES projects(id),
    title TEXT NOT NULL,
    message TEXT NOT NULL,
    context_json TEXT,
    fired_at TEXT NOT NULL,
    resolved_at TEXT,
    notified_at TEXT,
    notification_channel TEXT,
    notification_status TEXT
);

CREATE INDEX idx_alert_history_project ON alert_history(project_id);
CREATE INDEX idx_alert_history_fired_at ON alert_history(fired_at);
CREATE INDEX idx_alert_history_severity ON alert_history(severity);
