-- Test engineer agent configuration
ALTER TABLE projects ADD COLUMN testing_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE projects ADD COLUMN testing_max_attempts INTEGER NOT NULL DEFAULT 3;
ALTER TABLE projects ADD COLUMN testing_max_turns INTEGER NOT NULL DEFAULT 12;
ALTER TABLE projects ADD COLUMN testing_skip_labels TEXT;
ALTER TABLE projects ADD COLUMN testing_allowed_commands TEXT;
ALTER TABLE projects ADD COLUMN testing_service_status TEXT NOT NULL DEFAULT 'stopped';
ALTER TABLE projects ADD COLUMN testing_service_pid INTEGER;
ALTER TABLE projects ADD COLUMN testing_service_instance_id TEXT;
ALTER TABLE projects ADD COLUMN testing_service_generation INTEGER NOT NULL DEFAULT 0;
