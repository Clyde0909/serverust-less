//! Audit service - business logic for audit logging

use crate::db::AuditRepository;
use crate::error::AppError;
use crate::models::{AuditAction, AuditLog, AuditLogListResponse, ListAuditLogsQuery};

/// Service for audit logging
#[derive(Clone)]
pub struct AuditService {
    repo: AuditRepository,
    enabled: bool,
}

impl AuditService {
    /// Create a new AuditService
    pub fn new(repo: AuditRepository, enabled: bool) -> Self {
        Self { repo, enabled }
    }

    /// Log an action
    pub async fn log(
        &self,
        action: AuditAction,
        resource_id: Option<String>,
        user_id: Option<String>,
        details: Option<serde_json::Value>,
        ip_address: Option<String>,
    ) -> Result<(), AppError> {
        if !self.enabled {
            return Ok(());
        }

        let log = AuditLog::new(action, resource_id, user_id, details, ip_address);
        self.repo.create(&log).await?;
        Ok(())
    }

    /// Log a job action
    pub async fn log_job_action(
        &self,
        action: AuditAction,
        job_id: &str,
        details: Option<serde_json::Value>,
    ) -> Result<(), AppError> {
        self.log(action, Some(job_id.to_string()), None, details, None)
            .await
    }

    /// Log an execution action
    pub async fn log_execution_action(
        &self,
        action: AuditAction,
        execution_id: &str,
        details: Option<serde_json::Value>,
    ) -> Result<(), AppError> {
        self.log(action, Some(execution_id.to_string()), None, details, None)
            .await
    }

    /// Log a package action
    pub async fn log_package_action(
        &self,
        action: AuditAction,
        package_name: &str,
        details: Option<serde_json::Value>,
    ) -> Result<(), AppError> {
        self.log(action, Some(package_name.to_string()), None, details, None)
            .await
    }

    /// List audit logs
    pub async fn list(&self, query: ListAuditLogsQuery) -> Result<AuditLogListResponse, AppError> {
        let limit = query.limit.clamp(1, 500);
        let offset = query.offset.max(0);

        let validated_query = ListAuditLogsQuery {
            limit,
            offset,
            action: query.action,
            resource_type: query.resource_type,
            resource_id: query.resource_id,
            from: query.from,
            to: query.to,
        };

        let (logs, total) = self.repo.list(&validated_query).await?;

        Ok(AuditLogListResponse {
            logs,
            total,
            limit,
            offset,
        })
    }

    /// Get logs for a specific resource
    pub async fn get_resource_logs(
        &self,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<Vec<AuditLog>, AppError> {
        self.repo.get_by_resource(resource_type, resource_id).await
    }

    /// Clean up old logs
    pub async fn cleanup(&self, older_than_days: i32) -> Result<u64, AppError> {
        self.repo.cleanup_old(older_than_days).await
    }
}
