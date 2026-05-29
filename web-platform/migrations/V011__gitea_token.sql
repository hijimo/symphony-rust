-- Add Gitea token and host columns to user_configs
ALTER TABLE user_configs ADD COLUMN gitea_token TEXT;
ALTER TABLE user_configs ADD COLUMN gitea_host TEXT;
