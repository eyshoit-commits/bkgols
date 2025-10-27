use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

pub type PluginResult<T> = std::result::Result<T, PluginError>;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin already registered: {0}")]
    PluginAlreadyRegistered(String),
    #[error("plugin not found: {0}")]
    PluginNotFound(String),
    #[error("runtime already registered: {0}")]
    RuntimeAlreadyRegistered(String),
    #[error("runtime not found: {0}")]
    RuntimeNotFound(String),
    #[error("sandbox not found: {0}")]
    SandboxNotFound(String),
    #[error("sandbox execution error: {0}")]
    SandboxExecution(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: PluginCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginCapabilities {
    pub provides_runtime: bool,
    pub telemetry: bool,
    pub audit: bool,
}

#[async_trait]
pub trait CavePlugin: Send + Sync {
    fn metadata(&self) -> PluginMetadata;
    async fn initialize(&self, ctx: PluginContext) -> PluginResult<()>;
    async fn shutdown(&self) -> PluginResult<()>;
}

#[derive(Clone)]
pub struct PluginContext {
    runtime_registry: RuntimeRegistry,
}

impl PluginContext {
    pub fn new(runtime_registry: RuntimeRegistry) -> Self {
        Self { runtime_registry }
    }

    pub fn runtime_registry(&self) -> RuntimeRegistry {
        self.runtime_registry.clone()
    }
}

impl fmt::Debug for PluginContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginContext")
            .field("runtime_registry", &"<opaque>")
            .finish()
    }
}

