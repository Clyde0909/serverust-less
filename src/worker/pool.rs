//! Worker pool - manages concurrent job execution

use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::models::{Execution, ExecutionLog, ExecutionStatus, LogType, QueueItem};
use crate::worker::executor::JobExecutor;
use crate::worker::python_runner::PythonRunner;

/// Message types for the worker pool
#[derive(Debug)]
pub enum WorkerMessage {
    /// Execute a job
    Execute(QueueItem),
    /// Cancel an execution
    Cancel(String),
    /// Shutdown the worker pool
    Shutdown,
}

/// Result from worker execution
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

/// Worker pool for concurrent job execution
pub struct WorkerPool {
    /// Pool size
    pool_size: usize,
    /// Sender for work items
    sender: mpsc::Sender<WorkerMessage>,
    /// Receiver for results
    result_receiver: Arc<Mutex<mpsc::Receiver<WorkerResult>>>,
    /// In-memory priority queue
    queue: Arc<Mutex<BinaryHeap<QueueItem>>>,
    /// Maximum in-memory queue size
    max_queue_size: usize,
    /// Currently running executions
    running: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// Worker handles
    workers: Vec<tokio::task::JoinHandle<()>>,
}

impl WorkerPool {
    /// Create a new worker pool
    pub fn new(
        pool_size: usize,
        main_venv_path: PathBuf,
        custom_venv_base_path: PathBuf,
        max_queue_size: usize,
        python_executable: &str,
    ) -> (Self, mpsc::Receiver<WorkerResult>) {
        let (work_sender, work_receiver) = mpsc::channel::<WorkerMessage>(max_queue_size);
        let (result_sender, result_receiver) = mpsc::channel::<WorkerResult>(max_queue_size);

        let work_receiver = Arc::new(Mutex::new(work_receiver));
        let runner = Arc::new(PythonRunner::new(python_executable));

        let mut workers = Vec::with_capacity(pool_size);

        // Spawn worker tasks
        for worker_id in 0..pool_size {
            let receiver = work_receiver.clone();
            let sender = result_sender.clone();
            let executor = JobExecutor::new(
                runner.clone(),
                main_venv_path.clone(),
                custom_venv_base_path.clone(),
            );

            let handle = tokio::spawn(async move {
                loop {
                    let message = {
                        let mut rx = receiver.lock().await;
                        rx.recv().await
                    };

                    match message {
                        Some(WorkerMessage::Execute(item)) => {
                            debug!("Worker {} executing: {}", worker_id, item.execution_id);

                            let result = executor.execute(&item).await;

                            let worker_result = WorkerResult {
                                execution_id: item.execution_id.clone(),
                                job_id: item.job_id.clone(),
                                success: result.success,
                                output: if result.success {
                                    Some(result.stdout)
                                } else {
                                    None
                                },
                                error: if !result.success {
                                    Some(if result.memory_exceeded {
                                        "Memory limit exceeded".to_string()
                                    } else if result.stderr.is_empty() {
                                        format!("Exit code: {:?}", result.exit_code)
                                    } else {
                                        result.stderr
                                    })
                                } else {
                                    None
                                },
                                duration_ms: result.duration_ms,
                                timed_out: result.timed_out,
                                memory_exceeded: result.memory_exceeded,
                            };

                            if sender.send(worker_result).await.is_err() {
                                error!("Failed to send worker result");
                            }
                        }
                        Some(WorkerMessage::Cancel(_execution_id)) => {
                            // Cancellation is handled by dropping the task
                            debug!("Worker {} received cancel request", worker_id);
                        }
                        Some(WorkerMessage::Shutdown) => {
                            info!("Worker {} shutting down", worker_id);
                            break;
                        }
                        None => {
                            debug!("Worker {} channel closed", worker_id);
                            break;
                        }
                    }
                }
            });

            workers.push(handle);
        }

        let pool = Self {
            pool_size,
            sender: work_sender,
            result_receiver: Arc::new(Mutex::new(result_receiver)),
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
            max_queue_size,
            running: Arc::new(RwLock::new(HashMap::new())),
            workers,
        };

        // Create a new receiver for external use
        let (_, external_receiver) = mpsc::channel(1);

        (pool, external_receiver)
    }

    /// Submit a job for execution
    pub async fn submit(&self, item: QueueItem) -> Result<(), String> {
        // Check queue capacity
        let queue_size = {
            let queue = self.queue.lock().await;
            queue.len()
        };

        if queue_size >= self.max_queue_size {
            return Err("Queue is full".to_string());
        }

        // Send to worker
        self.sender
            .send(WorkerMessage::Execute(item))
            .await
            .map_err(|e| format!("Failed to submit job: {}", e))?;

        Ok(())
    }

    /// Cancel an execution
    pub async fn cancel(&self, execution_id: &str) -> Result<(), String> {
        self.sender
            .send(WorkerMessage::Cancel(execution_id.to_string()))
            .await
            .map_err(|e| format!("Failed to cancel: {}", e))?;

        Ok(())
    }

    /// Get the number of items in the in-memory queue
    pub async fn queue_size(&self) -> usize {
        let queue = self.queue.lock().await;
        queue.len()
    }

    /// Get the number of running executions
    pub async fn running_count(&self) -> usize {
        let running = self.running.read().await;
        running.len()
    }

    /// Shutdown the worker pool
    pub async fn shutdown(&self) {
        info!("Shutting down worker pool");

        // Send shutdown to all workers
        for _ in 0..self.pool_size {
            let _ = self.sender.send(WorkerMessage::Shutdown).await;
        }
    }

    /// Get pool size
    pub fn pool_size(&self) -> usize {
        self.pool_size
    }
}
