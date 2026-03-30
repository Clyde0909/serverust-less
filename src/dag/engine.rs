//! DAG execution engine

use std::collections::HashMap;
use std::sync::Arc;

use tracing::{error, info, warn};

use crate::db::{DagRepository, JobRepository};
use crate::error::AppError;
use crate::models::{DagEdge, DagNodeExecution, DagRun, QueueItem};
use crate::queue::QueueManager;
use crate::services::{DagService, ExecutionService};

/// DAG execution engine - orchestrates DAG runs
pub struct DagEngine {
    dag_repo: DagRepository,
    execution_service: ExecutionService,
    queue_manager: Arc<QueueManager>,
    job_repo: JobRepository,
}

impl DagEngine {
    pub fn new(
        dag_repo: DagRepository,
        execution_service: ExecutionService,
        queue_manager: Arc<QueueManager>,
        job_repo: JobRepository,
    ) -> Self {
        Self {
            dag_repo,
            execution_service,
            queue_manager,
            job_repo,
        }
    }

    /// Trigger a new DAG run
    pub async fn trigger_dag(&self, dag_id: &str, trigger_type: &str) -> Result<DagRun, AppError> {
        let dag = self.dag_repo.get_by_id(dag_id).await?;
        if !dag.enabled {
            return Err(AppError::BadRequest(format!(
                "DAG '{}' is disabled",
                dag.name
            )));
        }

        let edges = self.dag_repo.get_edges_by_dag_id(dag_id).await?;
        if edges.is_empty() {
            return Err(AppError::BadRequest(
                "DAG has no edges — nothing to execute".to_string(),
            ));
        }

        // Validate no cycles
        DagService::validate_no_cycles(&edges)?;

        // Collect all unique job IDs
        let mut job_ids = std::collections::HashSet::new();
        for edge in &edges {
            job_ids.insert(edge.upstream_job_id.clone());
            job_ids.insert(edge.downstream_job_id.clone());
        }
        let job_ids: Vec<String> = job_ids.into_iter().collect();

        // Create DAG run
        let mut run = DagRun::new(dag_id, trigger_type, job_ids.len() as i32);
        run.status = "running".to_string();
        run.started_at = Some(chrono::Utc::now().to_rfc3339());
        let run = self.dag_repo.create_run(&run).await?;

        info!(dag_run_id = %run.id, dag_id = %dag_id, total_nodes = job_ids.len(), "DAG run started");

        // Create node executions for all jobs
        for job_id in &job_ids {
            let node = DagNodeExecution::new(&run.id, job_id);
            self.dag_repo.create_node_execution(&node).await?;
        }

        // Find root nodes (in-degree = 0) and mark them as ready
        let root_nodes = self.find_root_nodes(&edges, &job_ids);
        let nodes = self.dag_repo.get_node_executions_by_run_id(&run.id).await?;
        for node in &nodes {
            if root_nodes.contains(&node.job_id) {
                let mut updated = node.clone();
                updated.status = "ready".to_string();
                self.dag_repo.update_node_execution(&updated).await?;
            }
        }

        // Advance: start executing ready nodes
        self.advance_dag_run(&run.id, &edges, &dag).await?;

        // Return fresh run
        self.dag_repo.get_run_by_id(&run.id).await
    }

