//! Package and dependency repository

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{JobDependency, PackageCache, PythonPackage};

/// Repository for package database operations
#[derive(Clone)]
pub struct PackageRepository {
    pool: SqlitePool,
}

impl PackageRepository {
    /// Create a new PackageRepository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // ============ Python Packages ============

    /// Create or update a package
    pub async fn upsert_package(&self, package: &PythonPackage) -> Result<PythonPackage, AppError> {
        sqlx::query(
            r#"
            INSERT INTO python_packages (id, name, version, description, pypi_url, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(name, version) DO UPDATE SET
                description = excluded.description,
                pypi_url = excluded.pypi_url
            "#,
        )
        .bind(&package.id)
        .bind(&package.name)
        .bind(&package.version)
        .bind(&package.description)
        .bind(&package.pypi_url)
        .bind(&package.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(package.clone())
    }

    /// Get a package by name and version
    pub async fn get_package(&self, name: &str, version: &str) -> Result<Option<PythonPackage>, AppError> {
        sqlx::query_as::<_, PythonPackage>(
            "SELECT id, name, version, description, pypi_url, created_at FROM python_packages WHERE name = ? AND version = ?",
        )
        .bind(name)
        .bind(version)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// List all packages
    pub async fn list_packages(&self) -> Result<Vec<PythonPackage>, AppError> {
        sqlx::query_as::<_, PythonPackage>(
            "SELECT id, name, version, description, pypi_url, created_at FROM python_packages ORDER BY name, version",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    // ============ Job Dependencies ============

    /// Add a dependency to a job
    pub async fn add_dependency(&self, dep: &JobDependency) -> Result<JobDependency, AppError> {
        sqlx::query(
            r#"
            INSERT INTO job_dependencies (id, job_id, package_name, version_constraint, created_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(job_id, package_name) DO UPDATE SET
                version_constraint = excluded.version_constraint
            "#,
        )
        .bind(&dep.id)
        .bind(&dep.job_id)
        .bind(&dep.package_name)
        .bind(&dep.version_constraint)
        .bind(&dep.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(dep.clone())
    }

    /// Get dependencies for a job
    pub async fn get_dependencies(&self, job_id: &str) -> Result<Vec<JobDependency>, AppError> {
        sqlx::query_as::<_, JobDependency>(
            "SELECT id, job_id, package_name, version_constraint, created_at FROM job_dependencies WHERE job_id = ? ORDER BY package_name",
        )
        .bind(job_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Delete a dependency
    pub async fn delete_dependency(&self, job_id: &str, package_name: &str) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM job_dependencies WHERE job_id = ? AND package_name = ?")
            .bind(job_id)
            .bind(package_name)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "Dependency {} not found for job {}",
                package_name, job_id
            )));
        }

        Ok(())
    }

    /// Delete all dependencies for a job
    pub async fn delete_job_dependencies(&self, job_id: &str) -> Result<u64, AppError> {
        let result = sqlx::query("DELETE FROM job_dependencies WHERE job_id = ?")
            .bind(job_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(result.rows_affected())
    }

    // ============ Package Cache ============

    /// Create or update a cache entry
    /// Handles NULL venv_id separately since SQLite treats NULLs as distinct in UNIQUE constraints
    pub async fn upsert_cache(&self, cache: &PackageCache) -> Result<PackageCache, AppError> {
        if cache.venv_id.is_none() {
            // For NULL venv_id (main venv), ON CONFLICT won't trigger because
            // SQLite considers each NULL distinct. Use update-or-insert pattern.
            let updated = sqlx::query(
                r#"
                UPDATE package_cache SET
                    status = ?, error_message = ?, size_bytes = ?,
                    last_used_at = ?, use_count = ?, installation_path = ?
                WHERE package_name = ? AND version = ? AND venv_type = ? AND venv_id IS NULL
                "#,
            )
            .bind(&cache.status)
            .bind(&cache.error_message)
            .bind(cache.size_bytes)
            .bind(&cache.last_used_at)
            .bind(cache.use_count)
            .bind(&cache.installation_path)
            .bind(&cache.package_name)
            .bind(&cache.version)
            .bind(&cache.venv_type)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

            if updated.rows_affected() == 0 {
                // No existing row — insert new
                sqlx::query(
                    r#"
                    INSERT INTO package_cache (
                        id, venv_type, venv_id, package_name, version, installation_path,
                        size_bytes, status, error_message, installed_at, last_used_at, use_count
                    )
                    VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    "#,
                )
                .bind(&cache.id)
                .bind(&cache.venv_type)
                .bind(&cache.package_name)
                .bind(&cache.version)
                .bind(&cache.installation_path)
                .bind(cache.size_bytes)
                .bind(&cache.status)
                .bind(&cache.error_message)
                .bind(&cache.installed_at)
                .bind(&cache.last_used_at)
                .bind(cache.use_count)
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::Database(e.to_string()))?;
            }
        } else {
            // Non-NULL venv_id: ON CONFLICT works correctly
            sqlx::query(
                r#"
                INSERT INTO package_cache (
                    id, venv_type, venv_id, package_name, version, installation_path,
                    size_bytes, status, error_message, installed_at, last_used_at, use_count
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(package_name, version, venv_type, venv_id) DO UPDATE SET
                    status = excluded.status,
                    error_message = excluded.error_message,
                    size_bytes = excluded.size_bytes,
                    last_used_at = excluded.last_used_at,
                    use_count = excluded.use_count
                "#,
            )
            .bind(&cache.id)
            .bind(&cache.venv_type)
            .bind(&cache.venv_id)
            .bind(&cache.package_name)
            .bind(&cache.version)
            .bind(&cache.installation_path)
            .bind(cache.size_bytes)
            .bind(&cache.status)
            .bind(&cache.error_message)
            .bind(&cache.installed_at)
            .bind(&cache.last_used_at)
            .bind(cache.use_count)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Ok(cache.clone())
    }

    /// Get cached packages for a venv
    pub async fn get_cache_by_venv(&self, venv_type: &str, venv_id: Option<&str>) -> Result<Vec<PackageCache>, AppError> {
        let query = if let Some(id) = venv_id {
            sqlx::query_as::<_, PackageCache>(
                r#"
                SELECT id, venv_type, venv_id, package_name, version, installation_path,
                       size_bytes, status, error_message, installed_at, last_used_at, use_count
                FROM package_cache
                WHERE venv_type = ? AND venv_id = ?
                ORDER BY package_name
                "#,
            )
            .bind(venv_type)
            .bind(id)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, PackageCache>(
                r#"
                SELECT id, venv_type, venv_id, package_name, version, installation_path,
                       size_bytes, status, error_message, installed_at, last_used_at, use_count
                FROM package_cache
                WHERE venv_type = ? AND venv_id IS NULL
                ORDER BY package_name
                "#,
            )
            .bind(venv_type)
            .fetch_all(&self.pool)
            .await
        };

        query.map_err(|e| AppError::Database(e.to_string()))
    }

    /// Get a specific cached package
    pub async fn get_cached_package(
        &self,
        venv_type: &str,
        venv_id: Option<&str>,
        package_name: &str,
    ) -> Result<Option<PackageCache>, AppError> {
        let query = if let Some(id) = venv_id {
            sqlx::query_as::<_, PackageCache>(
                r#"
                SELECT id, venv_type, venv_id, package_name, version, installation_path,
                       size_bytes, status, error_message, installed_at, last_used_at, use_count
                FROM package_cache
                WHERE venv_type = ? AND venv_id = ? AND package_name = ?
                "#,
            )
            .bind(venv_type)
            .bind(id)
            .bind(package_name)
            .fetch_optional(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, PackageCache>(
                r#"
                SELECT id, venv_type, venv_id, package_name, version, installation_path,
                       size_bytes, status, error_message, installed_at, last_used_at, use_count
                FROM package_cache
                WHERE venv_type = ? AND venv_id IS NULL AND package_name = ?
                "#,
            )
            .bind(venv_type)
            .bind(package_name)
            .fetch_optional(&self.pool)
            .await
        };

        query.map_err(|e| AppError::Database(e.to_string()))
    }

    /// Delete a cached package
    pub async fn delete_cache(&self, venv_type: &str, venv_id: Option<&str>, package_name: &str, version: &str) -> Result<(), AppError> {
        let result = if let Some(id) = venv_id {
            sqlx::query("DELETE FROM package_cache WHERE venv_type = ? AND venv_id = ? AND package_name = ? AND version = ?")
                .bind(venv_type)
                .bind(id)
                .bind(package_name)
                .bind(version)
                .execute(&self.pool)
                .await
        } else {
            sqlx::query("DELETE FROM package_cache WHERE venv_type = ? AND venv_id IS NULL AND package_name = ? AND version = ?")
                .bind(venv_type)
                .bind(package_name)
                .bind(version)
                .execute(&self.pool)
                .await
        };

        result.map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// List all cached packages
    pub async fn list_all_cached(&self) -> Result<(Vec<PackageCache>, i64), AppError> {
        let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM package_cache WHERE status = 'ready'")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        let packages = sqlx::query_as::<_, PackageCache>(
            r#"
            SELECT id, venv_type, venv_id, package_name, version, installation_path,
                   size_bytes, status, error_message, installed_at, last_used_at, use_count
            FROM package_cache
            WHERE status = 'ready'
            ORDER BY package_name, version
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok((packages, total))
    }
}
