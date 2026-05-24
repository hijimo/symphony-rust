-- Resume/recovery lifecycle fencing fields.
ALTER TABLE projects ADD COLUMN web_instance_id TEXT;
ALTER TABLE projects ADD COLUMN lifecycle_op_id TEXT;
ALTER TABLE projects ADD COLUMN lifecycle_lease_expires_at TEXT;
ALTER TABLE projects ADD COLUMN service_owner_web_instance_id TEXT;
ALTER TABLE projects ADD COLUMN service_owner_lease_expires_at TEXT;
ALTER TABLE projects ADD COLUMN service_owner_heartbeat_at TEXT;
ALTER TABLE projects ADD COLUMN service_generation INTEGER NOT NULL DEFAULT 0;
ALTER TABLE projects ADD COLUMN service_instance_id TEXT;
ALTER TABLE projects ADD COLUMN service_pgid INTEGER;
ALTER TABLE projects ADD COLUMN service_session_id INTEGER;
ALTER TABLE projects ADD COLUMN service_cmdline_hash TEXT;
ALTER TABLE projects ADD COLUMN service_workdir TEXT;
ALTER TABLE projects ADD COLUMN last_lifecycle_op TEXT;

CREATE INDEX idx_projects_service_instance_id ON projects(service_instance_id);
CREATE INDEX idx_projects_service_owner ON projects(service_owner_web_instance_id);
