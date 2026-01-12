//! Queue manager - coordinates in-memory and persistent queues

use std::collections::BinaryHeap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::db::QueueRepository;
use crate::error::AppError;
use crate::models::{QueueEntry, QueueItem, QueueStatus};

/// Queue manager coordinating in-memory and SQLite queues
pub struct QueueManager {
    /// In-memory priority queue
    memory_queue: Arc<Mutex<BinaryHeap<QueueItem>>>,
    /// Maximum in-memory queue size
    max_memory_size: usize,
    /// Persistent queue repository
    repo: QueueRepository,
}

impl QueueManager {
    /// Create a new QueueManager
    pub fn new(repo: QueueRepository, max_memory_size: usize) -> Self {
        Self {
            memory_queue: Arc::new(Mutex::new(BinaryHeap::new())),
            max_memory_size,
            repo,
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

    /// Dequeue the next item
    pub async fn dequeue(&self) -> Result<Option<QueueItem>, AppError> {
        let mut queue = self.memory_queue.lock().await;

        if let Some(item) = queue.pop() {
            // Update persistent entry status
            if let Some(mut entry) = self.repo.get_by_execution(&item.execution_id).await? {
                entry.mark_processing();
                self.repo.update(&entry).await?;
            }
            return Ok(Some(item));
        }

        // Check overflow queue
        if let Some(entry) = self.repo.dequeue().await? {
            // Need to reconstruct the QueueItem from the entry
            // This requires additional data that should be stored
            // For now, return None as we need the job details
            debug!("Found item in overflow queue: {}", entry.execution_id);
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

    /// Recover queue from database on startup
    pub async fn recover(&self) -> Result<usize, AppError> {
        let entries = self.repo.get_all_queued().await?;
        info!("Recovering {} queue entries from database", entries.len());

        // Note: We can't fully reconstruct QueueItems without job details
        // This would need to be enhanced in a real implementation
        Ok(entries.len())
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
