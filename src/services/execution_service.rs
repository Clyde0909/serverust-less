//! Execution service - business logic for execution management

use crate::db::{ExecutionLogRepository, ExecutionRepository, JobRepository};
use crate::error::AppError;
use crate::models::{
    Execution, ExecuteJobRequest, ExecutionListResponse, ExecutionLog, ExecutionLogsResponse,
    ExecutionStatus, ListExecutionsQuery, ListLogsQuery,
};
use tracing::{debug, info, instrument, warn};

/// Service for execution management
#[derive(Clone)]
pub struct ExecutionService {
    execution_repo: ExecutionRepository,
    log_repo: ExecutionLogRepository,
    job_repo: JobRepository,
}

impl ExecutionService {
    /// Create a new ExecutionService
    pub fn new(
        execution_repo: ExecutionRepository,
        log_repo: ExecutionLogRepository,
        job_repo: JobRepository,
    ) -> Self {
        Self {
            execution_repo,
            log_repo,
            job_repo,
        }
    }

    /// Create a new execution for a job
    #[instrument(skip(self, req))]
    pub async fn create_execution(
        &self,
        job_id: &str,
        req: Option<ExecuteJobRequest>,
    ) -> Result<Execution, AppError> {
        debug!("Creating execution for job");
        
        // Verify job exists and is enabled
        let job = self.job_repo.get_by_id(job_id).await?;
        if !job.enabled {
            warn!(job_id = %job_id, job_name = %job.name, "Attempted to execute disabled job");
            return Err(AppError::BadRequest(format!(
                "Job '{}' is disabled",
                job.name
            )));
        }

        let input_data = req.and_then(|r| r.input_data);
        let execution = Execution::new(job_id, input_data);
        
        info!(
            execution_id = %execution.id,
            job_id = %job_id,
            job_name = %job.name,
            "Execution created"
        );

        self.execution_repo.create(&execution).await
    }

    /// Get an execution by ID
    #[instrument(skip(self))]
    pub async fn get_execution(&self, id: &str) -> Result<Execution, AppError> {
        debug!("Fetching execution");
        self.execution_repo.get_by_id(id).await
    }

    /// List executions with filters
    #[instrument(skip(self))]
    pub async fn list_executions(
        &self,
        query: ListExecutionsQuery,
    ) -> Result<ExecutionListResponse, AppError> {
        debug!(
            limit = query.limit,
            offset = query.offset,
            status = ?query.status,
            job_id = ?query.job_id,
            "Listing executions"
        );
        
        let limit = query.limit.clamp(1, 100);
        let offset = query.offset.max(0);

        let validated_query = ListExecutionsQuery {
            limit,
            offset,
            status: query.status,
            job_id: query.job_id,
            from: query.from,
            to: query.to,
        };

        let (executions, total) = self.execution_repo.list(&validated_query).await?;
        debug!(count = executions.len(), total = total, "Executions fetched");

        Ok(ExecutionListResponse {
            executions,
            total,
            limit,
            offset,
        })
    }

    /// List executions for a specific job
    #[instrument(skip(self))]
    pub async fn list_job_executions(
        &self,
        job_id: &str,
        limit: i32,
        offset: i32,
    ) -> Result<ExecutionListResponse, AppError> {
        debug!("Listing executions for job");
        
        // Verify job exists
        let _ = self.job_repo.get_by_id(job_id).await?;

        let limit = limit.clamp(1, 100);
        let offset = offset.max(0);

        let (executions, total) = self.execution_repo.list_by_job(job_id, limit, offset).await?;

        Ok(ExecutionListResponse {
            executions,
            total,
            limit,
            offset,
        })
    }

