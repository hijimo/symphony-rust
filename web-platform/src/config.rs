use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub jwt_secret: String,
    pub encryption_key: String,
    pub database_url: String,
    pub server_host: String,
    pub server_port: u16,
    pub static_dir: Option<String>,
    pub symphony_bin: String,
    pub workspace_root: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let jwt_secret = required_env("JWT_SECRET");
        let encryption_key = required_env("ENCRYPTION_KEY");
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "data.db".to_string());
        let server_host = env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let server_port = env::var("SERVER_PORT")
            .unwrap_or_else(|_| "3000".to_string())
            .parse::<u16>()
            .expect("SERVER_PORT must be a valid port number");
        let static_dir = env::var("STATIC_DIR").ok().and_then(|value| {
            let value = value.trim().to_string();
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        });

        if jwt_secret.len() < 32 {
            panic!("JWT_SECRET must be at least 32 characters");
        }

        let symphony_bin =
            env::var("SYMPHONY_BIN").unwrap_or_else(|_| "symphony-platform".to_string());
        let workspace_root =
            env::var("SYMPHONY_WORKSPACE_ROOT").unwrap_or_else(|_| "./workspaces".to_string());

        Self {
            jwt_secret,
            encryption_key,
            database_url,
            server_host,
            server_port,
            static_dir,
            symphony_bin,
            workspace_root,
        }
    }
}

fn required_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| {
        panic!(
            "Environment variable {} is required but not set. Please set it before starting the server.",
            name
        )
    })
}
