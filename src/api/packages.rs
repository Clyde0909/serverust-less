//! Package API endpoints

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use reqwest::{self, StatusCode as ReqwestStatusCode};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use utoipa::ToSchema;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::{
    AddDependencyRequest, DependencyListResponse, DependencyStatusResponse, InstallPackageRequest,
    JobDependency, PackageCache, PackageListResponse, BulkOperationResponse, PythonPackage,
    FluentInstallRequest,
};

/// Create the packages router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/packages", get(list_packages))
        .route("/packages/search", get(search_pypi))
        .route("/packages/pypi/:name", get(get_pypi_package_details))
        .route("/packages/install", post(install_package))
        .route("/packages/install_fluent", post(install_fluent_package))
        .route("/packages/uninstall", post(uninstall_package))
        .route("/packages/main-venv", get(get_main_venv_packages).delete(clear_main_venv))
        .route("/packages/main-venv/update", post(update_main_venv_packages))
        .route("/packages/:name/:version", delete(delete_package))
        .route("/jobs/:id/dependencies", get(get_job_dependencies).post(add_job_dependency))
        .route("/jobs/:id/dependencies/install", post(install_job_dependencies))
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchQuery {
    q: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PyPiSearchResult {
    name: String,
    version: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requires_python: Option<String>,
    cached: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResponse {
    results: Vec<PyPiSearchResult>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PyPiReleaseSummary {
    version: String,
    yanked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    upload_time: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PyPiPackageDetailResponse {
    name: String,
    version: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requires_python: Option<String>,
    releases: Vec<PyPiReleaseSummary>,
    cached: bool,
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
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, AppError> {
    let query = params.q.trim();
    if query.is_empty() {
        return Err(AppError::Validation("Search query cannot be empty".to_string()));
    }

    let client = build_pypi_client()?;
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for candidate in build_candidate_package_names(query) {
        if let Some(detail) = fetch_pypi_package_detail(&client, &candidate).await? {
            let normalized_name = normalize_pypi_name(&detail.name);
            if seen.insert(normalized_name) {
                persist_discovered_package(&state, &detail).await;
                results.push(detail.to_search_result(false));
            }
            break;
        }
    }

    let known_packages = state.package_service.search_known_packages(query, 10).await?;
    for package in known_packages {
        let normalized_name = normalize_pypi_name(&package.name);
        if seen.insert(normalized_name) {
            results.push(PyPiSearchResult {
                name: package.name,
                version: package.version,
                description: package.description.unwrap_or_default(),
                author: None,
                project_url: package.pypi_url,
                requires_python: None,
                cached: true,
            });
        }
    }

    Ok(Json(SearchResponse {
        results,
    }))
}

/// Get rich PyPI metadata for an exact package name.
#[utoipa::path(
    get,
    path = "/api/v1/packages/pypi/{name}",
    tag = "packages",
    params(
        ("name" = String, Path, description = "Exact package name")
    ),
    responses(
        (status = 200, description = "Rich PyPI package metadata", body = PyPiPackageDetailResponse),
        (status = 404, description = "Package not found", body = ErrorResponse)
    )
)]
pub async fn get_pypi_package_details(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<PyPiPackageDetailResponse>, AppError> {
    let client = build_pypi_client()?;
    let detail = fetch_pypi_package_detail(&client, &name)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("PyPI package not found: {}", name)))?;

    persist_discovered_package(&state, &detail).await;

    Ok(Json(detail))
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

/// Install a package using the fluent builder API.
#[utoipa::path(
    post,
    path = "/api/v1/packages/install_fluent",
    tag = "packages",
    request_body = FluentInstallRequest,
    responses(
        (status = 200, description = "Package installed", body = PackageCache),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Installation failed")
    )
)]
pub async fn install_fluent_package(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FluentInstallRequest>,
) -> Result<Json<PackageCache>, AppError> {
    // Forward to the existing install logic – the fluent API currently mirrors the same behavior.
    let fallback_req = InstallPackageRequest {
        name: req.name,
        version: req.version,
    };
    state.package_service.install_package(fallback_req).await.map(Json)
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
    // Verify job exists first to return 404 instead of a DB constraint error
    state.job_service.get_job(&job_id).await?;
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

/// Update all packages in main venv
#[utoipa::path(
    post,
    path = "/api/v1/packages/main-venv/update",
    tag = "packages",
    responses(
        (status = 200, description = "Update results", body = BulkOperationResponse),
        (status = 500, description = "Update failed")
    )
)]
pub async fn update_main_venv_packages(
    State(state): State<Arc<AppState>>,
) -> Result<Json<BulkOperationResponse>, AppError> {
    // Get currently installed packages
    let packages = state.package_service.get_main_venv_packages().await?;

    if packages.is_empty() {
        return Ok(Json(BulkOperationResponse {
            success_count: 0,
            failure_count: 0,
            errors: vec![],
        }));
    }

    let mut success_count: u64 = 0;
    let mut failure_count: u64 = 0;
    let mut errors = Vec::new();

    // Reinstall each package with latest version
    for pkg in &packages {
        let req = InstallPackageRequest {
            name: pkg.package_name.clone(),
            version: None, // latest
        };
        match state.package_service.install_package(req).await {
            Ok(_) => success_count += 1,
            Err(e) => {
                failure_count += 1;
                errors.push(format!("{}: {}", pkg.package_name, e));
            }
        }
    }

    Ok(Json(BulkOperationResponse {
        success_count,
        failure_count,
        errors,
    }))
}