#[derive(Clone, Default)]
pub struct PluginRegistry {
    plugins: Arc<RwLock<HashMap<String, Arc<dyn CavePlugin>>>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, plugin: Arc<dyn CavePlugin>) -> PluginResult<()> {
        let mut plugins = self.plugins.write();
        let metadata = plugin.metadata();
        if plugins.contains_key(&metadata.name) {
            return Err(PluginError::PluginAlreadyRegistered(metadata.name));
        }
        info!(name = metadata.name, "registering plugin");
        plugins.insert(metadata.name.clone(), plugin);
        Ok(())
    }

    pub fn list(&self) -> Vec<PluginMetadata> {
        self.plugins
            .read()
            .values()
            .map(|plugin| plugin.metadata())
            .collect()
    }

    pub async fn initialize_all(&self, ctx: PluginContext) -> PluginResult<()> {
        for plugin in self.plugins.read().values().cloned() {
            plugin.initialize(ctx.clone()).await?;
        }
        Ok(())
    }

    pub async fn shutdown_all(&self) -> PluginResult<()> {
        for plugin in self.plugins.read().values().cloned() {
            plugin.shutdown().await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSpec {
    pub project: String,
    pub runtime: String,
    pub limits: SandboxLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxLimits {
    pub cpu_cores: u32,
    pub memory_mb: u32,
    pub disk_mb: u32,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            cpu_cores: 1,
            memory_mb: 512,
            disk_mb: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxCommand {
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxExecution {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SandboxState {
    Created,
    Running,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxStatus {
    pub id: String,
    pub state: SandboxState,
    pub runtime: String,
    pub project: String,
    pub limits: SandboxLimits,
    pub last_execution: Option<SandboxExecution>,
}

#[async_trait]
pub trait ManagedSandbox: Send + Sync {
    async fn start(&self) -> PluginResult<()>;
    async fn stop(&self) -> PluginResult<()>;
    async fn exec(&self, command: SandboxCommand) -> PluginResult<SandboxExecution>;
    fn status(&self) -> SandboxStatus;
}

#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    fn name(&self) -> &'static str;
    async fn create(&self, id: String, spec: SandboxSpec) -> PluginResult<Arc<dyn ManagedSandbox>>;
}

#[derive(Clone, Default)]
pub struct RuntimeRegistry {
    runtimes: Arc<RwLock<HashMap<String, Arc<dyn SandboxRuntime>>>>,
}

impl fmt::Debug for RuntimeRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let runtimes = self.runtimes.read();
        let names: Vec<_> = runtimes.keys().cloned().collect();
        f.debug_struct("RuntimeRegistry")
            .field("runtimes", &names)
            .finish()
    }
}

impl RuntimeRegistry {
    pub fn register_runtime(&self, runtime: Arc<dyn SandboxRuntime>) -> PluginResult<()> {
        let mut runtimes = self.runtimes.write();
        let name = runtime.name();
        if runtimes.contains_key(name) {
            return Err(PluginError::RuntimeAlreadyRegistered(name.to_string()));
        }
        info!(runtime = name, "registering runtime");
        runtimes.insert(name.to_string(), runtime);
        Ok(())
    }

    pub fn get(&self, runtime: &str) -> PluginResult<Arc<dyn SandboxRuntime>> {
        self.runtimes
            .read()
            .get(runtime)
            .cloned()
            .ok_or_else(|| PluginError::RuntimeNotFound(runtime.to_string()))
    }

    pub fn list(&self) -> Vec<String> {
        self.runtimes.read().keys().cloned().collect()
    }
}

#[derive(Clone, Default)]
pub struct SandboxManager {
    sandboxes: Arc<RwLock<HashMap<String, Arc<dyn ManagedSandbox>>>>,
    runtime_registry: RuntimeRegistry,
}

impl SandboxManager {
    pub fn new(runtime_registry: RuntimeRegistry) -> Self {
        Self {
            sandboxes: Arc::new(RwLock::new(HashMap::new())),
            runtime_registry,
        }
    }

    pub fn runtime_registry(&self) -> RuntimeRegistry {
        self.runtime_registry.clone()
    }

    pub async fn create(&self, id: String, spec: SandboxSpec) -> PluginResult<SandboxStatus> {
        let runtime = self.runtime_registry.get(&spec.runtime)?;
        let sandbox = runtime.create(id.clone(), spec.clone()).await?;
        let status = sandbox.status();
        self.sandboxes.write().insert(id, sandbox);
        Ok(status)
    }

    pub async fn start(&self, id: &str) -> PluginResult<SandboxStatus> {
        let sandbox = self
            .sandboxes
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| PluginError::SandboxNotFound(id.to_string()))?;
        sandbox.start().await?;
        Ok(sandbox.status())
    }

    pub async fn stop(&self, id: &str) -> PluginResult<SandboxStatus> {
        let sandbox = self
            .sandboxes
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| PluginError::SandboxNotFound(id.to_string()))?;
        sandbox.stop().await?;
        Ok(sandbox.status())
    }

    pub async fn exec(&self, id: &str, command: SandboxCommand) -> PluginResult<SandboxExecution> {
        let sandbox = self
            .sandboxes
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| PluginError::SandboxNotFound(id.to_string()))?;
        sandbox.exec(command).await
    }

    pub fn list(&self) -> Vec<SandboxStatus> {
        self.sandboxes
            .read()
            .values()
            .map(|sandbox| sandbox.status())
            .collect()
    }
}

#[derive(Clone)]
pub struct PlatformState {
    pub plugin_registry: PluginRegistry,
    pub runtime_registry: RuntimeRegistry,
    pub sandbox_manager: SandboxManager,
}

impl PlatformState {
    pub fn new() -> Self {
        let runtime_registry = RuntimeRegistry::default();
        let sandbox_manager = SandboxManager::new(runtime_registry.clone());
        let plugin_registry = PluginRegistry::new();
        Self {
            plugin_registry,
            runtime_registry,
            sandbox_manager,
        }
    }

    pub async fn initialize_plugins(&self) -> PluginResult<()> {
        let ctx = PluginContext::new(self.runtime_registry.clone());
        self.plugin_registry.initialize_all(ctx).await
    }

    pub async fn shutdown_plugins(&self) -> PluginResult<()> {
        self.plugin_registry.shutdown_all().await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub plugins: Vec<PluginMetadata>,
    pub runtimes: Vec<String>,
}

pub fn build_health_status(state: &PlatformState) -> HealthStatus {
    HealthStatus {
        status: "ok".to_string(),
        plugins: state.plugin_registry.list(),
        runtimes: state.runtime_registry.list(),
    }
}
