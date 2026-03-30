//! Serverust-Less - AWS Lambda-like serverless platform for Python REPL execution

use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

use serverust_less::api::{create_router, AppState};
use serverust_less::config::AppConfig;
use serverust_less::db::{
    init_pool, run_migrations, AuditRepository, DagRepository, ExecutionLogRepository,
    ExecutionRepository, JobRepository, PackageRepository, QueueRepository, ScheduleRepository,
    VenvRepository,
};
use serverust_less::queue::QueueManager;
use serverust_less::services::{
    AuditService, DagService, ExecutionService, JobService, PackageService, QueueService,
    ScheduleService, VenvService,
};
use serverust_less::worker::{PackageManager, ProcessManager, VenvManager, WorkerPool};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // -------------------------------------------------------------------------
    // Logging
    // -------------------------------------------------------------------------
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,serverust_less=debug,tower_http=debug"));

    let subscriber = fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting Serverust-Less…");

    // -------------------------------------------------------------------------
    // Configuration
    // -------------------------------------------------------------------------
    let config = AppConfig::load()?;

    // -------------------------------------------------------------------------
    // Resolve all file-system paths to absolute so that relative config values
    // (e.g. "./venvs/main") work correctly regardless of the process CWD.
    // -------------------------------------------------------------------------
    let cwd = std::env::current_dir()?;
    let abs_main_venv_path = if config.packages.main_venv_path.is_absolute() {
        config.packages.main_venv_path.clone()
    } else {
        cwd.join(&config.packages.main_venv_path)
    };
    let abs_custom_venv_base_path = if config.packages.custom_venv_base_path.is_absolute() {
        config.packages.custom_venv_base_path.clone()
    } else {
        cwd.join(&config.packages.custom_venv_base_path)
    };
    let abs_db_path = if config.database.path.is_absolute() {
        config.database.path.clone()
    } else {
        cwd.join(&config.database.path)
    };

    info!(
        host = %config.server.host,
        port = config.server.port,
        db_path = %abs_db_path.display(),
        main_venv = %abs_main_venv_path.display(),
        pool_size = config.worker.pool_size,
        "Configuration loaded"
    );

    // -------------------------------------------------------------------------
    // Directory setup
    // -------------------------------------------------------------------------
    if let Some(parent) = abs_db_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
            info!("Created database directory: {}", parent.display());
        }
    }

    if !abs_custom_venv_base_path.exists() {
        std::fs::create_dir_all(&abs_custom_venv_base_path)?;
        info!("Created venv base directory: {}", abs_custom_venv_base_path.display());
    }

    // -------------------------------------------------------------------------
    // Auto-create main venv if missing
    // -------------------------------------------------------------------------
    let main_venv_python = if cfg!(windows) {
        abs_main_venv_path.join("Scripts").join("python.exe")
    } else {
        abs_main_venv_path.join("bin").join("python")
    };

    if !main_venv_python.exists() {
        info!(
            "Main venv not found, creating at: {}",
            abs_main_venv_path.display()
        );
        match std::process::Command::new(&config.worker.python_executable)
            .args(["-m", "venv", abs_main_venv_path.to_str().unwrap()])
            .output()
        {
            Ok(output) if output.status.success() => {
                info!("Main venv created successfully");
            }
            Ok(output) => {
                warn!(
                    "Failed to create main venv: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => warn!("Failed to execute {} -m venv: {}", config.worker.python_executable, e),
        }
    } else {
        info!(
            "Main venv already exists at: {}",
            abs_main_venv_path.display()
        );
    }

    // -------------------------------------------------------------------------
    // Database
    // -------------------------------------------------------------------------
    let db_url = format!("sqlite:{}?mode=rwc", abs_db_path.display());
    let pool = init_pool(&db_url).await?;
    info!("Database connection established");

    run_migrations(&pool).await?;
    info!("Database migrations completed");

    // -------------------------------------------------------------------------
    // Repositories
    // -------------------------------------------------------------------------
    let job_repo = JobRepository::new(pool.clone());
    let execution_repo = ExecutionRepository::new(pool.clone());
    let execution_log_repo = ExecutionLogRepository::new(pool.clone());
    let package_repo = PackageRepository::new(pool.clone());
    let venv_repo = VenvRepository::new(pool.clone());
    let queue_repo = QueueRepository::new(pool.clone());
    let audit_repo = AuditRepository::new(pool.clone());
    let schedule_repo = ScheduleRepository::new(pool.clone());
    let dag_repo = DagRepository::new(pool.clone());

    // -------------------------------------------------------------------------
    // Queue manager — shared between API (enqueue) and worker pool (dequeue)
    // -------------------------------------------------------------------------
    let queue_manager = Arc::new(QueueManager::with_config(
        queue_repo.clone(),
        execution_repo.clone(),
        job_repo.clone(),
        config.queue.max_size,
        config.queue.max_retries,
        config.queue.retry_delay_seconds,
    ));

    // Recover any queued items that survived a previous crash / restart
    if let Err(e) = queue_manager.recover().await {
        warn!("Queue recovery failed (non-fatal): {}", e);
    }

    // -------------------------------------------------------------------------
    // Process manager — shared between worker pool and cancel API handler
    // -------------------------------------------------------------------------
    let process_manager = Arc::new(ProcessManager::new(
        config.worker.graceful_shutdown_seconds,
    ));

    // -------------------------------------------------------------------------
    // Execution service (created early, needed by DAG engine + worker pool)
    // -------------------------------------------------------------------------
    let execution_service = ExecutionService::new(
        execution_repo.clone(),
        execution_log_repo.clone(),
        job_repo.clone(),
    );

    // -------------------------------------------------------------------------
    // DAG Engine (created before worker pool so workers get the callback)
    // -------------------------------------------------------------------------
    let dag_engine = Arc::new(serverust_less::dag::DagEngine::new(
        dag_repo.clone(),
        execution_service.clone(),
        queue_manager.clone(),
        job_repo.clone(),
    ));

    // -------------------------------------------------------------------------
    // Worker pool
    // -------------------------------------------------------------------------
    let (worker_pool, mut result_rx) = WorkerPool::new(
        config.worker.pool_size,
        abs_main_venv_path.clone(),
        abs_custom_venv_base_path.clone(),
        &config.worker.python_executable,
        queue_manager.clone(),
        process_manager.clone(),
        execution_repo.clone(),
        execution_log_repo.clone(),
        job_repo.clone(),
        Some(dag_engine.clone()),
    );
    info!(
        "Worker pool started with {} workers",
        config.worker.pool_size
    );

    // Background task: consume WorkerResult notifications for observability logging
    tokio::spawn(async move {
        while let Some(result) = result_rx.recv().await {
            if result.success {
                info!(
                    execution_id = %result.execution_id,
                    job_id = %result.job_id,
                    duration_ms = result.duration_ms,
                    "Execution succeeded"
                );
            } else {
                warn!(
                    execution_id = %result.execution_id,
                    job_id = %result.job_id,
                    timed_out = result.timed_out,
                    memory_exceeded = result.memory_exceeded,
                    error = ?result.error,
                    "Execution failed"
                );
            }
        }
    });

    // -------------------------------------------------------------------------
    // Retention policy scheduler (F4)
    // Periodically cleans up old executions, logs, and queue entries.
    // -------------------------------------------------------------------------
    {
        let retention_config = config.retention.clone();
        let exec_repo_retention = execution_repo.clone();
        let log_repo_retention = execution_log_repo.clone();
        let queue_repo_retention = queue_repo.clone();

        let interval_hours = retention_config.cleanup_interval_hours.max(1);
        let history_days = retention_config.execution_history_days;

        info!(
            "Retention scheduler started: cleanup every {}h, keep {}d of execution history",
            interval_hours, history_days
        );

        tokio::spawn(async move {
            // Wait a short period before the first cleanup to let the server stabilize
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(interval_hours as u64 * 3600));
            // The first tick fires immediately, but we already waited 60s above
            interval.tick().await;

            loop {
                interval.tick().await;
                info!("Retention scheduler: starting cleanup cycle");

                // 1. Delete old terminal executions
                match exec_repo_retention.delete_older_than_days(history_days).await {
                    Ok(count) if count > 0 => {
                        info!("Retention: deleted {} old execution(s)", count);
                    }
                    Ok(_) => {
                        info!("Retention: no old executions to clean up");
                    }
                    Err(e) => {
                        error!("Retention: failed to delete old executions: {}", e);
                    }
                }

                // 2. Delete orphaned execution logs (whose execution was deleted)
                match log_repo_retention.delete_orphaned().await {
                    Ok(count) if count > 0 => {
                        info!("Retention: deleted {} orphaned execution log(s)", count);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Retention: failed to delete orphaned logs: {}", e);
                    }
                }

                // 3. Clean up old completed/failed queue entries (older than history_days * 24h)
                let queue_cleanup_hours = (history_days * 24) as i32;
                match queue_repo_retention.cleanup_old(queue_cleanup_hours).await {
                    Ok(count) if count > 0 => {
                        info!("Retention: deleted {} old queue entries", count);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Retention: failed to clean up queue entries: {}", e);
                    }
                }

                info!("Retention scheduler: cleanup cycle complete");
            }
        });
    }

    // -------------------------------------------------------------------------
    // Services
    // -------------------------------------------------------------------------
    let job_service = JobService::new(job_repo.clone());

    let venv_manager = Arc::new(VenvManager::new(
        &abs_custom_venv_base_path,
        &config.worker.python_executable,
    ));
    let package_manager_worker = Arc::new(PackageManager::new(
        config.packages.pip_timeout_seconds,
        config.packages.enable_pip_cache,
        None,
    ));

    let package_service =
        PackageService::with_config(
            package_repo.clone(),
            Arc::clone(&venv_manager),
            package_manager_worker,
            config.packages.conflict_resolution.strategy.clone(),
        );
    let venv_service = VenvService::new(venv_repo.clone());

    // Ensure main venv record exists in the database
    {
        let python_version = venv_manager
            .get_python_version(&abs_main_venv_path)
            .await
            .ok();
        match venv_service
            .ensure_main_venv(abs_main_venv_path.to_str().unwrap_or(""), python_version)
            .await
        {
            Ok(mut venv) => {
                if !venv.is_ready() {
                    venv.mark_ready();
                    if let Err(e) = venv_service.update_venv(&venv).await {
                        warn!("Failed to mark main venv as ready: {}", e);
                    }
                }
                info!("Main venv record ensured in database");
            }
            Err(e) => warn!("Failed to ensure main venv in database: {}", e),
        }
    }

    let queue_service = QueueService::new(queue_repo.clone());
    let audit_service =
        AuditService::new(audit_repo.clone(), config.security.enable_audit_log);

    let schedule_service = ScheduleService::new(schedule_repo.clone());
    let dag_service = DagService::new(dag_repo.clone());

    // -------------------------------------------------------------------------
    // Scheduler (background cron ticker)
    // -------------------------------------------------------------------------
    if config.scheduler.enabled {
        let scheduler_runner = serverust_less::scheduler::SchedulerRunner::new(
            schedule_repo.clone(),
            execution_service.clone(),
            queue_manager.clone(),
            job_repo.clone(),
            config.scheduler.tick_interval_seconds,
        );

        tokio::spawn(async move {
            scheduler_runner.run().await;
        });

        info!("Cron scheduler enabled (tick interval: {}s)", config.scheduler.tick_interval_seconds);
    } else {
        info!("Cron scheduler disabled");
    }

    // -------------------------------------------------------------------------
    // Application state & router
    // -------------------------------------------------------------------------
    let state = AppState {
        job_service,
        execution_service,
        package_service,
        venv_service,
        queue_service,
        audit_service,
        schedule_service,
        dag_service,
        queue_manager,
        process_manager,
        worker_pool_size: config.worker.pool_size,
        venv_manager,
        dag_engine: Some(dag_engine),
    };

    let app = create_router(state);

    // -------------------------------------------------------------------------
    // HTTP server
    // -------------------------------------------------------------------------
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .expect("Invalid server address");

    info!("Server listening on http://{}", addr);
    info!("Web UI:      http://{}/", addr);
    info!("Swagger UI:  http://{}/swagger-ui/", addr);

    // Keep worker_pool in scope so workers are not dropped
    let _pool = worker_pool;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}


