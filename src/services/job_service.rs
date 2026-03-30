//! Job service - business logic layer for job management

use crate::db::JobRepository;
use crate::error::AppError;
use crate::models::{CreateJobRequest, Job, JobListResponse, ListJobsQuery, UpdateJobRequest};
use tracing::{debug, info, instrument, warn};
use validator::Validate;

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
    #[instrument(skip(self, req), fields(job_name = %req.name))]
    pub async fn create_job(&self, req: CreateJobRequest) -> Result<Job, AppError> {
        debug!("Creating new job");
        
        // Validate the request
        Validate::validate(&req).map_err(|e| {
            warn!(error = %format_validation_errors(&e), "Job validation failed");
            AppError::Validation(format_validation_errors(&e))
        })?;

        // Check for duplicate name
        if self.repo.name_exists(&req.name, None).await? {
            warn!(name = %req.name, "Duplicate job name");
            return Err(AppError::Conflict(format!(
                "Job with name '{}' already exists",
                req.name
            )));
        }

        // Create the job entity
        let job = Job::new(req);
        info!(job_id = %job.id, job_name = %job.name, "Job created");

        // Persist to database
        self.repo.create(&job).await
    }

    /// Get a job by ID
    #[instrument(skip(self))]
    pub async fn get_job(&self, id: &str) -> Result<Job, AppError> {
        debug!("Fetching job");
        self.repo.get_by_id(id).await
    }

    /// List jobs with pagination and filters
    #[instrument(skip(self))]
    pub async fn list_jobs(&self, query: ListJobsQuery) -> Result<JobListResponse, AppError> {
        debug!(limit = query.limit, offset = query.offset, "Listing jobs");
        
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
    #[instrument(skip(self, req))]
    pub async fn update_job(&self, id: &str, req: UpdateJobRequest) -> Result<Job, AppError> {
        debug!("Updating job");
        
        // Validate the request using validator crate
        Validate::validate(&req).map_err(|e| {
            warn!(error = %format_validation_errors(&e), "Job update validation failed");
            AppError::Validation(format_validation_errors(&e))
        })?;

        // Fetch existing job
        let mut job = self.repo.get_by_id(id).await?;

        // Check for duplicate name if name is being updated
        if let Some(ref new_name) = req.name {
            if new_name != &job.name && self.repo.name_exists(new_name, Some(id)).await? {
                warn!(name = %new_name, "Duplicate job name on update");
                return Err(AppError::Conflict(format!(
                    "Job with name '{}' already exists",
                    new_name
                )));
            }
        }

        // Apply updates
        job.apply_update(req);
        info!(job_id = %id, "Job updated");

        // Persist changes
        self.repo.update(&job).await
    }

    /// Delete a job
    #[instrument(skip(self))]
    pub async fn delete_job(&self, id: &str) -> Result<(), AppError> {
        info!("Deleting job");
        self.repo.delete(id).await
    }

    /// Enable a job
    #[instrument(skip(self))]
    pub async fn enable_job(&self, id: &str) -> Result<Job, AppError> {
        info!("Enabling job");
        let update = UpdateJobRequest {
            name: None,
            description: None,
            python_code: None,
            timeout_seconds: None,
            memory_limit_mb: None,
            use_custom_venv: None,
            venv_id: None,
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
            venv_id: None,
            priority: None,
            max_retries: None,
            enabled: Some(false),
        };
        self.update_job(id, update).await
    }

    /// Bulk create jobs
    pub async fn bulk_create_jobs(&self, requests: Vec<CreateJobRequest>) -> Result<(Vec<Job>, Vec<String>), AppError> {
        let mut jobs = Vec::new();
        let mut errors = Vec::new();

        for req in requests {
            match self.create_job(req).await {
                Ok(job) => jobs.push(job),
                Err(e) => errors.push(e.to_string()),
            }
        }

        Ok((jobs, errors))
    }

    /// Bulk delete jobs
    pub async fn bulk_delete_jobs(&self, ids: Vec<String>) -> Result<u64, AppError> {
        self.repo.delete_bulk(&ids).await
    }

    /// Clone a job with a new name
    pub async fn clone_job(&self, id: &str, new_name: Option<String>) -> Result<Job, AppError> {
        let source = self.repo.get_by_id(id).await?;

        let cloned_name = new_name.unwrap_or_else(|| format!("{} (copy)", source.name));

        // Check for duplicate name
        if self.repo.name_exists(&cloned_name, None).await? {
            return Err(AppError::Conflict(format!(
                "Job with name '{}' already exists",
                cloned_name
            )));
        }

        let req = CreateJobRequest {
            name: cloned_name,
            description: source.description,
            python_code: source.python_code,
            timeout_seconds: source.timeout_seconds,
            memory_limit_mb: source.memory_limit_mb,
            use_custom_venv: source.use_custom_venv,
            venv_id: source.venv_id,
            priority: source.priority,
            max_retries: source.max_retries,
        };

        let job = Job::new(req);
        self.repo.create(&job).await
    }

    /// Count total jobs (efficient)
    pub async fn count_all(&self) -> Result<i64, AppError> {
        self.repo.count_all().await
    }

    /// Count enabled jobs (efficient)
    pub async fn count_enabled(&self) -> Result<i64, AppError> {
        self.repo.count_enabled().await
    }
}

/// Format validation errors into a readable string
fn format_validation_errors(errors: &validator::ValidationErrors) -> String {
    let mut messages = Vec::new();

    for (field, field_errors) in errors.field_errors() {
        for error in field_errors {
            if let Some(message) = &error.message {
                messages.push(format!("{}: {}", field, message));
            } else {
                messages.push(format!("{}: invalid value", field));
            }
        }
    }

    if messages.is_empty() {
        "Validation failed".to_string()
    } else {
        messages.join("; ")
    }
}
