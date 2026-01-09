//! Job service - business logic layer for job management

use crate::db::JobRepository;
use crate::error::AppError;
use crate::models::{CreateJobRequest, Job, JobListResponse, ListJobsQuery, UpdateJobRequest};

/// Service for job management business logic
#[derive(Clone)]
pub struct JobService {
    repo: JobRepository,
}

impl JobService {
    /// Create a new JobService
    pub fn new(repo: JobRepository) -> Self {
        Self { repo }
    }

    /// Create a new job
    pub async fn create_job(&self, req: CreateJobRequest) -> Result<Job, AppError> {
        // Validate the request
        req.validate().map_err(AppError::Validation)?;

        // Check for duplicate name
        if self.repo.name_exists(&req.name, None).await? {
            return Err(AppError::Conflict(format!(
                "Job with name '{}' already exists",
                req.name
            )));
        }

        // Create the job entity
        let job = Job::new(req);

        // Persist to database
        self.repo.create(&job).await
    }

    /// Get a job by ID
    pub async fn get_job(&self, id: &str) -> Result<Job, AppError> {
        self.repo.get_by_id(id).await
    }

    /// List jobs with pagination and filters
    pub async fn list_jobs(&self, query: ListJobsQuery) -> Result<JobListResponse, AppError> {
        // Validate pagination params
        let limit = query.limit.clamp(1, 100);
        let offset = query.offset.max(0);

        let validated_query = ListJobsQuery {
            limit,
            offset,
            enabled: query.enabled,
            search: query.search,
        };

        let (jobs, total) = self.repo.list(&validated_query).await?;

        Ok(JobListResponse {
            jobs,
            total,
            limit,
            offset,
        })
    }

    /// Update an existing job
    pub async fn update_job(&self, id: &str, req: UpdateJobRequest) -> Result<Job, AppError> {
        // Fetch existing job
        let mut job = self.repo.get_by_id(id).await?;

        // Check for duplicate name if name is being updated
        if let Some(ref new_name) = req.name {
            if new_name != &job.name && self.repo.name_exists(new_name, Some(id)).await? {
                return Err(AppError::Conflict(format!(
                    "Job with name '{}' already exists",
                    new_name
                )));
            }
        }

        // Validate optional fields
        if let Some(timeout) = req.timeout_seconds {
            if timeout <= 0 {
                return Err(AppError::Validation("Timeout must be positive".to_string()));
            }
            if timeout > 3600 {
                return Err(AppError::Validation(
                    "Timeout cannot exceed 3600 seconds".to_string(),
                ));
            }
        }

        if let Some(memory) = req.memory_limit_mb {
            if memory <= 0 {
                return Err(AppError::Validation(
                    "Memory limit must be positive".to_string(),
                ));
            }
            if memory > 4096 {
                return Err(AppError::Validation(
                    "Memory limit cannot exceed 4096 MB".to_string(),
                ));
            }
        }

        // Apply updates
        job.apply_update(req);

        // Persist changes
        self.repo.update(&job).await
    }

    /// Delete a job
    pub async fn delete_job(&self, id: &str) -> Result<(), AppError> {
        self.repo.delete(id).await
    }

    /// Enable a job
    pub async fn enable_job(&self, id: &str) -> Result<Job, AppError> {
        let update = UpdateJobRequest {
            name: None,
            description: None,
            python_code: None,
            timeout_seconds: None,
            memory_limit_mb: None,
            use_custom_venv: None,
            priority: None,
            max_retries: None,
            enabled: Some(true),
        };
        self.update_job(id, update).await
    }

    /// Disable a job
    pub async fn disable_job(&self, id: &str) -> Result<Job, AppError> {
        let update = UpdateJobRequest {
            name: None,
            description: None,
            python_code: None,
            timeout_seconds: None,
            memory_limit_mb: None,
            use_custom_venv: None,
            priority: None,
            max_retries: None,
            enabled: Some(false),
        };
        self.update_job(id, update).await
    }
}
