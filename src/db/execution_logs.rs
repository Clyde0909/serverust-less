//! Execution log repository

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::ExecutionLog;

/// Repository for execution log database operations
#[derive(Clone)]
pub struct ExecutionLogRepository {
    pool: SqlitePool,
}

impl ExecutionLogRepository {
    /// Create a new ExecutionLogRepository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new log entry
    pub async fn create(&self, log: &ExecutionLog) -> Result<ExecutionLog, AppError> {
        sqlx::query(
            r#"
            INSERT INTO execution_logs (id, execution_id, log_type, log_content, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&log.id)
        .bind(&log.execution_id)
        .bind(&log.log_type)
        .bind(&log.log_content)
        .bind(&log.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(log.clone())
    }

    /// Get logs for an execution
    pub async fn get_by_execution(&self, execution_id: &str, log_type: Option<&str>) -> Result<Vec<ExecutionLog>, AppError> {
        let query = if let Some(lt) = log_type {
            sqlx::query_as::<_, ExecutionLog>(
                r#"
                SELECT id, execution_id, log_type, log_content, created_at
                FROM execution_logs
                WHERE execution_id = ? AND log_type = ?
                ORDER BY created_at ASC
                "#,
            )
            .bind(execution_id)
            .bind(lt)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, ExecutionLog>(
                r#"
                SELECT id, execution_id, log_type, log_content, created_at
                FROM execution_logs
                WHERE execution_id = ?
                ORDER BY created_at ASC
                "#,
            )
            .bind(execution_id)
            .fetch_all(&self.pool)
            .await
        };

        query.map_err(|e| AppError::Database(e.to_string()))
    }

    /// Append stdout log
    pub async fn append_stdout(&self, execution_id: &str, content: &str) -> Result<ExecutionLog, AppError> {
        let log = ExecutionLog::stdout(execution_id, content);
        self.create(&log).await
    }

    /// Append stderr log
    pub async fn append_stderr(&self, execution_id: &str, content: &str) -> Result<ExecutionLog, AppError> {
        let log = ExecutionLog::stderr(execution_id, content);
        self.create(&log).await
    }

    /// Create a log entry with explicit type
    pub async fn create_with_type(
        &self,
        execution_id: &str,
        log_type: crate::models::LogType,
        content: &str,
    ) -> Result<ExecutionLog, AppError> {
        let log = ExecutionLog::new(execution_id, log_type, content);
        self.create(&log).await
    }

    /// Delete logs for an execution
    pub async fn delete_by_execution(&self, execution_id: &str) -> Result<u64, AppError> {
        let result = sqlx::query("DELETE FROM execution_logs WHERE execution_id = ?")
            .bind(execution_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(result.rows_affected())
    }

    /// Get combined stdout for an execution
    pub async fn get_stdout(&self, execution_id: &str) -> Result<String, AppError> {
        let logs = self.get_by_execution(execution_id, Some("stdout")).await?;
        Ok(logs.into_iter().map(|l| l.log_content).collect::<Vec<_>>().join(""))
    }

    /// Get combined stderr for an execution
    pub async fn get_stderr(&self, execution_id: &str) -> Result<String, AppError> {
        let logs = self.get_by_execution(execution_id, Some("stderr")).await?;
        Ok(logs.into_iter().map(|l| l.log_content).collect::<Vec<_>>().join(""))
    }

    /// Get logs with pagination for streaming
    pub async fn get_by_execution_paginated(
        &self,
        execution_id: &str,
        log_type: Option<&str>,
        offset: i32,
        limit: i32,
    ) -> Result<(Vec<ExecutionLog>, i64), AppError> {
        // Get total count
        let count_query = if log_type.is_some() {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM execution_logs WHERE execution_id = ? AND log_type = ?",
            )
            .bind(execution_id)
            .bind(log_type.unwrap())
            .fetch_one(&self.pool)
            .await
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM execution_logs WHERE execution_id = ?",
            )
            .bind(execution_id)
            .fetch_one(&self.pool)
            .await
        };

        let total = count_query.map_err(|e| AppError::Database(e.to_string()))?;

        // Get paginated logs
        let logs = if let Some(lt) = log_type {
            sqlx::query_as::<_, ExecutionLog>(
                r#"
                SELECT id, execution_id, log_type, log_content, created_at
                FROM execution_logs
                WHERE execution_id = ? AND log_type = ?
                ORDER BY created_at ASC
                LIMIT ? OFFSET ?
                "#,
            )
            .bind(execution_id)
            .bind(lt)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, ExecutionLog>(
                r#"
                SELECT id, execution_id, log_type, log_content, created_at
                FROM execution_logs
                WHERE execution_id = ?
                ORDER BY created_at ASC
                LIMIT ? OFFSET ?
                "#,
            )
            .bind(execution_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
        };

        let logs = logs.map_err(|e| AppError::Database(e.to_string()))?;
        Ok((logs, total))
    }

    /// Delete orphaned execution logs whose parent execution no longer exists.
    /// This is called after execution cleanup to remove stale log entries.
    pub async fn delete_orphaned(&self) -> Result<u64, AppError> {
        let result = sqlx::query(
            r#"
            DELETE FROM execution_logs
            WHERE execution_id NOT IN (SELECT id FROM executions)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(result.rows_affected())
    }
}
