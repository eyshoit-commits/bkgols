use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use tracing::info;

use bkg_core::{
    CavePlugin, ManagedSandbox, PluginCapabilities, PluginContext, PluginError, PluginMetadata,
    PluginResult, SandboxCommand, SandboxExecution, SandboxRuntime, SandboxSpec, SandboxState,
    SandboxStatus,
};

const PLUGIN_NAME: &str = "plugin_cave";
const PLUGIN_VERSION: &str = "0.1.0";
const RUNTIME_NAME: &str = "wasm_quick";

#[derive(Default)]
pub struct CavePluginRuntime;

#[async_trait]
impl CavePlugin for CavePluginRuntime {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: PLUGIN_NAME.to_string(),
            version: PLUGIN_VERSION.to_string(),
            description: "In-Memory sandbox runtime used for development and testing".to_string(),
            capabilities: PluginCapabilities {
                provides_runtime: true,
                telemetry: true,
                audit: true,
            },
        }
    }

    async fn initialize(&self, ctx: PluginContext) -> PluginResult<()> {
        let registry = ctx.runtime_registry();
        registry.register_runtime(Arc::new(InMemoryRuntime::default()))?;
        Ok(())
    }

    async fn shutdown(&self) -> PluginResult<()> {
        info!(plugin = PLUGIN_NAME, "plugin shutdown invoked");
        Ok(())
    }
}

#[derive(Default)]
struct InMemoryRuntime;

#[async_trait]
impl SandboxRuntime for InMemoryRuntime {
    fn name(&self) -> &'static str {
        RUNTIME_NAME
    }

    async fn create(&self, id: String, spec: SandboxSpec) -> PluginResult<Arc<dyn ManagedSandbox>> {
        Ok(Arc::new(InMemorySandbox::new(id, spec)))
    }
}

struct InMemorySandbox {
    state: RwLock<SandboxStatus>,
}

impl InMemorySandbox {
    fn new(id: String, spec: SandboxSpec) -> Self {
        let status = SandboxStatus {
            id,
            state: SandboxState::Created,
            runtime: spec.runtime,
            project: spec.project,
            limits: spec.limits,
            last_execution: None,
        };
        Self {
            state: RwLock::new(status),
        }
    }
}

#[async_trait]
impl ManagedSandbox for InMemorySandbox {
    async fn start(&self) -> PluginResult<()> {
        let mut status = self.state.write();
        status.state = SandboxState::Running;
        Ok(())
    }

    async fn stop(&self) -> PluginResult<()> {
        let mut status = self.state.write();
        status.state = SandboxState::Stopped;
        Ok(())
    }

    async fn exec(&self, command: SandboxCommand) -> PluginResult<SandboxExecution> {
        let mut status = self.state.write();
        if status.state != SandboxState::Running {
            return Err(PluginError::SandboxExecution(format!(
                "sandbox {} is not running",
                status.id
            )));
        }
        let execution = SandboxExecution {
            stdout: format!("executed command: {}", command.command),
            stderr: String::new(),
            exit_code: 0,
            timestamp: chrono::Utc::now(),
        };
        status.last_execution = Some(execution.clone());
        Ok(execution)
    }

    fn status(&self) -> SandboxStatus {
        self.state.read().clone()
    }
}

pub fn register_plugin(registry: &bkg_core::PluginRegistry) -> PluginResult<()> {
    registry.register(Arc::new(CavePluginRuntime::default()))
}
