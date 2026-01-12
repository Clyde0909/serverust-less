//! Job repository for database operations

use sqlx::SqlitePool;
use tracing::{debug, instrument, trace};

use crate::error::AppError;
use crate::models::{Job, ListJobsQuery};

/// Repository for job database operations
#[derive(Clone)]
pub struct JobRepository {
    pool: SqlitePool,
}

impl JobRepository {
    /// Create a new JobRepository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new job
    #[instrument(skip(self, job), fields(job_id = %job.id, job_name = %job.name))]
    pub async fn create(&self, job: &Job) -> Result<Job, AppError> {
        trace!("Inserting job into database");
        sqlx::query(
            r#"
            INSERT INTO jobs (
                id, name, description, python_code, timeout_seconds, memory_limit_mb,
                use_custom_venv, priority, max_retries, created_at, updated_at, enabled
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&job.id)
        .bind(&job.name)
        .bind(&job.description)
        .bind(&job.python_code)
        .bind(job.timeout_seconds)
        .bind(job.memory_limit_mb)
        .bind(job.use_custom_venv)
        .bind(job.priority)
        .bind(job.max_retries)
        .bind(&job.created_at)
        .bind(&job.updated_at)
        .bind(job.enabled)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        debug!("Job inserted successfully");
        self.get_by_id(&job.id).await
    }

    /// Get a job by ID
    #[instrument(skip(self))]
    pub async fn get_by_id(&self, id: &str) -> Result<Job, AppError> {
        trace!("Fetching job by ID");
        sqlx::query_as::<_, Job>(
            r#"
            SELECT id, name, description, python_code, timeout_seconds, memory_limit_mb,
                   use_custom_venv, priority, max_retries, created_at, updated_at, enabled
            FROM jobs
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Job not found: {}", id)))
    }

    /// Get a job by name
    #[instrument(skip(self))]
    pub async fn get_by_name(&self, name: &str) -> Result<Option<Job>, AppError> {
        trace!("Fetching job by name");
        sqlx::query_as::<_, Job>(
            r#"
            SELECT id, name, description, python_code, timeout_seconds, memory_limit_mb,
                   use_custom_venv, priority, max_retries, created_at, updated_at, enabled
            FROM jobs
            WHERE name = ?
            "#,
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// List jobs with pagination and filters
    pub async fn list(&self, query: &ListJobsQuery) -> Result<(Vec<Job>, i64), AppError> {
        let mut sql_query = String::from(
            r#"
            SELECT id, name, description, python_code, timeout_seconds, memory_limit_mb,
                   use_custom_venv, priority, max_retries, created_at, updated_at, enabled
            FROM jobs
            WHERE 1=1
            "#,
        );

        let mut count_query = String::from("SELECT COUNT(*) as count FROM jobs WHERE 1=1");

        // Build dynamic WHERE clauses
        if query.enabled.is_some() {
            sql_query.push_str(" AND enabled = ?");
            count_query.push_str(" AND enabled = ?");
        }

        if query.search.is_some() {
            sql_query.push_str(" AND name LIKE ?");
            count_query.push_str(" AND name LIKE ?");
        }

        sql_query.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");

        // Build and execute the count query
        let mut count_qb = sqlx::query_scalar::<_, i64>(&count_query);
        if let Some(enabled) = query.enabled {
            count_qb = count_qb.bind(enabled);
        }
        if let Some(ref search) = query.search {
            count_qb = count_qb.bind(format!("%{}%", search));
        }

        let total = count_qb
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        // Build and execute the main query
        let mut main_qb = sqlx::query_as::<_, Job>(&sql_query);
        if let Some(enabled) = query.enabled {
            main_qb = main_qb.bind(enabled);
        }
        if let Some(ref search) = query.search {
            main_qb = main_qb.bind(format!("%{}%", search));
        }
        main_qb = main_qb.bind(query.limit).bind(query.offset);

        let jobs = main_qb
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok((jobs, total))
    }

    /// Update a job
    pub async fn update(&self, job: &Job) -> Result<Job, AppError> {
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET name = ?, description = ?, python_code = ?, timeout_seconds = ?,
                memory_limit_mb = ?, use_custom_venv = ?, priority = ?, max_retries = ?,
                updated_at = ?, enabled = ?
            WHERE id = ?
            "#,
        )
        .bind(&job.name)
        .bind(&job.description)
        .bind(&job.python_code)
        .bind(job.timeout_seconds)
        .bind(job.memory_limit_mb)
        .bind(job.use_custom_venv)
        .bind(job.priority)
        .bind(job.max_retries)
        .bind(&job.updated_at)
        .bind(job.enabled)
        .bind(&job.id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Job not found: {}", job.id)));
        }

        self.get_by_id(&job.id).await
    }

    /// Delete a job by ID
    pub async fn delete(&self, id: &str) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM jobs WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Job not found: {}", id)));
        }

        Ok(())
    }

    /// Check if a job name already exists (excluding a specific ID)
    pub async fn name_exists(&self, name: &str, exclude_id: Option<&str>) -> Result<bool, AppError> {
        let query = match exclude_id {
            Some(id) => {
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM jobs WHERE name = ? AND id != ?",
                )
                .bind(name)
                .bind(id)
            }
            None => {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM jobs WHERE name = ?")
                    .bind(name)
            }
        };

        let count = query
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(count > 0)
    }
}
