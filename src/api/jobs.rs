//! Job API endpoints

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::{CreateJobRequest, Job, JobListResponse, ListJobsQuery, UpdateJobRequest};

/// Create the jobs router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/jobs", get(list_jobs).post(create_job))
        .route("/jobs/:id", get(get_job).put(update_job).delete(delete_job))
        .route("/jobs/:id/enable", post(enable_job))
        .route("/jobs/:id/disable", post(disable_job))
}

/// List all jobs with pagination
#[utoipa::path(
    get,
    path = "/api/v1/jobs",
    tag = "jobs",
    params(
        ("limit" = Option<i32>, Query, description = "Maximum number of results (default: 20, max: 100)"),
        ("offset" = Option<i32>, Query, description = "Offset for pagination (default: 0)"),
        ("enabled" = Option<bool>, Query, description = "Filter by enabled status"),
        ("search" = Option<String>, Query, description = "Search by job name")
    ),
    responses(
        (status = 200, description = "List of jobs", body = JobListResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn list_jobs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListJobsQuery>,
) -> Result<Json<JobListResponse>, AppError> {
    let response = state.job_service.list_jobs(query).await?;
    Ok(Json(response))
}

/// Create a new job
#[utoipa::path(
    post,
    path = "/api/v1/jobs",
    tag = "jobs",
    request_body = CreateJobRequest,
    responses(
        (status = 201, description = "Job created successfully", body = Job),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 409, description = "Job name already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn create_job(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateJobRequest>,
) -> Result<(axum::http::StatusCode, Json<Job>), AppError> {
    let job = state.job_service.create_job(req).await?;
    Ok((axum::http::StatusCode::CREATED, Json(job)))
}

/// Get a job by ID
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}",
    tag = "jobs",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Job found", body = Job),
        (status = 404, description = "Job not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let job = state.job_service.get_job(&id).await?;
    Ok(Json(job))
}

/// Update a job
#[utoipa::path(
    put,
    path = "/api/v1/jobs/{id}",
    tag = "jobs",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    request_body = UpdateJobRequest,
    responses(
        (status = 200, description = "Job updated successfully", body = Job),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 404, description = "Job not found", body = ErrorResponse),
        (status = 409, description = "Job name already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateJobRequest>,
) -> Result<Json<Job>, AppError> {
    let job = state.job_service.update_job(&id, req).await?;
    Ok(Json(job))
}

/// Delete a job
#[utoipa::path(
    delete,
    path = "/api/v1/jobs/{id}",
    tag = "jobs",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 204, description = "Job deleted successfully"),
        (status = 404, description = "Job not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn delete_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, AppError> {
    state.job_service.delete_job(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Enable a job
#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/enable",
    tag = "jobs",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Job enabled successfully", body = Job),
        (status = 404, description = "Job not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn enable_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let job = state.job_service.enable_job(&id).await?;
    Ok(Json(job))
}

/// Disable a job
#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/disable",
    tag = "jobs",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Job disabled successfully", body = Job),
        (status = 404, description = "Job not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn disable_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let job = state.job_service.disable_job(&id).await?;
    Ok(Json(job))
}
