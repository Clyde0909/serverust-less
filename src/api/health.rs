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
    // Get various statistics
    let jobs = state.job_service.list_jobs(crate::models::ListJobsQuery {
        limit: 1,
        offset: 0,
        enabled: None,
        search: None,
    }).await?;

    let enabled_jobs = state.job_service.list_jobs(crate::models::ListJobsQuery {
        limit: 1,
        offset: 0,
        enabled: Some(true),
        search: None,
    }).await?;

    let executions = state.execution_service.list_executions(crate::models::ListExecutionsQuery {
        limit: 1,
        offset: 0,
        status: None,
        job_id: None,
        from: None,
        to: None,
    }).await?;

    let running = state.execution_service.get_running_executions().await?;
    let queue_depth = state.queue_service.get_depth().await?;
    let venvs = state.venv_service.list_venvs().await?;

    Ok(Json(StatsResponse {
        total_jobs: jobs.total,
        enabled_jobs: enabled_jobs.total,
        total_executions: executions.total,
        running_executions: running.len() as i64,
        queue_depth,
        venv_count: venvs.total,
    }))
}
