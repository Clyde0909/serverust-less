//! Package service - business logic for package management

use crate::db::PackageRepository;
use crate::error::AppError;
use crate::models::{
    AddDependencyRequest, DependencyListResponse, DependencyStatusResponse, InstallPackageRequest,
    JobDependency, PackageCache, PackageInstallStatus, PackageListResponse, PackageStatus,
};
use crate::worker::{PackageManager, VenvManager};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

/// Service for package management
#[derive(Clone)]
pub struct PackageService {
    repo: PackageRepository,
    venv_manager: Option<Arc<VenvManager>>,
    package_manager: Option<Arc<PackageManager>>,
}

impl PackageService {
    /// Create a new PackageService
    pub fn new(repo: PackageRepository) -> Self {
        Self { 
            repo,
            venv_manager: None,
            package_manager: None,
        }
    }

    /// Create a new PackageService with worker integration
    pub fn with_workers(
        repo: PackageRepository,
        venv_manager: Arc<VenvManager>,
        package_manager: Arc<PackageManager>,
    ) -> Self {
        Self {
            repo,
            venv_manager: Some(venv_manager),
            package_manager: Some(package_manager),
        }
    }

    /// List all cached packages
    pub async fn list_packages(&self) -> Result<PackageListResponse, AppError> {
        let (packages, total) = self.repo.list_all_cached().await?;
        Ok(PackageListResponse { packages, total })
    }

    /// Install a package to main venv
    pub async fn install_package(&self, req: InstallPackageRequest) -> Result<PackageCache, AppError> {
        // Validate package name
        if req.name.trim().is_empty() {
            return Err(AppError::Validation("Package name cannot be empty".to_string()));
        }

        info!("Installing package: {} (version: {:?})", req.name, req.version);

        // Check if we have worker integration
        let (venv_manager, package_manager) = match (&self.venv_manager, &self.package_manager) {
            (Some(v), Some(p)) => (v, p),
            _ => {
                warn!("Package installation requested but worker managers not configured");
                return Err(AppError::Internal(
                    "Package installation service not available".to_string(),
                ));
            }
        };

        // Get main venv path
        let main_venv_path = venv_manager.main_venv_path();

        // Check if venv exists, create if needed
        if !venv_manager.main_venv_exists() {
            info!("Main venv does not exist, creating...");
            venv_manager.create_main_venv().await.map_err(|e| {
                AppError::Internal(format!("Failed to create main venv: {}", e))
            })?;
        }

        // Create cache entry to track installation
        let cache_id = uuid::Uuid::new_v4().to_string();
        let mut cache = PackageCache {
            id: cache_id,
            venv_type: "main".to_string(),
            venv_id: None,
            package_name: req.name.clone(),
            version: req.version.clone().unwrap_or_else(|| "latest".to_string()),
            installation_path: main_venv_path.to_string_lossy().to_string(),
            size_bytes: None,
            status: PackageStatus::Installing.as_str().to_string(),
            error_message: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
            last_used_at: Some(chrono::Utc::now().to_rfc3339()),
            use_count: 0,
        };

        // Perform actual installation (don't save cache yet to avoid duplicate ID issues)
        let version_constraint = req.version.as_deref();
        let result = package_manager
            .install_to_main_venv(&main_venv_path, &req.name, version_constraint)
            .await;

        // Update cache based on result
        match result {
            crate::worker::InstallResult {
                success: true,
                version: Some(installed_version),
                ..
            } => {
                info!("Package {} installed successfully: {}", req.name, installed_version);
                cache.version = installed_version;
                cache.status = PackageStatus::Ready.as_str().to_string();
                cache.error_message = None;
            }
            crate::worker::InstallResult {
                success: true,
                version: None,
                ..
            } => {
                info!("Package {} installed successfully (version unknown)", req.name);
                cache.status = PackageStatus::Ready.as_str().to_string();
                cache.error_message = None;
            }
            crate::worker::InstallResult {
                success: false,
                error: Some(err),
                ..
            } => {
                warn!("Package {} installation failed: {}", req.name, err);
                cache.status = PackageStatus::Failed.as_str().to_string();
                cache.error_message = Some(err.clone());
                // Save failed status to cache
                self.repo.upsert_cache(&cache).await?;
                return Err(AppError::Internal(format!("Package installation failed: {}", err)));
            }
            _ => {
                warn!("Package {} installation failed with unknown error", req.name);
                cache.status = PackageStatus::Failed.as_str().to_string();
                cache.error_message = Some("Unknown installation error".to_string());
                // Save failed status to cache
                self.repo.upsert_cache(&cache).await?;
                return Err(AppError::Internal("Package installation failed".to_string()));
            }
        }

        // Update cache with final status
        self.repo.upsert_cache(&cache).await
    }

