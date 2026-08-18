//! Health API endpoints

use axum::{
    extract::State,
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

use crate::api::AppState;
use crate::error::AppError;

/// Create the health router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health_check))
        .route("/stats", get(get_stats))
        .route("/workers/status", get(get_workers_status))
}

/// Health check response
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub timestamp: String,
    pub checks: HealthChecksResponse,
}

/// Status for a single subsystem health check
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthSubsystemResponse {
    pub status: String,
    pub detail: String,
}

/// Aggregated subsystem checks for the health endpoint
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthChecksResponse {
    pub database: HealthSubsystemResponse,
    pub queue: HealthSubsystemResponse,
    pub workers: HealthSubsystemResponse,
    pub main_venv: HealthSubsystemResponse,
    pub scheduler: HealthSubsystemResponse,
    pub disk: HealthSubsystemResponse,
}

/// System statistics response
#[derive(Debug, Serialize, ToSchema)]
pub struct StatsResponse {
    pub total_jobs: i64,
    pub enabled_jobs: i64,
    pub total_executions: i64,
    pub running_executions: i64,
    pub queue_depth: i64,
    pub venv_count: i64,
}

/// Worker pool status response
#[derive(Debug, Serialize, ToSchema)]
pub struct WorkerStatusResponse {
    /// Total number of worker tasks in the pool.
    pub pool_size: usize,
    /// Number of executions currently tracked as running.
    pub running: usize,
    /// Number of idle worker slots (pool_size - running, may be approximate).
    pub idle: usize,
    /// Number of items currently in the in-memory priority queue.
    pub queue_memory_size: usize,
}

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/api/v1/health",
    tag = "health",
    responses(
        (status = 200, description = "Service health status", body = HealthResponse),
        (status = 503, description = "Service is unhealthy", body = HealthResponse)
    )
)]
pub async fn health_check(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<HealthResponse>) {
    let database = assess_database(state.as_ref()).await;
    let queue = assess_queue(state.as_ref()).await;
    let workers = assess_workers(state.as_ref()).await;
    let main_venv = assess_main_venv(state.as_ref()).await;
    let scheduler = assess_scheduler(state.as_ref()).await;
    let disk = assess_disk(state.as_ref()).await;

    let overall = [database.level, queue.level, workers.level, main_venv.level, scheduler.level, disk.level]
        .into_iter()
        .max_by_key(|level| level.severity())
        .unwrap_or(HealthLevel::Healthy);

    let response = HealthResponse {
        status: overall.as_str().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        checks: HealthChecksResponse {
            database: database.into_response(),
            queue: queue.into_response(),
            workers: workers.into_response(),
            main_venv: main_venv.into_response(),
            scheduler: scheduler.into_response(),
            disk: disk.into_response(),
        },
    };

    let status_code = match overall {
        HealthLevel::Unhealthy => StatusCode::SERVICE_UNAVAILABLE,
        HealthLevel::Healthy | HealthLevel::Degraded => StatusCode::OK,
    };

    (status_code, Json(response))
}

/// Get system statistics
#[utoipa::path(
    get,
    path = "/api/v1/stats",
    tag = "health",
    responses(
        (status = 200, description = "System statistics", body = StatsResponse)
    )
)]
pub async fn get_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatsResponse>, AppError> {
    // Use dedicated count queries instead of full list queries
    let total_jobs = state.job_service.count_all().await?;
    let enabled_jobs = state.job_service.count_enabled().await?;
    let total_executions = state.execution_service.count_all().await?;
    let running_executions = state.execution_service.count_running().await?;
    let queue_depth = state.queue_service.get_depth().await?;
    let venvs = state.venv_service.list_venvs().await?;

    Ok(Json(StatsResponse {
        total_jobs,
        enabled_jobs,
        total_executions,
        running_executions,
        queue_depth,
        venv_count: venvs.total,
    }))
}

