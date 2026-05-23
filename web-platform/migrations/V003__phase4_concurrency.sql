-- Phase 4: Concurrency control tables

-- 并行控制历史记录表
CREATE TABLE concurrency_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,  -- 'agent_started', 'agent_completed', 'throttle_on', 'throttle_off'
    agent_id TEXT,
    issue_iid INTEGER,
    issue_title TEXT,
    duration_seconds INTEGER,  -- 仅 agent_completed 时有值
    metadata_json TEXT,        -- 额外元数据（JSON）
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_concurrency_events_project ON concurrency_events(project_id);
CREATE INDEX idx_concurrency_events_type ON concurrency_events(event_type);
CREATE INDEX idx_concurrency_events_created_at ON concurrency_events(created_at);
-- 用于今日统计的复合索引
CREATE INDEX idx_concurrency_events_project_date ON concurrency_events(project_id, created_at);

-- 并行状态快照表（由 watcher 定期写入，用于断电恢复）
CREATE TABLE concurrency_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    active_agents INTEGER NOT NULL DEFAULT 0,
    queued_tasks INTEGER NOT NULL DEFAULT 0,
    agents_json TEXT,  -- JSON array of active agent details
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project_id)
);

CREATE INDEX idx_concurrency_snapshots_updated ON concurrency_snapshots(updated_at);

-- 新增配置项
INSERT OR IGNORE INTO system_configs (key, value, description) VALUES
('concurrency_poll_interval_ms', '5000', 'Symphony 实例状态轮询间隔（毫秒）'),
('concurrency_heartbeat_timeout_s', '30', '心跳超时阈值（秒），超时视为实例异常'),
('concurrency_history_retention_days', '30', '并行事件历史保留天数');
