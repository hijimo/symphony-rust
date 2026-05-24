-- Track the proxy configuration version applied when a project service starts.

ALTER TABLE projects ADD COLUMN service_proxy_config_version TEXT;

CREATE INDEX idx_projects_service_proxy_version
    ON projects(service_status, service_proxy_config_version);
