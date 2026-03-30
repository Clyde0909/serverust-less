//! Schedule API endpoints

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::{CreateScheduleRequest, JobSchedule, ScheduleListResponse, UpdateScheduleRequest};

/// Create the schedules router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/jobs/:id/schedule", post(create_schedule).get(get_schedule).put(update_schedule).delete(delete_schedule))
        .route("/jobs/:id/schedule/toggle", post(toggle_schedule))
        .route("/schedules", get(list_schedules))
}

/// Create a schedule for a job
#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/schedule",
    tag = "schedules",
    params(("id" = String, Path, description = "Job ID")),
    request_body = CreateScheduleRequest,
    responses(
        (status = 201, description = "Schedule created", body = JobSchedule),
        (status = 400, description = "Invalid cron expression"),
        (status = 409, description = "Schedule already exists for this job")
    )
)]
pub async fn create_schedule(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Json(req): Json<CreateScheduleRequest>,
) -> Result<(axum::http::StatusCode, Json<JobSchedule>), AppError> {
    // Verify job exists
    state.job_service.get_job(&job_id).await?;
    let schedule = state.schedule_service.create_schedule(&job_id, req).await?;
    Ok((axum::http::StatusCode::CREATED, Json(schedule)))
}

/// Get schedule for a job
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}/schedule",
    tag = "schedules",
    params(("id" = String, Path, description = "Job ID")),
    responses(
        (status = 200, description = "Schedule found", body = JobSchedule),
        (status = 404, description = "No schedule for this job")
    )
)]
pub async fn get_schedule(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobSchedule>, AppError> {
    let schedule = state.schedule_service.get_schedule_by_job_id(&job_id).await?;
    Ok(Json(schedule))
}

/// Update schedule for a job
#[utoipa::path(
    put,
    path = "/api/v1/jobs/{id}/schedule",
    tag = "schedules",
    params(("id" = String, Path, description = "Job ID")),
    request_body = UpdateScheduleRequest,
    responses(
        (status = 200, description = "Schedule updated", body = JobSchedule),
        (status = 400, description = "Invalid cron expression"),
        (status = 404, description = "No schedule for this job")
    )
)]
pub async fn update_schedule(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Json(req): Json<UpdateScheduleRequest>,
) -> Result<Json<JobSchedule>, AppError> {
    let schedule = state.schedule_service.update_schedule(&job_id, req).await?;
    Ok(Json(schedule))
}

/// Delete schedule for a job
#[utoipa::path(
    delete,
    path = "/api/v1/jobs/{id}/schedule",
    tag = "schedules",
    params(("id" = String, Path, description = "Job ID")),
    responses(
        (status = 204, description = "Schedule deleted"),
        (status = 404, description = "No schedule for this job")
    )
)]
pub async fn delete_schedule(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<axum::http::StatusCode, AppError> {
    state.schedule_service.delete_schedule(&job_id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Toggle schedule enabled/disabled
#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/schedule/toggle",
    tag = "schedules",
    params(("id" = String, Path, description = "Job ID")),
    responses(
        (status = 200, description = "Schedule toggled", body = JobSchedule),
        (status = 404, description = "No schedule for this job")
    )
)]
pub async fn toggle_schedule(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobSchedule>, AppError> {
    let schedule = state.schedule_service.toggle_schedule(&job_id).await?;
    Ok(Json(schedule))
}

/// List all schedules
#[utoipa::path(
    get,
    path = "/api/v1/schedules",
    tag = "schedules",
    responses(
        (status = 200, description = "List of all schedules", body = ScheduleListResponse)
    )
)]
pub async fn list_schedules(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ScheduleListResponse>, AppError> {
    let response = state.schedule_service.list_schedules().await?;
    Ok(Json(response))
}
