use std::collections::BTreeMap;
use std::ffi::c_void;
use std::sync::Arc;

use async_trait::async_trait;
use bkg_plugin_api::{
    Plugin, PluginCapabilities, PluginConfig, PluginContext, PluginMetadata, PluginResult,
};
use tracing::info;

#[derive(Default)]
struct LoggerPlugin {
    metadata: Arc<PluginMetadata>,
}

impl LoggerPlugin {
    fn new() -> Self {
        Self {
            metadata: Arc::new(PluginMetadata {
                id: "bkg.logger".to_string(),
                name: "Logger Plugin".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                description: Some("Writes sandbox lifecycle events to tracing".to_string()),
                labels: BTreeMap::new(),
            }),
        }
    }
}

#[async_trait]
impl Plugin for LoggerPlugin {
    fn metadata(&self) -> PluginMetadata {
        (*self.metadata).clone()
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            provides_runtime: false,
            provides_telemetry: true,
            provides_api: false,
        }
    }

    async fn initialize(&self, config: PluginConfig) -> PluginResult<()> {
        if !config.options.is_empty() {
            info!(target: "bkg.plugins.logger", ?config.options, "initializing logger plugin with custom config");
        }
        Ok(())
    }

    async fn start(&self, ctx: PluginContext<'_>) -> PluginResult<()> {
        info!(
            target: "bkg.plugins.logger",
            namespace = ctx.namespace,
            timeout_secs = ctx.operation_timeout.as_secs_f32(),
            env_len = ctx.environment.len(),
            "logger plugin started"
        );
        Ok(())
    }

    async fn shutdown(&self) -> PluginResult<()> {
        info!(target: "bkg.plugins.logger", "logger plugin shutting down");
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn bkg_plugin_create() -> *mut c_void {
    let plugin: Box<dyn Plugin> = Box::new(LoggerPlugin::new());
    Box::into_raw(plugin) as *mut c_void
}

#[no_mangle]
pub extern "C" fn bkg_plugin_drop(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(ptr as *mut dyn Plugin));
    }
}
