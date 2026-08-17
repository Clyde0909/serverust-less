//! Job model and DTOs

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

/// Job entity representing a Python code job
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Job {
    /// Unique identifier
    pub id: String,
    /// Job name
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Python code to execute
    pub python_code: String,
    /// Execution timeout in seconds
    pub timeout_seconds: i32,
    /// Memory limit in MB
    pub memory_limit_mb: i32,
    /// Whether to use a custom virtual environment
    pub use_custom_venv: bool,
    /// Selected venv ID (None = main venv)
    pub venv_id: Option<String>,
    /// Job priority (higher = more priority)
    pub priority: i32,
    /// Maximum retry attempts
    pub max_retries: i32,
    /// Current immutable version number for this job definition
    pub current_version: i32,
    /// Creation timestamp
    pub created_at: String,
    /// Last update timestamp
    pub updated_at: String,
    /// Whether the job is enabled
    pub enabled: bool,
    /// Environment variables to inject at execution time (JSON object)
    pub env_vars: Option<serde_json::Value>,
}

/// Request to create a new job
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct CreateJobRequest {
    /// Job name (required, 1-100 characters)
    #[validate(length(min = 1, max = 100, message = "Name must be 1-100 characters"))]
    pub name: String,
    /// Optional description (max 1000 characters)
    #[validate(length(max = 1000, message = "Description must be at most 1000 characters"))]
    pub description: Option<String>,
    /// Python code to execute (required, max 100KB)
    #[validate(length(min = 1, max = 102400, message = "Python code must be 1-102400 characters"))]
    pub python_code: String,
    /// Execution timeout in seconds (default: 30, range: 1-3600)
    #[serde(default = "default_timeout")]
    #[validate(range(min = 1, max = 3600, message = "Timeout must be 1-3600 seconds"))]
    pub timeout_seconds: i32,
    /// Memory limit in MB (default: 128, range: 16-4096)
    #[serde(default = "default_memory_limit")]
    #[validate(range(min = 16, max = 4096, message = "Memory limit must be 16-4096 MB"))]
    pub memory_limit_mb: i32,
    /// Whether to use a custom virtual environment (default: false)
    #[serde(default)]
    pub use_custom_venv: bool,
    /// Venv ID to use for this job (None = main venv)
    pub venv_id: Option<String>,
    /// Job priority (default: 0, range: -100 to 100)
    #[serde(default)]
    #[validate(range(min = -100, max = 100, message = "Priority must be -100 to 100"))]
    pub priority: i32,
    /// Maximum retry attempts (default: 0, range: 0-10)
    #[serde(default)]
    #[validate(range(min = 0, max = 10, message = "Max retries must be 0-10"))]
    pub max_retries: i32,
    /// Environment variables to inject at execution time (JSON object, default: null)
    #[serde(default)]
    pub env_vars: Option<serde_json::Value>,
}

/// Request to update an existing job
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct UpdateJobRequest {
    /// Job name (1-100 characters)
    #[validate(length(min = 1, max = 100, message = "Name must be 1-100 characters"))]
    pub name: Option<String>,
    /// Description (max 1000 characters)
    #[validate(length(max = 1000, message = "Description must be at most 1000 characters"))]
    pub description: Option<String>,
    /// Python code (max 100KB)
    #[validate(length(min = 1, max = 102400, message = "Python code must be 1-102400 characters"))]
    pub python_code: Option<String>,
    /// Execution timeout in seconds (range: 1-3600)
    #[validate(range(min = 1, max = 3600, message = "Timeout must be 1-3600 seconds"))]
    pub timeout_seconds: Option<i32>,
    /// Memory limit in MB (range: 16-4096)
    #[validate(range(min = 16, max = 4096, message = "Memory limit must be 16-4096 MB"))]
    pub memory_limit_mb: Option<i32>,
    /// Whether to use a custom virtual environment
    pub use_custom_venv: Option<bool>,
    /// Venv ID to use for this job (None = main venv, "" clears selection)
    pub venv_id: Option<String>,
    /// Job priority (range: -100 to 100)
    #[validate(range(min = -100, max = 100, message = "Priority must be -100 to 100"))]
    pub priority: Option<i32>,
    /// Maximum retry attempts (range: 0-10)
    #[validate(range(min = 0, max = 10, message = "Max retries must be 0-10"))]
    pub max_retries: Option<i32>,
    /// Whether the job is enabled
    pub enabled: Option<bool>,
    /// Optional human-readable summary for the new version snapshot
    #[validate(length(max = 500, message = "Change summary must be at most 500 characters"))]
    pub change_summary: Option<String>,
    /// Environment variables to inject at execution time (JSON object, null = no change)
    pub env_vars: Option<serde_json::Value>,
}

/// Immutable snapshot of a job definition at a specific version.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct JobVersion {
    /// Unique identifier for the version snapshot
    pub id: String,
    /// Job ID this snapshot belongs to
    pub job_id: String,
    /// Monotonic version number for the job
    pub version_number: i32,
    /// Job name at this version
    pub name: String,
    /// Job description at this version
    pub description: Option<String>,
    /// Python code at this version
    pub python_code: String,
    /// Execution timeout in seconds at this version
    pub timeout_seconds: i32,
    /// Memory limit in MB at this version
    pub memory_limit_mb: i32,
    /// Whether this version used a custom virtual environment
    pub use_custom_venv: bool,
    /// Selected venv ID at this version
    pub venv_id: Option<String>,
    /// Queue priority at this version
    pub priority: i32,
    /// Maximum retries at this version
    pub max_retries: i32,
    /// Enabled state captured in this snapshot
    pub enabled: bool,
    /// When the version snapshot was created
    pub created_at: String,
    /// Optional summary describing why this version exists
    pub change_summary: Option<String>,
    /// Origin of this snapshot: create, update, restore, clone, etc.
    pub source: String,
    /// Environment variables at this version
    pub env_vars: Option<serde_json::Value>,
}

