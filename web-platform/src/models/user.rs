use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip_serializing)]
    #[schema(ignore)]
    pub password_hash: String,
    pub display_name: Option<String>,
    pub role: String,
    #[schema(value_type = Option<String>)]
    pub deleted_at: Option<NaiveDateTime>,
    #[schema(value_type = String)]
    pub created_at: NaiveDateTime,
    #[schema(value_type = String)]
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserConfig {
    pub id: i64,
    pub user_id: i64,
    pub gitlab_token: Option<String>,
    pub gitlab_host: Option<String>,
    pub github_token: Option<String>,
    pub gitea_token: Option<String>,
    pub gitea_host: Option<String>,
    #[schema(value_type = String)]
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBlacklistEntry {
    pub id: i64,
    pub user_id: i64,
    pub invalidated_at: NaiveDateTime,
    pub reason: Option<String>,
}
