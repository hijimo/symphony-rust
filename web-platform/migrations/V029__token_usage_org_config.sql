-- Organization rules version a model call ran with (P3 design §4 O4).
ALTER TABLE agent_token_usage ADD COLUMN org_config_sha TEXT;
