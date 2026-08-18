//! Package service - business logic for package management

use crate::db::PackageRepository;
use crate::error::AppError;
use crate::models::{
    AddDependencyRequest, DependencyListResponse, DependencyStatusResponse, InstallPackageRequest,
    JobDependency, PackageCache, PackageInstallStatus, PackageListResponse, PackageStatus,
    PythonPackage,
};
use crate::worker::{PackageManager, VenvManager};
use std::sync::Arc;
use tracing::{info, warn};

/// Service for package management
#[derive(Clone)]
pub struct PackageService {
    repo: PackageRepository,
    venv_manager: Option<Arc<VenvManager>>,
    package_manager: Option<Arc<PackageManager>>,
    /// Conflict resolution strategy: "suggest_custom_venv", "force_upgrade", or "fail"
    conflict_strategy: String,
}

impl PackageService {
    /// Create a new PackageService
    pub fn new(repo: PackageRepository) -> Self {
        Self { 
            repo,
            venv_manager: None,
            package_manager: None,
            conflict_strategy: "suggest_custom_venv".to_string(),
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
            conflict_strategy: "suggest_custom_venv".to_string(),
        }
    }

    /// Create a new PackageService with worker integration and conflict resolution config
    pub fn with_config(
        repo: PackageRepository,
        venv_manager: Arc<VenvManager>,
        package_manager: Arc<PackageManager>,
        conflict_strategy: String,
    ) -> Self {
        Self {
            repo,
            venv_manager: Some(venv_manager),
            package_manager: Some(package_manager),
            conflict_strategy,
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

        // --- Conflict detection (F3) ---
        // Check if a different version of this package is already installed in main venv
        if let Some(ref requested_version) = req.version {
            if let Ok(Some(existing)) = self.repo.get_cached_package("main", None, &req.name).await {
                if existing.status == PackageStatus::Ready.as_str()
                    && existing.version != *requested_version
                    && requested_version != "*"
                {
                    warn!(
                        "Package conflict: {} is installed at v{} but v{} requested (strategy: {})",
                        req.name, existing.version, requested_version, self.conflict_strategy
                    );

                    match self.conflict_strategy.as_str() {
                        "fail" => {
                            return Err(AppError::Validation(format!(
                                "Package conflict: {} v{} is already installed in main-venv. \
                                 Requested v{}. Use a custom venv or change the conflict resolution strategy.",
                                req.name, existing.version, requested_version
                            )));
                        }
                        "suggest_custom_venv" => {
                            // Allow the install but log a warning — the API response indicates
                            // the conflict via the returned cache entry. In a more sophisticated
                            // implementation this would return a structured warning, but for now
                            // we log and proceed with the upgrade (same as force_upgrade behavior
                            // but with an explicit warning).
                            warn!(
                                "Conflict detected for {}: consider using a custom venv. \
                                 Proceeding with upgrade from v{} to v{}.",
                                req.name, existing.version, requested_version
                            );
                        }
                        _ => {
                            // "force_upgrade" or any unknown strategy: proceed with upgrade
                            info!(
                                "Force upgrading {} from v{} to v{}",
                                req.name, existing.version, requested_version
                            );
                        }
                    }
                }
            }
        }

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
        let cache = self.repo.upsert_cache(&cache).await?;

        // Best-effort metadata persistence to make future PyPI lookups and local search richer.
        let discovered = PythonPackage::new(&cache.package_name, &cache.version, None);
        if let Err(err) = self.repo.upsert_package(&discovered).await {
            warn!(
                package = %cache.package_name,
                version = %cache.version,
                error = %err,
                "Failed to persist discovered package metadata"
            );
        }

        Ok(cache)
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
    /// Uninstall a package from the main venv and remove it from the cache.
    pub async fn uninstall_package(&self, package_name: &str) -> Result<(), AppError> {
        if package_name.trim().is_empty() {
            return Err(AppError::Validation("Package name cannot be empty".to_string()));
        }

        let (venv_manager, package_manager) = match (&self.venv_manager, &self.package_manager) {
            (Some(v), Some(p)) => (v, p),
            _ => {
                return Err(AppError::Internal(
                    "Package uninstall service not available".to_string(),
                ));
            }
        };

        let main_venv_path = venv_manager.main_venv_path();

        info!("Uninstalling package: {}", package_name);
        let result = package_manager
            .uninstall_from_main_venv(&main_venv_path, package_name)
            .await;

        if !result.success {
            let err = result.error.unwrap_or_else(|| "Unknown error".to_string());
            warn!("Package {} uninstall failed: {}", package_name, err);
            return Err(AppError::Internal(format!("Package uninstall failed: {}", err)));
        }

        // Remove all cache entries for this package in the main venv (any version).
        let cached = self.repo.get_cache_by_venv("main", None).await?;
        for entry in cached.into_iter().filter(|e| e.package_name.to_lowercase() == package_name.to_lowercase()) {
            let _ = self.repo.delete_cache("main", None, &entry.package_name, &entry.version).await;
        }

        info!("Package {} uninstalled successfully", package_name);
        Ok(())
    }

    /// Remove package cache entry (DB only, no pip)
    pub async fn remove_package(
        &self,
        package_name: &str,
        version: &str,
    ) -> Result<(), AppError> {
        self.repo
            .delete_cache("main", None, package_name, version)
            .await
    }

    /// Persist discovered package metadata for future local search.
    pub async fn record_discovered_package(
        &self,
        package: &PythonPackage,
    ) -> Result<PythonPackage, AppError> {
        self.repo.upsert_package(package).await
    }

    /// Search known package metadata cached in the local database.
    pub async fn search_known_packages(
        &self,
        query: &str,
        limit: i32,
    ) -> Result<Vec<PythonPackage>, AppError> {
        self.repo.search_packages_by_name(query, limit).await
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
        let name = req.package_name.trim();
        if name.is_empty() {
            return Err(AppError::Validation("Package name cannot be empty".to_string()));
        }
        if let Err(msg) = validate_package_name(name) {
            return Err(AppError::Validation(msg));
        }

        let dep = JobDependency::new(job_id, name, req.version_constraint);
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

/// Validate a Python package name against PyPI conventions and reject shell metacharacters.
///
/// Rules:
/// 1. Must be non-empty and at most 214 characters (PEP 508 / PyPI limit).
/// 2. Must start and end with an ASCII letter or digit.
/// 3. Interior characters may be letters, digits, underscores, hyphens, or dots only.
/// 4. Must not contain whitespace or shell metacharacters that could enable injection.
pub(crate) fn validate_package_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Package name cannot be empty".to_string());
    }
    if name.len() > 214 {
        return Err("Package name must be at most 214 characters".to_string());
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphanumeric() {
        return Err(format!(
            "Package name must start with a letter or digit; got '{}'",
            first
        ));
    }
    let last = name.chars().last().unwrap();
    if !last.is_ascii_alphanumeric() {
        return Err(format!(
            "Package name must end with a letter or digit; got '{}'",
            last
        ));
    }
    for c in name.chars() {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') {
            return Err(format!(
                "Package name contains invalid character '{}'; only letters, digits, '_', '-', and '.' are allowed",
                c
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_package_name_valid() {
        assert!(validate_package_name("requests").is_ok());
        assert!(validate_package_name("numpy").is_ok());
        assert!(validate_package_name("my_pkg.name").is_ok());
        assert!(validate_package_name("my-pkg-name").is_ok());
        assert!(validate_package_name("A").is_ok());
        assert!(validate_package_name("package123").is_ok());
    }

    #[test]
    fn test_validate_package_name_empty() {
        let result = validate_package_name("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_validate_package_name_shell_injection() {
        // "rm -rf /" should be rejected: contains spaces and forward slash
        let result = validate_package_name("rm -rf /");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_package_name_shell_metacharacter() {
        // Shell dangerous characters must be rejected
        for bad in &["$HOME", "pkg;ls", "pkg|cat", "pkg&bg", "pkg$(whoami)", "pkg`x`"] {
            let result = validate_package_name(bad);
            assert!(result.is_err(), "expected rejection for {:?}", bad);
        }
    }

    #[test]
    fn test_validate_package_name_leading_trailing_special() {
        // Leading underscore / hyphen / dot not allowed
        assert!(validate_package_name("_pkg").is_err());
        assert!(validate_package_name("-pkg").is_err());
        assert!(validate_package_name(".pkg").is_err());
        // Trailing underscore / hyphen / dot not allowed
        assert!(validate_package_name("pkg_").is_err());
        assert!(validate_package_name("pkg-").is_err());
        assert!(validate_package_name("pkg.").is_err());
    }

    #[test]
    fn test_validate_package_name_too_long() {
        let long = "a".repeat(215);
        let result = validate_package_name(&long);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("214"));
    }
}
