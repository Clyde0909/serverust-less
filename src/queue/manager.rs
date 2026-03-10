//! Queue manager - coordinates in-memory and persistent queues

use std::collections::BinaryHeap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::db::{ExecutionRepository, JobRepository, QueueRepository};
use crate::error::AppError;
use crate::models::{QueueEntry, QueueItem};

/// Queue manager coordinating in-memory and SQLite queues
pub struct QueueManager {
    /// In-memory priority queue
    memory_queue: Arc<Mutex<BinaryHeap<QueueItem>>>,
    /// Maximum in-memory queue size
    max_memory_size: usize,
    /// Persistent queue repository
    repo: QueueRepository,
    /// Execution repository (for overflow item reconstruction)
    execution_repo: ExecutionRepository,
    /// Job repository (for overflow item reconstruction)
    job_repo: JobRepository,
    /// Maximum retries before moving to dead letter queue
    max_retries: i32,
    /// Delay in seconds before re-queuing a failed item
    retry_delay_seconds: u64,
}

impl QueueManager {
    /// Create a new QueueManager
    pub fn new(
        repo: QueueRepository,
        execution_repo: ExecutionRepository,
        job_repo: JobRepository,
        max_memory_size: usize,
    ) -> Self {
        Self {
            memory_queue: Arc::new(Mutex::new(BinaryHeap::new())),
            max_memory_size,
            repo,
            execution_repo,
            job_repo,
            max_retries: 3,
            retry_delay_seconds: 5,
        }
    }

    /// Create a new QueueManager with retry/DLQ configuration
    pub fn with_config(
        repo: QueueRepository,
        execution_repo: ExecutionRepository,
        job_repo: JobRepository,
        max_memory_size: usize,
        max_retries: i32,
        retry_delay_seconds: u64,
    ) -> Self {
        Self {
            memory_queue: Arc::new(Mutex::new(BinaryHeap::new())),
            max_memory_size,
            repo,
            execution_repo,
            job_repo,
            max_retries,
            retry_delay_seconds,
        }
    }

    /// Reconstruct a QueueItem from a QueueEntry by fetching execution + job from DB
    async fn reconstruct_item(&self, entry: &QueueEntry) -> Option<QueueItem> {
        match self.execution_repo.get_by_id(&entry.execution_id).await {
            Ok(exec) => match self.job_repo.get_by_id(&exec.job_id).await {
                Ok(job) => Some(QueueItem::new(
                    &exec.id,
                    &job.id,
                    entry.priority,
                    &job.python_code,
                    job.timeout_seconds,
                    job.memory_limit_mb,
                    exec.input_data.clone(),
                    job.use_custom_venv,
                )),
                Err(e) => {
                    warn!("Failed to get job {} for queue reconstruction: {}", exec.job_id, e);
                    None
                }
            },
            Err(e) => {
                warn!("Failed to get execution {} for queue reconstruction: {}", entry.execution_id, e);
                None
            }
        }
    }

    /// Enqueue an item
    pub async fn enqueue(&self, item: QueueItem) -> Result<(), AppError> {
        let mut queue = self.memory_queue.lock().await;

        // Create persistent entry first
        let entry = QueueEntry::new(&item.execution_id, &item.job_id, item.priority);
        self.repo.enqueue(&entry).await?;

        // Add to in-memory queue if space available
        if queue.len() < self.max_memory_size {
            queue.push(item);
            debug!("Item added to in-memory queue");
        } else {
            // Overflow to SQLite (already persisted above)
            debug!("Item added to overflow queue");
        }

        Ok(())
    }

    /// Dequeue the highest-priority item.
    /// Falls back to SQLite overflow when the in-memory queue is empty.
    /// The overflow path uses an atomic UPDATE … RETURNING to prevent double-dequeue.
    pub async fn dequeue(&self) -> Result<Option<QueueItem>, AppError> {
        let mut queue = self.memory_queue.lock().await;

        // Fast path: in-memory queue (Mutex guarantees single consumer)
        if let Some(item) = queue.pop() {
            if let Some(mut entry) = self.repo.get_by_execution(&item.execution_id).await? {
                entry.mark_processing();
                self.repo.update(&entry).await?;
            }
            return Ok(Some(item));
        }

        // Slow path: SQLite overflow — atomic dequeue (SELECT + UPDATE in one statement)
        if let Some(entry) = self.repo.dequeue_atomic().await? {
            debug!("Dequeuing overflow item: execution={}", entry.execution_id);
            if let Some(item) = self.reconstruct_item(&entry).await {
                return Ok(Some(item));
            } else {
                // Cannot reconstruct — mark failed so it doesn't permanently block the queue
                warn!(
                    "Could not reconstruct QueueItem for execution {}, marking failed",
                    entry.execution_id
                );
                let mut failed_entry = entry;
                failed_entry.mark_failed();
                self.repo.update(&failed_entry).await?;
            }
        }

        Ok(None)
    }

