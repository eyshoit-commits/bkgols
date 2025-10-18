use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum SandboxRuntime {
    Python,
    Node,
    Rust,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SandboxState {
    Created,
    Running,
    Stopped,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    #[serde(default = "ResourceLimits::default_memory")]
    pub memory_mb: u64,
    #[serde(default = "ResourceLimits::default_cpu")]
    pub cpu_millicores: u64,
    #[serde(default = "ResourceLimits::default_execution")]
    pub execution_timeout_secs: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            memory_mb: Self::default_memory(),
            cpu_millicores: Self::default_cpu(),
            execution_timeout_secs: Self::default_execution(),
        }
    }
}

impl ResourceLimits {
    fn default_memory() -> u64 {
        512
    }

    fn default_cpu() -> u64 {
        500
    }

    fn default_execution() -> u64 {
        300
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub project: String,
    pub runtime: SandboxRuntime,
    #[serde(default)]
    pub limits: ResourceLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sandbox {
    pub id: Uuid,
    pub project: String,
    pub runtime: SandboxRuntime,
    pub state: SandboxState,
    pub limits: ResourceLimits,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub audit_log: Vec<ExecRecord>,
}

impl Sandbox {
    fn new(config: SandboxConfig) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            project: config.project,
            runtime: config.runtime,
            state: SandboxState::Created,
            limits: config.limits,
            created_at: now,
            updated_at: now,
            audit_log: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRecord {
    pub id: Uuid,
    pub command: String,
    pub timestamp: DateTime<Utc>,
    pub status: ExecStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecStatus {
    Accepted,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Default)]
pub struct SandboxManager {
    inner: Arc<RwLock<HashMap<Uuid, Sandbox>>>,
}

impl SandboxManager {
    pub async fn create_sandbox(&self, config: SandboxConfig) -> Result<Uuid> {
        let sandbox = Sandbox::new(config);
        let id = sandbox.id;
        self.inner.write().insert(id, sandbox);
        Ok(id)
    }

    pub async fn start_sandbox(&self, id: Uuid) -> Result<()> {
        let mut guard = self.inner.write();
        let sandbox = guard
            .get_mut(&id)
            .ok_or_else(|| anyhow!("sandbox {id} not found"))?;
        sandbox.state = SandboxState::Running;
        sandbox.updated_at = Utc::now();
        Ok(())
    }

    pub async fn stop_sandbox(&self, id: Uuid) -> Result<()> {
        let mut guard = self.inner.write();
        let sandbox = guard
            .get_mut(&id)
            .ok_or_else(|| anyhow!("sandbox {id} not found"))?;
        sandbox.state = SandboxState::Stopped;
        sandbox.updated_at = Utc::now();
        Ok(())
    }

    pub async fn exec_in_sandbox(&self, id: Uuid, command: String) -> Result<Uuid> {
        let mut guard = self.inner.write();
        let sandbox = guard
            .get_mut(&id)
            .ok_or_else(|| anyhow!("sandbox {id} not found"))?;
        if sandbox.state != SandboxState::Running {
            return Err(anyhow!("sandbox {id} must be running"));
        }
        let record = ExecRecord {
            id: Uuid::new_v4(),
            command,
            timestamp: Utc::now(),
            status: ExecStatus::Accepted,
        };
        let exec_id = record.id;
        sandbox.audit_log.push(record);
        sandbox.updated_at = Utc::now();
        Ok(exec_id)
    }

    pub async fn sandbox_status(&self, id: Uuid) -> Result<SandboxState> {
        let guard = self.inner.read();
        let sandbox = guard
            .get(&id)
            .ok_or_else(|| anyhow!("sandbox {id} not found"))?;
        Ok(sandbox.state.clone())
    }

    pub async fn remove_sandbox(&self, id: Uuid) -> Result<()> {
        let mut guard = self.inner.write();
        let mut sandbox = guard
            .remove(&id)
            .ok_or_else(|| anyhow!("sandbox {id} not found"))?;
        sandbox.state = SandboxState::Deleted;
        Ok(())
    }

    pub fn list(&self) -> Vec<Sandbox> {
        self.inner.read().values().cloned().collect()
    }
}
