//! Queue service - business logic for job queue management

use crate::db::QueueRepository;
use crate::error::AppError;
use crate::models::{PriorityCount, QueueEntry, QueueStatus, QueueStatusResponse};

/// Service for job queue management
#[derive(Clone)]
pub struct QueueService {
    repo: QueueRepository,
}

impl QueueService {
    /// Create a new QueueService
    pub fn new(repo: QueueRepository) -> Self {
        Self { repo }
    }

    /// Enqueue an execution
    pub async fn enqueue(
        &self,
        execution_id: &str,
        job_id: &str,
        priority: i32,
    ) -> Result<QueueEntry, AppError> {
        let entry = QueueEntry::new(execution_id, job_id, priority);
        self.repo.enqueue(&entry).await
    }

    /// Dequeue the next item
    pub async fn dequeue(&self) -> Result<Option<QueueEntry>, AppError> {
        let entry = self.repo.dequeue().await?;

        if let Some(mut e) = entry {
            e.mark_processing();
            let updated = self.repo.update(&e).await?;
            return Ok(Some(updated));
        }

        Ok(None)
    }

    /// Mark queue entry as completed
    pub async fn mark_completed(&self, execution_id: &str) -> Result<(), AppError> {
        if let Some(mut entry) = self.repo.get_by_execution(execution_id).await? {
            entry.mark_completed();
            self.repo.update(&entry).await?;
        }
        Ok(())
    }

    /// Mark queue entry as failed
    pub async fn mark_failed(&self, execution_id: &str) -> Result<(), AppError> {
        if let Some(mut entry) = self.repo.get_by_execution(execution_id).await? {
            entry.mark_failed();
            self.repo.update(&entry).await?;
        }
        Ok(())
    }

    /// Remove entry from queue (for cancellation)
    pub async fn remove(&self, execution_id: &str) -> Result<(), AppError> {
        self.repo.remove_by_execution(execution_id).await
    }

    /// Get queue status
    pub async fn get_status(&self, in_memory_size: usize) -> Result<QueueStatusResponse, AppError> {
        let total_queued = self.repo.count_queued().await?;
        let processing = self.repo.count_processing().await?;
        let completed_last_hour = self.repo.count_completed_last_hour().await?;
        let failed_last_hour = self.repo.count_failed_last_hour().await?;
        let dead_letter_count = self.repo.count_dead_letter().await?;

        let depth = self.repo.get_depth_by_priority().await?;
        let by_priority: Vec<PriorityCount> = depth
            .into_iter()
            .map(|(priority, count)| PriorityCount { priority, count })
            .collect();

        // Overflow is total minus in-memory
        let overflow_size = total_queued.saturating_sub(in_memory_size as i64);

        Ok(QueueStatusResponse {
            total_queued,
            processing,
            completed_last_hour,
            failed_last_hour,
            dead_letter_count,
            by_priority,
            in_memory_size,
            overflow_size,
        })
    }

    /// Get all queued entries (for recovery)
    pub async fn get_all_queued(&self) -> Result<Vec<QueueEntry>, AppError> {
        self.repo.get_all_queued().await
    }

    /// Check if execution is in queue
    pub async fn is_queued(&self, execution_id: &str) -> Result<bool, AppError> {
        let entry = self.repo.get_by_execution(execution_id).await?;
        Ok(entry.map(|e| e.status == QueueStatus::Queued.as_str()).unwrap_or(false))
    }

    /// Clean up old completed/failed entries
    pub async fn cleanup(&self, older_than_hours: i32) -> Result<u64, AppError> {
        self.repo.cleanup_old(older_than_hours).await
    }

    /// Get queue depth
    pub async fn get_depth(&self) -> Result<i64, AppError> {
        self.repo.count_queued().await
    }
}
