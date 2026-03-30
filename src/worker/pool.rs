//! Worker pool - manages concurrent job execution.
//!
//! Workers dequeue items from [`QueueManager`], execute them via [`JobExecutor`],
//! update the database directly, and register processes with [`ProcessManager`]
//! so that in-flight executions can be cancelled.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::db::{ExecutionLogRepository, ExecutionRepository, JobRepository};
use crate::error::AppError;
use crate::models::{ExecutionStatus, LogType};
use crate::queue::QueueManager;
use crate::worker::executor::JobExecutor;
use crate::worker::process_manager::ProcessManager;
use crate::worker::python_runner::PythonRunner;
use crate::dag::DagEngine;

/// Result reported by each worker after completing an execution.
#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub execution_id: String,
    pub job_id: String,
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub memory_exceeded: bool,
}

/// Worker pool that drives concurrent Python job execution.
pub struct WorkerPool {
    pool_size: usize,
    process_manager: Arc<ProcessManager>,
    /// Handles to the spawned worker tasks (kept alive for the pool lifetime).
    workers: Vec<tokio::task::JoinHandle<()>>,
}

impl WorkerPool {
    /// Spawn `pool_size` worker tasks and return the pool together with a
    /// receiver that emits [`WorkerResult`] notifications after each execution.
    pub fn new(
        pool_size: usize,
        main_venv_path: PathBuf,
        custom_venv_base_path: PathBuf,
        python_executable: &str,
        queue_manager: Arc<QueueManager>,
        process_manager: Arc<ProcessManager>,
        execution_repo: ExecutionRepository,
        log_repo: ExecutionLogRepository,
        job_repo: JobRepository,
        dag_engine: Option<Arc<DagEngine>>,
    ) -> (Self, mpsc::Receiver<WorkerResult>) {
        // Channel sized to accommodate all workers producing results simultaneously.
        let (result_tx, result_rx) = mpsc::channel::<WorkerResult>(pool_size * 4);
        let runner = Arc::new(PythonRunner::new(python_executable));
        let mut workers = Vec::with_capacity(pool_size);

        for worker_id in 0..pool_size {
            let qm = queue_manager.clone();
            let pm = process_manager.clone();
            let exec_repo = execution_repo.clone();
            let log_repo = log_repo.clone();
            let job_repo_ref = job_repo.clone();
            let result_tx = result_tx.clone();
            let runner = runner.clone();
            let main_venv = main_venv_path.clone();
            let custom_venv_base = custom_venv_base_path.clone();
            let dag_engine = dag_engine.clone();

            let handle = tokio::spawn(async move {
                info!("Worker {} started", worker_id);
                let executor = JobExecutor::new(runner, main_venv, custom_venv_base);

                loop {
                    // --- Dequeue ---
                    let item = match qm.dequeue().await {
                        Ok(Some(item)) => item,
                        Ok(None) => {
                            // No work available; back off briefly before polling again.
                            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                            continue;
                        }
                        Err(e) => {
                            error!("Worker {} dequeue error: {}", worker_id, e);
                            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                            continue;
                        }
                    };

                    let exec_id = item.execution_id.clone();
                    let job_id = item.job_id.clone();
                    info!("Worker {} picked up execution {}", worker_id, exec_id);

                    // --- Mark execution as running ---
                    if let Err(e) = set_execution_running(
                        &exec_repo,
                        &exec_id,
                        &format!("worker-{}", worker_id),
                    )
                    .await
                    {
                        error!(
                            "Worker {} failed to mark execution {} running: {}",
                            worker_id, exec_id, e
                        );
                        let _ = qm.mark_failed(&exec_id).await;
                        continue;
                    }

                    let _ = log_repo
                        .create_with_type(&exec_id, LogType::System, "Execution started")
                        .await;

                    // --- Register with ProcessManager; obtain cancel signal receiver ---
                    let reg = pm.register(&exec_id, &job_id, None, worker_id).await;

                    // --- PID tracking: report PID to ProcessManager right after spawn ---
                    // This allows process_manager.cancel() to send SIGTERM/SIGKILL while running.
                    let (pid_tx, pid_rx) = tokio::sync::oneshot::channel::<u32>();
                    let pm_pid = pm.clone();
                    let exec_id_pid = exec_id.clone();
                    tokio::spawn(async move {
                        if let Ok(pid) = pid_rx.await {
                            pm_pid.update_pid(&exec_id_pid, pid).await;
                        }
                    });

                    // --- Execute with cancellation via select! ---
                    // If cancel_rx fires (API cancel handler) the executor future is dropped,
                    // which kills the child process because kill_on_drop(true) is set.
                    let (result, cancelled_externally) = tokio::select! {
                        r = executor.execute_with_pid(&item, pid_tx) => (r, false),
                        _ = reg.cancel_rx => {
                            info!(
                                "Worker {} received cancel signal for execution {}",
                                worker_id, exec_id
                            );
                            (crate::worker::python_runner::ExecutionResult {
                                success: false,
                                stdout: String::new(),
                                stderr: "Execution was cancelled".to_string(),
                                exit_code: None,
                                duration_ms: 0,
                                timed_out: false,
                                memory_exceeded: false,
                            }, true)
                        }
                    };
                    let _ = cancelled_externally; // used implicitly via DB status check below

                    // --- Unregister ---
                    pm.unregister(&exec_id).await;

                    // --- Persist logs ---
                    if !result.stdout.is_empty() {
                        let _ = log_repo
                            .create_with_type(&exec_id, LogType::Stdout, &result.stdout)
                            .await;
                    }
                    if !result.stderr.is_empty() {
                        let _ = log_repo
                            .create_with_type(&exec_id, LogType::Stderr, &result.stderr)
                            .await;
                    }

                    // --- Determine final status ---
                    let (status, error_msg) = if result.timed_out {
                        (ExecutionStatus::Timeout, Some("Execution timed out".to_string()))
                    } else if result.memory_exceeded {
                        (ExecutionStatus::Failed, Some("Memory limit exceeded".to_string()))
                    } else if result.success {
                        (ExecutionStatus::Success, None)
                    } else {
                        let err = if result.stderr.is_empty() {
                            format!("Exit code: {:?}", result.exit_code)
                        } else {
                            result.stderr.clone()
                        };
                        (ExecutionStatus::Failed, Some(err))
                    };

                    // --- Update execution in DB (respect pre-existing cancellation) ---
                    match exec_repo.get_by_id(&exec_id).await {
                        Ok(current)
                            if current.status == ExecutionStatus::Cancelled.as_str() =>
                        {
                            debug!(
                                "Execution {} was cancelled externally; skipping result write",
                                exec_id
                            );
                        }
                        _ => {
                            if let Err(e) = finalize_execution(
                                &exec_repo,
                                &exec_id,
                                status.clone(),
                                error_msg.clone(),
                                if result.success {
                                    Some(result.stdout.clone())
                                } else {
                                    None
                                },
                                result.duration_ms,
                            )
                            .await
                            {
                                error!(
                                    "Worker {} failed to finalize execution {}: {}",
                                    worker_id, exec_id, e
                                );
                            }
                        }
                    }

                    // --- Update queue entry ---
                    // For failures, use retry-with-delay logic:
                    // If retries remain, re-queue after a delay; otherwise move to DLQ.
                    let queue_result =
                        if matches!(status, ExecutionStatus::Success) {
                            qm.mark_completed(&exec_id).await
                        } else {
                            // Get the execution's retry count to decide retry vs DLQ
                            let retry_count = match exec_repo.get_by_id(&exec_id).await {
                                Ok(exec) => exec.retry_count,
                                Err(_) => 0,
                            };
                            // Get the job's max_retries setting
                            let max_retries = match exec_repo.get_by_id(&exec_id).await {
                                Ok(exec) => {
                                    match job_repo_ref.get_by_id(&exec.job_id).await {
                                        Ok(job) => Some(job.max_retries),
                                        Err(_) => None,
                                    }
                                }
                                Err(_) => None,
                            };
                            qm.mark_failed_with_retry(&exec_id, retry_count, max_retries)
                                .await
                                .map(|_| ())
                        };
                    if let Err(e) = queue_result {
                        warn!(
                            "Worker {} failed to update queue entry for {}: {}",
                            worker_id, exec_id, e
                        );
                    }

                    // DAG engine callback: advance DAG run if this is a DAG node
                    if let Some(ref engine) = dag_engine {
                        if let Err(e) = engine.on_execution_complete(&exec_id).await {
                            warn!(
                                "Worker {} DAG engine callback error for {}: {}",
                                worker_id, exec_id, e
                            );
                        }
                    }

                    let _ = log_repo
                        .create_with_type(
                            &exec_id,
                            LogType::System,
                            &format!("Execution completed: {}", status.as_str()),
                        )
                        .await;

                    // --- Emit result notification ---
                    let worker_result = WorkerResult {
                        execution_id: exec_id,
                        job_id,
                        success: result.success,
                        output: if result.success {
                            Some(result.stdout)
                        } else {
                            None
                        },
                        error: error_msg,
                        duration_ms: result.duration_ms,
                        timed_out: result.timed_out,
                        memory_exceeded: result.memory_exceeded,
                    };
                    if result_tx.send(worker_result).await.is_err() {
                        // Receiver dropped; all workers can safely stop.
                        debug!("Worker {} result channel closed, stopping", worker_id);
                        break;
                    }
                }
            });

            workers.push(handle);
        }

        let pool = Self {
            pool_size,
            process_manager,
            workers,
        };

        (pool, result_rx)
    }

