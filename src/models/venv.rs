//! Virtual environment models

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

/// Venv type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum VenvType {
    Main,
    Custom,
}

impl VenvType {
    pub fn as_str(&self) -> &'static str {
        match self {
            VenvType::Main => "main",
            VenvType::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "main" => Some(VenvType::Main),
            "custom" => Some(VenvType::Custom),
            _ => None,
        }
    }
}

/// Venv status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum VenvStatus {
    Creating,
    Ready,
    Updating,
    Failed,
    Deleted,
}

impl VenvStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            VenvStatus::Creating => "creating",
            VenvStatus::Ready => "ready",
            VenvStatus::Updating => "updating",
            VenvStatus::Failed => "failed",
            VenvStatus::Deleted => "deleted",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "creating" => Some(VenvStatus::Creating),
            "ready" => Some(VenvStatus::Ready),
            "updating" => Some(VenvStatus::Updating),
            "failed" => Some(VenvStatus::Failed),
            "deleted" => Some(VenvStatus::Deleted),
            _ => None,
        }
    }
}

/// Virtual environment entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Venv {
    /// Unique identifier
    pub id: String,
    /// Type (main/custom)
    pub venv_type: String,
    /// Associated job ID (NULL for main)
    pub job_id: Option<String>,
    /// Filesystem path
    pub path: String,
    /// Python version
    pub python_version: Option<String>,
    /// Current status
    pub status: String,
    /// Size in bytes
    pub size_bytes: Option<i64>,
    /// Number of installed packages
    pub package_count: i32,
    /// Error message if failed
    pub error_message: Option<String>,
    /// Creation timestamp
    pub created_at: String,
    /// Update timestamp
    pub updated_at: String,
    /// Last used timestamp
    pub last_used_at: Option<String>,
}

/// Response for venv list
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct VenvListResponse {
    /// List of venvs
    pub venvs: Vec<Venv>,
    /// Total count
    pub total: i64,
}

/// Venv info for a job
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobVenvInfo {
    /// Job ID
    pub job_id: String,
    /// Whether job uses custom venv
    pub use_custom_venv: bool,
    /// Venv details
    pub venv: Option<Venv>,
    /// Main venv details (for fallback info)
    pub main_venv: Option<Venv>,
}

/// Main venv status
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MainVenvStatus {
    /// Venv info
    pub venv: Option<Venv>,
    /// Package count
    pub package_count: i32,
    /// Status
    pub status: String,
    /// Installed packages
    pub packages: Vec<String>,
}

impl Venv {
    /// Create a new main venv
    pub fn new_main(path: &str, python_version: Option<String>) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            id: "main-venv".to_string(),
            venv_type: VenvType::Main.as_str().to_string(),
            job_id: None,
            path: path.to_string(),
            python_version,
            status: VenvStatus::Creating.as_str().to_string(),
            size_bytes: None,
            package_count: 0,
            error_message: None,
            created_at: now.clone(),
            updated_at: now,
            last_used_at: None,
        }
    }

    /// Create a new custom venv for a job
    pub fn new_custom(job_id: &str, path: &str, python_version: Option<String>) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4().to_string(),
            venv_type: VenvType::Custom.as_str().to_string(),
            job_id: Some(job_id.to_string()),
            path: path.to_string(),
            python_version,
            status: VenvStatus::Creating.as_str().to_string(),
            size_bytes: None,
            package_count: 0,
            error_message: None,
            created_at: now.clone(),
            updated_at: now,
            last_used_at: None,
        }
    }

    /// Mark as ready
    pub fn mark_ready(&mut self) {
        self.status = VenvStatus::Ready.as_str().to_string();
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Mark as updating
    pub fn mark_updating(&mut self) {
        self.status = VenvStatus::Updating.as_str().to_string();
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Mark as failed
    pub fn mark_failed(&mut self, error: &str) {
        self.status = VenvStatus::Failed.as_str().to_string();
        self.error_message = Some(error.to_string());
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Record usage
    pub fn record_usage(&mut self) {
        self.last_used_at = Some(Utc::now().to_rfc3339());
    }

    /// Update package count
    pub fn set_package_count(&mut self, count: i32) {
        self.package_count = count;
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Check if venv is usable
    pub fn is_ready(&self) -> bool {
        self.status == VenvStatus::Ready.as_str()
    }
}
