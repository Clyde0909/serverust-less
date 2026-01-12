//! Venv API endpoints

use axum::{
    extract::{Path, State},
    routing::{delete, get},
    Json, Router,
};
use std::sync::Arc;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::{JobVenvInfo, MainVenvStatus, Venv, VenvListResponse};

/// Create the venvs router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/venvs", get(list_venvs))
        .route("/venvs/:id", get(get_venv).delete(delete_venv))
        .route("/jobs/:id/venv/info", get(get_job_venv_info))
}

/// List all virtual environments
#[utoipa::path(
    get,
    path = "/api/v1/venvs",
    tag = "venvs",
    responses(
        (status = 200, description = "List of virtual environments", body = VenvListResponse)
    )
)]
pub async fn list_venvs(
    State(state): State<Arc<AppState>>,
) -> Result<Json<VenvListResponse>, AppError> {
    let response = state.venv_service.list_venvs().await?;
    Ok(Json(response))
}

/// Get a virtual environment by ID
#[utoipa::path(
    get,
    path = "/api/v1/venvs/{id}",
    tag = "venvs",
    params(
        ("id" = String, Path, description = "Venv ID")
    ),
    responses(
        (status = 200, description = "Venv details", body = Venv),
        (status = 404, description = "Venv not found")
    )
)]
pub async fn get_venv(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Venv>, AppError> {
    let venv = state.venv_service.get_venv(&id).await?;
    Ok(Json(venv))
}

/// Delete a virtual environment
#[utoipa::path(
    delete,
    path = "/api/v1/venvs/{id}",
    tag = "venvs",
    params(
        ("id" = String, Path, description = "Venv ID")
    ),
    responses(
        (status = 204, description = "Venv deleted"),
        (status = 400, description = "Cannot delete main venv"),
        (status = 404, description = "Venv not found")
    )
)]
pub async fn delete_venv(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, AppError> {
    state.venv_service.delete_venv(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Get venv info for a job
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}/venv/info",
    tag = "venvs",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Job venv info", body = JobVenvInfo),
        (status = 404, description = "Job not found")
    )
)]
pub async fn get_job_venv_info(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobVenvInfo>, AppError> {
    // Get job to check use_custom_venv
    let job = state.job_service.get_job(&job_id).await?;

    // Get custom venv if exists
    let custom_venv = state.venv_service.get_job_venv(&job_id).await?;

    // Get main venv
    let main_venv = state.venv_service.get_main_venv().await?;

    let response = JobVenvInfo {
        job_id,
        use_custom_venv: job.use_custom_venv,
        venv: if job.use_custom_venv { custom_venv } else { None },
        main_venv,
    };

    Ok(Json(response))
}
