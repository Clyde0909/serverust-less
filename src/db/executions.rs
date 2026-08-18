//! Execution repository for database operations

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{Execution, ListExecutionsQuery};

/// Repository for execution database operations
#[derive(Clone)]
pub struct ExecutionRepository {
    pool: SqlitePool,
}

impl ExecutionRepository {
    /// Create a new ExecutionRepository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new execution
    pub async fn create(&self, execution: &Execution) -> Result<Execution, AppError> {
        sqlx::query(
            r#"
            INSERT INTO executions (
                id, job_id, job_version, status, input_data, output_data, error_message,
                retry_count, worker_id, started_at, completed_at, duration_ms, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&execution.id)
        .bind(&execution.job_id)
        .bind(execution.job_version)
        .bind(&execution.status)
        .bind(&execution.input_data)
        .bind(&execution.output_data)
        .bind(&execution.error_message)
        .bind(execution.retry_count)
        .bind(&execution.worker_id)
        .bind(&execution.started_at)
        .bind(&execution.completed_at)
        .bind(execution.duration_ms)
        .bind(&execution.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.get_by_id(&execution.id).await
    }

    /// Get an execution by ID
    pub async fn get_by_id(&self, id: &str) -> Result<Execution, AppError> {
        sqlx::query_as::<_, Execution>(
            r#"
                 SELECT id, job_id, job_version, status, input_data, output_data, error_message,
                   retry_count, worker_id, started_at, completed_at, duration_ms, created_at
            FROM executions
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Execution not found: {}", id)))
    }

    /// List executions with pagination and filters
    pub async fn list(&self, query: &ListExecutionsQuery) -> Result<(Vec<Execution>, i64), AppError> {
        let mut sql_query = String::from(
            r#"
                 SELECT id, job_id, job_version, status, input_data, output_data, error_message,
                   retry_count, worker_id, started_at, completed_at, duration_ms, created_at
            FROM executions
            WHERE 1=1
            "#,
        );

        let mut count_query = String::from("SELECT COUNT(*) as count FROM executions WHERE 1=1");

        // Build dynamic WHERE clauses
        if query.status.is_some() {
            sql_query.push_str(" AND status = ?");
            count_query.push_str(" AND status = ?");
        }

        if query.job_id.is_some() {
            sql_query.push_str(" AND job_id = ?");
            count_query.push_str(" AND job_id = ?");
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

        // Build and execute the count query
        let mut count_qb = sqlx::query_scalar::<_, i64>(&count_query);
        if let Some(ref status) = query.status {
            count_qb = count_qb.bind(status);
        }
        if let Some(ref job_id) = query.job_id {
            count_qb = count_qb.bind(job_id);
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

        // Build and execute the main query
        let mut main_qb = sqlx::query_as::<_, Execution>(&sql_query);
        if let Some(ref status) = query.status {
            main_qb = main_qb.bind(status);
        }
        if let Some(ref job_id) = query.job_id {
            main_qb = main_qb.bind(job_id);
        }
        if let Some(ref from) = query.from {
            main_qb = main_qb.bind(from);
        }
        if let Some(ref to) = query.to {
            main_qb = main_qb.bind(to);
        }
        main_qb = main_qb.bind(query.limit).bind(query.offset);

        let executions = main_qb
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok((executions, total))
    }

    /// List executions for a specific job
    pub async fn list_by_job(&self, job_id: &str, limit: i32, offset: i32) -> Result<(Vec<Execution>, i64), AppError> {
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM executions WHERE job_id = ?"
        )
        .bind(job_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        let executions = sqlx::query_as::<_, Execution>(
            r#"
                 SELECT id, job_id, job_version, status, input_data, output_data, error_message,
                   retry_count, worker_id, started_at, completed_at, duration_ms, created_at
            FROM executions
            WHERE job_id = ?
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(job_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok((executions, total))
    }

    /// Update an execution
    pub async fn update(&self, execution: &Execution) -> Result<Execution, AppError> {
        let result = sqlx::query(
            r#"
            UPDATE executions
            SET status = ?, output_data = ?, error_message = ?, retry_count = ?,
                worker_id = ?, started_at = ?, completed_at = ?, duration_ms = ?
            WHERE id = ?
            "#,
        )
        .bind(&execution.status)
        .bind(&execution.output_data)
        .bind(&execution.error_message)
        .bind(execution.retry_count)
        .bind(&execution.worker_id)
        .bind(&execution.started_at)
        .bind(&execution.completed_at)
        .bind(execution.duration_ms)
        .bind(&execution.id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Execution not found: {}", execution.id)));
        }

        self.get_by_id(&execution.id).await
    }

    /// Delete an execution by ID
    pub async fn delete(&self, id: &str) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM executions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Execution not found: {}", id)));
        }

        Ok(())
    }

    /// Delete multiple executions
    pub async fn delete_bulk(&self, ids: &[String]) -> Result<u64, AppError> {
        if ids.is_empty() {
            return Ok(0);
        }

        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "DELETE FROM executions WHERE id IN ({})",
            placeholders.join(", ")
        );

        let mut qb = sqlx::query(&query);
        for id in ids {
            qb = qb.bind(id);
        }

        let result = qb
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(result.rows_affected())
    }

    /// Get running executions
    pub async fn get_running(&self) -> Result<Vec<Execution>, AppError> {
        sqlx::query_as::<_, Execution>(
            r#"
                 SELECT id, job_id, job_version, status, input_data, output_data, error_message,
                   retry_count, worker_id, started_at, completed_at, duration_ms, created_at
            FROM executions
            WHERE status = 'running'
            ORDER BY started_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Get pending executions that can be retried.
    /// Uses per-job `max_retries` from the `jobs` table (parameter removed — was unused).
    pub async fn get_retriable(&self) -> Result<Vec<Execution>, AppError> {
        sqlx::query_as::<_, Execution>(
            r#"
                 SELECT e.id, e.job_id, e.job_version, e.status, e.input_data, e.output_data, e.error_message,
                   e.retry_count, e.worker_id, e.started_at, e.completed_at, e.duration_ms, e.created_at
            FROM executions e
            JOIN jobs j ON e.job_id = j.id
            WHERE e.status = 'failed' AND e.retry_count < j.max_retries
            ORDER BY e.created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Get next pending execution (FIFO order)
    pub async fn get_pending(&self) -> Result<Option<Execution>, AppError> {
        sqlx::query_as::<_, Execution>(
            r#"
                 SELECT id, job_id, job_version, status, input_data, output_data, error_message,
                   retry_count, worker_id, started_at, completed_at, duration_ms, created_at
            FROM executions
            WHERE status = 'pending'
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Update execution status with optional error message.
    /// Only sets `completed_at` and `duration_ms` for terminal statuses.
    pub async fn update_status(
        &self,
        id: &str,
        status: crate::models::ExecutionStatus,
        error_message: Option<String>,
    ) -> Result<(), AppError> {
        if status.is_terminal() {
            // For terminal statuses, compute duration from started_at and set completed_at
            let now = chrono::Utc::now();
            // Fetch started_at to compute duration_ms
            let execution = self.get_by_id(id).await.ok();
            let duration_ms: Option<i64> = execution.and_then(|e| {
                e.started_at.and_then(|started| {
                    chrono::DateTime::parse_from_rfc3339(&started)
                        .ok()
                        .map(|start| (now - start.with_timezone(&chrono::Utc)).num_milliseconds())
                })
            });

            sqlx::query(
                r#"
                UPDATE executions
                SET status = ?, error_message = ?, completed_at = ?, duration_ms = ?
                WHERE id = ?
                "#,
            )
            .bind(status.as_str())
            .bind(&error_message)
            .bind(now)
            .bind(duration_ms)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;
        } else {
            // Non-terminal statuses: only update status and error_message
            sqlx::query(
                r#"
                UPDATE executions
                SET status = ?, error_message = ?
                WHERE id = ?
                "#,
            )
            .bind(status.as_str())
            .bind(&error_message)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Ok(())
    }

    /// Delete executions older than the specified number of days.
    /// Only deletes terminal executions (completed, failed, timeout, cancelled).
    /// Returns the number of deleted rows.
    pub async fn delete_older_than_days(&self, days: u32) -> Result<u64, AppError> {
        let result = sqlx::query(
            r#"
            DELETE FROM executions
            WHERE status IN ('success', 'failed', 'timeout', 'cancelled')
            AND completed_at IS NOT NULL
            AND completed_at < datetime('now', '-' || ? || ' days')
            "#,
        )
        .bind(days)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(result.rows_affected())
    }

    /// Count total executions (efficient single-value query)
    pub async fn count_all(&self) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM executions")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Count currently running executions
    pub async fn count_running(&self) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM executions WHERE status = 'running'")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }
}
