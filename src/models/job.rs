//! Job model and DTOs

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

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
    /// Job priority (higher = more priority)
    pub priority: i32,
    /// Maximum retry attempts
    pub max_retries: i32,
    /// Creation timestamp
    pub created_at: String,
    /// Last update timestamp
    pub updated_at: String,
    /// Whether the job is enabled
    pub enabled: bool,
}

/// Request to create a new job
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateJobRequest {
    /// Job name (required)
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Python code to execute (required)
    pub python_code: String,
    /// Execution timeout in seconds (default: 30)
    #[serde(default = "default_timeout")]
    pub timeout_seconds: i32,
    /// Memory limit in MB (default: 128)
    #[serde(default = "default_memory_limit")]
    pub memory_limit_mb: i32,
    /// Whether to use a custom virtual environment (default: false)
    #[serde(default)]
    pub use_custom_venv: bool,
    /// Job priority (default: 0)
    #[serde(default)]
    pub priority: i32,
    /// Maximum retry attempts (default: 0)
    #[serde(default)]
    pub max_retries: i32,
}

/// Request to update an existing job
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateJobRequest {
    /// Job name
    pub name: Option<String>,
    /// Description
    pub description: Option<String>,
    /// Python code
    pub python_code: Option<String>,
    /// Execution timeout in seconds
    pub timeout_seconds: Option<i32>,
    /// Memory limit in MB
    pub memory_limit_mb: Option<i32>,
    /// Whether to use a custom virtual environment
    pub use_custom_venv: Option<bool>,
    /// Job priority
    pub priority: Option<i32>,
    /// Maximum retry attempts
    pub max_retries: Option<i32>,
    /// Whether the job is enabled
    pub enabled: Option<bool>,
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
            priority: req.priority,
            max_retries: req.max_retries,
            created_at: now.clone(),
            updated_at: now,
            enabled: true,
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
        if let Some(priority) = req.priority {
            self.priority = priority;
        }
        if let Some(max_retries) = req.max_retries {
            self.max_retries = max_retries;
        }
        if let Some(enabled) = req.enabled {
            self.enabled = enabled;
        }
        self.updated_at = Utc::now().to_rfc3339();
    }
}

impl CreateJobRequest {
    /// Validate the create request
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Job name cannot be empty".to_string());
        }
        if self.name.len() > 255 {
            return Err("Job name cannot exceed 255 characters".to_string());
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
