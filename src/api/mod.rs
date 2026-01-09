//! API layer module

pub mod jobs;

use axum::Router;
use std::sync::Arc;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::services::JobService;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub job_service: JobService,
}

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        jobs::list_jobs,
        jobs::create_job,
        jobs::get_job,
        jobs::update_job,
        jobs::delete_job,
        jobs::enable_job,
        jobs::disable_job,
    ),
    components(
        schemas(
            crate::models::Job,
            crate::models::CreateJobRequest,
            crate::models::UpdateJobRequest,
            crate::models::ListJobsQuery,
            crate::models::JobListResponse,
            crate::error::ErrorResponse,
        )
    ),
    tags(
        (name = "jobs", description = "Job management endpoints")
    ),
    info(
        title = "Serverust-Less API",
        version = "1.0.0",
        description = "AWS Lambda-like serverless platform for Python REPL execution"
    )
)]
pub struct ApiDoc;

/// Create the API router
pub fn create_router(state: AppState) -> Router {
    let api_routes = Router::new()
        .merge(jobs::router())
        .with_state(Arc::new(state));

    Router::new()
        .nest("/api/v1", api_routes)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
}
