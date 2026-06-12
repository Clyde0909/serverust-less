//! DAG service - DAG CRUD, cycle detection, topological sorting

use std::collections::{HashMap, HashSet, VecDeque};

use tracing::{debug, info};

use crate::db::DagRepository;
use crate::error::AppError;
use crate::models::{
    AddEdgeRequest, CreateDagRequest, Dag, DagDetailResponse, DagEdge, DagListResponse,
    DagValidationResponse, TopologyResponse, UpdateDagRequest,
};

/// Service for DAG management
#[derive(Clone)]
pub struct DagService {
    dag_repo: DagRepository,
}

impl DagService {
    pub fn new(dag_repo: DagRepository) -> Self {
        Self { dag_repo }
    }

    /// Get the repository (for DAG engine access)
    pub fn repo(&self) -> &DagRepository {
        &self.dag_repo
    }

    // -------------------------------------------------------------------------
    // DAG CRUD
    // -------------------------------------------------------------------------

    pub async fn create_dag(&self, req: CreateDagRequest) -> Result<Dag, AppError> {
        let name = req.name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::Validation("DAG name cannot be empty".to_string()));
        }

        let on_failure = match req.on_failure.as_str() {
            "stop" | "continue" => req.on_failure.clone(),
            _ => return Err(AppError::Validation(
                "on_failure must be one of: stop, continue".to_string(),
            )),
        };

        let dag = Dag::new(
            &name,
            req.description,
            req.max_concurrent_nodes.max(1),
            &on_failure,
        );

        info!(dag_id = %dag.id, name = %name, "DAG created");
        self.dag_repo.create(&dag).await
    }

    pub async fn get_dag(&self, id: &str) -> Result<Dag, AppError> {
        self.dag_repo.get_by_id(id).await
    }

    pub async fn get_dag_detail(&self, id: &str) -> Result<DagDetailResponse, AppError> {
        let dag = self.dag_repo.get_by_id(id).await?;
        let edges = self.dag_repo.get_edges_by_dag_id(id).await?;
        Ok(DagDetailResponse { dag, edges })
    }

    pub async fn update_dag(&self, id: &str, req: UpdateDagRequest) -> Result<Dag, AppError> {
        let mut dag = self.dag_repo.get_by_id(id).await?;

        if let Some(name) = &req.name {
            let trimmed = name.trim().to_string();
            if trimmed.is_empty() {
                return Err(AppError::Validation("DAG name cannot be empty".to_string()));
            }
            dag.name = trimmed;
        }

        if let Some(desc) = req.description {
            dag.description = Some(desc);
        }

        if let Some(enabled) = req.enabled {
            dag.enabled = enabled;
        }

        if let Some(max) = req.max_concurrent_nodes {
            dag.max_concurrent_nodes = max.max(1);
        }

        if let Some(on_failure) = &req.on_failure {
            match on_failure.as_str() {
                "stop" | "continue" => dag.on_failure = on_failure.clone(),
                _ => return Err(AppError::Validation(
                    "on_failure must be one of: stop, continue".to_string(),
                )),
            }
        }

        dag.updated_at = chrono::Utc::now().to_rfc3339();
        debug!(dag_id = %id, "DAG updated");
        self.dag_repo.update(&dag).await
    }

    pub async fn delete_dag(&self, id: &str) -> Result<(), AppError> {
        self.dag_repo.delete(id).await
    }

    pub async fn list_dags(&self) -> Result<DagListResponse, AppError> {
        let (dags, total) = self.dag_repo.list_all().await?;
        Ok(DagListResponse { dags, total })
    }

    // -------------------------------------------------------------------------
    // Edges
    // -------------------------------------------------------------------------

    pub async fn add_edge(&self, dag_id: &str, req: AddEdgeRequest) -> Result<DagEdge, AppError> {
        // Verify DAG exists
        let _dag = self.dag_repo.get_by_id(dag_id).await?;

        if req.upstream_job_id == req.downstream_job_id {
            return Err(AppError::Validation("A job cannot depend on itself".to_string()));
        }

        let condition = match req.condition.as_str() {
            "success" | "failure" | "always" | "skipped" => req.condition.clone(),
            _ => return Err(AppError::Validation(
                "condition must be one of: success, failure, always, skipped".to_string(),
            )),
        };

        let edge = DagEdge::new(dag_id, &req.upstream_job_id, &req.downstream_job_id, &condition);

        // Temporarily add edge and check for cycles
        let mut edges = self.dag_repo.get_edges_by_dag_id(dag_id).await?;
        edges.push(edge.clone());

        if let Err(e) = Self::validate_no_cycles(&edges) {
            return Err(e);
        }

        info!(dag_id = %dag_id, upstream = %req.upstream_job_id, downstream = %req.downstream_job_id, "Edge added");
        self.dag_repo.create_edge(&edge).await
    }

    pub async fn delete_edge(&self, dag_id: &str, edge_id: &str) -> Result<(), AppError> {
        let edge = self.dag_repo.get_edge_by_id(edge_id).await?;
        if edge.dag_id != dag_id {
            return Err(AppError::NotFound("Edge not found in this DAG".to_string()));
        }
        self.dag_repo.delete_edge(edge_id).await
    }

    // -------------------------------------------------------------------------
    // Topology & Validation
    // -------------------------------------------------------------------------

    /// Validate that edges have no cycles using Kahn's algorithm
    pub fn validate_no_cycles(edges: &[DagEdge]) -> Result<Vec<String>, AppError> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut all_nodes: HashSet<&str> = HashSet::new();

        for edge in edges {
            all_nodes.insert(&edge.upstream_job_id);
            all_nodes.insert(&edge.downstream_job_id);
            *in_degree.entry(&edge.downstream_job_id).or_insert(0) += 1;
            in_degree.entry(&edge.upstream_job_id).or_insert(0);
            adj.entry(&edge.upstream_job_id)
                .or_default()
                .push(&edge.downstream_job_id);
        }

        let mut queue: VecDeque<&str> = VecDeque::new();
        for (node, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(node);
            }
        }

        let mut sorted = Vec::new();
        while let Some(node) = queue.pop_front() {
            sorted.push(node.to_string());
            if let Some(neighbors) = adj.get(node) {
                for &neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        if sorted.len() != all_nodes.len() {
            return Err(AppError::Validation(
                "DAG contains a cycle — cannot add this edge".to_string(),
            ));
        }

        Ok(sorted)
    }

    /// Compute topological levels (BFS depth) for parallel execution planning
    pub fn topological_levels(edges: &[DagEdge]) -> Result<Vec<Vec<String>>, AppError> {
        if edges.is_empty() {
            return Ok(vec![]);
        }

        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

        for edge in edges {
            *in_degree.entry(&edge.downstream_job_id).or_insert(0) += 1;
            in_degree.entry(&edge.upstream_job_id).or_insert(0);
            adj.entry(&edge.upstream_job_id)
                .or_default()
                .push(&edge.downstream_job_id);
        }

        let mut levels: Vec<Vec<String>> = Vec::new();
        let mut queue: VecDeque<&str> = VecDeque::new();

        for (node, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(node);
            }
        }

        while !queue.is_empty() {
            let level_size = queue.len();
            let mut level = Vec::new();

            for _ in 0..level_size {
                let node = queue.pop_front().unwrap();
                level.push(node.to_string());

                if let Some(neighbors) = adj.get(node) {
                    for &neighbor in neighbors {
                        if let Some(deg) = in_degree.get_mut(neighbor) {
                            *deg -= 1;
                            if *deg == 0 {
                                queue.push_back(neighbor);
                            }
                        }
                    }
                }
            }

            levels.push(level);
        }

        let total_nodes: usize = in_degree.len();
        let sorted_count: usize = levels.iter().map(|l| l.len()).sum();
        if sorted_count != total_nodes {
            return Err(AppError::Validation("DAG contains a cycle".to_string()));
        }

        Ok(levels)
    }

    /// Get topology for a DAG
    pub async fn get_topology(&self, dag_id: &str) -> Result<TopologyResponse, AppError> {
        let edges = self.dag_repo.get_edges_by_dag_id(dag_id).await?;
        let levels = Self::topological_levels(&edges)?;
        let total_nodes = levels.iter().map(|l| l.len()).sum();
        Ok(TopologyResponse {
            levels,
            total_nodes,
        })
    }

    /// Validate a DAG (cycle detection, orphan check)
    pub async fn validate_dag(&self, dag_id: &str) -> Result<DagValidationResponse, AppError> {
        let _dag = self.dag_repo.get_by_id(dag_id).await?;
        let edges = self.dag_repo.get_edges_by_dag_id(dag_id).await?;
        let mut errors = Vec::new();

        if edges.is_empty() {
            return Ok(DagValidationResponse {
                valid: true,
                total_nodes: 0,
                total_edges: 0,
                levels: vec![],
                errors: vec!["DAG has no edges defined".to_string()],
            });
        }

        let levels = match Self::topological_levels(&edges) {
            Ok(l) => l,
            Err(e) => {
                errors.push(format!("Cycle detected: {}", e));
                vec![]
            }
        };

        let total_nodes: usize = levels.iter().map(|l| l.len()).sum();

        Ok(DagValidationResponse {
            valid: errors.is_empty(),
            total_nodes,
            total_edges: edges.len(),
            levels,
            errors,
        })
    }
}
