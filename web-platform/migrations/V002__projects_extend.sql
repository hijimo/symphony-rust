-- Add Phase 2 columns to projects table
ALTER TABLE projects ADD COLUMN max_concurrent_agents INTEGER NOT NULL DEFAULT 2;
ALTER TABLE projects ADD COLUMN auto_restart INTEGER NOT NULL DEFAULT 1;
ALTER TABLE projects ADD COLUMN restart_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE projects ADD COLUMN last_started_at TEXT;
ALTER TABLE projects ADD COLUMN last_stopped_at TEXT;
ALTER TABLE projects ADD COLUMN error_message TEXT;
