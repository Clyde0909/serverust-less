//! Venv service - business logic for virtual environment management

use crate::db::VenvRepository;
use crate::error::AppError;
use crate::models::{Venv, VenvListResponse, VenvStatus};

/// Service for virtual environment management
#[derive(Clone)]
pub struct VenvService {
    repo: VenvRepository,
}

impl VenvService {
    /// Create a new VenvService
    pub fn new(repo: VenvRepository) -> Self {
        Self { repo }
    }

    /// Get the main venv
    pub async fn get_main_venv(&self) -> Result<Option<Venv>, AppError> {
        self.repo.get_main().await
    }

    /// Get or create main venv record
    pub async fn ensure_main_venv(&self, path: &str, python_version: Option<String>) -> Result<Venv, AppError> {
        if let Some(venv) = self.repo.get_main().await? {
            return Ok(venv);
        }

        let venv = Venv::new_main(path, python_version);
        self.repo.create(&venv).await
    }

    /// Get venv by ID
    pub async fn get_venv(&self, id: &str) -> Result<Venv, AppError> {
        self.repo.get_by_id(id).await
    }

    /// Get venv for a job
    pub async fn get_job_venv(&self, job_id: &str) -> Result<Option<Venv>, AppError> {
        self.repo.get_by_job(job_id).await
    }

    /// Create a custom venv for a job
    pub async fn create_custom_venv(
        &self,
        job_id: &str,
        path: &str,
        python_version: Option<String>,
    ) -> Result<Venv, AppError> {
        // Check if venv already exists
        if let Some(existing) = self.repo.get_by_job(job_id).await? {
            return Ok(existing);
        }

        let venv = Venv::new_custom(job_id, path, python_version);
        self.repo.create(&venv).await
    }

    /// List all venvs
    pub async fn list_venvs(&self) -> Result<VenvListResponse, AppError> {
        let (venvs, total) = self.repo.list().await?;
        Ok(VenvListResponse { venvs, total })
    }

    /// Update a venv
    pub async fn update_venv(&self, venv: &Venv) -> Result<Venv, AppError> {
        self.repo.update(venv).await
    }

    /// Mark venv as ready
    pub async fn mark_ready(&self, id: &str) -> Result<Venv, AppError> {
        let mut venv = self.repo.get_by_id(id).await?;
        venv.mark_ready();
        self.repo.update(&venv).await
    }

    /// Mark venv as failed
    pub async fn mark_failed(&self, id: &str, error: &str) -> Result<Venv, AppError> {
        let mut venv = self.repo.get_by_id(id).await?;
        venv.mark_failed(error);
        self.repo.update(&venv).await
    }

    /// Delete a venv
    pub async fn delete_venv(&self, id: &str) -> Result<(), AppError> {
        let venv = self.repo.get_by_id(id).await?;

        // Don't allow deleting main venv through this method
        if venv.venv_type == "main" {
            return Err(AppError::BadRequest(
                "Cannot delete main venv through this endpoint".to_string(),
            ));
        }

        self.repo.delete(id).await
    }

    /// Count custom venvs
    pub async fn count_custom_venvs(&self) -> Result<i64, AppError> {
        self.repo.count_custom().await
    }

    /// Get least recently used custom venvs for cleanup
    pub async fn get_lru_venvs(&self, limit: i32) -> Result<Vec<Venv>, AppError> {
        self.repo.get_lru_custom(limit).await
    }

    /// Record venv usage
    pub async fn record_usage(&self, id: &str) -> Result<(), AppError> {
        let mut venv = self.repo.get_by_id(id).await?;
        venv.record_usage();
        self.repo.update(&venv).await?;
        Ok(())
    }

    /// Check if venv is ready to use
    pub async fn is_ready(&self, id: &str) -> Result<bool, AppError> {
        let venv = self.repo.get_by_id(id).await?;
        Ok(venv.status == VenvStatus::Ready.as_str())
    }
}
