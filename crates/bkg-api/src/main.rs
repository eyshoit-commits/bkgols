use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use bkg_core::{
    build_health_status, PlatformState, PluginError, PluginRegistry, SandboxCommand,
    SandboxManager, SandboxSpec, SandboxState, SandboxStatus,
};
use prometheus::{Encoder, IntGauge, Registry, TextEncoder};
use serde::{Deserialize, Serialize};
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let platform = PlatformState::new();
    register_plugins(&platform.plugin_registry)?;
    platform.initialize_plugins().await?;

    let metrics = AppMetrics::new();
    metrics.refresh(&platform.sandbox_manager);

    let app_state = AppState { platform, metrics };

    let app = Router::new()
        .route("/healthz", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route(
            "/api/v1/sandboxes",
            get(list_sandboxes).post(create_sandbox),
        )
        .route("/api/v1/sandboxes/:id/start", post(start_sandbox))
        .route("/api/v1/sandboxes/:id/stop", post(stop_sandbox))
        .route("/api/v1/sandboxes/:id/exec", post(exec_sandbox))
        .route("/api/v1/sandboxes/:id/status", get(status_sandbox))
        .with_state(app_state)
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    info!(%addr, "starting BKG API server");

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn register_plugins(registry: &PluginRegistry) -> Result<()> {
    plugin_cave::register_plugin(registry)?;
    plugin_p2p::register_plugin(registry)?;
    Ok(())
}

#[derive(Clone)]
struct AppState {
    platform: PlatformState,
    metrics: AppMetrics,
}

#[derive(Clone)]
struct AppMetrics {
    registry: Arc<Registry>,
    total_sandboxes: IntGauge,
    running_sandboxes: IntGauge,
}

impl AppMetrics {
    fn new() -> Self {
        let registry = Registry::new();
        let total_sandboxes =
            IntGauge::new("bkg_sandboxes_total", "Total number of sandboxes").expect("gauge");
        let running_sandboxes =
            IntGauge::new("bkg_sandboxes_running", "Number of running sandboxes").expect("gauge");
        registry
            .register(Box::new(total_sandboxes.clone()))
            .expect("register total_sandboxes");
        registry
            .register(Box::new(running_sandboxes.clone()))
            .expect("register running_sandboxes");
        Self {
            registry: Arc::new(registry),
            total_sandboxes,
            running_sandboxes,
        }
    }

    fn refresh(&self, manager: &SandboxManager) {
        let sandboxes = manager.list();
        self.total_sandboxes.set(sandboxes.len() as i64);
        let running = sandboxes
            .into_iter()
            .filter(|s| s.state == SandboxState::Running)
            .count();
        self.running_sandboxes.set(running as i64);
    }

    fn gather(&self) -> Vec<prometheus::proto::MetricFamily> {
        self.registry.gather()
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install signal handler");
        sigterm.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[derive(Debug, Deserialize)]
struct CreateSandboxRequest {
    project: String,
    runtime: String,
    limits: Option<SandboxLimitsRequest>,
}

#[derive(Debug, Deserialize)]
struct SandboxLimitsRequest {
    cpu_cores: Option<u32>,
    memory_mb: Option<u32>,
    disk_mb: Option<u32>,
}

#[derive(Debug, Serialize)]
struct CreateSandboxResponse {
    sandbox: SandboxStatus,
}

async fn create_sandbox(
    State(state): State<AppState>,
    Json(payload): Json<CreateSandboxRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let limits = payload
        .limits
        .map(|l| bkg_core::SandboxLimits {
            cpu_cores: l.cpu_cores.unwrap_or(1),
            memory_mb: l.memory_mb.unwrap_or(512),
            disk_mb: l.disk_mb.unwrap_or(1024),
        })
        .unwrap_or_default();

    let spec = SandboxSpec {
        project: payload.project,
        runtime: payload.runtime,
        limits,
    };

    let id = Uuid::new_v4().to_string();
    let status = state
        .platform
        .sandbox_manager
        .create(id.clone(), spec)
        .await
        .map_err(ApiError::from)?;

    state.metrics.refresh(&state.platform.sandbox_manager);

    Ok((
        StatusCode::CREATED,
        Json(CreateSandboxResponse { sandbox: status }),
    ))
}

#[derive(Debug, Deserialize)]
struct ExecRequest {
    command: String,
}

async fn start_sandbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SandboxStatus>, ApiError> {
    let status = state
        .platform
        .sandbox_manager
        .start(&id)
        .await
        .map_err(ApiError::from)?;
    state.metrics.refresh(&state.platform.sandbox_manager);
    Ok(Json(status))
}

async fn stop_sandbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SandboxStatus>, ApiError> {
    let status = state
        .platform
        .sandbox_manager
        .stop(&id)
        .await
        .map_err(ApiError::from)?;
    state.metrics.refresh(&state.platform.sandbox_manager);
    Ok(Json(status))
}

async fn exec_sandbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ExecRequest>,
) -> Result<Json<bkg_core::SandboxExecution>, ApiError> {
    let result = state
        .platform
        .sandbox_manager
        .exec(
            &id,
            SandboxCommand {
                command: payload.command,
            },
        )
        .await
        .map_err(ApiError::from)?;
    state.metrics.refresh(&state.platform.sandbox_manager);
    Ok(Json(result))
}

async fn status_sandbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SandboxStatus>, ApiError> {
    let status = state
        .platform
        .sandbox_manager
        .list()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "sandbox not found".to_string()))?;
    Ok(Json(status))
}

async fn list_sandboxes(State(state): State<AppState>) -> Json<Vec<SandboxStatus>> {
    Json(state.platform.sandbox_manager.list())
}

async fn health_handler(State(state): State<AppState>) -> Json<bkg_core::HealthStatus> {
    Json(build_health_status(&state.platform))
}

async fn metrics_handler(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let metric_families = state.metrics.gather();
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    encoder
        .encode(&metric_families, &mut buffer)
        .map_err(|err| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok((StatusCode::OK, buffer))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: String) -> Self {
        Self { status, message }
    }
}

impl From<PluginError> for ApiError {
    fn from(value: PluginError) -> Self {
        match value {
            PluginError::SandboxNotFound(id) => {
                ApiError::new(StatusCode::NOT_FOUND, format!("sandbox {id} not found"))
            }
            PluginError::RuntimeNotFound(name) => ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("runtime {name} is not registered"),
            ),
            other => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = Json(serde_json::json!({
            "error": self.message,
        }));
        (self.status, body).into_response()
    }
}
