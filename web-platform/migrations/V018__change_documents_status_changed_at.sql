-- Time of the last status transition (kanban "已等待时长" counts from here).
ALTER TABLE change_documents ADD COLUMN status_changed_at TEXT;
UPDATE change_documents SET status_changed_at = updated_at WHERE status_changed_at IS NULL;
