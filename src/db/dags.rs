//! DAG repository

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{Dag, DagEdge, DagNodeExecution, DagRun};

/// Repository for DAG database operations
#[derive(Clone)]
pub struct DagRepository {
    pool: SqlitePool,
}

impl DagRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // -------------------------------------------------------------------------
    // DAG CRUD
    // -------------------------------------------------------------------------

    pub async fn create(&self, dag: &Dag) -> Result<Dag, AppError> {
        sqlx::query(
            r#"
            INSERT INTO dags (id, name, description, enabled, max_concurrent_nodes, on_failure, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&dag.id)
        .bind(&dag.name)
        .bind(&dag.description)
        .bind(dag.enabled)
        .bind(dag.max_concurrent_nodes)
        .bind(&dag.on_failure)
        .bind(&dag.created_at)
        .bind(&dag.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.get_by_id(&dag.id).await
    }

    pub async fn get_by_id(&self, id: &str) -> Result<Dag, AppError> {
        sqlx::query_as::<_, Dag>(
            "SELECT id, name, description, enabled, max_concurrent_nodes, on_failure, created_at, updated_at FROM dags WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("DAG not found: {}", id)))
    }

    pub async fn update(&self, dag: &Dag) -> Result<Dag, AppError> {
        sqlx::query(
            r#"
            UPDATE dags
            SET name = ?, description = ?, enabled = ?, max_concurrent_nodes = ?, on_failure = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&dag.name)
        .bind(&dag.description)
        .bind(dag.enabled)
        .bind(dag.max_concurrent_nodes)
        .bind(&dag.on_failure)
        .bind(&dag.updated_at)
        .bind(&dag.id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.get_by_id(&dag.id).await
    }

    pub async fn delete(&self, id: &str) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM dags WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("DAG not found: {}", id)));
        }
        Ok(())
    }

    pub async fn list_all(&self) -> Result<(Vec<Dag>, i64), AppError> {
        let dags = sqlx::query_as::<_, Dag>(
            "SELECT id, name, description, enabled, max_concurrent_nodes, on_failure, created_at, updated_at FROM dags ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        let total = dags.len() as i64;
        Ok((dags, total))
    }

    // -------------------------------------------------------------------------
    // Edges
    // -------------------------------------------------------------------------

    pub async fn create_edge(&self, edge: &DagEdge) -> Result<DagEdge, AppError> {
        sqlx::query(
            r#"
            INSERT INTO dag_edges (id, dag_id, upstream_job_id, downstream_job_id, condition, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&edge.id)
        .bind(&edge.dag_id)
        .bind(&edge.upstream_job_id)
        .bind(&edge.downstream_job_id)
        .bind(&edge.condition)
        .bind(&edge.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE constraint") {
                AppError::Conflict("Edge already exists between these jobs in this DAG".to_string())
            } else {
                AppError::Database(e.to_string())
            }
        })?;

        self.get_edge_by_id(&edge.id).await
    }

    pub async fn get_edge_by_id(&self, id: &str) -> Result<DagEdge, AppError> {
        sqlx::query_as::<_, DagEdge>(
            "SELECT id, dag_id, upstream_job_id, downstream_job_id, condition, created_at FROM dag_edges WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Edge not found: {}", id)))
    }

    pub async fn get_edges_by_dag_id(&self, dag_id: &str) -> Result<Vec<DagEdge>, AppError> {
        sqlx::query_as::<_, DagEdge>(
            "SELECT id, dag_id, upstream_job_id, downstream_job_id, condition, created_at FROM dag_edges WHERE dag_id = ? ORDER BY created_at ASC",
        )
        .bind(dag_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    pub async fn delete_edge(&self, edge_id: &str) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM dag_edges WHERE id = ?")
            .bind(edge_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Edge not found: {}", edge_id)));
        }
        Ok(())
    }

    /// Get all unique job IDs referenced in a DAG's edges
    pub async fn get_dag_job_ids(&self, dag_id: &str) -> Result<Vec<String>, AppError> {
        let edges = self.get_edges_by_dag_id(dag_id).await?;
        let mut job_ids = std::collections::HashSet::new();
        for edge in &edges {
            job_ids.insert(edge.upstream_job_id.clone());
            job_ids.insert(edge.downstream_job_id.clone());
        }
        Ok(job_ids.into_iter().collect())
    }

    // -------------------------------------------------------------------------
    // DAG Runs
    // -------------------------------------------------------------------------

    pub async fn create_run(&self, run: &DagRun) -> Result<DagRun, AppError> {
        sqlx::query(
            r#"
            INSERT INTO dag_runs (id, dag_id, status, trigger_type, started_at, completed_at, total_nodes, completed_nodes, failed_nodes, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&run.id)
        .bind(&run.dag_id)
        .bind(&run.status)
        .bind(&run.trigger_type)
        .bind(&run.started_at)
        .bind(&run.completed_at)
        .bind(run.total_nodes)
        .bind(run.completed_nodes)
        .bind(run.failed_nodes)
        .bind(&run.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.get_run_by_id(&run.id).await
    }

    pub async fn get_run_by_id(&self, id: &str) -> Result<DagRun, AppError> {
        sqlx::query_as::<_, DagRun>(
            "SELECT id, dag_id, status, trigger_type, started_at, completed_at, total_nodes, completed_nodes, failed_nodes, created_at FROM dag_runs WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("DAG run not found: {}", id)))
    }

    pub async fn update_run(&self, run: &DagRun) -> Result<DagRun, AppError> {
        sqlx::query(
            r#"
            UPDATE dag_runs
            SET status = ?, started_at = ?, completed_at = ?, completed_nodes = ?, failed_nodes = ?
            WHERE id = ?
            "#,
        )
        .bind(&run.status)
        .bind(&run.started_at)
        .bind(&run.completed_at)
        .bind(run.completed_nodes)
        .bind(run.failed_nodes)
        .bind(&run.id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.get_run_by_id(&run.id).await
    }

    pub async fn list_runs_by_dag_id(&self, dag_id: &str) -> Result<(Vec<DagRun>, i64), AppError> {
        let runs = sqlx::query_as::<_, DagRun>(
            "SELECT id, dag_id, status, trigger_type, started_at, completed_at, total_nodes, completed_nodes, failed_nodes, created_at FROM dag_runs WHERE dag_id = ? ORDER BY created_at DESC",
        )
        .bind(dag_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        let total = runs.len() as i64;
        Ok((runs, total))
    }

    // -------------------------------------------------------------------------
    // Node Executions
    // -------------------------------------------------------------------------

    pub async fn create_node_execution(&self, node: &DagNodeExecution) -> Result<DagNodeExecution, AppError> {
        sqlx::query(
            r#"
            INSERT INTO dag_node_executions (id, dag_run_id, job_id, execution_id, status, started_at, completed_at, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&node.id)
        .bind(&node.dag_run_id)
        .bind(&node.job_id)
        .bind(&node.execution_id)
        .bind(&node.status)
        .bind(&node.started_at)
        .bind(&node.completed_at)
        .bind(&node.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.get_node_execution_by_id(&node.id).await
    }

    pub async fn get_node_execution_by_id(&self, id: &str) -> Result<DagNodeExecution, AppError> {
        sqlx::query_as::<_, DagNodeExecution>(
            "SELECT id, dag_run_id, job_id, execution_id, status, started_at, completed_at, created_at FROM dag_node_executions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Node execution not found: {}", id)))
    }

    pub async fn update_node_execution(&self, node: &DagNodeExecution) -> Result<DagNodeExecution, AppError> {
        sqlx::query(
            r#"
            UPDATE dag_node_executions
            SET execution_id = ?, status = ?, started_at = ?, completed_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&node.execution_id)
        .bind(&node.status)
        .bind(&node.started_at)
        .bind(&node.completed_at)
        .bind(&node.id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        self.get_node_execution_by_id(&node.id).await
    }

    pub async fn get_node_executions_by_run_id(&self, run_id: &str) -> Result<Vec<DagNodeExecution>, AppError> {
        sqlx::query_as::<_, DagNodeExecution>(
            "SELECT id, dag_run_id, job_id, execution_id, status, started_at, completed_at, created_at FROM dag_node_executions WHERE dag_run_id = ? ORDER BY created_at ASC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }

    /// Find a node execution by its execution_id (for DAG engine callback)
    pub async fn find_node_by_execution_id(&self, execution_id: &str) -> Result<Option<DagNodeExecution>, AppError> {
        sqlx::query_as::<_, DagNodeExecution>(
            "SELECT id, dag_run_id, job_id, execution_id, status, started_at, completed_at, created_at FROM dag_node_executions WHERE execution_id = ?",
        )
        .bind(execution_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))
    }
}
