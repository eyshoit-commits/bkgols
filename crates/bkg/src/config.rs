use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_with::{serde_as, DurationSecondsWithFrac};

use bkg_plugin_api::PluginConfig;

const DEFAULT_CONFIG_PATH: &str = "cave.yaml";

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default = "AppConfig::default_namespace")]
    pub namespace: String,
    #[serde(default = "AppConfig::default_plugin_directory")]
    pub plugin_directory: PathBuf,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            namespace: Self::default_namespace(),
            plugin_directory: Self::default_plugin_directory(),
            environment: BTreeMap::new(),
            telemetry: TelemetryConfig::default(),
            server: ServerConfig::default(),
            plugins: BTreeMap::new(),
        }
    }
}

impl AppConfig {
    pub fn load(path: Option<&PathBuf>) -> Result<Self> {
        let path = path
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));

        if !path.exists() {
            tracing::warn!(
                "config file not found at {:?}, falling back to defaults",
                path
            );
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config file at {:?}", path))?;
        let mut config: AppConfig = serde_yaml::from_str(&contents)
            .with_context(|| format!("failed to parse config file at {:?}", path))?;

        if config.namespace.is_empty() {
            config.namespace = Self::default_namespace();
        }

        Ok(config)
    }

    fn default_namespace() -> String {
        "default".to_string()
    }

    fn default_plugin_directory() -> PathBuf {
        PathBuf::from("plugins/dist")
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default)]
    pub otel_endpoint: Option<String>,
    #[serde_as(as = "Option<DurationSecondsWithFrac>")]
    #[serde(default = "TelemetryConfig::default_sampling_rate")]
    pub sampling_interval: Option<Duration>,
    #[serde(default = "TelemetryConfig::default_enabled")]
    pub enabled: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            otel_endpoint: None,
            sampling_interval: Self::default_sampling_rate(),
            enabled: true,
        }
    }
}

impl TelemetryConfig {
    fn default_sampling_rate() -> Option<Duration> {
        Some(Duration::from_secs(1))
    }

    fn default_enabled() -> bool {
        true
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "ServerConfig::default_bind_address")]
    pub bind_address: String,
    #[serde(default = "ServerConfig::default_port")]
    pub port: u16,
    #[serde(default = "ServerConfig::default_health_path")]
    pub health_path: String,
    #[serde(default = "ServerConfig::default_metrics_path")]
    pub metrics_path: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: Self::default_bind_address(),
            port: Self::default_port(),
            health_path: Self::default_health_path(),
            metrics_path: Self::default_metrics_path(),
        }
    }
}

impl ServerConfig {
    fn default_bind_address() -> String {
        "0.0.0.0".to_string()
    }

    fn default_port() -> u16 {
        8080
    }

    fn default_health_path() -> String {
        "/healthz".to_string()
    }

    fn default_metrics_path() -> String {
        "/metrics".to_string()
    }

    pub fn address(&self) -> String {
        format!("{}:{}", self.bind_address, self.port)
    }

    pub fn bind_socket_addr(&self) -> std::net::SocketAddr {
        format!("{}:{}", self.bind_address, self.port)
            .parse()
            .expect("invalid bind address")
    }
}
