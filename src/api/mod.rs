//! API layer module

pub mod dags;
pub mod executions;
pub mod health;
pub mod jobs;
pub mod packages;
pub mod queue;
pub mod schedules;
pub mod venvs;

use axum::Router;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::config::CorsConfig;
use crate::config::RateLimitConfig;
use crate::dag::DagEngine;
use crate::queue::QueueManager;
use crate::services::{
    AuditService, DagService, ExecutionService, JobService, PackageService, QueueService,
    ScheduleService, VenvService,
};
use crate::worker::{ProcessManager, VenvManager};

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub job_service: JobService,
    pub execution_service: ExecutionService,
    pub package_service: PackageService,
    pub venv_service: VenvService,
    pub queue_service: QueueService,
    pub audit_service: AuditService,
    pub schedule_service: ScheduleService,
    pub dag_service: DagService,
    /// Shared queue manager — API handlers enqueue here, workers dequeue from here.
    pub queue_manager: Arc<QueueManager>,
    /// Process manager — used by cancel handlers to kill running executions.
    pub process_manager: Arc<ProcessManager>,
    /// Total number of worker tasks in the pool (for monitoring).
    pub worker_pool_size: usize,
    /// Venv manager — used by venv creation endpoint.
    pub venv_manager: Arc<VenvManager>,
    /// DAG engine — used by DAG trigger/callback endpoints.
    pub dag_engine: Option<Arc<DagEngine>>,
    /// CORS configuration from config file.
    pub cors_config: CorsConfig,
    /// Rate limiting configuration from config file.
    pub rate_limit_config: RateLimitConfig,
    /// Whether the background cron scheduler is enabled.
    pub scheduler_enabled: bool,
}

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        // Jobs
        jobs::list_jobs,
        jobs::create_job,
        jobs::get_job,
        jobs::update_job,
        jobs::delete_job,
        jobs::enable_job,
        jobs::disable_job,
        jobs::bulk_create_jobs,
        jobs::bulk_delete_jobs,
        jobs::clone_job,
        jobs::list_job_versions,
        jobs::get_job_version,
        jobs::restore_job_version,
        // Executions
        executions::list_executions,
        executions::get_execution,
        executions::delete_execution,
        executions::bulk_delete_executions,
        executions::get_execution_logs,
        executions::stream_execution_logs,
        executions::cancel_execution,
        executions::retry_execution,
        executions::execute_job,
        executions::list_job_executions,
        // Packages
        packages::list_packages,
        packages::install_package,
        packages::uninstall_package,
        packages::get_main_venv_packages,
        packages::update_main_venv_packages,
        packages::clear_main_venv,
        packages::delete_package,
        packages::get_job_dependencies,
        packages::add_job_dependency,
        packages::update_dependency,
        packages::remove_dependency,
        packages::get_dependency_status,
        packages::install_job_dependencies,
        packages::search_pypi,
        packages::get_pypi_package_details,
        // Venvs
        venvs::list_venvs,
        venvs::create_venv,
        venvs::get_venv,
        venvs::delete_venv,
        venvs::get_job_venv_info,
        venvs::toggle_job_venv,
        venvs::delete_job_venv,
        // Queue
        queue::get_queue_status,
        // Health
        health::health_check,
        health::get_stats,
        health::get_workers_status,
        // Schedules
        schedules::create_schedule,
        schedules::get_schedule,
        schedules::update_schedule,
        schedules::delete_schedule,
        schedules::toggle_schedule,
        schedules::list_schedules,
        // DAGs
        dags::create_dag,
        dags::list_dags,
        dags::get_dag,
        dags::update_dag,
        dags::delete_dag,
        dags::add_edge,
        dags::delete_edge,
        dags::get_topology,
        dags::validate_dag,
        dags::trigger_dag,
        dags::list_dag_runs,
        dags::get_dag_run,
        dags::cancel_dag_run,
    ),
    components(
        schemas(
            // Job schemas
            crate::models::Job,
            crate::models::CreateJobRequest,
            crate::models::UpdateJobRequest,
            crate::models::ListJobsQuery,
            crate::models::JobListResponse,
            crate::models::BulkDeleteRequest,
            crate::models::BulkOperationResponse,
            crate::models::CloneJobRequest,
            crate::models::JobVersion,
            crate::models::JobVersionListResponse,
            crate::models::RestoreJobVersionRequest,
            // Execution schemas
            crate::models::Execution,
            crate::models::ExecuteJobRequest,
            crate::models::ListExecutionsQuery,
            crate::models::ExecutionListResponse,
            crate::models::ExecutionLog,
            crate::models::ExecutionLogsResponse,
            // Package schemas
            crate::models::PackageCache,
            crate::models::PackageListResponse,
            crate::models::InstallPackageRequest,
            crate::models::JobDependency,
            crate::models::AddDependencyRequest,
            crate::models::DependencyListResponse,
            crate::models::DependencyStatusResponse,
            crate::models::PackageInstallStatus,
            // Venv schemas
            crate::models::Venv,
            crate::models::VenvListResponse,
            crate::models::JobVenvInfo,
            crate::models::CreateVenvRequest,
            // Queue schemas
            crate::models::QueueStatusResponse,
            crate::models::PriorityCount,
            // Health schemas
            health::HealthResponse,
            health::HealthChecksResponse,
            health::HealthSubsystemResponse,
            health::StatsResponse,
            health::WorkerStatusResponse,
            // Package search schemas
            packages::SearchResponse,
            packages::PyPiSearchResult,
            packages::PyPiPackageDetailResponse,
            packages::PyPiReleaseSummary,
            // Error schemas
            crate::error::ErrorResponse,
            // Schedule schemas
            crate::models::JobSchedule,
            crate::models::CreateScheduleRequest,
            crate::models::UpdateScheduleRequest,
            crate::models::ScheduleListResponse,
            // DAG schemas
            crate::models::Dag,
            crate::models::DagEdge,
            crate::models::DagRun,
            crate::models::DagNodeExecution,
            crate::models::CreateDagRequest,
            crate::models::UpdateDagRequest,
            crate::models::AddEdgeRequest,
            crate::models::DagDetailResponse,
            crate::models::DagListResponse,
            crate::models::DagRunDetailResponse,
            crate::models::DagRunListResponse,
            crate::models::TopologyResponse,
            crate::models::DagValidationResponse,
        )
    ),
    tags(
        (name = "jobs", description = "Job management endpoints"),
        (name = "executions", description = "Execution management endpoints"),
        (name = "packages", description = "Package management endpoints"),
        (name = "venvs", description = "Virtual environment management endpoints"),
        (name = "queue", description = "Queue management endpoints"),
        (name = "health", description = "Health and monitoring endpoints"),
        (name = "schedules", description = "Schedule management endpoints"),
        (name = "dags", description = "DAG management endpoints")
    ),
    info(
        title = "Serverust-Less API",
        version = "1.0.0",
        description = "AWS Lambda-like serverless platform for Python REPL execution"
    )
)]
pub struct ApiDoc;