    /// Cancel an execution
    #[instrument(skip(self))]
    pub async fn cancel_execution(&self, id: &str) -> Result<Execution, AppError> {
        info!("Cancelling execution");
        let mut execution = self.execution_repo.get_by_id(id).await?;

        match execution.status_enum() {
            ExecutionStatus::Pending | ExecutionStatus::Queued => {
                debug!(status = %execution.status, "Cancelling pending/queued execution");
                execution.mark_cancelled();
                self.execution_repo.update(&execution).await
            }
            ExecutionStatus::Running => {
                // For running executions, we need the worker to handle cancellation
                // Here we just mark it for cancellation, the worker will pick it up
                debug!("Marking running execution for cancellation");
                execution.mark_cancelled();
                self.execution_repo.update(&execution).await
            }
            _ => {
                warn!(status = %execution.status, "Cannot cancel execution with this status");
                Err(AppError::BadRequest(format!(
                    "Cannot cancel execution with status '{}'",
                    execution.status
                )))
            }
        }
    }

    /// Retry a failed execution
    #[instrument(skip(self))]
    pub async fn retry_execution(&self, id: &str) -> Result<Execution, AppError> {
        info!("Retrying execution");
        let execution = self.execution_repo.get_by_id(id).await?;

        // Check if execution can be retried
        if !matches!(
            execution.status_enum(),
            ExecutionStatus::Failed | ExecutionStatus::Timeout | ExecutionStatus::Cancelled
        ) {
            warn!(status = %execution.status, "Cannot retry execution with this status");
            return Err(AppError::BadRequest(format!(
                "Cannot retry execution with status '{}'",
                execution.status
            )));
        }

        // Get the job to check max retries
        let job = self.job_repo.get_by_id(&execution.job_id).await?;

        if execution.retry_count >= job.max_retries {
            warn!(
                retry_count = execution.retry_count,
                max_retries = job.max_retries,
                "Maximum retry count reached"
            );
            return Err(AppError::BadRequest(format!(
                "Execution has reached maximum retry count ({})",
                job.max_retries
            )));
        }

        debug!(retry_count = execution.retry_count + 1, "Creating retry execution");
        
        // Create a new execution for retry
        let input_data = execution
            .input_data
            .and_then(|s| serde_json::from_str(&s).ok());
        let mut new_execution = Execution::new(&execution.job_id, input_data);
        new_execution.retry_count = execution.retry_count + 1;

        self.execution_repo.create(&new_execution).await
    }

    /// Delete an execution
    pub async fn delete_execution(&self, id: &str) -> Result<(), AppError> {
        // Delete logs first (cascade should handle this, but just in case)
        let _ = self.log_repo.delete_by_execution(id).await;
        self.execution_repo.delete(id).await
    }

    /// Delete multiple executions
    pub async fn delete_executions(&self, ids: Vec<String>) -> Result<u64, AppError> {
        self.execution_repo.delete_bulk(&ids).await
    }

    /// Update execution status
    pub async fn update_execution(&self, execution: &Execution) -> Result<Execution, AppError> {
        self.execution_repo.update(execution).await
    }

    /// Get execution logs
    pub async fn get_logs(
        &self,
        execution_id: &str,
        query: ListLogsQuery,
    ) -> Result<ExecutionLogsResponse, AppError> {
        // Verify execution exists
        let _ = self.execution_repo.get_by_id(execution_id).await?;

        let offset = query.offset.unwrap_or(0).max(0);
        let limit = query.limit.unwrap_or(1000).clamp(1, 10000);

        let (logs, total) = self
            .log_repo
            .get_by_execution_paginated(execution_id, query.log_type.as_deref(), offset, limit)
            .await?;

        Ok(ExecutionLogsResponse {
            execution_id: execution_id.to_string(),
            logs,
            total,
        })
    }

    /// Append log entry
    pub async fn append_log(&self, log: ExecutionLog) -> Result<ExecutionLog, AppError> {
        self.log_repo.create(&log).await
    }

    /// Get running executions
    pub async fn get_running_executions(&self) -> Result<Vec<Execution>, AppError> {
        self.execution_repo.get_running().await
    }
}
