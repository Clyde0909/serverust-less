//! Job queue models

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

/// Queue entry status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum QueueStatus {
    Queued,
    Processing,
    Completed,
    Failed,
    DeadLetter,
}

impl QueueStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueueStatus::Queued => "queued",
            QueueStatus::Processing => "processing",
            QueueStatus::Completed => "completed",
            QueueStatus::Failed => "failed",
            QueueStatus::DeadLetter => "dead_letter",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "queued" => Some(QueueStatus::Queued),
            "processing" => Some(QueueStatus::Processing),
            "completed" => Some(QueueStatus::Completed),
            "failed" => Some(QueueStatus::Failed),
            "dead_letter" => Some(QueueStatus::DeadLetter),
            _ => None,
        }
    }
}

/// Job queue entry
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct QueueEntry {
    /// Unique identifier
    pub id: String,
    /// Execution ID
    pub execution_id: String,
    /// Job ID
    pub job_id: String,
    /// Priority (higher = more priority)
    pub priority: i32,
    /// Status
    pub status: String,
    /// When queued
    pub queued_at: String,
    /// When started processing
    pub started_at: Option<String>,
    /// When completed
    pub completed_at: Option<String>,
}

/// Queue status response
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QueueStatusResponse {
    /// Total items in queue
    pub total_queued: i64,
    /// Items currently processing
    pub processing: i64,
    /// Items completed in last hour
    pub completed_last_hour: i64,
    /// Items failed in last hour
    pub failed_last_hour: i64,
    /// Items in dead letter queue
    pub dead_letter_count: i64,
    /// Queue depth by priority
    pub by_priority: Vec<PriorityCount>,
    /// In-memory queue size
    pub in_memory_size: usize,
    /// Overflow queue size (SQLite)
    pub overflow_size: i64,
}

/// Priority count
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PriorityCount {
    pub priority: i32,
    pub count: i64,
}

/// Internal queue item for in-memory queue
#[derive(Debug, Clone)]
pub struct QueueItem {
    /// Execution ID
    pub execution_id: String,
    /// Job ID
    pub job_id: String,
    /// Priority
    pub priority: i32,
    /// Python code
    pub python_code: String,
    /// Timeout in seconds
    pub timeout_seconds: i32,
    /// Memory limit in MB
    pub memory_limit_mb: i32,
    /// Input data
    pub input_data: Option<String>,
    /// Use custom venv
    pub use_custom_venv: bool,
    /// Environment variables to inject (JSON object)
    pub env_vars: Option<serde_json::Value>,
    /// DAG run ID if this execution is part of a DAG
    pub dag_run_id: Option<String>,
    /// DAG node execution ID if this execution is part of a DAG
    pub dag_node_id: Option<String>,
}

impl QueueEntry {
    /// Create a new queue entry
    pub fn new(execution_id: &str, job_id: &str, priority: i32) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            execution_id: execution_id.to_string(),
            job_id: job_id.to_string(),
            priority,
            status: QueueStatus::Queued.as_str().to_string(),
            queued_at: Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
        }
    }

    /// Mark as processing
    pub fn mark_processing(&mut self) {
        self.status = QueueStatus::Processing.as_str().to_string();
        self.started_at = Some(Utc::now().to_rfc3339());
    }

    /// Mark as completed
    pub fn mark_completed(&mut self) {
        self.status = QueueStatus::Completed.as_str().to_string();
        self.completed_at = Some(Utc::now().to_rfc3339());
    }

    /// Mark as failed
    pub fn mark_failed(&mut self) {
        self.status = QueueStatus::Failed.as_str().to_string();
        self.completed_at = Some(Utc::now().to_rfc3339());
    }

    /// Mark as dead letter (permanently failed after max retries)
    pub fn mark_dead_letter(&mut self) {
        self.status = QueueStatus::DeadLetter.as_str().to_string();
        self.completed_at = Some(Utc::now().to_rfc3339());
    }
}

impl QueueItem {
    /// Create from execution and job
    pub fn new(
        execution_id: &str,
        job_id: &str,
        priority: i32,
        python_code: &str,
        timeout_seconds: i32,
        memory_limit_mb: i32,
        input_data: Option<String>,
        use_custom_venv: bool,
    ) -> Self {
        Self {
            execution_id: execution_id.to_string(),
            job_id: job_id.to_string(),
            priority,
            python_code: python_code.to_string(),
            timeout_seconds,
            memory_limit_mb,
            input_data,
            use_custom_venv,
            env_vars: None,
            dag_run_id: None,
            dag_node_id: None,
        }
    }

    /// Create with full context including env vars and DAG metadata
    pub fn new_with_context(
        execution_id: &str,
        job_id: &str,
        priority: i32,
        python_code: &str,
        timeout_seconds: i32,
        memory_limit_mb: i32,
        input_data: Option<String>,
        use_custom_venv: bool,
        env_vars: Option<serde_json::Value>,
        dag_run_id: Option<String>,
        dag_node_id: Option<String>,
    ) -> Self {
        Self {
            execution_id: execution_id.to_string(),
            job_id: job_id.to_string(),
            priority,
            python_code: python_code.to_string(),
            timeout_seconds,
            memory_limit_mb,
            input_data,
            use_custom_venv,
            env_vars,
            dag_run_id,
            dag_node_id,
        }
    }
}

impl Ord for QueueItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher priority first, then by execution_id for deterministic ordering
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.execution_id.cmp(&other.execution_id))
    }
}

impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for QueueItem {
    fn eq(&self, other: &Self) -> bool {
        // Two items are the same if they represent the same execution (identity by execution_id)
        self.execution_id == other.execution_id
    }
}

impl Eq for QueueItem {}