    /// Remove an item from the queue
    pub async fn remove(&self, execution_id: &str) -> Result<(), AppError> {
        // Remove from memory queue
        let mut queue = self.memory_queue.lock().await;
        let items: Vec<_> = queue.drain().filter(|i| i.execution_id != execution_id).collect();
        for item in items {
            queue.push(item);
        }

        // Remove from persistent queue
        self.repo.remove_by_execution(execution_id).await?;

        Ok(())
    }

    /// Mark execution as completed
    pub async fn mark_completed(&self, execution_id: &str) -> Result<(), AppError> {
        if let Some(mut entry) = self.repo.get_by_execution(execution_id).await? {
            entry.mark_completed();
            self.repo.update(&entry).await?;
        }
        Ok(())
    }

    /// Mark execution as failed
    pub async fn mark_failed(&self, execution_id: &str) -> Result<(), AppError> {
        if let Some(mut entry) = self.repo.get_by_execution(execution_id).await? {
            entry.mark_failed();
            self.repo.update(&entry).await?;
        }
        Ok(())
    }

    /// Get in-memory queue size
    pub async fn memory_queue_size(&self) -> usize {
        let queue = self.memory_queue.lock().await;
        queue.len()
    }

    /// Get overflow queue size
    pub async fn overflow_queue_size(&self) -> Result<i64, AppError> {
        let total = self.repo.count_queued().await?;
        let memory_size = self.memory_queue_size().await as i64;
        Ok(total.saturating_sub(memory_size))
    }

    /// Cleanup old entries
    pub async fn cleanup(&self, older_than_hours: i32) -> Result<u64, AppError> {
        self.repo.cleanup_old(older_than_hours).await
    }