/// Get worker pool status
#[utoipa::path(
    get,
    path = "/api/v1/workers/status",
    tag = "health",
    responses(
        (status = 200, description = "Worker pool status", body = WorkerStatusResponse)
    )
)]
pub async fn get_workers_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<WorkerStatusResponse>, AppError> {
    let running = state.process_manager.running_count().await;
    let queue_memory_size = state.queue_manager.memory_queue_size().await;
    let pool_size = state.worker_pool_size;
    let idle = pool_size.saturating_sub(running);

    Ok(Json(WorkerStatusResponse {
        pool_size,
        running,
        idle,
        queue_memory_size,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HealthLevel {
    Healthy,
    Degraded,
    Unhealthy,
}

impl HealthLevel {
    fn as_str(self) -> &'static str {
        match self {
            HealthLevel::Healthy => "healthy",
            HealthLevel::Degraded => "degraded",
            HealthLevel::Unhealthy => "unhealthy",
        }
    }

    fn severity(self) -> u8 {
        match self {
            HealthLevel::Healthy => 0,
            HealthLevel::Degraded => 1,
            HealthLevel::Unhealthy => 2,
        }
    }
}

#[derive(Debug)]
struct HealthAssessment {
    level: HealthLevel,
    detail: String,
}

impl HealthAssessment {
    fn healthy(detail: impl Into<String>) -> Self {
        Self {
            level: HealthLevel::Healthy,
            detail: detail.into(),
        }
    }

    fn degraded(detail: impl Into<String>) -> Self {
        Self {
            level: HealthLevel::Degraded,
            detail: detail.into(),
        }
    }

    fn unhealthy(detail: impl Into<String>) -> Self {
        Self {
            level: HealthLevel::Unhealthy,
            detail: detail.into(),
        }
    }

    fn into_response(self) -> HealthSubsystemResponse {
        HealthSubsystemResponse {
            status: self.level.as_str().to_string(),
            detail: self.detail,
        }
    }
}

async fn assess_database(state: &AppState) -> HealthAssessment {
    let total_jobs = match state.job_service.count_all().await {
        Ok(total_jobs) => total_jobs,
        Err(err) => {
            return HealthAssessment::unhealthy(format!(
                "job count query failed: {}",
                err
            ));
        }
    };

    let total_executions = match state.execution_service.count_all().await {
        Ok(total_executions) => total_executions,
        Err(err) => {
            return HealthAssessment::unhealthy(format!(
                "execution count query failed: {}",
                err
            ));
        }
    };

    HealthAssessment::healthy(format!(
        "reachable; jobs={}, executions={}",
        total_jobs, total_executions
    ))
}

async fn assess_queue(state: &AppState) -> HealthAssessment {
    let queue_depth = match state.queue_service.get_depth().await {
        Ok(queue_depth) => queue_depth,
        Err(err) => {
            return HealthAssessment::unhealthy(format!(
                "queue depth query failed: {}",
                err
            ));
        }
    };

    let memory_queue_size = state.queue_manager.memory_queue_size().await;
    HealthAssessment::healthy(format!(
        "queue reachable; persistent_depth={}, memory_depth={}",
        queue_depth, memory_queue_size
    ))
}

async fn assess_workers(state: &AppState) -> HealthAssessment {
    let pool_size = state.worker_pool_size;
    let running = state.process_manager.running_count().await;
    let idle = pool_size.saturating_sub(running);

    if pool_size == 0 {
        return HealthAssessment::unhealthy("worker pool size is zero");
    }

    HealthAssessment::healthy(format!(
        "worker pool available; pool_size={}, running={}, idle={}",
        pool_size, running, idle
    ))
}

async fn assess_main_venv(state: &AppState) -> HealthAssessment {
    let main_venv_path = state.venv_manager.main_venv_path();
    let interpreter_path = state.venv_manager.get_python_path(&main_venv_path);
    let interpreter_exists = interpreter_path.exists();

    match state.venv_service.get_main_venv().await {
        Ok(Some(venv)) if venv.is_ready() && interpreter_exists => {
            HealthAssessment::healthy(format!(
                "ready; record_path={}, interpreter={}",
                venv.path,
                interpreter_path.display()
            ))
        }
        Ok(Some(venv)) if venv.is_ready() => {
            HealthAssessment::degraded(format!(
                "database record is ready but interpreter is missing at {}",
                interpreter_path.display()
            ))
        }
        Ok(Some(venv)) if interpreter_exists => {
            HealthAssessment::degraded(format!(
                "interpreter exists at {} but database status is '{}'",
                interpreter_path.display(),
                venv.status
            ))
        }
        Ok(Some(venv)) => {
            HealthAssessment::degraded(format!(
                "database status is '{}' and interpreter is missing at {}",
                venv.status,
                interpreter_path.display()
            ))
        }
        Ok(None) if interpreter_exists => {
            HealthAssessment::degraded(format!(
                "interpreter exists at {} but the database record is missing",
                interpreter_path.display()
            ))
        }
        Ok(None) => HealthAssessment::degraded(format!(
            "main venv interpreter is missing at {}",
            interpreter_path.display()
        )),
        Err(err) => HealthAssessment::unhealthy(format!(
            "failed to inspect main venv state: {}",
            err
        )),
    }
}

async fn assess_scheduler(state: &AppState) -> HealthAssessment {
    if !state.scheduler_enabled {
        return HealthAssessment::healthy("scheduler is disabled by configuration");
    }

    // Check if there are any schedules in the database
    match state.schedule_service.list_schedules().await {
        Ok(list) => {
            let enabled_count = list.schedules.iter().filter(|s| s.enabled).count();
            let overdue_count = list
                .schedules
                .iter()
                .filter(|s| {
                    s.enabled
                        && s.next_run_at
                            .as_ref()
                            .map(|n| n.as_str() <= chrono::Utc::now().to_rfc3339().as_str())
                            .unwrap_or(false)
                })
                .count();
            if overdue_count > 0 {
                HealthAssessment::degraded(format!(
                    "scheduler enabled; total_schedules={}, enabled={}, overdue={}",
                    list.total, enabled_count, overdue_count
                ))
            } else {
                HealthAssessment::healthy(format!(
                    "scheduler enabled; total_schedules={}, enabled={}",
                    list.total, enabled_count
                ))
            }
        }
        Err(err) => HealthAssessment::unhealthy(format!(
            "failed to query schedules: {}",
            err
        )),
    }
}

async fn assess_disk(state: &AppState) -> HealthAssessment {
    let main_venv_path = state.venv_manager.main_venv_path();
    let db_dir = main_venv_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let mut details = Vec::new();
    let mut degraded = false;

    for (label, path) in [("db_dir", db_dir), ("main_venv", main_venv_path.as_path())] {
        match std::fs::metadata(path) {
            Ok(_) => {
                // Try to get available space via statvfs on Unix
                #[cfg(unix)]
                {
                    let stat = nix::sys::statvfs::statvfs(path);
                    match stat {
                        Ok(s) => {
                            let avail_bytes = s.blocks_available() * s.block_size();
                            let gb = avail_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                            if gb < 0.1 {
                                degraded = true;
                                details.push(format!("{}: {:.2} GB (critical)", label, gb));
                            } else if gb < 1.0 {
                                details.push(format!("{}: {:.2} GB (low)", label, gb));
                            } else {
                                details.push(format!("{}: {:.2} GB", label, gb));
                            }
                        }
                        Err(_) => {
                            details.push(format!("{}: accessible (space unknown)", label));
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    details.push(format!("{}: accessible", label));
                }
            }
            Err(_) => {
                degraded = true;
                details.push(format!("{}: not accessible", label));
            }
        }
    }

    let detail_str = details.join("; ");

    if degraded {
        HealthAssessment::degraded(format!("disk check — {}", detail_str))
    } else {
        HealthAssessment::healthy(format!("disk check — {}", detail_str))
    }
}