    /// Get packages in main venv
    pub async fn get_main_venv_packages(&self) -> Result<Vec<PackageCache>, AppError> {
        self.repo.get_cache_by_venv("main", None).await
    }

    /// Check if a package is installed in main venv
    pub async fn is_package_installed(&self, package_name: &str) -> Result<bool, AppError> {
        let package = self
            .repo
            .get_cached_package("main", None, package_name)
            .await?;
        Ok(package.is_some())
    }

    /// Record package installation
    pub async fn record_installation(&self, cache: &PackageCache) -> Result<PackageCache, AppError> {
        self.repo.upsert_cache(cache).await
    }

    /// Remove package from cache
    pub async fn remove_package(
        &self,
        package_name: &str,
        version: &str,
    ) -> Result<(), AppError> {
        self.repo
            .delete_cache("main", None, package_name, version)
            .await
    }

    // ============ Job Dependencies ============

    /// Get dependencies for a job
    pub async fn get_job_dependencies(&self, job_id: &str) -> Result<DependencyListResponse, AppError> {
        let dependencies = self.repo.get_dependencies(job_id).await?;
        Ok(DependencyListResponse {
            job_id: job_id.to_string(),
            dependencies,
        })
    }

    /// Add a dependency to a job
    pub async fn add_dependency(
        &self,
        job_id: &str,
        req: AddDependencyRequest,
    ) -> Result<JobDependency, AppError> {
        // Validate package name
        if req.package_name.trim().is_empty() {
            return Err(AppError::Validation("Package name cannot be empty".to_string()));
        }

        let dep = JobDependency::new(job_id, &req.package_name, req.version_constraint);
        self.repo.add_dependency(&dep).await
    }

    /// Update a dependency version
    pub async fn update_dependency(
        &self,
        job_id: &str,
        package_name: &str,
        version_constraint: String,
    ) -> Result<JobDependency, AppError> {
        // Delete old and create new
        let _ = self.repo.delete_dependency(job_id, package_name).await;
        let dep = JobDependency::new(job_id, package_name, Some(version_constraint));
        self.repo.add_dependency(&dep).await
    }

    /// Remove a dependency
    pub async fn remove_dependency(&self, job_id: &str, package_name: &str) -> Result<(), AppError> {
        self.repo.delete_dependency(job_id, package_name).await
    }

    /// Get dependency installation status
    pub async fn get_dependency_status(
        &self,
        job_id: &str,
        venv_type: &str,
        venv_id: Option<&str>,
    ) -> Result<DependencyStatusResponse, AppError> {
        let dependencies = self.repo.get_dependencies(job_id).await?;
        let cached = self.repo.get_cache_by_venv(venv_type, venv_id).await?;

        let mut packages = Vec::new();
        let mut all_ready = true;

        for dep in &dependencies {
            let cached_pkg = cached
                .iter()
                .find(|c| c.package_name == dep.package_name);

            let (status, installed) = match cached_pkg {
                Some(c) if c.status == PackageStatus::Ready.as_str() => {
                    ("ready".to_string(), Some(c.version.clone()))
                }
                Some(c) if c.status == PackageStatus::Installing.as_str() => {
                    all_ready = false;
                    ("installing".to_string(), None)
                }
                Some(c) if c.status == PackageStatus::Failed.as_str() => {
                    all_ready = false;
                    ("failed".to_string(), None)
                }
                _ => {
                    all_ready = false;
                    ("not_installed".to_string(), None)
                }
            };

            packages.push(PackageInstallStatus {
                name: dep.package_name.clone(),
                required: dep.version_constraint.clone(),
                installed,
                status,
            });
        }

        let overall_status = if packages.is_empty() {
            "no_dependencies".to_string()
        } else if all_ready {
            "ready".to_string()
        } else {
            "pending".to_string()
        };

        Ok(DependencyStatusResponse {
            job_id: job_id.to_string(),
            status: overall_status,
            packages,
        })
    }
}