    /// Called when an execution completes — check if it's part of a DAG run
    pub async fn on_execution_complete(&self, execution_id: &str) -> Result<(), AppError> {
        // Find the node execution by execution_id
        let node = match self.dag_repo.find_node_by_execution_id(execution_id).await? {
            Some(node) => node,
            None => return Ok(()), // Not a DAG execution
        };

        // Get the actual execution status
        let execution = self.execution_service.get_execution(execution_id).await?;
        let node_status = match execution.status.as_str() {
            "success" => "success",
            "cancelled" => "cancelled",
            _ => "failed",
        };

        // Update node execution
        let mut updated_node = node.clone();
        updated_node.status = node_status.to_string();
        updated_node.completed_at = Some(chrono::Utc::now().to_rfc3339());
        self.dag_repo.update_node_execution(&updated_node).await?;

        // Update DAG run counters
        let mut run = self.dag_repo.get_run_by_id(&node.dag_run_id).await?;
        run.completed_nodes += 1;
        if node_status == "failed" || node_status == "cancelled" {
            run.failed_nodes += 1;
        }
        self.dag_repo.update_run(&run).await?;

        info!(
            dag_run_id = %run.id,
            job_id = %node.job_id,
            execution_id = %execution_id,
            status = %node_status,
            "DAG node completed ({}/{})",
            run.completed_nodes, run.total_nodes
        );

        // Get DAG and edges for advance logic
        let dag = self.dag_repo.get_by_id(&run.dag_id).await?;
        let edges = self.dag_repo.get_edges_by_dag_id(&run.dag_id).await?;

        // Handle failure based on on_failure policy
        if (node_status == "failed" || node_status == "cancelled") && dag.on_failure == "stop" {
            // Mark all waiting/ready nodes as skipped
            let all_nodes = self.dag_repo.get_node_executions_by_run_id(&run.id).await?;
            for n in &all_nodes {
                if n.status == "waiting" || n.status == "ready" {
                    let mut skip = n.clone();
                    skip.status = "skipped".to_string();
                    skip.completed_at = Some(chrono::Utc::now().to_rfc3339());
                    self.dag_repo.update_node_execution(&skip).await?;
                }
            }

            // Check if all running nodes are done
            let still_running = all_nodes.iter().any(|n| n.status == "running");
            if !still_running {
                let mut final_run = self.dag_repo.get_run_by_id(&run.id).await?;
                final_run.status = "failed".to_string();
                final_run.completed_at = Some(chrono::Utc::now().to_rfc3339());
                // Recount completed
                let refreshed = self.dag_repo.get_node_executions_by_run_id(&run.id).await?;
                final_run.completed_nodes = refreshed.iter().filter(|n| n.status != "waiting" && n.status != "ready" && n.status != "running").count() as i32;
                self.dag_repo.update_run(&final_run).await?;
                info!(dag_run_id = %run.id, "DAG run failed (on_failure=stop)");
            }
            return Ok(());
        }

        // Advance: check for newly ready nodes
        self.advance_dag_run(&run.id, &edges, &dag).await?;

        // Check if all nodes are in terminal state
        let all_nodes = self.dag_repo.get_node_executions_by_run_id(&run.id).await?;
        let all_terminal = all_nodes
            .iter()
            .all(|n| matches!(n.status.as_str(), "success" | "failed" | "skipped" | "cancelled"));

        if all_terminal {
            let mut final_run = self.dag_repo.get_run_by_id(&run.id).await?;
            let any_failed = all_nodes.iter().any(|n| n.status == "failed" || n.status == "cancelled");
            let any_skipped = all_nodes.iter().any(|n| n.status == "skipped");

            final_run.status = if any_failed {
                if any_skipped { "partial" } else { "failed" }
            } else {
                "success"
            }
            .to_string();
            final_run.completed_at = Some(chrono::Utc::now().to_rfc3339());
            final_run.completed_nodes = all_nodes.len() as i32;
            self.dag_repo.update_run(&final_run).await?;
            info!(dag_run_id = %final_run.id, status = %final_run.status, "DAG run completed");
        }

        Ok(())
    }

