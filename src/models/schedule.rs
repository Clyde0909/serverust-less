//! Schedule model and DTOs

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

/// Job schedule entity for cron-based scheduling
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct JobSchedule {
    pub id: String,
    pub job_id: String,
    pub cron_expression: String,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl JobSchedule {
    pub fn new(job_id: &str, cron_expression: &str, next_run_at: Option<String>) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4().to_string(),
            job_id: job_id.to_string(),
            cron_expression: cron_expression.to_string(),
            next_run_at,
            last_run_at: None,
            enabled: true,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

/// Request to create a schedule
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateScheduleRequest {
    /// Cron expression (6-field: sec min hour day month weekday)
    pub cron_expression: String,
}

/// Request to update a schedule
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateScheduleRequest {
    pub cron_expression: Option<String>,
    pub enabled: Option<bool>,
}

/// Schedule list response
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ScheduleListResponse {
    pub schedules: Vec<JobSchedule>,
    pub total: i64,
}
