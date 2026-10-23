-- Multi-repo workspace: related repositories of a project (design 4.2).
-- The primary repository stays implicit in `projects`; only secondary repos live here.
CREATE TABLE project_repos (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id         INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    alias              TEXT    NOT NULL,
    linked_project_id  INTEGER REFERENCES projects(id) ON DELETE RESTRICT,
    namespace          TEXT,
    repo_name          TEXT,
    default_branch     TEXT,
    mode               TEXT    NOT NULL DEFAULT 'read' CHECK (mode IN ('read','write')),
    checkout           TEXT    NOT NULL DEFAULT 'partial' CHECK (checkout IN ('full','partial')),
    merge_after        TEXT    NOT NULL DEFAULT '[]',
    status             TEXT    NOT NULL DEFAULT 'active' CHECK (status IN ('active','invalid')),
    status_reason      TEXT,
    write_granted_by   INTEGER REFERENCES users(id),
    write_granted_at   TEXT,
    created_by         INTEGER NOT NULL REFERENCES users(id),
    created_at         TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (project_id, alias),
    CHECK (
        (linked_project_id IS NOT NULL AND namespace IS NULL AND repo_name IS NULL)
     OR (linked_project_id IS NULL AND namespace IS NOT NULL AND repo_name IS NOT NULL)
    ),
    CHECK (linked_project_id IS NULL OR linked_project_id <> project_id),
    CHECK (linked_project_id IS NOT NULL OR default_branch IS NOT NULL),
    CHECK (mode = 'read' OR write_granted_by IS NOT NULL)
);
CREATE UNIQUE INDEX uq_project_repos_linked ON project_repos(project_id, linked_project_id)
    WHERE linked_project_id IS NOT NULL;
CREATE UNIQUE INDEX uq_project_repos_raw ON project_repos(project_id, namespace, repo_name)
    WHERE linked_project_id IS NULL;
CREATE INDEX idx_project_repos_linked_project ON project_repos(linked_project_id);
CREATE INDEX idx_project_repos_created_by ON project_repos(created_by);
