use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;

use bkg_core::{CavePlugin, PluginCapabilities, PluginContext, PluginMetadata, PluginResult};

const PLUGIN_NAME: &str = "plugin_p2p";
const PLUGIN_VERSION: &str = "0.1.0";

#[derive(Default)]
pub struct P2pPlugin;

#[async_trait]
impl CavePlugin for P2pPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: PLUGIN_NAME.to_string(),
            version: PLUGIN_VERSION.to_string(),
            description: "P2P networking facade with signed manifest support".to_string(),
            capabilities: PluginCapabilities {
                provides_runtime: false,
                telemetry: true,
                audit: true,
            },
        }
    }

    async fn initialize(&self, _ctx: PluginContext) -> PluginResult<()> {
        info!(plugin = PLUGIN_NAME, "initializing P2P plugin");
        Ok(())
    }

    async fn shutdown(&self) -> PluginResult<()> {
        info!(plugin = PLUGIN_NAME, "shutting down P2P plugin");
        Ok(())
    }
}

pub fn register_plugin(registry: &bkg_core::PluginRegistry) -> PluginResult<()> {
    registry.register(Arc::new(P2pPlugin::default()))
}