    /// Cancel a running execution by signalling its process.
    pub async fn cancel(&self, execution_id: &str) -> Result<bool, String> {
        self.process_manager.cancel(execution_id).await
    }

    /// Expose the underlying `ProcessManager` (e.g. for API cancel handlers).
    pub fn process_manager(&self) -> Arc<ProcessManager> {
        self.process_manager.clone()
    }

    /// Number of executions currently tracked as running.
    pub async fn running_count(&self) -> usize {
        self.process_manager.running_count().await
    }

    /// Abort all worker tasks and cancel in-flight processes.
    pub async fn shutdown(&self) {
        info!("Shutting down worker pool ({} workers)…", self.pool_size);
        self.process_manager.cancel_all().await;
        for handle in &self.workers {
            handle.abort();
        }
    }

    pub fn pool_size(&self) -> usize {
        self.pool_size
    }
}

// ---------------------------------------------------------------------------
// Internal helpers used inside worker tasks
// ---------------------------------------------------------------------------

/// Set an execution's status to `Running` and record the assigned worker.
async fn set_execution_running(
    exec_repo: &ExecutionRepository,
    exec_id: &str,
    worker_id: &str,
) -> Result<(), AppError> {
    let mut execution = exec_repo.get_by_id(exec_id).await?;
    execution.status = ExecutionStatus::Running.as_str().to_string();
    execution.started_at = Some(chrono::Utc::now().to_rfc3339());
    execution.worker_id = Some(worker_id.to_string());
    exec_repo.update(&execution).await?;
    Ok(())
}

/// Persist the final outcome of an execution.
async fn finalize_execution(
    exec_repo: &ExecutionRepository,
    exec_id: &str,
    status: ExecutionStatus,
    error_msg: Option<String>,
    output: Option<String>,
    duration_ms: u64,
) -> Result<(), AppError> {
    let mut execution = exec_repo.get_by_id(exec_id).await?;
    execution.status = status.as_str().to_string();
    execution.completed_at = Some(chrono::Utc::now().to_rfc3339());
    execution.duration_ms = Some(duration_ms as i64);
    execution.output_data = output;
    execution.error_message = error_msg;
    exec_repo.update(&execution).await?;
    Ok(())
}
