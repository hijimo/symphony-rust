-- Global default Codex launch command.

INSERT OR IGNORE INTO system_configs (key, value, description) VALUES
('codex.default_command', 'codex app-server -c ''model_provider="kiro"''', 'Codex 启动命令：项目未单独填写 codex_command 时生效，修改后需重启项目服务');
