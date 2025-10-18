use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Json;
use axum::Router;
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use uuid::Uuid;

use crate::config::ServerConfig;
use crate::plugins::{PluginManager, PluginSummary};
use crate::sandbox::{ResourceLimits, SandboxConfig, SandboxManager, SandboxRuntime, SandboxState};

pub struct ApiServer {
    config: ServerConfig,
    state: Arc<AppState>,
}

type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Clone)]
struct AppState {
    sandbox_manager: Arc<SandboxManager>,
    plugin_manager: Arc<PluginManager>,
}

impl ApiServer {
    pub fn new(
        config: ServerConfig,
        sandbox_manager: Arc<SandboxManager>,
        plugin_manager: Arc<PluginManager>,
    ) -> Self {
        Self {
            config,
            state: Arc::new(AppState {
                sandbox_manager,
                plugin_manager,
            }),
        }
    }

    pub async fn serve(self) {
        if let Err(err) = self.run().await {
            error!(%err, "api server terminated with error");
        }
    }

    async fn run(self) -> anyhow::Result<()> {
        let addr = self.config.bind_socket_addr();
        let health_path = self.config.health_path.clone();
        let metrics_path = self.config.metrics_path.clone();
        let state = self.state.clone();

        let api_routes = Router::new()
            .route(
                "/api/v1/sandboxes",
                get(list_sandboxes).post(create_sandbox),
            )
            .route("/api/v1/sandboxes/:id/start", post(start_sandbox))
            .route("/api/v1/sandboxes/:id/stop", post(stop_sandbox))
            .route("/api/v1/sandboxes/:id/exec", post(exec_sandbox))
            .route("/api/v1/sandboxes/:id/status", get(sandbox_status))
            .route("/api/v1/sandboxes/:id", delete(delete_sandbox))
            .route("/api/v1/plugins", get(list_plugins));

        let app = Router::new()
            .route(&health_path, get(health_handler))
            .route(&metrics_path, get(metrics_handler))
            .merge(api_routes)
            .with_state(state);

        info!("starting API server on {}", addr);

        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await?;

        Ok(())
    }
}

async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sandboxes = state.sandbox_manager.list();
    let running = sandboxes
        .iter()
        .filter(|s| s.state == SandboxState::Running)
        .count();
    let content = format!(
        "# TYPE bkg_sandboxes gauge\n\
         bkg_sandboxes {}\n\
         # TYPE bkg_sandboxes_running gauge\n\
         bkg_sandboxes_running {}\n",
        sandboxes.len(),
        running,
    );
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        content,
    )
}

async fn create_sandbox(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateSandboxRequest>,
) -> ApiResult<(StatusCode, Json<CreateSandboxResponse>)> {
    let config = request.into_config();
    let id = state
        .sandbox_manager
        .create_sandbox(config)
        .await
        .map_err(ApiError::internal)?;

    Ok((
        StatusCode::CREATED,
        Json(CreateSandboxResponse {
            id,
            state: SandboxState::Created,
        }),
    ))
}

async fn list_sandboxes(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<SandboxSnapshot>>> {
    let sandboxes = state.sandbox_manager.list();
    let snapshots = sandboxes.into_iter().map(SandboxSnapshot::from).collect();
    Ok(Json(snapshots))
}

async fn start_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SandboxOperationResponse>> {
    state
        .sandbox_manager
        .start_sandbox(id)
        .await
        .map_err(|err| ApiError::not_found(err.to_string()))?;
    Ok(Json(SandboxOperationResponse {
        id,
        state: SandboxState::Running,
    }))
}

async fn stop_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SandboxOperationResponse>> {
    state
        .sandbox_manager
        .stop_sandbox(id)
        .await
        .map_err(|err| ApiError::not_found(err.to_string()))?;
    Ok(Json(SandboxOperationResponse {
        id,
        state: SandboxState::Stopped,
    }))
}

async fn exec_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(request): Json<ExecSandboxRequest>,
) -> ApiResult<Json<ExecSandboxResponse>> {
    let exec_id = state
        .sandbox_manager
        .exec_in_sandbox(id, request.command)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(ExecSandboxResponse { id, exec_id }))
}

async fn sandbox_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SandboxOperationResponse>> {
    let status = state
        .sandbox_manager
        .sandbox_status(id)
        .await
        .map_err(|err| ApiError::not_found(err.to_string()))?;
    Ok(Json(SandboxOperationResponse { id, state: status }))
}

async fn delete_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .sandbox_manager
        .remove_sandbox(id)
        .await
        .map_err(|err| ApiError::not_found(err.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_plugins(State(state): State<Arc<AppState>>) -> ApiResult<Json<Vec<PluginResponse>>> {
    let plugins = state.plugin_manager.plugin_summaries();
    Ok(Json(
        plugins.into_iter().map(PluginResponse::from).collect(),
    ))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CreateSandboxRequest {
    project: String,
    runtime: SandboxRuntime,
    #[serde(default)]
    limits: ResourceLimits,
}

impl CreateSandboxRequest {
    fn into_config(self) -> SandboxConfig {
        SandboxConfig {
            project: self.project,
            runtime: self.runtime,
            limits: self.limits,
        }
    }
}

#[derive(Debug, Serialize)]
struct CreateSandboxResponse {
    id: Uuid,
    state: SandboxState,
}

#[derive(Debug, Serialize)]
struct SandboxOperationResponse {
    id: Uuid,
    state: SandboxState,
}

#[derive(Debug, Serialize)]
struct ExecSandboxResponse {
    id: Uuid,
    exec_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct ExecSandboxRequest {
    command: String,
}

#[derive(Debug, Serialize)]
struct SandboxSnapshot {
    id: Uuid,
    project: String,
    runtime: SandboxRuntime,
    state: SandboxState,
}

impl From<crate::sandbox::Sandbox> for SandboxSnapshot {
    fn from(value: crate::sandbox::Sandbox) -> Self {
        Self {
            id: value.id,
            project: value.project,
            runtime: value.runtime,
            state: value.state,
        }
    }
}

#[derive(Debug, Serialize)]
struct PluginResponse {
    id: String,
    name: String,
    version: Option<String>,
    description: Option<String>,
    provides_runtime: bool,
    provides_telemetry: bool,
    provides_api: bool,
}

impl From<PluginSummary> for PluginResponse {
    fn from(summary: PluginSummary) -> Self {
        Self {
            id: summary.metadata.id,
            name: summary.metadata.name,
            version: summary.metadata.version,
            description: summary.metadata.description,
            provides_runtime: summary.capabilities.provides_runtime,
            provides_telemetry: summary.capabilities.provides_telemetry,
            provides_api: summary.capabilities.provides_api,
        }
    }
}

#[derive(Debug)]
enum ApiError {
    NotFound(String),
    Internal(String),
}

impl ApiError {
    fn not_found(message: String) -> Self {
        Self::NotFound(message)
    }

    fn internal<E: std::fmt::Display>(err: E) -> Self {
        Self::Internal(err.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::NotFound(message) => (StatusCode::NOT_FOUND, message).into_response(),
            ApiError::Internal(message) => {
                (StatusCode::INTERNAL_SERVER_ERROR, message).into_response()
            }
        }
    }
}
