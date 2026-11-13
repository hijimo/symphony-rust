-- Deterministic self-verification gate (docs/方案/self-verification-and-agents-md.md §3.5).
-- Default off; enable per project after the new workflow template is deployed.
ALTER TABLE projects ADD COLUMN validation_gate INTEGER NOT NULL DEFAULT 0;
