//! Scheduler module - background ticker for cron-based job scheduling

use std::str::FromStr;
use std::sync::Arc;

use cron::Schedule;
use tokio::time::Duration;
use tracing::{error, info, warn};

use crate::db::{JobRepository, ScheduleRepository};
use crate::models::QueueItem;
use crate::queue::QueueManager;
use crate::services::ExecutionService;

/// Background scheduler that triggers jobs based on cron expressions
pub struct SchedulerRunner {
    schedule_repo: ScheduleRepository,
    execution_service: ExecutionService,
    queue_manager: Arc<QueueManager>,
    job_repo: JobRepository,
    tick_interval: Duration,
}

impl SchedulerRunner {
    pub fn new(
        schedule_repo: ScheduleRepository,
        execution_service: ExecutionService,
        queue_manager: Arc<QueueManager>,
        job_repo: JobRepository,
        tick_interval_seconds: u64,
    ) -> Self {
        Self {
            schedule_repo,
            execution_service,
            queue_manager,
            job_repo,
            tick_interval: Duration::from_secs(tick_interval_seconds),
        }
    }

    /// Main scheduling loop
    pub async fn run(self) {
        info!(
            "Scheduler started (tick interval: {:?})",
            self.tick_interval
        );

        // Short startup delay
        tokio::time::sleep(Duration::from_secs(5)).await;

        let mut interval = tokio::time::interval(self.tick_interval);
        interval.tick().await; // first tick is immediate

        loop {
            interval.tick().await;

            if let Err(e) = self.tick().await {
                error!("Scheduler tick error: {}", e);
            }
        }
    }

    /// Single tick: find due schedules and trigger them
    async fn tick(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let due_schedules = self.schedule_repo.get_due_schedules().await?;

        if due_schedules.is_empty() {
            return Ok(());
        }

        info!("Scheduler tick: {} due schedule(s)", due_schedules.len());

        for schedule in due_schedules {
            // Verify job exists and is enabled
            let job = match self.job_repo.get_by_id(&schedule.job_id).await {
                Ok(job) => job,
                Err(e) => {
                    warn!(
                        "Scheduled job {} not found (schedule {}): {}",
                        schedule.job_id, schedule.id, e
                    );
                    continue;
                }
            };

            if !job.enabled {
                warn!(
                    "Skipping disabled job '{}' (schedule {})",
                    job.name, schedule.id
                );
                continue;
            }

            // Create execution
            let execution = match self
                .execution_service
                .create_execution(&schedule.job_id, None)
                .await
            {
                Ok(exec) => exec,
                Err(e) => {
                    error!(
                        "Failed to create execution for scheduled job {}: {}",
                        schedule.job_id, e
                    );
                    continue;
                }
            };

            // Enqueue
            let queue_item = QueueItem::new(
                &execution.id,
                &job.id,
                job.priority,
                &job.python_code,
                job.timeout_seconds,
                job.memory_limit_mb,
                execution.input_data.clone(),
                job.use_custom_venv,
            );

            if let Err(e) = self.queue_manager.enqueue(queue_item).await {
                error!(
                    "Failed to enqueue scheduled execution {}: {}",
                    execution.id, e
                );
                continue;
            }

            // Calculate next run
            let now = chrono::Utc::now().to_rfc3339();
            let next_run = match Schedule::from_str(&schedule.cron_expression) {
                Ok(sched) => match sched.upcoming(chrono::Utc).next() {
                    Some(next) => next.to_rfc3339(),
                    None => {
                        warn!(
                            "No next occurrence for schedule {}: cron '{}'",
                            schedule.id, schedule.cron_expression
                        );
                        continue;
                    }
                },
                Err(e) => {
                    error!(
                        "Invalid cron expression for schedule {}: {}",
                        schedule.id, e
                    );
                    continue;
                }
            };

            if let Err(e) = self
                .schedule_repo
                .mark_triggered(&schedule.id, &now, &next_run)
                .await
            {
                error!("Failed to mark schedule {} as triggered: {}", schedule.id, e);
            }

            info!(
                execution_id = %execution.id,
                job_id = %job.id,
                job_name = %job.name,
                schedule_id = %schedule.id,
                next_run = %next_run,
                "Scheduled execution triggered"
            );
        }

        Ok(())
    }
}
