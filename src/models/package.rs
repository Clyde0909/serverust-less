//! Package related models

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

/// Python package info
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct PythonPackage {
    /// Unique identifier
    pub id: String,
    /// Package name
    pub name: String,
    /// Version
    pub version: String,
    /// Description
    pub description: Option<String>,
    /// PyPI URL
    pub pypi_url: Option<String>,
    /// Creation timestamp
    pub created_at: String,
}

/// Job dependency
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct JobDependency {
    /// Unique identifier
    pub id: String,
    /// Job ID
    pub job_id: String,
    /// Package name
    pub package_name: String,
    /// Version constraint (e.g., ">=1.0.0,<2.0.0", "==1.5.0", "*")
    pub version_constraint: String,
    /// Creation timestamp
    pub created_at: String,
}

/// Package installation status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PackageStatus {
    Installing,
    Ready,
    Failed,
}

impl PackageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PackageStatus::Installing => "installing",
            PackageStatus::Ready => "ready",
            PackageStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "installing" => Some(PackageStatus::Installing),
            "ready" => Some(PackageStatus::Ready),
            "failed" => Some(PackageStatus::Failed),
            _ => None,
        }
    }
}

/// Cached package entry
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct PackageCache {
    /// Unique identifier
    pub id: String,
    /// Venv type ("main" or "custom")
    pub venv_type: String,
    /// Venv ID (NULL for main, job_id for custom)
    pub venv_id: Option<String>,
    /// Package name
    pub package_name: String,
    /// Installed version
    pub version: String,
    /// Installation path
    pub installation_path: String,
    /// Size in bytes
    pub size_bytes: Option<i64>,
    /// Installation status
    pub status: String,
    /// Error message if failed
    pub error_message: Option<String>,
    /// When installed
    pub installed_at: String,
    /// Last used time
    pub last_used_at: Option<String>,
    /// Use count
    pub use_count: i32,
}

/// Request to install a package
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct InstallPackageRequest {
    /// Package name (required, valid PyPI package name pattern)
    #[validate(length(min = 1, max = 100, message = "Package name must be 1-100 characters"))]
    #[validate(custom(function = "validate_package_name"))]
    pub name: String,
    /// Version constraint (optional, defaults to latest)
    #[validate(length(max = 50, message = "Version must be at most 50 characters"))]
    pub version: Option<String>,
}

/// Validate Python package name format
fn validate_package_name(name: &str) -> Result<(), validator::ValidationError> {
    // Package names must start with a letter or digit, and contain only letters, digits, underscores, hyphens, and dots
    let is_valid = !name.is_empty()
        && name.chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false)
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.');

    if is_valid {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_package_name"))
    }
}

/// Request to add a dependency to a job
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct AddDependencyRequest {
    /// Package name (required)
    #[validate(length(min = 1, max = 100, message = "Package name must be 1-100 characters"))]
    pub package_name: String,
    /// Version constraint (optional, defaults to "*")
    #[validate(length(max = 100, message = "Version constraint must be at most 100 characters"))]
    pub version_constraint: Option<String>,
}

/// Response for package list
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PackageListResponse {
    /// List of packages
    pub packages: Vec<PackageCache>,
    /// Total count
    pub total: i64,
}

/// Response for dependency list
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DependencyListResponse {
    /// Job ID
    pub job_id: String,
    /// List of dependencies
    pub dependencies: Vec<JobDependency>,
}

/// Response for dependency installation status
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DependencyStatusResponse {
    /// Job ID
    pub job_id: String,
    /// Overall status
    pub status: String,
    /// Package statuses
    pub packages: Vec<PackageInstallStatus>,
}

/// Individual package installation status
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PackageInstallStatus {
    /// Package name
    pub name: String,
    /// Required version constraint
    pub required: String,
    /// Installed version (if any)
    pub installed: Option<String>,
    /// Status
    pub status: String,
}

/// PyPI search result
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PypiSearchResult {
    /// Package name
    pub name: String,
    /// Latest version
    pub version: String,
    /// Description
    pub description: Option<String>,
    /// Author
    pub author: Option<String>,
    /// Project URL
    pub project_url: Option<String>,
}

impl PythonPackage {
    /// Create a new package
    pub fn new(name: &str, version: &str, description: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            version: version.to_string(),
            description,
            pypi_url: Some(format!("https://pypi.org/project/{}/", name)),
            created_at: Utc::now().to_rfc3339(),
        }
    }
}

impl JobDependency {
    /// Create a new dependency
    pub fn new(job_id: &str, package_name: &str, version_constraint: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            job_id: job_id.to_string(),
            package_name: package_name.to_string(),
            version_constraint: version_constraint.unwrap_or_else(|| "*".to_string()),
            created_at: Utc::now().to_rfc3339(),
        }
    }
}

impl PackageCache {
    /// Create a new cache entry
    pub fn new(
        venv_type: &str,
        venv_id: Option<String>,
        package_name: &str,
        version: &str,
        installation_path: &str,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            venv_type: venv_type.to_string(),
            venv_id,
            package_name: package_name.to_string(),
            version: version.to_string(),
            installation_path: installation_path.to_string(),
            size_bytes: None,
            status: PackageStatus::Installing.as_str().to_string(),
            error_message: None,
            installed_at: Utc::now().to_rfc3339(),
            last_used_at: None,
            use_count: 0,
        }
    }

    /// Mark as ready
    pub fn mark_ready(&mut self, size_bytes: Option<i64>) {
        self.status = PackageStatus::Ready.as_str().to_string();
        self.size_bytes = size_bytes;
    }

    /// Mark as failed
    pub fn mark_failed(&mut self, error: &str) {
        self.status = PackageStatus::Failed.as_str().to_string();
        self.error_message = Some(error.to_string());
    }

    /// Record usage
    pub fn record_usage(&mut self) {
        self.use_count += 1;
        self.last_used_at = Some(Utc::now().to_rfc3339());
    }
}
