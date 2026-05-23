use reqwest::{ClientBuilder, NoProxy, Proxy};
use tokio::process::Command;

use crate::error::PlatformError;

pub const STANDARD_PROXY_VARS: [&str; 8] = [
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    Disabled,
    InheritEnv,
    Manual,
}

impl ProxyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::InheritEnv => "inherit_env",
            Self::Manual => "manual",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "manual" => Self::Manual,
            "inherit_env" => Self::InheritEnv,
            _ => Self::Disabled,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EffectiveProxyConfig {
    pub mode: ProxyMode,
    pub version: String,
    pub source: String,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub all_proxy: Option<String>,
    pub no_proxy: Option<String>,
}

impl EffectiveProxyConfig {
    pub fn from_env() -> Self {
        let mode = std::env::var("SYMPHONY_PROXY_MODE")
            .map(|value| ProxyMode::parse(&value))
            .unwrap_or(ProxyMode::Disabled);
        let version = std::env::var("SYMPHONY_PROXY_VERSION").unwrap_or_else(|_| "0".to_string());
        let source = std::env::var("SYMPHONY_PROXY_SOURCE")
            .unwrap_or_else(|_| "fallback_disabled".to_string());

        if mode == ProxyMode::Disabled {
            return Self {
                mode,
                version,
                source,
                http_proxy: None,
                https_proxy: None,
                all_proxy: None,
                no_proxy: None,
            };
        }

        Self {
            mode,
            version,
            source,
            http_proxy: first_env(&["HTTP_PROXY", "http_proxy"]),
            https_proxy: first_env(&["HTTPS_PROXY", "https_proxy"]),
            all_proxy: first_env(&["ALL_PROXY", "all_proxy"]),
            no_proxy: first_env(&["NO_PROXY", "no_proxy"]),
        }
    }

    pub fn apply_to_builder(
        &self,
        mut builder: ClientBuilder,
    ) -> Result<ClientBuilder, PlatformError> {
        if self.mode == ProxyMode::Disabled {
            return Ok(builder.no_proxy());
        }

        let no_proxy = self.no_proxy.as_deref().and_then(NoProxy::from_string);
        if let Some(url) = &self.http_proxy {
            builder = builder.proxy(with_no_proxy(Proxy::http(url)?, no_proxy.clone()));
        }
        if let Some(url) = &self.https_proxy {
            builder = builder.proxy(with_no_proxy(Proxy::https(url)?, no_proxy.clone()));
        }
        if let Some(url) = &self.all_proxy {
            builder = builder.proxy(with_no_proxy(Proxy::all(url)?, no_proxy.clone()));
        }
        Ok(builder)
    }

    pub fn apply_to_command(&self, cmd: &mut tokio::process::Command) {
        for var in STANDARD_PROXY_VARS {
            cmd.env_remove(var);
        }
        cmd.env("SYMPHONY_PROXY_MODE", self.mode.as_str());
        cmd.env("SYMPHONY_PROXY_VERSION", &self.version);
        cmd.env("SYMPHONY_PROXY_SOURCE", &self.source);

        if self.mode == ProxyMode::Disabled {
            return;
        }
        set_both(cmd, "HTTP_PROXY", "http_proxy", self.http_proxy.as_deref());
        set_both(
            cmd,
            "HTTPS_PROXY",
            "https_proxy",
            self.https_proxy.as_deref(),
        );
        set_both(cmd, "ALL_PROXY", "all_proxy", self.all_proxy.as_deref());
        set_both(cmd, "NO_PROXY", "no_proxy", self.no_proxy.as_deref());
    }
}

pub fn proxy_aware_client_builder() -> Result<ClientBuilder, PlatformError> {
    EffectiveProxyConfig::from_env().apply_to_builder(reqwest::Client::builder())
}

pub fn proxy_command(program: &str) -> Command {
    let mut command = Command::new(program);
    EffectiveProxyConfig::from_env().apply_to_command(&mut command);
    command
}

