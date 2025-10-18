use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use bkg_plugin_api::{
    Plugin, PluginCapabilities, PluginConfig, PluginContext, PluginMetadata, PluginResult,
    PLUGIN_CREATE_SYMBOL,
};
use futures::future::join_all;
use libloading::{Library, Symbol};
use notify::{Config as NotifyConfig, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct PluginSummary {
    pub metadata: PluginMetadata,
    pub capabilities: PluginCapabilities,
    pub path: PathBuf,
}

#[derive(Clone)]
pub struct PluginManager {
    inner: Arc<PluginManagerInner>,
}

struct PluginManagerInner {
    plugin_configs: BTreeMap<String, PluginConfig>,
    handles: RwLock<HashMap<String, Arc<PluginHandle>>>,
}

impl PluginManager {
    pub fn new(plugin_configs: BTreeMap<String, PluginConfig>) -> Self {
        Self {
            inner: Arc::new(PluginManagerInner {
                plugin_configs,
                handles: RwLock::new(HashMap::new()),
            }),
        }
    }

    pub fn preview(&self, directory: PathBuf) -> Result<()> {
        if !directory.exists() {
            println!("plugin directory {:?} does not exist", directory);
            return Ok(());
        }
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            if is_dynamic_library(entry.path().as_path()) {
                println!("found plugin candidate: {}", entry.path().display());
            }
        }
        Ok(())
    }

    pub async fn load_from_directory(
        &self,
        directory: &Path,
        namespace: &str,
        environment: &BTreeMap<String, String>,
    ) -> Result<()> {
        if !directory.exists() {
            warn!("plugin directory {:?} missing", directory);
            return Ok(());
        }
        let mut errors = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            match entry {
                Ok(dir_entry) => {
                    let path = dir_entry.path();
                    if !is_dynamic_library(&path) {
                        continue;
                    }
                    if let Err(err) = self
                        .load_plugin(&path, namespace.to_string(), environment.clone())
                        .await
                    {
                        errors.push((path.clone(), err));
                    }
                }
                Err(err) => errors.push((directory.to_path_buf(), anyhow!(err))),
            }
        }

        for (path, err) in errors {
            error!(?path, %err, "failed to load plugin");
        }

        Ok(())
    }

    pub fn plugin_summaries(&self) -> Vec<PluginSummary> {
        self.inner
            .handles
            .read()
            .values()
            .map(|handle| handle.summary())
            .collect()
    }

    async fn load_plugin(
        &self,
        path: &Path,
        namespace: String,
        environment: BTreeMap<String, String>,
    ) -> Result<()> {
        unsafe {
            let library = Library::new(path)
                .with_context(|| format!("failed to load dynamic library {:?}", path))?;
            let constructor: Symbol<unsafe extern "C" fn() -> *mut c_void> = library
                .get(PLUGIN_CREATE_SYMBOL)
                .with_context(|| format!("plugin {} missing constructor", path.display()))?;
            let raw_ptr = constructor();
            if raw_ptr.is_null() {
                return Err(anyhow!("plugin constructor returned null"));
            }
            let mut plugin: Box<dyn Plugin> = Box::from_raw(raw_ptr as *mut dyn Plugin);
            let metadata = plugin.metadata();
            let capabilities = plugin.capabilities();
            let plugin_config = self
                .inner
                .plugin_configs
                .get(&metadata.id)
                .cloned()
                .unwrap_or_default();
            let operation_timeout = plugin_config
                .options
                .get("operation_timeout_secs")
                .and_then(|value| value.as_u64())
                .map(Duration::from_secs)
                .unwrap_or_else(|| Duration::from_secs(30));

            plugin
                .initialize(plugin_config.clone())
                .await
                .with_context(|| format!("failed to initialize plugin {}", metadata.id))?;

            let handle = Arc::new(PluginHandle::new(
                path.to_path_buf(),
                metadata.clone(),
                capabilities,
                library,
                plugin,
                operation_timeout,
            ));

            handle
                .start(namespace.clone(), environment.clone())
                .await
                .with_context(|| format!("failed to start plugin {}", metadata.id))?;

            let previous = {
                let mut guard = self.inner.handles.write();
                guard.insert(metadata.id.clone(), handle.clone())
            };

            if let Some(old) = previous {
                old.shutdown().await;
            }

            info!(plugin = %metadata.id, name = %metadata.name, ?capabilities, path = %path.display(), "loaded plugin");
        }

        Ok(())
    }

    pub async fn shutdown_all(&self) {
        let handles: Vec<Arc<PluginHandle>> = self.inner.handles.read().values().cloned().collect();
        let futures = handles.iter().map(|handle| handle.shutdown());
        join_all(futures).await;
        self.inner.handles.write().clear();
    }

    pub async fn watch_directory(
        &self,
        directory: PathBuf,
        namespace: String,
        environment: BTreeMap<String, String>,
    ) -> Result<()> {
        if !directory.exists() {
            warn!("plugin watch directory {:?} does not exist", directory);
        }

        let (tx, mut rx) = mpsc::channel(32);
        let tx_clone = tx.clone();
        let mut watcher: RecommendedWatcher = RecommendedWatcher::new(
            move |event| {
                if let Ok(event) = event {
                    if let Err(err) = tx_clone.blocking_send(event) {
                        error!(%err, "failed to send plugin watch event");
                    }
                }
            },
            NotifyConfig::default(),
        )?;

        watcher.watch(&directory, RecursiveMode::Recursive)?;
        info!("watching plugin directory {}", directory.display());

        while let Some(event) = rx.recv().await {
            match event.kind {
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                    info!(?event, "plugin change detected, reloading");
                    if let Err(err) = self
                        .load_from_directory(&directory, &namespace, &environment)
                        .await
                    {
                        error!(%err, "failed to reload plugins after change");
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}

struct PluginHandle {
    path: PathBuf,
    metadata: PluginMetadata,
    capabilities: PluginCapabilities,
    library: Library,
    instance: Box<dyn Plugin>,
    operation_timeout: Duration,
}

impl PluginHandle {
    fn new(
        path: PathBuf,
        metadata: PluginMetadata,
        capabilities: PluginCapabilities,
        library: Library,
        instance: Box<dyn Plugin>,
        operation_timeout: Duration,
    ) -> Self {
        Self {
            path,
            metadata,
            capabilities,
            library,
            instance,
            operation_timeout,
        }
    }

    async fn start(
        &self,
        namespace: String,
        environment: BTreeMap<String, String>,
    ) -> PluginResult<()> {
        let ctx = PluginContext::new(&namespace, &environment, self.operation_timeout);
        self.instance.start(ctx).await
    }

    async fn shutdown(&self) {
        if let Err(err) = self.instance.shutdown().await {
            error!(plugin = %self.metadata.id, %err, "plugin shutdown failed");
        }
    }

    fn summary(&self) -> PluginSummary {
        PluginSummary {
            metadata: self.metadata.clone(),
            capabilities: self.capabilities,
            path: self.path.clone(),
        }
    }
}

fn is_dynamic_library(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("so") | Some("dylib") | Some("dll") => true,
        _ => false,
    }
}
