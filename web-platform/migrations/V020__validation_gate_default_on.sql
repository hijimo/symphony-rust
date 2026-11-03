-- Validation gate on by default (requested 2026-11-03). SQLite cannot change
-- a column default, so existing projects are switched here and new projects
-- set the value explicitly on insert (repository/sqlite.rs create_project).
UPDATE projects SET validation_gate = 1;
