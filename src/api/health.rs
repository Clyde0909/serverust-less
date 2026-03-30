//! Health API endpoints

use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

use crate::api::AppState;
use crate::error::AppError;

/// Create the health router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health_check))
        .route("/stats", get(get_stats))
        .route("/workers/status", get(get_workers_status))
}

/// Health check response
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub timestamp: String,
}

/// System statistics response
#[derive(Debug, Serialize, ToSchema)]
pub struct StatsResponse {
    pub total_jobs: i64,
    pub enabled_jobs: i64,
    pub total_executions: i64,
    pub running_executions: i64,
    pub queue_depth: i64,
    pub venv_count: i64,
}

/// Worker pool status response
#[derive(Debug, Serialize, ToSchema)]
pub struct WorkerStatusResponse {
    /// Total number of worker tasks in the pool.
    pub pool_size: usize,
    /// Number of executions currently tracked as running.
    pub running: usize,
    /// Number of idle worker slots (pool_size - running, may be approximate).
    pub idle: usize,
    /// Number of items currently in the in-memory priority queue.
    pub queue_memory_size: usize,
}

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/api/v1/health",
    tag = "health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse)
    )
)]
pub async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Get system statistics
#[utoipa::path(
    get,
    path = "/api/v1/stats",
    tag = "health",
    responses(
        (status = 200, description = "System statistics", body = StatsResponse)
    )
)]
pub async fn get_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatsResponse>, AppError> {
    // Use dedicated count queries instead of full list queries
    let total_jobs = state.job_service.count_all().await?;
    let enabled_jobs = state.job_service.count_enabled().await?;
    let total_executions = state.execution_service.count_all().await?;
    let running_executions = state.execution_service.count_running().await?;
    let queue_depth = state.queue_service.get_depth().await?;
    let venvs = state.venv_service.list_venvs().await?;

    Ok(Json(StatsResponse {
        total_jobs,
        enabled_jobs,
        total_executions,
        running_executions,
        queue_depth,
        venv_count: venvs.total,
    }))
}

/// Get worker pool status
#[utoipa::path(
    get,
    path = "/api/v1/workers/status",
    tag = "health",
    responses(
        (status = 200, description = "Worker pool status", body = WorkerStatusResponse)
    )
)]
pub async fn get_workers_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<WorkerStatusResponse>, AppError> {
    let running = state.process_manager.running_count().await;
    let queue_memory_size = state.queue_manager.memory_queue_size().await;
    let pool_size = state.worker_pool_size;
    let idle = pool_size.saturating_sub(running);

    Ok(Json(WorkerStatusResponse {
        pool_size,
        running,
        idle,
        queue_memory_size,
    }))
}
