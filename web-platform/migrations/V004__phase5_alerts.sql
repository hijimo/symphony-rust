-- Phase 5: Alert & Notification

-- 告警规则配置表
CREATE TABLE alert_rules (
    rule_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'warning',
    enabled INTEGER NOT NULL DEFAULT 1,
    threshold_json TEXT NOT NULL DEFAULT '{}',
    cooldown_seconds INTEGER NOT NULL DEFAULT 300,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 预置告警规则
INSERT INTO alert_rules (rule_id, name, description, severity, enabled, threshold_json, cooldown_seconds) VALUES
('task_timeout', '任务超时', 'Codex 单任务运行时间超过阈值时触发', 'warning', 1, '{"timeout_minutes":30}', 300),
('task_failure', '任务失败', 'Codex 任务异常退出且重试耗尽时触发', 'critical', 1, '{}', 300),
('service_crash', '服务异常退出', 'Symphony 实例进程意外退出时触发', 'critical', 1, '{}', 300),
('concurrency_saturation', '并行饱和', '全局并行数达到上限持续超过阈值时间时触发', 'warning', 1, '{"saturation_minutes":10}', 600),
('consecutive_failures', '连续失败', '同一项目连续 N 个任务失败时触发', 'critical', 1, '{"failure_count":3}', 300),
('api_unreachable', 'API 不可达', 'GitLab/GitHub API 连续请求失败时触发', 'critical', 1, '{"failure_count":5}', 600);

-- 通知渠道配置表
CREATE TABLE notification_channels (
    channel_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    channel_type TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    config_encrypted TEXT NOT NULL,
    severity_filter_json TEXT NOT NULL DEFAULT '["critical","warning"]',
    last_test_at TEXT,
    last_test_success INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- alert_history 表已在 V001 中创建，此处补充 created_at 列和额外索引
ALTER TABLE alert_history ADD COLUMN created_at TEXT NOT NULL DEFAULT (datetime('now'));

-- 补充 V001 中未创建的索引
CREATE INDEX IF NOT EXISTS idx_alert_history_rule ON alert_history(rule_id);
CREATE INDEX IF NOT EXISTS idx_alert_history_status ON alert_history(notification_status);
-- 复合索引：分页查询优化（按时间倒序 + 筛选条件）
CREATE INDEX IF NOT EXISTS idx_alert_history_fired_severity ON alert_history(fired_at, severity);
CREATE INDEX IF NOT EXISTS idx_alert_history_project_fired ON alert_history(project_id, fired_at);

-- 告警冷却状态表（内存为主，DB 用于重启恢复）
CREATE TABLE alert_cooldowns (
    rule_id TEXT NOT NULL,
    scope_key TEXT NOT NULL,
    last_fired_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    PRIMARY KEY (rule_id, scope_key)
);

CREATE INDEX idx_alert_cooldowns_expires ON alert_cooldowns(expires_at);

-- system_configs 新增告警相关配置
INSERT OR IGNORE INTO system_configs (key, value, description) VALUES
('alert_enabled', 'true', '全局告警开关'),
('alert_evaluation_interval_seconds', '30', '规则评估间隔（秒）'),
('alert_history_retention_days', '90', '告警历史保留天数');
