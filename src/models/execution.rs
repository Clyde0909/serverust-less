//! Execution model and DTOs

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

/// Execution status enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionStatus {
    Pending,
    Queued,
    Running,
    Success,
    Failed,
    Timeout,
    Cancelled,
}

impl ExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionStatus::Pending => "pending",
            ExecutionStatus::Queued => "queued",
            ExecutionStatus::Running => "running",
            ExecutionStatus::Success => "success",
            ExecutionStatus::Failed => "failed",
            ExecutionStatus::Timeout => "timeout",
            ExecutionStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pending" => Some(ExecutionStatus::Pending),
            "queued" => Some(ExecutionStatus::Queued),
            "running" => Some(ExecutionStatus::Running),
            "success" => Some(ExecutionStatus::Success),
            "failed" => Some(ExecutionStatus::Failed),
            "timeout" => Some(ExecutionStatus::Timeout),
            "cancelled" => Some(ExecutionStatus::Cancelled),
            _ => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ExecutionStatus::Success
                | ExecutionStatus::Failed
                | ExecutionStatus::Timeout
                | ExecutionStatus::Cancelled
        )
    }
}

impl Default for ExecutionStatus {
    fn default() -> Self {
        ExecutionStatus::Pending
    }
}

/// Execution entity representing a job execution instance
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Execution {
    /// Unique identifier
    pub id: String,
    /// Associated job ID
    pub job_id: String,
    /// Immutable job version that this execution will run
    pub job_version: i32,
    /// Current status
    pub status: String,
    /// Input data (JSON)
    pub input_data: Option<String>,
    /// Output data (JSON or text)
    pub output_data: Option<String>,
    /// Error message if failed
    pub error_message: Option<String>,
    /// Number of retry attempts
    pub retry_count: i32,
    /// Worker handling this execution
    pub worker_id: Option<String>,
    /// When execution started
    pub started_at: Option<String>,
    /// When execution completed
    pub completed_at: Option<String>,
    /// Duration in milliseconds
    pub duration_ms: Option<i64>,
    /// Creation timestamp
    pub created_at: String,
}

/// Request to execute a job
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ExecuteJobRequest {
    /// Input data to pass to the job
    pub input_data: Option<serde_json::Value>,
    /// Override priority for this execution
    pub priority: Option<i32>,
}

/// Query parameters for listing executions
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListExecutionsQuery {
    /// Maximum number of results (default: 20)
    #[serde(default = "default_limit")]
    pub limit: i32,
    /// Offset for pagination (default: 0)
    #[serde(default)]
    pub offset: i32,
    /// Filter by status
    pub status: Option<String>,
    /// Filter by job ID
    pub job_id: Option<String>,
    /// Filter from date
    pub from: Option<String>,
    /// Filter to date
    pub to: Option<String>,
}

/// Response for paginated execution list
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExecutionListResponse {
    /// List of executions
    pub executions: Vec<Execution>,
    /// Total count
    pub total: i64,
    /// Current limit
    pub limit: i32,
    /// Current offset
    pub offset: i32,
}

/// Execution with job details
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExecutionWithJob {
    #[serde(flatten)]
    pub execution: Execution,
    pub job_name: String,
}

fn default_limit() -> i32 {
    20
}

impl Execution {
    /// Create a new execution for a job
    pub fn new(job_id: &str, input_data: Option<serde_json::Value>, job_version: i32) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4().to_string(),
            job_id: job_id.to_string(),
            job_version,
            status: ExecutionStatus::Pending.as_str().to_string(),
            input_data: input_data.map(|v| v.to_string()),
            output_data: None,
            error_message: None,
            retry_count: 0,
            worker_id: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            created_at: now,
        }
    }

    /// Get the status as enum
    pub fn status_enum(&self) -> ExecutionStatus {
        ExecutionStatus::from_str(&self.status).unwrap_or_default()
    }

    /// Mark as queued
    pub fn mark_queued(&mut self) {
        self.status = ExecutionStatus::Queued.as_str().to_string();
    }

    /// Mark as running
    pub fn mark_running(&mut self, worker_id: &str) {
        self.status = ExecutionStatus::Running.as_str().to_string();
        self.worker_id = Some(worker_id.to_string());
        self.started_at = Some(Utc::now().to_rfc3339());
    }

    /// Mark as completed successfully
    pub fn mark_success(&mut self, output: String) {
        self.status = ExecutionStatus::Success.as_str().to_string();
        self.output_data = Some(output);
        self.complete();
    }

    /// Mark as failed
    pub fn mark_failed(&mut self, error: String) {
        self.status = ExecutionStatus::Failed.as_str().to_string();
        self.error_message = Some(error);
        self.complete();
    }

    /// Mark as timed out
    pub fn mark_timeout(&mut self) {
        self.status = ExecutionStatus::Timeout.as_str().to_string();
        self.error_message = Some("Execution timed out".to_string());
        self.complete();
    }

    /// Mark as cancelled
    pub fn mark_cancelled(&mut self) {
        self.status = ExecutionStatus::Cancelled.as_str().to_string();
        self.complete();
    }

    /// Complete the execution
    fn complete(&mut self) {
        let now = Utc::now();
        self.completed_at = Some(now.to_rfc3339());
        if let Some(ref started) = self.started_at {
            if let Ok(start_time) = chrono::DateTime::parse_from_rfc3339(started) {
                self.duration_ms = Some((now - start_time.with_timezone(&Utc)).num_milliseconds());
            }
        }
    }

    /// Increment retry count
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
        self.status = ExecutionStatus::Pending.as_str().to_string();
        self.error_message = None;
    }
}
