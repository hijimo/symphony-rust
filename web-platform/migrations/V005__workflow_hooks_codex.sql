-- Add hooks and codex configuration columns to projects table
ALTER TABLE projects ADD COLUMN hooks_after_create TEXT;
ALTER TABLE projects ADD COLUMN hooks_before_remove TEXT;
ALTER TABLE projects ADD COLUMN codex_command TEXT;
ALTER TABLE projects ADD COLUMN codex_approval_policy TEXT DEFAULT 'never';
ALTER TABLE projects ADD COLUMN codex_sandbox TEXT DEFAULT 'workspace-write';