pub fn redact_proxy_url(value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return "<invalid proxy url>".to_string();
    };
    url.set_query(None);
    if !url.username().is_empty() {
        let masked = mask_username(url.username());
        let _ = url.set_username(&masked);
    }
    if url.password().is_some() {
        let _ = url.set_password(Some("***"));
    }
    url.to_string()
}

fn with_no_proxy(proxy: Proxy, no_proxy: Option<NoProxy>) -> Proxy {
    proxy.no_proxy(no_proxy)
}

fn first_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| std::env::var(name).ok())
}

fn set_both(cmd: &mut tokio::process::Command, upper: &str, lower: &str, value: Option<&str>) {
    if let Some(value) = value {
        cmd.env(upper, value);
        cmd.env(lower, value);
    }
}

fn mask_username(username: &str) -> String {
    let chars: Vec<char> = username.chars().collect();
    match chars.len() {
        0 => String::new(),
        1 => "*".to_string(),
        2 => format!("{}*", chars[0]),
        _ => format!("{}***{}", chars[0], chars[chars.len() - 1]),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, OnceLock};

    pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    pub(crate) fn clear_env() {
        for var in super::STANDARD_PROXY_VARS {
            std::env::remove_var(var);
        }
        for var in [
            "SYMPHONY_PROXY_MODE",
            "SYMPHONY_PROXY_VERSION",
            "SYMPHONY_PROXY_SOURCE",
        ] {
            std::env::remove_var(var);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use test_support::{clear_env, env_lock};

    #[test]
    fn disabled_is_fail_closed_when_sentinel_is_missing() {
        let _guard = env_lock();
        clear_env();
        std::env::set_var("HTTP_PROXY", "http://proxy.example.com:8080");

        let config = EffectiveProxyConfig::from_env();

        assert_eq!(config.mode, ProxyMode::Disabled);
        assert!(config.http_proxy.is_none());
        clear_env();
    }

    #[test]
    fn manual_mode_reads_normalized_proxy_environment() {
        let _guard = env_lock();
        clear_env();
        std::env::set_var("SYMPHONY_PROXY_MODE", "manual");
        std::env::set_var("SYMPHONY_PROXY_VERSION", "42");
        std::env::set_var("SYMPHONY_PROXY_SOURCE", "system_config");
        std::env::set_var("HTTP_PROXY", "http://proxy.example.com:8080");
        std::env::set_var("NO_PROXY", "localhost,127.0.0.1");

        let config = EffectiveProxyConfig::from_env();

        assert_eq!(config.mode, ProxyMode::Manual);
        assert_eq!(config.version, "42");
        assert_eq!(
            config.http_proxy.as_deref(),
            Some("http://proxy.example.com:8080")
        );
        assert_eq!(config.no_proxy.as_deref(), Some("localhost,127.0.0.1"));
        clear_env();
    }

    #[test]
    fn inherit_env_mode_reads_lowercase_proxy_environment() {
        let _guard = env_lock();
        clear_env();
        std::env::set_var("SYMPHONY_PROXY_MODE", "inherit_env");
        std::env::set_var("SYMPHONY_PROXY_VERSION", "7");
        std::env::set_var("SYMPHONY_PROXY_SOURCE", "environment");
        std::env::set_var("https_proxy", "http://lowercase.example.com:8080");
        std::env::set_var("all_proxy", "http://fallback.example.com:8080");
        std::env::set_var("no_proxy", "localhost,.example.com,10.0.0.0/8");

        let config = EffectiveProxyConfig::from_env();

        assert_eq!(config.mode, ProxyMode::InheritEnv);
        assert_eq!(
            config.https_proxy.as_deref(),
            Some("http://lowercase.example.com:8080")
        );
        assert_eq!(
            config.all_proxy.as_deref(),
            Some("http://fallback.example.com:8080")
        );
        assert_eq!(
            config.no_proxy.as_deref(),
            Some("localhost,.example.com,10.0.0.0/8")
        );
        clear_env();
    }

    #[tokio::test]
    async fn disabled_builder_ignores_parent_proxy_environment() {
        let _guard = env_lock();
        clear_env();
        let server = OneShotHttpServer::start();
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:9");
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:9");

        let client = EffectiveProxyConfig::from_env()
            .apply_to_builder(reqwest::Client::builder())
            .unwrap()
            .build()
            .unwrap();
        let response = client.get(server.url()).send().await.unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        clear_env();
    }

    #[tokio::test]
    async fn no_proxy_is_bound_to_manual_proxy_entries() {
        let _guard = env_lock();
        clear_env();
        let target = OneShotHttpServer::start();
        let proxy_trap = OneShotHttpServer::start();
        std::env::set_var("SYMPHONY_PROXY_MODE", "manual");
        std::env::set_var("HTTP_PROXY", proxy_trap.url());
        std::env::set_var("NO_PROXY", "127.0.0.1");

        let client = EffectiveProxyConfig::from_env()
            .apply_to_builder(reqwest::Client::builder())
            .unwrap()
            .build()
            .unwrap();
        let response = client.get(target.url()).send().await.unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(!proxy_trap.was_hit());
        clear_env();
    }

    #[tokio::test]
    async fn proxy_command_clears_standard_proxy_vars_when_disabled() {
        let _guard = env_lock();
        clear_env();
        std::env::set_var("HTTP_PROXY", "http://parent.example.com:8080");
        std::env::set_var("https_proxy", "http://parent.example.com:8443");

        let output = proxy_command("env").output().await.unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();

        assert!(stdout.contains("SYMPHONY_PROXY_MODE=disabled"));
        assert!(!stdout.contains("HTTP_PROXY=http://parent.example.com:8080"));
        assert!(!stdout.contains("https_proxy=http://parent.example.com:8443"));
        clear_env();
    }

    #[tokio::test]
    async fn proxy_command_injects_upper_and_lowercase_vars_when_manual() {
        let _guard = env_lock();
        clear_env();
        std::env::set_var("SYMPHONY_PROXY_MODE", "manual");
        std::env::set_var("SYMPHONY_PROXY_VERSION", "12");
        std::env::set_var("SYMPHONY_PROXY_SOURCE", "system_config");
        std::env::set_var("HTTP_PROXY", "http://proxy.example.com:8080");
        std::env::set_var("NO_PROXY", "localhost,127.0.0.1");

        let output = proxy_command("env").output().await.unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();

        assert!(stdout.contains("SYMPHONY_PROXY_MODE=manual"));
        assert!(stdout.contains("SYMPHONY_PROXY_VERSION=12"));
        assert!(stdout.contains("HTTP_PROXY=http://proxy.example.com:8080"));
        assert!(stdout.contains("http_proxy=http://proxy.example.com:8080"));
        assert!(stdout.contains("NO_PROXY=localhost,127.0.0.1"));
        assert!(stdout.contains("no_proxy=localhost,127.0.0.1"));
        clear_env();
    }

    #[test]
    fn redact_proxy_url_hides_credentials_and_query() {
        let redacted = redact_proxy_url("http://user:password@proxy.example.com:8080?token=secret");

        assert_eq!(redacted, "http://u***r:***@proxy.example.com:8080/");
    }

    struct OneShotHttpServer {
        url: String,
        hit: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl OneShotHttpServer {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let hit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let hit_for_thread = hit.clone();
            std::thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    hit_for_thread.store(true, std::sync::atomic::Ordering::SeqCst);
                    let mut buffer = [0; 1024];
                    let _ = stream.read(&mut buffer);
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                    );
                }
            });
            Self {
                url: format!("http://{}", addr),
                hit,
            }
        }

        fn url(&self) -> &str {
            &self.url
        }

        fn was_hit(&self) -> bool {
            self.hit.load(std::sync::atomic::Ordering::SeqCst)
        }
    }
}
