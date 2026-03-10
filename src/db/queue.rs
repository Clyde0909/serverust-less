//! Job queue repository

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::QueueEntry;

/// Repository for job queue database operations
#[derive(Clone)]
pub struct QueueRepository {
    pool: SqlitePool,
}

impl QueueRepository {
    /// Create a new QueueRepository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Add an entry to the queue
    pub async fn enqueue(&self, entry: &QueueEntry) -> Result<QueueEntry, AppError> {
        sqlx::query(
            r#"
            INSERT INTO job_queue (id, execution_id, job_id, priority, status, queued_at, started_at, completed_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&entry.id)
        .bind(&entry.execution_id)
        .bind(&entry.job_id)
        .bind(entry.priority)
        .bind(&entry.status)
        .bind(&entry.queued_at)
        .bind(&entry.started_at)
        .bind(&entry.completed_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(entry.clone())
    }

    /// Get next item from queue (highest priority, oldest)
    pub async fn dequeue(&self) -> Result<Option<QueueEntry>, AppError> {
        // Get the next item
        let entry = sqlx::query_as::<_, QueueEntry>(
            r#"
            SELECT id, execution_id, job_id, priority, status, queued_at, started_at, completed_at
            FROM job_queue
            WHERE status = 'queued'
            ORDER BY priority DESC, queued_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(entry)
    }

    /// Get entry by execution ID
    pub async fn get_by_execution(&self, execution_id: &str) -> Result<Option<QueueEntry>, AppError> {
        sqlx::query_as::<_, QueueEntry>(
            r#"
            SELECT id, execution_id, job_id, priority, status, queued_at, started_at, completed_at
            FROM job_queue
            WHERE execution_id = ?
            "#,
        )
        .bind(execution_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Update queue entry
    pub async fn update(&self, entry: &QueueEntry) -> Result<QueueEntry, AppError> {
        sqlx::query(
            r#"
            UPDATE job_queue
            SET status = ?, started_at = ?, completed_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&entry.status)
        .bind(&entry.started_at)
        .bind(&entry.completed_at)
        .bind(&entry.id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(entry.clone())
    }

    /// Remove entry from queue
    pub async fn remove(&self, id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM job_queue WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// Remove by execution ID
    pub async fn remove_by_execution(&self, execution_id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM job_queue WHERE execution_id = ?")
            .bind(execution_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// Count queued items
    pub async fn count_queued(&self) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM job_queue WHERE status = 'queued'")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Count processing items
    pub async fn count_processing(&self) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM job_queue WHERE status = 'processing'")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Get queue depth by priority
    pub async fn get_depth_by_priority(&self) -> Result<Vec<(i32, i64)>, AppError> {
        let rows = sqlx::query_as::<_, (i32, i64)>(
            "SELECT priority, COUNT(*) as count FROM job_queue WHERE status = 'queued' GROUP BY priority ORDER BY priority DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(rows)
    }

    /// Reset items stuck in "processing" state back to "queued" (crash recovery).
    pub async fn reset_processing_to_queued(&self) -> Result<u64, AppError> {
        let result = sqlx::query(
            "UPDATE job_queue SET status = 'queued', started_at = NULL WHERE status = 'processing'",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(result.rows_affected())
    }

    /// Atomically dequeue the highest-priority item: SELECT + UPDATE in a single statement.
    /// Prevents double-dequeue under concurrency.
    pub async fn dequeue_atomic(&self) -> Result<Option<QueueEntry>, AppError> {
        sqlx::query_as::<_, QueueEntry>(
            r#"
            UPDATE job_queue
            SET status = 'processing', started_at = datetime('now')
            WHERE id = (
                SELECT id FROM job_queue
                WHERE status = 'queued'
                ORDER BY priority DESC, queued_at ASC
                LIMIT 1
            )
            RETURNING id, execution_id, job_id, priority, status, queued_at, started_at, completed_at
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Get all queued entries (for recovery)
    pub async fn get_all_queued(&self) -> Result<Vec<QueueEntry>, AppError> {
        sqlx::query_as::<_, QueueEntry>(
            r#"
            SELECT id, execution_id, job_id, priority, status, queued_at, started_at, completed_at
            FROM job_queue
            WHERE status = 'queued'
            ORDER BY priority DESC, queued_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Clean up old completed entries
    pub async fn cleanup_old(&self, hours: i32) -> Result<u64, AppError> {
        let result = sqlx::query(
            r#"
            DELETE FROM job_queue 
            WHERE status IN ('completed', 'failed') 
            AND completed_at < datetime('now', '-' || ? || ' hours')
            "#,
        )
        .bind(hours)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(result.rows_affected())
    }

    /// Count completed in last hour
    pub async fn count_completed_last_hour(&self) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM job_queue WHERE status = 'completed' AND completed_at >= datetime('now', '-1 hour')",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Count failed in last hour
    pub async fn count_failed_last_hour(&self) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM job_queue WHERE status = 'failed' AND completed_at >= datetime('now', '-1 hour')",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Count items in dead letter queue
    pub async fn count_dead_letter(&self) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM job_queue WHERE status = 'dead_letter'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Get all dead letter queue entries
    pub async fn get_dead_letter_entries(&self) -> Result<Vec<QueueEntry>, AppError> {
        sqlx::query_as::<_, QueueEntry>(
            r#"
            SELECT id, execution_id, job_id, priority, status, queued_at, started_at, completed_at
            FROM job_queue
            WHERE status = 'dead_letter'
            ORDER BY completed_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Re-queue a failed item (reset back to queued status for retry)
    pub async fn requeue(&self, id: &str) -> Result<(), AppError> {
        sqlx::query(
            r#"
            UPDATE job_queue
            SET status = 'queued', started_at = NULL, completed_at = NULL
            WHERE id = ?
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}
