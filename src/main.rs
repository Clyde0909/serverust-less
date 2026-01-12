//! Serverust-Less - AWS Lambda-like serverless platform for Python REPL execution

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

use serverust_less::api::{create_router, AppState};
use serverust_less::config::AppConfig;
use serverust_less::db::{
    init_pool, run_migrations, AuditRepository, ExecutionLogRepository, ExecutionRepository,
    JobRepository, PackageRepository, QueueRepository, VenvRepository,
};
use serverust_less::models::{ExecutionStatus, LogType};
use serverust_less::services::{
    AuditService, ExecutionService, JobService, PackageService, QueueService, VenvService,
};
use serverust_less::worker::PythonRunner;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging with environment filter
    // Use RUST_LOG env var to control log levels (e.g., RUST_LOG=debug or RUST_LOG=serverust_less=trace)
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

    info!("Starting Serverust-Less...");
    debug!("Debug logging enabled");

    // Load configuration
    let config = AppConfig::load()?;
    info!("Configuration loaded");
    debug!(
        host = %config.server.host,
        port = config.server.port,
        db_path = %config.database.path.display(),
        "Server configuration"
    );

    // Ensure database directory exists
    if let Some(parent) = config.database.path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
            info!("Created database directory: {}", parent.display());
        }
    }

    // Ensure venv directories exist
    if let Some(parent) = config.packages.main_venv_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
            info!("Created venv directory: {}", parent.display());
        }
    }

    // Auto-create main venv if it doesn't exist
    let main_venv_python = if cfg!(windows) {
        config.packages.main_venv_path.join("Scripts").join("python.exe")
    } else {
        config.packages.main_venv_path.join("bin").join("python")
    };

    if !main_venv_python.exists() {
        info!("Main venv not found, creating at: {}", config.packages.main_venv_path.display());
        let venv_result = std::process::Command::new("python3")
            .args(["-m", "venv", config.packages.main_venv_path.to_str().unwrap()])
            .output();
        
        match venv_result {
            Ok(output) if output.status.success() => {
                info!("Main venv created successfully");
            }
            Ok(output) => {
                warn!(
                    "Failed to create main venv: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                warn!("Failed to execute python3 -m venv: {}", e);
            }
        }
    } else {
        debug!("Main venv already exists at: {}", config.packages.main_venv_path.display());
    }

    // Initialize database
    let db_url = format!("sqlite:{}?mode=rwc", config.database.path.display());
    let pool = init_pool(&db_url).await?;
    info!("Database connection established");

    // Run migrations
    run_migrations(&pool).await?;
    info!("Database migrations completed");

    // Initialize repositories
    let job_repo = JobRepository::new(pool.clone());
    let execution_repo = ExecutionRepository::new(pool.clone());
    let execution_log_repo = ExecutionLogRepository::new(pool.clone());
    let package_repo = PackageRepository::new(pool.clone());
    let venv_repo = VenvRepository::new(pool.clone());
    let queue_repo = QueueRepository::new(pool.clone());
    let audit_repo = AuditRepository::new(pool.clone());

    // Initialize services
    let job_service = JobService::new(job_repo.clone());
    let execution_service = ExecutionService::new(
        execution_repo.clone(),
        execution_log_repo.clone(),
        job_repo.clone(),
    );
    let package_service = PackageService::new(package_repo.clone());
    let venv_service = VenvService::new(venv_repo.clone());
    let queue_service = QueueService::new(queue_repo.clone());
    let audit_service = AuditService::new(audit_repo.clone(), config.security.enable_audit_log);

    // Create app state
    let state = AppState {
        job_service,
        execution_service: execution_service.clone(),
        package_service,
        venv_service,
        queue_service,
        audit_service,
    };

    // Create router
    let app = create_router(state);

    // Start background worker for processing executions
    let worker_exec_repo = execution_repo.clone();
    let worker_log_repo = execution_log_repo.clone();
    let worker_job_repo = job_repo.clone();
    let main_venv_path = config.packages.main_venv_path.clone();
    let default_timeout = config.worker.default_timeout_seconds;
    let default_memory_limit = config.worker.default_memory_limit_mb;
    
    tokio::spawn(async move {
        let runner = PythonRunner::new("python3");
        
        info!("Background worker started");
        
        loop {
            // Poll for pending executions
            match worker_exec_repo.get_pending().await {
                Ok(Some(mut execution)) => {
                    info!(execution_id = %execution.id, "Processing execution");
                    
                    // Get the job details
                    let job = match worker_job_repo.get_by_id(&execution.job_id).await {
                        Ok(job) => job,
                        Err(e) => {
                            error!(execution_id = %execution.id, error = %e, "Failed to get job");
                            let _ = worker_exec_repo.update_status(&execution.id, ExecutionStatus::Failed, Some(format!("Job not found: {}", e))).await;
                            continue;
                        }
                    };
                    
                    // Update status to running
                    execution.status = ExecutionStatus::Running.as_str().to_string();
                    execution.started_at = Some(chrono::Utc::now().to_rfc3339());
                    execution.worker_id = Some("worker-1".to_string());
                    if let Err(e) = worker_exec_repo.update(&execution).await {
                        error!(execution_id = %execution.id, error = %e, "Failed to update execution status");
                        continue;
                    }
                    
                    // Log execution start
                    let _ = worker_log_repo.create_with_type(&execution.id, LogType::System, "Execution started").await;
                    
                    // Execute the Python code
                    let timeout_secs = job.timeout_seconds as u64;
                    let memory_mb = job.memory_limit_mb as u64;
                    
                    let result = runner.execute(
                        &main_venv_path,
                        &job.python_code,
                        execution.input_data.as_deref(),
                        timeout_secs,
                        memory_mb,
                    ).await;
                    
                    // Log stdout/stderr
                    if !result.stdout.is_empty() {
                        let _ = worker_log_repo.create_with_type(&execution.id, LogType::Stdout, &result.stdout).await;
                    }
                    if !result.stderr.is_empty() {
                        let _ = worker_log_repo.create_with_type(&execution.id, LogType::Stderr, &result.stderr).await;
                    }
                    
                    // Update execution with result
                    let (status, error_msg) = if result.timed_out {
                        (ExecutionStatus::Timeout, Some("Execution timed out".to_string()))
                    } else if result.memory_exceeded {
                        (ExecutionStatus::Failed, Some("Memory limit exceeded".to_string()))
                    } else if result.success {
                        (ExecutionStatus::Success, None)
                    } else {
                        (ExecutionStatus::Failed, Some(result.stderr.clone()))
                    };
                    
                    execution.status = status.as_str().to_string();
                    execution.completed_at = Some(chrono::Utc::now().to_rfc3339());
                    execution.duration_ms = Some(result.duration_ms as i64);
                    execution.output_data = if result.success { Some(result.stdout) } else { None };
                    execution.error_message = error_msg;
                    
                    if let Err(e) = worker_exec_repo.update(&execution).await {
                        error!(execution_id = %execution.id, error = %e, "Failed to update execution result");
                    }
                    
                    info!(
                        execution_id = %execution.id,
                        status = ?status,
                        duration_ms = result.duration_ms,
                        "Execution completed"
                    );
                    
                    let _ = worker_log_repo.create_with_type(&execution.id, LogType::System, &format!("Execution completed: {:?}", status)).await;
                }
                Ok(None) => {
                    // No pending executions, sleep
                    tokio::time::sleep(Duration::from_millis(2000)).await;
                }
                Err(e) => {
                    error!(error = %e, "Failed to get pending executions");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });

    // Start server
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .expect("Invalid address");

    info!("Server listening on http://{}", addr);
    info!("Web UI available at http://{}/", addr);
    info!("Swagger UI available at http://{}/swagger-ui/", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
