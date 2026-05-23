use serde::{Deserialize, Serialize};

use crate::error::WebPlatformError;
use crate::handlers::admin_config::SystemConfigItem;

pub const MODE_KEY: &str = "network_proxy.mode";
pub const NO_PROXY_KEY: &str = "network_proxy.no_proxy";
pub const AUTO_BYPASS_LOCAL_KEY: &str = "network_proxy.auto_bypass_local";
pub const VERSION_KEY: &str = "network_proxy.version";
pub const HTTP_SECRET_KEY: &str = "network_proxy.http_url";
pub const HTTPS_SECRET_KEY: &str = "network_proxy.https_url";
pub const ALL_SECRET_KEY: &str = "network_proxy.all_url";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

    pub fn parse(value: &str) -> Result<Self, WebPlatformError> {
        match value {
            "disabled" => Ok(Self::Disabled),
            "inherit_env" => Ok(Self::InheritEnv),
            "manual" => Ok(Self::Manual),
            other => Err(WebPlatformError::BadRequest(format!(
                "invalid proxy mode '{}'",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySecretDisplay {
    pub configured: bool,
    pub display_value: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProxySecret {
    pub key: String,
    pub encrypted_value: String,
    pub kind: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ProxySecretMutation {
    pub key: String,
    pub kind: String,
    pub encrypted_value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyWarning {
    pub code: String,
    pub severity: String,
    pub blocking: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkProxyConfigResponse {
    pub mode: ProxyMode,
    pub version: String,
    pub source: String,
    pub http_proxy: ProxySecretDisplay,
    pub https_proxy: ProxySecretDisplay,
    pub all_proxy: ProxySecretDisplay,
    pub no_proxy: String,
    pub auto_bypass_local: bool,
    pub needs_restart_project_count: i64,
    pub updated_at: Option<String>,
    pub warnings: Vec<ProxyWarning>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SecretUpdate {
    pub action: String,
    pub value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct UpdateNetworkProxyRequest {
    pub expected_version: String,
    pub mode: ProxyMode,
    pub http_proxy: SecretUpdate,
    pub https_proxy: SecretUpdate,
    pub all_proxy: SecretUpdate,
    pub no_proxy: String,
    pub auto_bypass_local: bool,
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
    pub fn disabled(version: String, source: impl Into<String>) -> Self {
        Self {
            mode: ProxyMode::Disabled,
            version,
            source: source.into(),
            http_proxy: None,
            https_proxy: None,
            all_proxy: None,
            no_proxy: None,
        }
    }

    pub fn from_env(version: String) -> Self {
        Self {
            mode: ProxyMode::InheritEnv,
            version,
            source: "environment".to_string(),
            http_proxy: first_env(&["HTTP_PROXY", "http_proxy"]),
            https_proxy: first_env(&["HTTPS_PROXY", "https_proxy"]),
            all_proxy: first_env(&["ALL_PROXY", "all_proxy"]),
            no_proxy: first_env(&["NO_PROXY", "no_proxy"]),
        }
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

    pub fn apply_to_reqwest_builder(
        &self,
        mut builder: reqwest::ClientBuilder,
    ) -> Result<reqwest::ClientBuilder, WebPlatformError> {
        if self.mode == ProxyMode::Disabled {
            return Ok(builder.no_proxy());
        }

        let no_proxy = self
            .no_proxy
            .as_deref()
            .and_then(reqwest::NoProxy::from_string);
        if let Some(url) = &self.http_proxy {
            builder = builder.proxy(with_no_proxy(reqwest::Proxy::http(url)?, no_proxy.clone()));
        }
        if let Some(url) = &self.https_proxy {
            builder = builder.proxy(with_no_proxy(reqwest::Proxy::https(url)?, no_proxy.clone()));
        }
        if let Some(url) = &self.all_proxy {
            builder = builder.proxy(with_no_proxy(reqwest::Proxy::all(url)?, no_proxy.clone()));
        }
        Ok(builder)
    }

    pub fn proxy_env_summary(&self) -> Vec<(String, String)> {
        let mut rows = vec![
            (
                "SYMPHONY_PROXY_MODE".to_string(),
                self.mode.as_str().to_string(),
            ),
            ("SYMPHONY_PROXY_VERSION".to_string(), self.version.clone()),
            ("SYMPHONY_PROXY_SOURCE".to_string(), self.source.clone()),
        ];
        if self.mode != ProxyMode::Disabled {
            for (name, value) in [
                ("HTTP_PROXY", self.http_proxy.as_deref()),
                ("HTTPS_PROXY", self.https_proxy.as_deref()),
                ("ALL_PROXY", self.all_proxy.as_deref()),
                ("NO_PROXY", self.no_proxy.as_deref()),
            ] {
                if let Some(value) = value {
                    rows.push((name.to_string(), redact_proxy_env_value(name, value)));
                }
            }
        }
        rows
    }
}

pub fn is_network_proxy_key(key: &str) -> bool {
    key.starts_with("network_proxy.")
}

pub fn filter_system_configs(configs: Vec<SystemConfigItem>) -> Vec<SystemConfigItem> {
    configs
        .into_iter()
        .filter(|item| !is_network_proxy_key(&item.key))
        .collect()
}

pub fn validate_proxy_url(value: &str) -> Result<(), WebPlatformError> {
    if value.contains("***") {
        return Err(WebPlatformError::BadRequest(
            "proxy url cannot contain masked placeholder".to_string(),
        ));
    }
    let url = reqwest::Url::parse(value)
        .map_err(|_| WebPlatformError::BadRequest("invalid proxy url".to_string()))?;
    match url.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(WebPlatformError::BadRequest(
                "proxy url scheme must be http or https".to_string(),
            ))
        }
    }
    if url.host_str().is_none() {
        return Err(WebPlatformError::BadRequest(
            "proxy url host is required".to_string(),
        ));
    }
    if url.port_or_known_default().is_none() {
        return Err(WebPlatformError::BadRequest(
            "proxy url port is required".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_no_proxy(value: &str) -> Result<(), WebPlatformError> {
    for raw in value.split(',') {
        let item = raw.trim();
        if item.is_empty() || item == "*" {
            continue;
        }
        if item.starts_with('[') && item.ends_with(']') {
            item.trim_matches(&['[', ']'][..])
                .parse::<std::net::Ipv6Addr>()
                .map_err(|_| {
                    WebPlatformError::BadRequest(format!("invalid NO_PROXY rule '{}'", item))
                })?;
            continue;
        }
        if item.parse::<std::net::IpAddr>().is_ok() || item.contains('/') {
            if item.contains('/') {
                let mut parts = item.split('/');
                let ip = parts.next().unwrap_or_default();
                let prefix = parts.next().unwrap_or_default();
                if parts.next().is_some()
                    || ip.parse::<std::net::IpAddr>().is_err()
                    || prefix.parse::<u8>().is_err()
                {
                    return Err(WebPlatformError::BadRequest(format!(
                        "invalid NO_PROXY CIDR rule '{}'",
                        item
                    )));
                }
            }
            continue;
        }
        if item.matches(':').count() == 1 {
            return Err(WebPlatformError::BadRequest(format!(
                "NO_PROXY port-qualified rule '{}' is not supported",
                item
            )));
        }
    }
    Ok(())
}

pub fn normalize_no_proxy(value: &str, auto_bypass_local: bool) -> Option<String> {
    let mut entries: Vec<String> = value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if auto_bypass_local {
        for local in ["localhost", "127.0.0.1", "::1"] {
            if !entries.iter().any(|item| item == local) {
                entries.push(local.to_string());
            }
        }
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries.join(","))
    }
}

pub fn redact_proxy_url_or_text(value: &str) -> String {
    redact_proxy_url(value).unwrap_or_else(|_| "<invalid proxy url>".to_string())
}

fn redact_proxy_env_value(name: &str, value: &str) -> String {
    if name == "NO_PROXY" {
        value.to_string()
    } else {
        redact_proxy_url_or_text(value)
    }
}

pub fn redact_proxy_url(value: &str) -> Result<String, WebPlatformError> {
    let mut url = reqwest::Url::parse(value)
        .map_err(|_| WebPlatformError::BadRequest("invalid proxy url".to_string()))?;
    url.set_query(None);
    if !url.username().is_empty() {
        let masked = mask_username(url.username());
        let _ = url.set_username(&masked);
    }
    if url.password().is_some() {
        let _ = url.set_password(Some("***"));
    }
    Ok(url.to_string())
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

fn first_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| std::env::var(name).ok())
}

fn set_both(cmd: &mut tokio::process::Command, upper: &str, lower: &str, value: Option<&str>) {
    if let Some(value) = value {
        cmd.env(upper, value);
        cmd.env(lower, value);
    }
}

fn with_no_proxy(proxy: reqwest::Proxy, no_proxy: Option<reqwest::NoProxy>) -> reqwest::Proxy {
    proxy.no_proxy(no_proxy)
}

impl From<reqwest::Error> for WebPlatformError {
    fn from(value: reqwest::Error) -> Self {
        WebPlatformError::ExternalService(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_proxy_url_hides_credentials_and_query() {
        let redacted =
            redact_proxy_url("http://user:password@proxy.example.com:8080?token=secret").unwrap();

        assert_eq!(redacted, "http://u***r:***@proxy.example.com:8080/");
    }

    #[test]
    fn validate_no_proxy_rejects_port_qualified_host() {
        assert!(validate_no_proxy("localhost:3000").is_err());
        assert!(validate_no_proxy("example.com:443").is_err());
        assert!(validate_no_proxy("::1").is_ok());
    }

    #[test]
    fn proxy_env_summary_keeps_no_proxy_rules_readable() {
        let summary = EffectiveProxyConfig {
            mode: ProxyMode::Manual,
            version: "8".to_string(),
            source: "draft".to_string(),
            http_proxy: Some("http://user:password@127.0.0.1:7890".to_string()),
            https_proxy: None,
            all_proxy: None,
            no_proxy: Some("localhost,127.0.0.1,::1".to_string()),
        }
        .proxy_env_summary();

        assert!(summary.contains(&(
            "HTTP_PROXY".to_string(),
            "http://u***r:***@127.0.0.1:7890/".to_string(),
        )));
        assert!(summary.contains(&(
            "NO_PROXY".to_string(),
            "localhost,127.0.0.1,::1".to_string(),
        )));
    }
}