/// Clear and recreate main venv
#[utoipa::path(
    delete,
    path = "/api/v1/packages/main-venv",
    tag = "packages",
    responses(
        (status = 200, description = "Main venv cleared and recreated"),
        (status = 500, description = "Operation failed")
    )
)]
pub async fn clear_main_venv(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Remove all cache entries for main venv
    let packages = state.package_service.get_main_venv_packages().await?;
    for pkg in &packages {
        let _ = state
            .package_service
            .remove_package(&pkg.package_name, &pkg.version)
            .await;
    }

    Ok(Json(serde_json::json!({
        "message": "Main venv cache cleared",
        "packages_removed": packages.len()
    })))
}

/// Install all dependencies for a job
#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/dependencies/install",
    tag = "packages",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Installation results", body = BulkOperationResponse),
        (status = 404, description = "Job not found")
    )
)]
pub async fn install_job_dependencies(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<BulkOperationResponse>, AppError> {
    // Get job dependencies
    let deps = state.package_service.get_job_dependencies(&job_id).await?;

    if deps.dependencies.is_empty() {
        return Ok(Json(BulkOperationResponse {
            success_count: 0,
            failure_count: 0,
            errors: vec![],
        }));
    }

    // Check if job uses custom venv
    let _job = state.job_service.get_job(&job_id).await?;

    let mut success_count: u64 = 0;
    let mut failure_count: u64 = 0;
    let mut errors = Vec::new();

    // Install each dependency
    for dep in &deps.dependencies {
        let version = if dep.version_constraint == "*" {
            None
        } else {
            Some(dep.version_constraint.clone())
        };
        let req = InstallPackageRequest {
            name: dep.package_name.clone(),
            version,
        };
        match state.package_service.install_package(req).await {
            Ok(_) => success_count += 1,
            Err(e) => {
                failure_count += 1;
                errors.push(format!("{}: {}", dep.package_name, e));
            }
        }
    }

    Ok(Json(BulkOperationResponse {
        success_count,
        failure_count,
        errors,
    }))
}

#[derive(Debug, Deserialize)]
struct PypiJsonResponse {
    info: PypiInfo,
    #[serde(default)]
    releases: HashMap<String, Vec<PypiReleaseFile>>,
}

#[derive(Debug, Deserialize)]
struct PypiInfo {
    name: String,
    version: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    package_url: Option<String>,
    #[serde(default)]
    home_page: Option<String>,
    #[serde(default)]
    project_urls: Option<HashMap<String, String>>,
    #[serde(default)]
    requires_python: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PypiReleaseFile {
    #[serde(default)]
    upload_time_iso_8601: Option<String>,
    #[serde(default)]
    yanked: bool,
}

impl PyPiPackageDetailResponse {
    fn to_search_result(&self, cached: bool) -> PyPiSearchResult {
        PyPiSearchResult {
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            author: self.author.clone(),
            project_url: self.project_url.clone(),
            requires_python: self.requires_python.clone(),
            cached,
        }
    }
}

fn build_pypi_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent(format!("serverust-less/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| AppError::Internal(format!("Failed to build PyPI client: {}", e)))
}

fn build_candidate_package_names(query: &str) -> Vec<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let normalized = normalize_pypi_name(trimmed);
    let lowercase = trimmed.to_lowercase();
    let replaced_spaces = trimmed.replace(' ', "-");

    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    for candidate in [trimmed.to_string(), normalized, lowercase, replaced_spaces] {
        let normalized_candidate = normalize_pypi_name(&candidate);
        if !candidate.trim().is_empty() && seen.insert(normalized_candidate) {
            candidates.push(candidate);
        }
    }

    candidates
}

fn normalize_pypi_name(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    let mut last_was_separator = false;

    for ch in name.trim().chars() {
        let is_separator = matches!(ch, '-' | '_' | '.');
        if is_separator {
            if !last_was_separator {
                normalized.push('-');
                last_was_separator = true;
            }
        } else {
            normalized.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        }
    }

    normalized.trim_matches('-').to_string()
}

async fn fetch_pypi_package_detail(
    client: &reqwest::Client,
    package_name: &str,
) -> Result<Option<PyPiPackageDetailResponse>, AppError> {
    let url = format!("https://pypi.org/pypi/{}/json", package_name);
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("PyPI request failed: {}", e)))?;

    if response.status() == ReqwestStatusCode::NOT_FOUND {
        return Ok(None);
    }

    let response = response
        .error_for_status()
        .map_err(|e| AppError::Internal(format!("PyPI request failed: {}", e)))?;

    let payload = response
        .json::<PypiJsonResponse>()
        .await
        .map_err(|e| AppError::Internal(format!("Invalid PyPI response: {}", e)))?;

    let mut releases: Vec<PyPiReleaseSummary> = payload
        .releases
        .into_iter()
        .map(|(version, files)| {
            let upload_time = files
                .iter()
                .filter_map(|file| file.upload_time_iso_8601.clone())
                .max();
            let yanked = files.iter().all(|file| file.yanked);

            PyPiReleaseSummary {
                version,
                yanked,
                upload_time,
            }
        })
        .collect();
    releases.sort_by(|left, right| right.version.cmp(&left.version));

    let project_url = payload
        .info
        .project_urls
        .as_ref()
        .and_then(|urls| {
            urls.get("Homepage")
                .or_else(|| urls.get("Source"))
                .or_else(|| urls.get("Documentation"))
                .cloned()
        })
        .or_else(|| payload.info.home_page.clone());

    Ok(Some(PyPiPackageDetailResponse {
        name: payload.info.name,
        version: payload.info.version,
        description: payload.info.summary.unwrap_or_default(),
        author: payload.info.author,
        project_url,
        package_url: payload.info.package_url,
        requires_python: payload.info.requires_python,
        releases,
        cached: false,
    }))
}

async fn persist_discovered_package(state: &AppState, detail: &PyPiPackageDetailResponse) {
    let package = PythonPackage::new(
        &detail.name,
        &detail.version,
        Some(detail.description.clone()),
    );

    if let Err(err) = state.package_service.record_discovered_package(&package).await {
        tracing::warn!(
            package = %detail.name,
            version = %detail.version,
            error = %err,
            "Failed to cache discovered PyPI package metadata"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_pypi_name() {
        assert_eq!(normalize_pypi_name("Requests"), "requests");
        assert_eq!(normalize_pypi_name("my_pkg.name"), "my-pkg-name");
        assert_eq!(normalize_pypi_name("  pandas..core__ext  "), "pandas-core-ext");
    }

    #[test]
    fn test_build_candidate_package_names_deduplicates() {
        let candidates = build_candidate_package_names("Requests_Toolkit");

        assert!(!candidates.is_empty());
        assert_eq!(normalize_pypi_name(&candidates[0]), "requests-toolkit");
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| normalize_pypi_name(candidate))
                .collect::<HashSet<_>>()
                .len(),
            candidates.len()
        );
    }
}
