//! Schedule service - cron parsing and schedule management

use std::str::FromStr;

use cron::Schedule;
use tracing::{debug, info};

use crate::db::ScheduleRepository;
use crate::error::AppError;
use crate::models::{CreateScheduleRequest, JobSchedule, ScheduleListResponse, UpdateScheduleRequest};

/// Service for schedule management
#[derive(Clone)]
pub struct ScheduleService {
    schedule_repo: ScheduleRepository,
}

impl ScheduleService {
    pub fn new(schedule_repo: ScheduleRepository) -> Self {
        Self { schedule_repo }
    }

    /// Validate a cron expression and return the parsed Schedule
    fn validate_cron(cron_expression: &str) -> Result<Schedule, AppError> {
        Schedule::from_str(cron_expression)
            .map_err(|e| AppError::Validation(format!("Invalid cron expression: {}", e)))
    }

    /// Calculate the next run time from a cron expression
    fn calculate_next_run(cron_expression: &str) -> Result<String, AppError> {
        let schedule = Self::validate_cron(cron_expression)?;
        let next = schedule
            .upcoming(chrono::Utc)
            .next()
            .ok_or_else(|| AppError::Validation("Cron expression has no upcoming occurrences".to_string()))?;
        Ok(next.to_rfc3339())
    }

    /// Create a schedule for a job
    pub async fn create_schedule(
        &self,
        job_id: &str,
        req: CreateScheduleRequest,
    ) -> Result<JobSchedule, AppError> {
        // Validate cron expression
        Self::validate_cron(&req.cron_expression)?;

        // Check if schedule already exists for this job
        if let Some(_existing) = self.schedule_repo.get_by_job_id(job_id).await? {
            return Err(AppError::Conflict(format!(
                "Schedule already exists for job {}. Delete it first or update it.",
                job_id
            )));
        }

        let next_run = Self::calculate_next_run(&req.cron_expression)?;
        let schedule = JobSchedule::new(job_id, &req.cron_expression, Some(next_run));

        info!(job_id = %job_id, cron = %req.cron_expression, "Schedule created");
        self.schedule_repo.create(&schedule).await
    }

    /// Get schedule by job ID
    pub async fn get_schedule_by_job_id(&self, job_id: &str) -> Result<JobSchedule, AppError> {
        self.schedule_repo
            .get_by_job_id(job_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("No schedule found for job {}", job_id)))
    }

    /// Update a schedule
    pub async fn update_schedule(
        &self,
        job_id: &str,
        req: UpdateScheduleRequest,
    ) -> Result<JobSchedule, AppError> {
        let mut schedule = self.get_schedule_by_job_id(job_id).await?;

        if let Some(cron_expr) = &req.cron_expression {
            Self::validate_cron(cron_expr)?;
            schedule.cron_expression = cron_expr.clone();
            schedule.next_run_at = Some(Self::calculate_next_run(cron_expr)?);
        }

        if let Some(enabled) = req.enabled {
            schedule.enabled = enabled;
            // Recalculate next run if re-enabled
            if enabled && schedule.next_run_at.is_none() {
                schedule.next_run_at = Some(Self::calculate_next_run(&schedule.cron_expression)?);
            }
        }

        schedule.updated_at = chrono::Utc::now().to_rfc3339();
        debug!(schedule_id = %schedule.id, "Schedule updated");
        self.schedule_repo.update(&schedule).await
    }

    /// Delete schedule for a job
    pub async fn delete_schedule(&self, job_id: &str) -> Result<(), AppError> {
        let schedule = self.get_schedule_by_job_id(job_id).await?;
        self.schedule_repo.delete(&schedule.id).await
    }

    /// Toggle schedule enabled/disabled
    pub async fn toggle_schedule(&self, job_id: &str) -> Result<JobSchedule, AppError> {
        let mut schedule = self.get_schedule_by_job_id(job_id).await?;
        schedule.enabled = !schedule.enabled;

        if schedule.enabled {
            schedule.next_run_at = Some(Self::calculate_next_run(&schedule.cron_expression)?);
        }

        schedule.updated_at = chrono::Utc::now().to_rfc3339();
        info!(schedule_id = %schedule.id, enabled = schedule.enabled, "Schedule toggled");
        self.schedule_repo.update(&schedule).await
    }

    /// List all schedules
    pub async fn list_schedules(&self) -> Result<ScheduleListResponse, AppError> {
        let (schedules, total) = self.schedule_repo.list_all().await?;
        Ok(ScheduleListResponse { schedules, total })
    }

    /// Get the schedule repository (for scheduler runner)
    pub fn repo(&self) -> &ScheduleRepository {
        &self.schedule_repo
    }
}