/// Response payload for listing all versions of a job.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobVersionListResponse {
    /// Job ID these versions belong to
    pub job_id: String,
    /// Current version number of the job
    pub current_version: i32,
    /// All known immutable versions, newest first
    pub versions: Vec<JobVersion>,
}

/// Request body for restoring an older job version.
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct RestoreJobVersionRequest {
    /// Optional summary for the restore operation
    #[validate(length(max = 500, message = "Change summary must be at most 500 characters"))]
    pub change_summary: Option<String>,
}

/// Request body for bulk delete operations
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BulkDeleteRequest {
    /// List of IDs to delete
    pub ids: Vec<String>,
}

/// Response for bulk operations
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BulkOperationResponse {
    /// Number of items successfully processed
    pub success_count: u64,
    /// Number of items that failed
    pub failure_count: u64,
    /// Error messages for failed items
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// Request to clone a job
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CloneJobRequest {
    /// Optional new name for the cloned job (defaults to "{original_name} (copy)")
    pub name: Option<String>,
}

/// Query parameters for listing jobs
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListJobsQuery {
    /// Maximum number of results (default: 20)
    #[serde(default = "default_limit")]
    pub limit: i32,
    /// Offset for pagination (default: 0)
    #[serde(default)]
    pub offset: i32,
    /// Filter by enabled status
    pub enabled: Option<bool>,
    /// Search by name
    pub search: Option<String>,
}

/// Response for paginated job list
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobListResponse {
    /// List of jobs
    pub jobs: Vec<Job>,
    /// Total count of jobs matching the query
    pub total: i64,
    /// Current limit
    pub limit: i32,
    /// Current offset
    pub offset: i32,
}

// Default value functions
fn default_timeout() -> i32 { 30 }
fn default_memory_limit() -> i32 { 128 }
fn default_limit() -> i32 { 20 }

impl Job {
    /// Create a new Job from a CreateJobRequest
    pub fn new(req: CreateJobRequest) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4().to_string(),
            name: req.name,
            description: req.description,
            python_code: req.python_code,
            timeout_seconds: req.timeout_seconds,
            memory_limit_mb: req.memory_limit_mb,
            use_custom_venv: req.use_custom_venv,
            venv_id: req.venv_id.clone(),
            priority: req.priority,
            max_retries: req.max_retries,
            current_version: 1,
            created_at: now.clone(),
            updated_at: now,
            enabled: false,
            env_vars: req.env_vars,
        }
    }

    /// Apply updates from UpdateJobRequest
    pub fn apply_update(&mut self, req: UpdateJobRequest) {
        if let Some(name) = req.name {
            self.name = name;
        }
        if let Some(description) = req.description {
            self.description = Some(description);
        }
        if let Some(python_code) = req.python_code {
            self.python_code = python_code;
        }
        if let Some(timeout_seconds) = req.timeout_seconds {
            self.timeout_seconds = timeout_seconds;
        }
        if let Some(memory_limit_mb) = req.memory_limit_mb {
            self.memory_limit_mb = memory_limit_mb;
        }
        if let Some(use_custom_venv) = req.use_custom_venv {
            self.use_custom_venv = use_custom_venv;
        }
        // Empty string means "clear venv selection" (use main)
        match req.venv_id {
            Some(ref v) if v.is_empty() => self.venv_id = None,
            Some(v) => self.venv_id = Some(v),
            None => {}
        }
        if let Some(priority) = req.priority {
            self.priority = priority;
        }
        if let Some(max_retries) = req.max_retries {
            self.max_retries = max_retries;
        }
        if let Some(enabled) = req.enabled {
            self.enabled = enabled;
        }
        // env_vars: None = no change, Some(v) = replace
        if req.env_vars.is_some() {
            self.env_vars = req.env_vars;
        }
        self.updated_at = Utc::now().to_rfc3339();
    }
}

impl JobVersion {
    /// Create an immutable snapshot from the current job state.
    pub fn from_job(job: &Job, change_summary: Option<String>, source: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            job_id: job.id.clone(),
            version_number: job.current_version,
            name: job.name.clone(),
            description: job.description.clone(),
            python_code: job.python_code.clone(),
            timeout_seconds: job.timeout_seconds,
            memory_limit_mb: job.memory_limit_mb,
            use_custom_venv: job.use_custom_venv,
            venv_id: job.venv_id.clone(),
            priority: job.priority,
            max_retries: job.max_retries,
            enabled: job.enabled,
            created_at: Utc::now().to_rfc3339(),
            change_summary,
            source: source.to_string(),
            env_vars: job.env_vars.clone(),
        }
    }
}

impl CreateJobRequest {
    /// Validate the create request
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Job name cannot be empty".to_string());
        }
        if self.name.len() > 100 {
            return Err("Job name cannot exceed 100 characters".to_string());
        }
        if self.python_code.trim().is_empty() {
            return Err("Python code cannot be empty".to_string());
        }
        if self.timeout_seconds <= 0 {
            return Err("Timeout must be positive".to_string());
        }
        if self.timeout_seconds > 3600 {
            return Err("Timeout cannot exceed 3600 seconds (1 hour)".to_string());
        }
        if self.memory_limit_mb <= 0 {
            return Err("Memory limit must be positive".to_string());
        }
        if self.memory_limit_mb > 4096 {
            return Err("Memory limit cannot exceed 4096 MB".to_string());
        }
        Ok(())
    }
}
