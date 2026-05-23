-- Global network proxy configuration.

CREATE TABLE IF NOT EXISTS secret_configs (
    key TEXT PRIMARY KEY,
    encrypted_value TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO system_configs (key, value, description) VALUES
('network_proxy.mode', 'inherit_env', '网络代理模式：disabled、inherit_env、manual'),
('network_proxy.no_proxy', '', '网络代理绕过规则，逗号分隔'),
('network_proxy.auto_bypass_local', 'true', '网络代理是否自动绕过本机地址'),
('network_proxy.version', '1', '网络代理配置版本');
