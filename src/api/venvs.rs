//! Venv API endpoints

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::{JobVenvInfo, UpdateJobRequest, Venv, VenvListResponse};

/// Create the venvs router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/venvs", get(list_venvs))
        .route("/venvs/:id", get(get_venv).delete(delete_venv))
        .route("/jobs/:id/venv/info", get(get_job_venv_info))
        .route("/jobs/:id/venv/toggle", post(toggle_job_venv))
        .route("/jobs/:id/venv", delete(delete_job_venv))
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

/// Toggle between main-venv and custom venv for a job
#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/venv/toggle",
    tag = "venvs",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Venv toggled", body = JobVenvInfo),
        (status = 404, description = "Job not found")
    )
)]
pub async fn toggle_job_venv(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobVenvInfo>, AppError> {
    let job = state.job_service.get_job(&job_id).await?;

    // Toggle the use_custom_venv flag
    let new_value = !job.use_custom_venv;
    let update = UpdateJobRequest {
        name: None,
        description: None,
        python_code: None,
        timeout_seconds: None,
        memory_limit_mb: None,
        use_custom_venv: Some(new_value),
        priority: None,
        max_retries: None,
        enabled: None,
    };
    let updated_job = state.job_service.update_job(&job_id, update).await?;

    // Get venv info
    let custom_venv = state.venv_service.get_job_venv(&job_id).await?;
    let main_venv = state.venv_service.get_main_venv().await?;

    Ok(Json(JobVenvInfo {
        job_id,
        use_custom_venv: updated_job.use_custom_venv,
        venv: if updated_job.use_custom_venv { custom_venv } else { None },
        main_venv,
    }))
}

/// Delete custom venv for a job
#[utoipa::path(
    delete,
    path = "/api/v1/jobs/{id}/venv",
    tag = "venvs",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 204, description = "Custom venv deleted"),
        (status = 404, description = "Job or venv not found"),
        (status = 400, description = "No custom venv exists for this job")
    )
)]
pub async fn delete_job_venv(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<axum::http::StatusCode, AppError> {
    // Verify job exists
    let _ = state.job_service.get_job(&job_id).await?;

    // Get custom venv for this job
    let venv = state.venv_service.get_job_venv(&job_id).await?;
    match venv {
        Some(v) => {
            state.venv_service.delete_venv(&v.id).await?;
            Ok(axum::http::StatusCode::NO_CONTENT)
        }
        None => Err(AppError::NotFound(format!(
            "No custom venv found for job {}",
            job_id
        ))),
    }
}
