//! Execution log model

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

/// Log type enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum LogType {
    Stdout,
    Stderr,
    System,
}

impl LogType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogType::Stdout => "stdout",
            LogType::Stderr => "stderr",
            LogType::System => "system",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "stdout" => Some(LogType::Stdout),
            "stderr" => Some(LogType::Stderr),
            "system" => Some(LogType::System),
            _ => None,
        }
    }
}

/// Execution log entry
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct ExecutionLog {
    /// Unique identifier
    pub id: String,
    /// Associated execution ID
    pub execution_id: String,
    /// Log type (stdout/stderr)
    pub log_type: String,
    /// Log content
    pub log_content: String,
    /// Creation timestamp
    pub created_at: String,
}

/// Query parameters for execution logs
#[derive(Debug, Clone, Deserialize, ToSchema, Default)]
pub struct ListLogsQuery {
    /// Filter by log type
    pub log_type: Option<String>,
    /// Offset for pagination (used for streaming)
    pub offset: Option<i32>,
    /// Limit for pagination
    pub limit: Option<i32>,
}

/// Response for execution logs
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ExecutionLogsResponse {
    /// Execution ID
    pub execution_id: String,
    /// List of log entries
    pub logs: Vec<ExecutionLog>,
    /// Total count of logs
    pub total: i64,
}

impl ExecutionLog {
    /// Create a new log entry
    pub fn new(execution_id: &str, log_type: LogType, content: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            execution_id: execution_id.to_string(),
            log_type: log_type.as_str().to_string(),
            log_content: content.to_string(),
            created_at: Utc::now().to_rfc3339(),
        }
    }

    /// Create a stdout log
    pub fn stdout(execution_id: &str, content: &str) -> Self {
        Self::new(execution_id, LogType::Stdout, content)
    }

    /// Create a stderr log
    pub fn stderr(execution_id: &str, content: &str) -> Self {
        Self::new(execution_id, LogType::Stderr, content)
    }
}
