//! Audit log repository

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{AuditLog, ListAuditLogsQuery};

/// Repository for audit log database operations
#[derive(Clone)]
pub struct AuditRepository {
    pool: SqlitePool,
}

impl AuditRepository {
    /// Create a new AuditRepository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new audit log entry
    pub async fn create(&self, log: &AuditLog) -> Result<AuditLog, AppError> {
        sqlx::query(
            r#"
            INSERT INTO audit_logs (id, action, resource_type, resource_id, user_id, details, ip_address, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&log.id)
        .bind(&log.action)
        .bind(&log.resource_type)
        .bind(&log.resource_id)
        .bind(&log.user_id)
        .bind(&log.details)
        .bind(&log.ip_address)
        .bind(&log.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(log.clone())
    }

    /// List audit logs with filtering and pagination
    pub async fn list(&self, query: &ListAuditLogsQuery) -> Result<(Vec<AuditLog>, i64), AppError> {
        let mut sql_query = String::from(
            r#"
            SELECT id, action, resource_type, resource_id, user_id, details, ip_address, created_at
            FROM audit_logs
            WHERE 1=1
            "#,
        );

        let mut count_query = String::from("SELECT COUNT(*) FROM audit_logs WHERE 1=1");

        if query.action.is_some() {
            sql_query.push_str(" AND action = ?");
            count_query.push_str(" AND action = ?");
        }

        if query.resource_type.is_some() {
            sql_query.push_str(" AND resource_type = ?");
            count_query.push_str(" AND resource_type = ?");
        }

        if query.resource_id.is_some() {
            sql_query.push_str(" AND resource_id = ?");
            count_query.push_str(" AND resource_id = ?");
        }

        if query.from.is_some() {
            sql_query.push_str(" AND created_at >= ?");
            count_query.push_str(" AND created_at >= ?");
        }

        if query.to.is_some() {
            sql_query.push_str(" AND created_at <= ?");
            count_query.push_str(" AND created_at <= ?");
        }

        sql_query.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");

        // Build and execute count query
        let mut count_qb = sqlx::query_scalar::<_, i64>(&count_query);
        if let Some(ref action) = query.action {
            count_qb = count_qb.bind(action);
        }
        if let Some(ref resource_type) = query.resource_type {
            count_qb = count_qb.bind(resource_type);
        }
        if let Some(ref resource_id) = query.resource_id {
            count_qb = count_qb.bind(resource_id);
        }
        if let Some(ref from) = query.from {
            count_qb = count_qb.bind(from);
        }
        if let Some(ref to) = query.to {
            count_qb = count_qb.bind(to);
        }

        let total = count_qb
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        // Build and execute main query
        let mut main_qb = sqlx::query_as::<_, AuditLog>(&sql_query);
        if let Some(ref action) = query.action {
            main_qb = main_qb.bind(action);
        }
        if let Some(ref resource_type) = query.resource_type {
            main_qb = main_qb.bind(resource_type);
        }
        if let Some(ref resource_id) = query.resource_id {
            main_qb = main_qb.bind(resource_id);
        }
        if let Some(ref from) = query.from {
            main_qb = main_qb.bind(from);
        }
        if let Some(ref to) = query.to {
            main_qb = main_qb.bind(to);
        }
        main_qb = main_qb.bind(query.limit).bind(query.offset);

        let logs = main_qb
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok((logs, total))
    }

    /// Get logs for a specific resource
    pub async fn get_by_resource(&self, resource_type: &str, resource_id: &str) -> Result<Vec<AuditLog>, AppError> {
        sqlx::query_as::<_, AuditLog>(
            r#"
            SELECT id, action, resource_type, resource_id, user_id, details, ip_address, created_at
            FROM audit_logs
            WHERE resource_type = ? AND resource_id = ?
            ORDER BY created_at DESC
            "#,
        )
        .bind(resource_type)
        .bind(resource_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Clean up old audit logs
    pub async fn cleanup_old(&self, days: i32) -> Result<u64, AppError> {
        let result = sqlx::query(
            "DELETE FROM audit_logs WHERE created_at < datetime('now', '-' || ? || ' days')",
        )
        .bind(days)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(result.rows_affected())
    }
}
