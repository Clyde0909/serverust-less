//! Virtual environment repository

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::Venv;

/// Repository for venv database operations
#[derive(Clone)]
pub struct VenvRepository {
    pool: SqlitePool,
}

impl VenvRepository {
    /// Create a new VenvRepository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new venv
    pub async fn create(&self, venv: &Venv) -> Result<Venv, AppError> {
        sqlx::query(
            r#"
            INSERT INTO venvs (
                id, venv_type, job_id, path, python_version, status,
                size_bytes, package_count, error_message, created_at, updated_at, last_used_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&venv.id)
        .bind(&venv.venv_type)
        .bind(&venv.job_id)
        .bind(&venv.path)
        .bind(&venv.python_version)
        .bind(&venv.status)
        .bind(venv.size_bytes)
        .bind(venv.package_count)
        .bind(&venv.error_message)
        .bind(&venv.created_at)
        .bind(&venv.updated_at)
        .bind(&venv.last_used_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.get_by_id(&venv.id).await
    }

    /// Get a venv by ID
    pub async fn get_by_id(&self, id: &str) -> Result<Venv, AppError> {
        sqlx::query_as::<_, Venv>(
            r#"
            SELECT id, venv_type, job_id, path, python_version, status,
                   size_bytes, package_count, error_message, created_at, updated_at, last_used_at
            FROM venvs
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Venv not found: {}", id)))
    }

    /// Get the main venv
    pub async fn get_main(&self) -> Result<Option<Venv>, AppError> {
        sqlx::query_as::<_, Venv>(
            r#"
            SELECT id, venv_type, job_id, path, python_version, status,
                   size_bytes, package_count, error_message, created_at, updated_at, last_used_at
            FROM venvs
            WHERE venv_type = 'main'
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Get venv for a job
    pub async fn get_by_job(&self, job_id: &str) -> Result<Option<Venv>, AppError> {
        sqlx::query_as::<_, Venv>(
            r#"
            SELECT id, venv_type, job_id, path, python_version, status,
                   size_bytes, package_count, error_message, created_at, updated_at, last_used_at
            FROM venvs
            WHERE job_id = ?
            "#,
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// List all venvs
    pub async fn list(&self) -> Result<(Vec<Venv>, i64), AppError> {
        let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM venvs")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        let venvs = sqlx::query_as::<_, Venv>(
            r#"
            SELECT id, venv_type, job_id, path, python_version, status,
                   size_bytes, package_count, error_message, created_at, updated_at, last_used_at
            FROM venvs
            ORDER BY venv_type, created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok((venvs, total))
    }

    /// Update a venv
    pub async fn update(&self, venv: &Venv) -> Result<Venv, AppError> {
        let result = sqlx::query(
            r#"
            UPDATE venvs
            SET status = ?, size_bytes = ?, package_count = ?, error_message = ?,
                updated_at = ?, last_used_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&venv.status)
        .bind(venv.size_bytes)
        .bind(venv.package_count)
        .bind(&venv.error_message)
        .bind(&venv.updated_at)
        .bind(&venv.last_used_at)
        .bind(&venv.id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Venv not found: {}", venv.id)));
        }

        self.get_by_id(&venv.id).await
    }

    /// Delete a venv
    pub async fn delete(&self, id: &str) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM venvs WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Venv not found: {}", id)));
        }

        Ok(())
    }

    /// Count custom venvs
    pub async fn count_custom(&self) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM venvs WHERE venv_type = 'custom'")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Get least recently used custom venvs
    pub async fn get_lru_custom(&self, limit: i32) -> Result<Vec<Venv>, AppError> {
        sqlx::query_as::<_, Venv>(
            r#"
            SELECT id, venv_type, job_id, path, python_version, status,
                   size_bytes, package_count, error_message, created_at, updated_at, last_used_at
            FROM venvs
            WHERE venv_type = 'custom' AND status = 'ready'
            ORDER BY last_used_at ASC NULLS FIRST, created_at ASC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }
}
