-- Organization rules repository (docs/方案/ai-native-sdlc-p3-design.md §4 O1).
-- `-` = not configured (system config values cannot be empty).
INSERT OR IGNORE INTO system_configs (key, value, description) VALUES
  ('org_config.url', '-', '组织规则仓库地址（- 表示未启用）'),
  ('org_config.ref', 'main', '组织规则仓库跟随的分支或 tag'),
  ('org_config.sync_interval_secs', '300', '组织规则仓库同步间隔（秒）');
-- Tag or commit a project is locked to; NULL follows org_config.ref.
ALTER TABLE projects ADD COLUMN org_config_pin TEXT;
