-- Extra environment variables passed through to spawned Symphony workers.

INSERT OR IGNORE INTO system_configs (key, value, description) VALUES
('codex.env_passthrough', 'KIRO_GATEWAY_API_KEY', '额外透传给 Symphony/Codex 进程的环境变量名（逗号分隔），其余父进程变量一律不继承，修改后需重启项目服务');
