use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result type for plugin operations.
pub type PluginResult<T = ()> = Result<T, PluginError>;

/// Errors that may be produced by plugins or plugin host interactions.
#[derive(Debug, Error)]
pub enum PluginError {
    /// Any validation error while loading or configuring a plugin.
    #[error("plugin validation error: {0}")]
    Validation(String),
    /// I/O or environment setup failure.
    #[error("plugin environment error: {0}")]
    Environment(String),
    /// Generic error variant, primarily for unexpected situations.
    #[error("plugin error: {0}")]
    Other(String),
}

impl PluginError {
    /// Helper for creating an [`PluginError::Other`] variant from any error implementing
    /// [`std::fmt::Display`].
    pub fn other<E: std::fmt::Display>(err: E) -> Self {
        Self::Other(err.to_string())
    }
}

/// Lightweight metadata describing a plugin that can be surfaced to operators and telemetry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Stable plugin identifier.
    pub id: String,
    /// Human readable plugin name.
    pub name: String,
    /// Optional semantic version.
    pub version: Option<String>,
    /// Optional descriptive text for dashboards.
    pub description: Option<String>,
    /// Arbitrary labels used for governance or scheduling decisions.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

/// Capability flags expose the functionality offered by a plugin to the rest of the platform.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginCapabilities {
    /// Plugin can provide sandbox runtimes (e.g. Python, Node).
    pub provides_runtime: bool,
    /// Plugin contributes telemetry exporters.
    pub provides_telemetry: bool,
    /// Plugin offers custom API routes (e.g. WebSocket relays).
    pub provides_api: bool,
}

impl Default for PluginCapabilities {
    fn default() -> Self {
        Self {
            provides_runtime: false,
            provides_telemetry: false,
            provides_api: false,
        }
    }
}

/// Structured configuration passed from `cave.yaml` to plugins.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginConfig {
    /// Arbitrary configuration payload. Keys map to plugin specific options.
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
}

/// Information about the global runtime environment and helper utilities exposed to plugins.
#[derive(Clone)]
pub struct PluginContext<'a> {
    /// Namespace associated with the plugin execution scope.
    pub namespace: &'a str,
    /// Environment variables that should be set for sandbox processes.
    pub environment: &'a BTreeMap<String, String>,
    /// Timeout recommended for long running operations.
    pub operation_timeout: Duration,
}

impl<'a> PluginContext<'a> {
    pub fn new(
        namespace: &'a str,
        environment: &'a BTreeMap<String, String>,
        operation_timeout: Duration,
    ) -> Self {
        Self {
            namespace,
            environment,
            operation_timeout,
        }
    }
}

/// Entry point trait that every plugin must implement.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Metadata describing the plugin. Called once during registration.
    fn metadata(&self) -> PluginMetadata;

    /// Capabilities offered by the plugin.
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities::default()
    }

    /// Called after the plugin is constructed, allowing it to read configuration and prepare
    /// internal state.
    async fn initialize(&self, _config: PluginConfig) -> PluginResult<()> {
        Ok(())
    }

    /// Called before the plugin begins serving traffic or workloads.
    async fn start(&self, _ctx: PluginContext<'_>) -> PluginResult<()> {
        Ok(())
    }

    /// Called when the runtime is shutting down.
    async fn shutdown(&self) -> PluginResult<()> {
        Ok(())
    }
}

/// Plugins compiled as dynamic libraries must export this constructor symbol.
pub const PLUGIN_CREATE_SYMBOL: &[u8] = b"bkg_plugin_create";

/// Optional destructor symbol that is invoked when unloading a plugin.
pub const PLUGIN_DROP_SYMBOL: &[u8] = b"bkg_plugin_drop";
