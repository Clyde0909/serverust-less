//! Package API endpoints

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use reqwest;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::{
    AddDependencyRequest, DependencyListResponse, DependencyStatusResponse, InstallPackageRequest,
    JobDependency, PackageCache, PackageListResponse,
};

/// Create the packages router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/packages", get(list_packages))
        .route("/packages/search", get(search_pypi))
        .route("/packages/install", post(install_package))
        .route("/packages/uninstall", post(uninstall_package))
        .route("/packages/main-venv", get(get_main_venv_packages))
        .route("/packages/:name/:version", delete(delete_package))
        .route("/jobs/:id/dependencies", get(get_job_dependencies).post(add_job_dependency))
        .route("/jobs/:id/dependencies/:name", put(update_dependency).delete(remove_dependency))
        .route("/jobs/:id/dependencies/status", get(get_dependency_status))
}

/// List all cached packages
#[utoipa::path(
    get,
    path = "/api/v1/packages",
    tag = "packages",
    responses(
        (status = 200, description = "List of packages", body = PackageListResponse)
    )
)]
pub async fn list_packages(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PackageListResponse>, AppError> {
    let response = state.package_service.list_packages().await?;
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    q: String,
}

#[derive(Debug, Serialize)]
pub struct PyPiSearchResult {
    name: String,
    version: String,
    description: String,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    results: Vec<PyPiSearchResult>,
}

/// Search PyPI for packages
#[utoipa::path(
    get,
    path = "/api/v1/packages/search",
    tag = "packages",
    params(
        ("q" = String, Query, description = "Search query")
    ),
    responses(
        (status = 200, description = "Search results", body = SearchResponse)
    )
)]
pub async fn search_pypi(
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, AppError> {
    let client = reqwest::Client::new();
    let url = format!("https://pypi.org/pypi/{}/json", params.q);
    
    // Try to fetch exact package match
    match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => {
            #[derive(Deserialize)]
            struct PyPiResponse {
                info: PyPiInfo,
            }
            
            #[derive(Deserialize)]
            struct PyPiInfo {
                name: String,
                version: String,
                summary: Option<String>,
            }
            
            if let Ok(pypi_resp) = response.json::<PyPiResponse>().await {
                return Ok(Json(SearchResponse {
                    results: vec![PyPiSearchResult {
                        name: pypi_resp.info.name,
                        version: pypi_resp.info.version,
                        description: pypi_resp.info.summary.unwrap_or_default(),
                    }],
                }));
            }
        }
        _ => {}
    }
    
    // Return empty results if no exact match found
    Ok(Json(SearchResponse {
        results: vec![],
    }))
}

/// Install a package to main venv
#[utoipa::path(
    post,
    path = "/api/v1/packages/install",
    tag = "packages",
    request_body = InstallPackageRequest,
    responses(
        (status = 200, description = "Package installed", body = PackageCache),
        (status = 400, description = "Invalid request")
    )
)]
pub async fn install_package(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InstallPackageRequest>,
) -> Result<Json<PackageCache>, AppError> {
    let cache = state.package_service.install_package(req).await?;
    Ok(Json(cache))
}

#[derive(Debug, serde::Deserialize)]
pub struct UninstallPackageRequest {
    pub name: String,
}

/// Uninstall a package from the main venv
#[utoipa::path(
    post,
    path = "/api/v1/packages/uninstall",
    tag = "packages",
    request_body = UninstallPackageRequest,
    responses(
        (status = 204, description = "Package uninstalled"),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Uninstall failed")
    )
)]
pub async fn uninstall_package(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UninstallPackageRequest>,
) -> Result<axum::http::StatusCode, AppError> {
    state.package_service.uninstall_package(&req.name).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Get main venv packages
#[utoipa::path(
    get,
    path = "/api/v1/packages/main-venv",
    tag = "packages",
    responses(
        (status = 200, description = "Main venv packages", body = Vec<PackageCache>)
    )
)]
pub async fn get_main_venv_packages(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<PackageCache>>, AppError> {
    let packages = state.package_service.get_main_venv_packages().await?;
    Ok(Json(packages))
}

/// Delete a package from cache
#[utoipa::path(
    delete,
    path = "/api/v1/packages/{name}/{version}",
    tag = "packages",
    params(
        ("name" = String, Path, description = "Package name"),
        ("version" = String, Path, description = "Package version")
    ),
    responses(
        (status = 204, description = "Package deleted"),
        (status = 404, description = "Package not found")
    )
)]
pub async fn delete_package(
    State(state): State<Arc<AppState>>,
    Path((name, version)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, AppError> {
    state.package_service.remove_package(&name, &version).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Get job dependencies
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}/dependencies",
    tag = "packages",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Job dependencies", body = DependencyListResponse)
    )
)]
pub async fn get_job_dependencies(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<DependencyListResponse>, AppError> {
    let response = state.package_service.get_job_dependencies(&job_id).await?;
    Ok(Json(response))
}

/// Add a dependency to a job
#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/dependencies",
    tag = "packages",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    request_body = AddDependencyRequest,
    responses(
        (status = 201, description = "Dependency added", body = JobDependency),
        (status = 400, description = "Invalid request")
    )
)]
pub async fn add_job_dependency(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Json(req): Json<AddDependencyRequest>,
) -> Result<(axum::http::StatusCode, Json<JobDependency>), AppError> {
    let dep = state.package_service.add_dependency(&job_id, req).await?;
    Ok((axum::http::StatusCode::CREATED, Json(dep)))
}

/// Update request for dependency
#[derive(Debug, Deserialize)]
pub struct UpdateDependencyRequest {
    pub version_constraint: String,
}

/// Update a dependency version
#[utoipa::path(
    put,
    path = "/api/v1/jobs/{id}/dependencies/{name}",
    tag = "packages",
    params(
        ("id" = String, Path, description = "Job ID"),
        ("name" = String, Path, description = "Package name")
    ),
    responses(
        (status = 200, description = "Dependency updated", body = JobDependency)
    )
)]
pub async fn update_dependency(
    State(state): State<Arc<AppState>>,
    Path((job_id, package_name)): Path<(String, String)>,
    Json(req): Json<UpdateDependencyRequest>,
) -> Result<Json<JobDependency>, AppError> {
    let dep = state
        .package_service
        .update_dependency(&job_id, &package_name, req.version_constraint)
        .await?;
    Ok(Json(dep))
}

/// Remove a dependency
#[utoipa::path(
    delete,
    path = "/api/v1/jobs/{id}/dependencies/{name}",
    tag = "packages",
    params(
        ("id" = String, Path, description = "Job ID"),
        ("name" = String, Path, description = "Package name")
    ),
    responses(
        (status = 204, description = "Dependency removed"),
        (status = 404, description = "Dependency not found")
    )
)]
pub async fn remove_dependency(
    State(state): State<Arc<AppState>>,
    Path((job_id, package_name)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, AppError> {
    state
        .package_service
        .remove_dependency(&job_id, &package_name)
        .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Get dependency installation status
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}/dependencies/status",
    tag = "packages",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Dependency status", body = DependencyStatusResponse)
    )
)]
pub async fn get_dependency_status(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<DependencyStatusResponse>, AppError> {
    // Check main venv by default
    let response = state
        .package_service
        .get_dependency_status(&job_id, "main", None)
        .await?;
    Ok(Json(response))
}
