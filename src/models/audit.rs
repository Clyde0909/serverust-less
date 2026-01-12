//! Audit log models

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct AuditLog {
    /// Unique identifier
    pub id: String,
    /// Action performed
    pub action: String,
    /// Resource type
    pub resource_type: String,
    /// Resource ID
    pub resource_id: Option<String>,
    /// User ID (tenant ID if auth enabled)
    pub user_id: Option<String>,
    /// JSON details
    pub details: Option<String>,
    /// IP address
    pub ip_address: Option<String>,
    /// Creation timestamp
    pub created_at: String,
}

/// Audit log actions
#[derive(Debug, Clone, Copy)]
pub enum AuditAction {
    // Job actions
    JobCreate,
    JobUpdate,
    JobDelete,
    JobEnable,
    JobDisable,
    JobExecute,
    JobClone,
    // Execution actions
    ExecutionCancel,
    ExecutionRetry,
    ExecutionDelete,
    // Package actions
    PackageInstall,
    PackageUninstall,
    // Venv actions
    VenvCreate,
    VenvDelete,
    VenvUpdate,
    // System actions
    SystemStartup,
    SystemShutdown,
}

impl AuditAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditAction::JobCreate => "job.create",
            AuditAction::JobUpdate => "job.update",
            AuditAction::JobDelete => "job.delete",
            AuditAction::JobEnable => "job.enable",
            AuditAction::JobDisable => "job.disable",
            AuditAction::JobExecute => "job.execute",
            AuditAction::JobClone => "job.clone",
            AuditAction::ExecutionCancel => "execution.cancel",
            AuditAction::ExecutionRetry => "execution.retry",
            AuditAction::ExecutionDelete => "execution.delete",
            AuditAction::PackageInstall => "package.install",
            AuditAction::PackageUninstall => "package.uninstall",
            AuditAction::VenvCreate => "venv.create",
            AuditAction::VenvDelete => "venv.delete",
            AuditAction::VenvUpdate => "venv.update",
            AuditAction::SystemStartup => "system.startup",
            AuditAction::SystemShutdown => "system.shutdown",
        }
    }

    pub fn resource_type(&self) -> &'static str {
        match self {
            AuditAction::JobCreate
            | AuditAction::JobUpdate
            | AuditAction::JobDelete
            | AuditAction::JobEnable
            | AuditAction::JobDisable
            | AuditAction::JobExecute
            | AuditAction::JobClone => "job",
            AuditAction::ExecutionCancel
            | AuditAction::ExecutionRetry
            | AuditAction::ExecutionDelete => "execution",
            AuditAction::PackageInstall | AuditAction::PackageUninstall => "package",
            AuditAction::VenvCreate | AuditAction::VenvDelete | AuditAction::VenvUpdate => "venv",
            AuditAction::SystemStartup | AuditAction::SystemShutdown => "system",
        }
    }
}

/// Query parameters for listing audit logs
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ListAuditLogsQuery {
    /// Maximum number of results (default: 50)
    #[serde(default = "default_limit")]
    pub limit: i32,
    /// Offset for pagination (default: 0)
    #[serde(default)]
    pub offset: i32,
    /// Filter by action
    pub action: Option<String>,
    /// Filter by resource type
    pub resource_type: Option<String>,
    /// Filter by resource ID
    pub resource_id: Option<String>,
    /// Filter from date
    pub from: Option<String>,
    /// Filter to date
    pub to: Option<String>,
}

/// Response for audit log list
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AuditLogListResponse {
    /// List of audit logs
    pub logs: Vec<AuditLog>,
    /// Total count
    pub total: i64,
    /// Current limit
    pub limit: i32,
    /// Current offset
    pub offset: i32,
}

fn default_limit() -> i32 {
    50
}

impl AuditLog {
    /// Create a new audit log entry
    pub fn new(
        action: AuditAction,
        resource_id: Option<String>,
        user_id: Option<String>,
        details: Option<serde_json::Value>,
        ip_address: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            action: action.as_str().to_string(),
            resource_type: action.resource_type().to_string(),
            resource_id,
            user_id,
            details: details.map(|v| v.to_string()),
            ip_address,
            created_at: Utc::now().to_rfc3339(),
        }
    }
}