    /// Advance a DAG run: find ready nodes and execute them
    async fn advance_dag_run(
        &self,
        dag_run_id: &str,
        edges: &[DagEdge],
        dag: &crate::models::Dag,
    ) -> Result<(), AppError> {
        let nodes = self.dag_repo.get_node_executions_by_run_id(dag_run_id).await?;

        // Build status map
        let node_statuses: HashMap<String, String> = nodes
            .iter()
            .map(|n| (n.job_id.clone(), n.status.clone()))
            .collect();

        // Count currently running nodes
        let running_count = nodes.iter().filter(|n| n.status == "running").count() as i32;
        let available_slots = (dag.max_concurrent_nodes - running_count).max(0);

        if available_slots <= 0 {
            return Ok(());
        }

        // Find nodes that should become ready
        let mut newly_ready = Vec::new();
        for node in &nodes {
            if node.status != "waiting" {
                continue;
            }

            if self.is_node_ready(&node.job_id, edges, &node_statuses) {
                newly_ready.push(node.clone());
                if newly_ready.len() >= available_slots as usize {
                    break;
                }
            }
        }

        // Execute newly ready nodes
        for node in newly_ready {
            // Get the job
            let job = match self.job_repo.get_by_id(&node.job_id).await {
                Ok(job) => job,
                Err(e) => {
                    warn!("DAG node job {} not found: {}", node.job_id, e);
                    let mut failed = node.clone();
                    failed.status = "failed".to_string();
                    failed.completed_at = Some(chrono::Utc::now().to_rfc3339());
                    self.dag_repo.update_node_execution(&failed).await?;
                    continue;
                }
            };

            // Create execution
            let execution = match self
                .execution_service
                .create_execution(&node.job_id, None)
                .await
            {
                Ok(exec) => exec,
                Err(e) => {
                    error!("Failed to create execution for DAG node {}: {}", node.job_id, e);
                    let mut failed = node.clone();
                    failed.status = "failed".to_string();
                    failed.completed_at = Some(chrono::Utc::now().to_rfc3339());
                    self.dag_repo.update_node_execution(&failed).await?;
                    continue;
                }
            };

            // Update node with execution ID and status
            let mut running_node = node.clone();
            running_node.execution_id = Some(execution.id.clone());
            running_node.status = "running".to_string();
            running_node.started_at = Some(chrono::Utc::now().to_rfc3339());
            self.dag_repo.update_node_execution(&running_node).await?;

            // Enqueue for worker pool
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
                error!("Failed to enqueue DAG node execution {}: {}", execution.id, e);
            }

            info!(
                dag_run_id = %dag_run_id,
                job_id = %node.job_id,
                execution_id = %execution.id,
                "DAG node execution started"
            );
        }

        Ok(())
    }

    /// Check if a node's upstream dependencies are all satisfied
    fn is_node_ready(
        &self,
        job_id: &str,
        edges: &[DagEdge],
        node_statuses: &HashMap<String, String>,
    ) -> bool {
        // Find all upstream edges for this node
        let upstream_edges: Vec<&DagEdge> = edges
            .iter()
            .filter(|e| e.downstream_job_id == job_id)
            .collect();

        // If no upstream dependencies, it's a root node (should already be ready)
        if upstream_edges.is_empty() {
            return true;
        }

        // Check each upstream dependency
        for edge in upstream_edges {
            let upstream_status = match node_statuses.get(&edge.upstream_job_id) {
                Some(s) => s.as_str(),
                None => return false,
            };

            let satisfied = match edge.condition.as_str() {
                "success" => upstream_status == "success",
                "failure" => upstream_status == "failed",
                "always" => matches!(
                    upstream_status,
                    "success" | "failed" | "skipped" | "cancelled"
                ),
                "skipped" => upstream_status == "skipped",
                _ => upstream_status == "success",
            };

            if !satisfied {
                return false;
            }
        }

        true
    }

    /// Find root nodes (no incoming edges)
    fn find_root_nodes(&self, edges: &[DagEdge], all_job_ids: &[String]) -> Vec<String> {
        let downstream: std::collections::HashSet<&str> = edges
            .iter()
            .map(|e| e.downstream_job_id.as_str())
            .collect();

        all_job_ids
            .iter()
            .filter(|id| !downstream.contains(id.as_str()))
            .cloned()
            .collect()
    }

    /// Cancel a DAG run
    pub async fn cancel_dag_run(&self, run_id: &str) -> Result<DagRun, AppError> {
        let mut run = self.dag_repo.get_run_by_id(run_id).await?;

        if run.status != "running" && run.status != "pending" {
            return Err(AppError::BadRequest(format!(
                "Cannot cancel DAG run in '{}' status",
                run.status
            )));
        }

        // Mark all non-terminal nodes as cancelled
        let nodes = self.dag_repo.get_node_executions_by_run_id(run_id).await?;
        for node in &nodes {
            if matches!(node.status.as_str(), "waiting" | "ready" | "running") {
                let mut cancelled = node.clone();
                cancelled.status = "cancelled".to_string();
                cancelled.completed_at = Some(chrono::Utc::now().to_rfc3339());
                self.dag_repo.update_node_execution(&cancelled).await?;
            }
        }

        run.status = "cancelled".to_string();
        run.completed_at = Some(chrono::Utc::now().to_rfc3339());
        let run = self.dag_repo.update_run(&run).await?;

        info!(dag_run_id = %run.id, "DAG run cancelled");
        Ok(run)
    }
}