/// Create the API router
pub fn create_router(state: AppState) -> Router {
    // Build CORS layer from configuration
    let cors_config = state.cors_config.clone();
    let cors = if cors_config.enabled {
        let origins: Vec<axum::http::HeaderValue> = cors_config
            .allowed_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        let methods: Vec<axum::http::Method> = cors_config
            .allowed_methods
            .iter()
            .filter_map(|m| m.parse().ok())
            .collect();

        let origin_layer = if origins.is_empty() {
            AllowOrigin::any()
        } else {
            AllowOrigin::list(origins)
        };

        let method_layer = if methods.is_empty() {
            tower_http::cors::AllowMethods::any()
        } else {
            tower_http::cors::AllowMethods::list(methods)
        };

        CorsLayer::new()
            .allow_origin(origin_layer)
            .allow_methods(method_layer)
            .allow_headers(tower_http::cors::Any)
            .max_age(std::time::Duration::from_secs(cors_config.max_age_seconds))
    } else {
        // CORS disabled — use permissive defaults for development
        CorsLayer::permissive()
    };

    // Clone rate_limit_config before state is moved into Arc
    let rate_limit_config = state.rate_limit_config.clone();

    let api_routes = Router::new()
        .merge(jobs::router())
        .merge(executions::router())
        .merge(packages::router())
        .merge(venvs::router())
        .merge(queue::router())
        .merge(health::router())
        .merge(schedules::router())
        .merge(dags::router())
        .with_state(Arc::new(state));

    // Serve static files from web/ directory
    let static_files = ServeDir::new("web")
        .not_found_service(ServeFile::new("web/index.html"));

    let mut router = Router::new()
        .nest("/api/v1", api_routes)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .route("/api/openapi.json", axum::routing::get(|| async {
            axum::Json(ApiDoc::openapi())
        }))
        .nest_service("/css", ServeDir::new("web/css"))
        .nest_service("/js", ServeDir::new("web/js"))
        .fallback_service(static_files)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    // Apply rate limiting if enabled
    if rate_limit_config.enabled {
        let max_concurrent = rate_limit_config.burst_size.max(1) as usize;
        router = router.layer(tower::limit::ConcurrencyLimitLayer::new(max_concurrent));
    }

    router
}
