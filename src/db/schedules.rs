//! Schedule repository

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::JobSchedule;

/// Repository for schedule database operations
#[derive(Clone)]
pub struct ScheduleRepository {
    pool: SqlitePool,
}

impl ScheduleRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new schedule
    pub async fn create(&self, schedule: &JobSchedule) -> Result<JobSchedule, AppError> {
        sqlx::query(
            r#"
            INSERT INTO job_schedules (id, job_id, cron_expression, next_run_at, last_run_at, enabled, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&schedule.id)
        .bind(&schedule.job_id)
        .bind(&schedule.cron_expression)
        .bind(&schedule.next_run_at)
        .bind(&schedule.last_run_at)
        .bind(schedule.enabled)
        .bind(&schedule.created_at)
        .bind(&schedule.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.get_by_id(&schedule.id).await
    }

    /// Get a schedule by ID
    pub async fn get_by_id(&self, id: &str) -> Result<JobSchedule, AppError> {
        sqlx::query_as::<_, JobSchedule>(
            "SELECT id, job_id, cron_expression, next_run_at, last_run_at, enabled, created_at, updated_at FROM job_schedules WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Schedule not found: {}", id)))
    }

    /// Get schedule by job ID
    pub async fn get_by_job_id(&self, job_id: &str) -> Result<Option<JobSchedule>, AppError> {
        sqlx::query_as::<_, JobSchedule>(
            "SELECT id, job_id, cron_expression, next_run_at, last_run_at, enabled, created_at, updated_at FROM job_schedules WHERE job_id = ?",
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Update a schedule
    pub async fn update(&self, schedule: &JobSchedule) -> Result<JobSchedule, AppError> {
        sqlx::query(
            r#"
            UPDATE job_schedules
            SET cron_expression = ?, next_run_at = ?, last_run_at = ?, enabled = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&schedule.cron_expression)
        .bind(&schedule.next_run_at)
        .bind(&schedule.last_run_at)
        .bind(schedule.enabled)
        .bind(&schedule.updated_at)
        .bind(&schedule.id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.get_by_id(&schedule.id).await
    }

    /// Delete a schedule
    pub async fn delete(&self, id: &str) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM job_schedules WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Schedule not found: {}", id)));
        }
        Ok(())
    }

    /// Delete a schedule by job ID
    pub async fn delete_by_job_id(&self, job_id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM job_schedules WHERE job_id = ?")
            .bind(job_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// List all schedules
    pub async fn list_all(&self) -> Result<(Vec<JobSchedule>, i64), AppError> {
        let schedules = sqlx::query_as::<_, JobSchedule>(
            "SELECT id, job_id, cron_expression, next_run_at, last_run_at, enabled, created_at, updated_at FROM job_schedules ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        let total = schedules.len() as i64;
        Ok((schedules, total))
    }

    /// Get all due schedules (next_run_at <= now AND enabled)
    pub async fn get_due_schedules(&self) -> Result<Vec<JobSchedule>, AppError> {
        sqlx::query_as::<_, JobSchedule>(
            r#"
            SELECT id, job_id, cron_expression, next_run_at, last_run_at, enabled, created_at, updated_at
            FROM job_schedules
            WHERE enabled = 1
              AND next_run_at IS NOT NULL
              AND next_run_at <= datetime('now')
            ORDER BY next_run_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Atomically update last_run_at and next_run_at after triggering
    pub async fn mark_triggered(
        &self,
        id: &str,
        last_run_at: &str,
        next_run_at: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            r#"
            UPDATE job_schedules
            SET last_run_at = ?, next_run_at = ?, updated_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(last_run_at)
        .bind(next_run_at)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}