    /// Mark execution as failed and decide whether to retry or move to dead letter queue.
    /// Returns `true` if the item was re-queued for retry, `false` if moved to DLQ or finalized.
    pub async fn mark_failed_with_retry(
        &self,
        execution_id: &str,
        retry_count: i32,
        max_retries_override: Option<i32>,
    ) -> Result<bool, AppError> {
        let max_retries = max_retries_override.unwrap_or(self.max_retries);

        if retry_count < max_retries {
            // Re-queue with delay
            let delay_secs = self.retry_delay_seconds;
            info!(
                "Re-queuing execution {} for retry (attempt {}/{}) after {}s delay",
                execution_id,
                retry_count + 1,
                max_retries,
                delay_secs
            );

            if let Some(entry) = self.repo.get_by_execution(execution_id).await? {
                // Schedule re-queue after delay
                let repo = self.repo.clone();
                let entry_id = entry.id.clone();
                let memory_queue = self.memory_queue.clone();
                let max_mem = self.max_memory_size;
                let exec_repo = self.execution_repo.clone();
                let job_repo = self.job_repo.clone();
                let exec_id = execution_id.to_string();

                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                    debug!("Retry delay elapsed for execution {}, re-queuing", exec_id);

                    // Reset queue entry back to queued
                    if let Err(e) = repo.requeue(&entry_id).await {
                        warn!("Failed to re-queue entry {}: {}", entry_id, e);
                        return;
                    }

                    // Try to reconstruct and add to in-memory queue
                    let item = match exec_repo.get_by_id(&exec_id).await {
                        Ok(exec) => match job_repo.get_by_id(&exec.job_id).await {
                            Ok(job) => Some(QueueItem::new(
                                &exec.id,
                                &job.id,
                                job.priority,
                                &job.python_code,
                                job.timeout_seconds,
                                job.memory_limit_mb,
                                exec.input_data.clone(),
                                job.use_custom_venv,
                            )),
                            Err(_) => None,
                        },
                        Err(_) => None,
                    };

                    if let Some(item) = item {
                        let mut queue = memory_queue.lock().await;
                        if queue.len() < max_mem {
                            queue.push(item);
                            debug!("Re-queued execution {} to in-memory queue", exec_id);
                        }
                        // Otherwise it stays in SQLite overflow and will be picked up by dequeue
                    }
                });

                return Ok(true);
            }
        }

        // Max retries exhausted — move to dead letter queue
        self.move_to_dead_letter(execution_id).await?;
        Ok(false)
    }

    /// Move an execution to the dead letter queue
    pub async fn move_to_dead_letter(&self, execution_id: &str) -> Result<(), AppError> {
        if let Some(mut entry) = self.repo.get_by_execution(execution_id).await? {
            warn!(
                "Moving execution {} to dead letter queue (job: {})",
                execution_id, entry.job_id
            );
            entry.mark_dead_letter();
            self.repo.update(&entry).await?;
        }
        Ok(())
    }

    /// Get number of items in dead letter queue
    pub async fn dead_letter_count(&self) -> Result<i64, AppError> {
        self.repo.count_dead_letter().await
    }

    /// Get all dead letter queue entries
    pub async fn get_dead_letter_entries(&self) -> Result<Vec<QueueEntry>, AppError> {
        self.repo.get_dead_letter_entries().await
    }

    /// Retry a dead-lettered item by moving it back to the queue
    pub async fn retry_dead_letter(&self, execution_id: &str) -> Result<(), AppError> {
        if let Some(entry) = self.repo.get_by_execution(execution_id).await? {
            if entry.status != "dead_letter" {
                return Err(AppError::Validation(
                    "Entry is not in dead letter queue".to_string(),
                ));
            }
            self.repo.requeue(&entry.id).await?;

            // Try to add to in-memory queue
            if let Some(item) = self.reconstruct_item(&entry).await {
                let mut queue = self.memory_queue.lock().await;
                if queue.len() < self.max_memory_size {
                    queue.push(item);
                }
            }

            info!("Dead-lettered execution {} re-queued for retry", execution_id);
        }
        Ok(())
    }

    /// Recover queue state from DB after a restart.
    /// First resets any items stuck in "processing" state (interrupted by crash),
    /// then loads queued items (up to max_memory_size) back into the in-memory heap.
    pub async fn recover(&self) -> Result<usize, AppError> {
        // Reset items that were "processing" when the previous instance crashed
        let reset_count = self.repo.reset_processing_to_queued().await?;
        if reset_count > 0 {
            info!("Reset {} stuck 'processing' items back to 'queued'", reset_count);
        }

        let entries = self.repo.get_all_queued().await?;
        let total = entries.len();
        info!("Recovering {} queued entries from database", total);

        let mut queue = self.memory_queue.lock().await;
        let mut recovered = 0usize;

        for entry in &entries {
            if queue.len() >= self.max_memory_size {
                break;
            }
            if let Some(item) = self.reconstruct_item(entry).await {
                queue.push(item);
                recovered += 1;
            }
        }

        let overflow = total.saturating_sub(recovered);
        info!(
            "Queue recovered: {} in-memory, {} remain in SQLite overflow",
            recovered, overflow
        );
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_item_ordering() {
        // Higher priority should come first in BinaryHeap
        let item_high = QueueItem::new(
            "exec-1",
            "job-1",
            10, // high priority
            "print(1)",
            30,
            128,
            None,
            false,
        );

        let item_low = QueueItem::new(
            "exec-2",
            "job-2",
            1, // low priority
            "print(2)",
            30,
            128,
            None,
            false,
        );

        // Higher priority items are "greater" and pop first from max-heap
        assert!(item_high > item_low);
        
        // Verify the heap behavior works correctly
        let mut heap = std::collections::BinaryHeap::new();
        heap.push(item_low.clone());
        heap.push(item_high.clone());
        
        // Higher priority should come out first
        let first = heap.pop().unwrap();
        assert_eq!(first.priority, 10);
        let second = heap.pop().unwrap();
        assert_eq!(second.priority, 1);
    }

    #[test]
    fn test_queue_item_equality() {
        let item1 = QueueItem::new(
            "exec-1",
            "job-1",
            5,
            "print(1)",
            30,
            128,
            None,
            false,
        );

        let item2 = QueueItem::new(
            "exec-1", // Same execution_id
            "job-2",  // Different job_id
            10,       // Different priority
            "print(2)",
            60,
            256,
            Some("{}".to_string()),
            true,
        );

        // Items are equal if execution_id matches
        assert_eq!(item1, item2);
    }

    #[test]
    fn test_queue_entry_status_transitions() {
        let mut entry = QueueEntry::new("exec-1", "job-1", 5);
        assert_eq!(entry.status, "queued");
        assert!(entry.started_at.is_none());
        assert!(entry.completed_at.is_none());

        entry.mark_processing();
        assert_eq!(entry.status, "processing");
        assert!(entry.started_at.is_some());

        entry.mark_completed();
        assert_eq!(entry.status, "completed");
        assert!(entry.completed_at.is_some());
    }

    #[test]
    fn test_queue_entry_failed() {
        let mut entry = QueueEntry::new("exec-1", "job-1", 5);
        entry.mark_processing();
        entry.mark_failed();

        assert_eq!(entry.status, "failed");
        assert!(entry.completed_at.is_some());
    }
}
