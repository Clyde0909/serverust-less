//! Venv API endpoints

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::{CreateVenvRequest, JobVenvInfo, UpdateJobRequest, Venv, VenvListResponse};
use crate::worker::VenvManager;

/// Create the venvs router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/venvs", get(list_venvs).post(create_venv))
        .route("/venvs/:id", get(get_venv).delete(delete_venv))
        .route("/venvs/:id/packages", get(list_venv_packages))
        .route("/jobs/:id/venv/info", get(get_job_venv_info))
        .route("/jobs/:id/venv/toggle", post(toggle_job_venv))
        .route("/jobs/:id/venv", delete(delete_job_venv))
}

/// Create a new standalone virtual environment
#[utoipa::path(
    post,
    path = "/api/v1/venvs",
    tag = "venvs",
    request_body = CreateVenvRequest,
    responses(
        (status = 201, description = "Venv created", body = Venv),
        (status = 400, description = "Invalid request or name already exists")
    )
)]
pub async fn create_venv(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateVenvRequest>,
) -> Result<(axum::http::StatusCode, Json<Venv>), AppError> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("Venv name cannot be empty".to_string()));
    }
    // Reject reserved name
    if name == "main" {
        return Err(AppError::BadRequest("'main' is a reserved name".to_string()));
    }
    // Simple path-safety check
    if name.contains('/') || name.contains('\\') || name.contains('.') {
        return Err(AppError::BadRequest("Venv name must not contain path separators or dots".to_string()));
    }

    // Check filesystem collision
    if state.venv_manager.named_venv_exists(&name) {
        return Err(AppError::BadRequest(format!("Virtual environment '{}' already exists on disk", name)));
    }

    // Create on disk — use version-resolved Python binary if requested
    let python_exe = if let Some(ref version_hint) = req.python_version {
        VenvManager::resolve_python_for_version(version_hint)
            .await
            .ok_or_else(|| AppError::BadRequest(format!(
                "No Python executable found for version '{}'", version_hint
            )))?
    } else {
        // No version hint → fall back to default binary (same as create_named_venv)
        state.venv_manager.python_executable().to_string()
    };

    let venv_path = state
        .venv_manager
        .create_named_venv_with_python(&name, &python_exe)
        .await
        .map_err(|e| AppError::Internal(e))?;

    // Detect Python version
    let python_version = state
        .venv_manager
        .get_python_version(&venv_path)
        .await
        .ok()
        .or(req.python_version);

    // Persist record
    let mut venv = crate::models::Venv::new_standalone(
        &name,
        venv_path.to_str().unwrap_or(""),
        python_version,
    );
    venv.mark_ready();
    let created = state.venv_service.create_standalone_venv(venv).await?;

    Ok((axum::http::StatusCode::CREATED, Json(created)))
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
        venv_id: None,
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

#[derive(Serialize)]
pub struct VenvPackageItem {
    name: String,
    version: String,
}

#[derive(Serialize)]
pub struct VenvPackagesResponse {
    venv_id: String,
    packages: Vec<VenvPackageItem>,
    total: usize,
}

/// List packages installed in a specific virtual environment
#[utoipa::path(
    get,
    path = "/api/v1/venvs/{id}/packages",
    tag = "venvs",
    params(
        ("id" = String, Path, description = "Venv ID")
    ),
    responses(
        (status = 200, description = "List of packages in the venv"),
        (status = 404, description = "Venv not found")
    )
)]
pub async fn list_venv_packages(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<VenvPackagesResponse>, AppError> {
    let venv = state.venv_service.get_venv(&id).await?;
    let venv_path = std::path::Path::new(&venv.path);

    if !venv_path.exists() {
        return Err(AppError::NotFound(format!("Venv path does not exist: {}", venv.path)));
    }

    let pairs = state
        .venv_manager
        .list_packages(venv_path)
        .await
        .map_err(AppError::Internal)?;

    let packages: Vec<VenvPackageItem> = pairs
        .into_iter()
        .map(|(name, version)| VenvPackageItem { name, version })
        .collect();
    let total = packages.len();

    Ok(Json(VenvPackagesResponse {
        venv_id: id,
        packages,
        total,
    }))
}
