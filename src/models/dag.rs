//! DAG model and DTOs

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

/// DAG entity representing a directed acyclic graph of job dependencies
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Dag {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub max_concurrent_nodes: i32,
    pub on_failure: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Dag {
    pub fn new(name: &str, description: Option<String>, max_concurrent_nodes: i32, on_failure: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            description,
            enabled: true,
            max_concurrent_nodes,
            on_failure: on_failure.to_string(),
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

/// DAG edge representing a dependency between two jobs
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct DagEdge {
    pub id: String,
    pub dag_id: String,
    pub upstream_job_id: String,
    pub downstream_job_id: String,
    pub condition: String,
    pub created_at: String,
}

impl DagEdge {
    pub fn new(dag_id: &str, upstream_job_id: &str, downstream_job_id: &str, condition: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            dag_id: dag_id.to_string(),
            upstream_job_id: upstream_job_id.to_string(),
            downstream_job_id: downstream_job_id.to_string(),
            condition: condition.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// DAG run instance
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct DagRun {
    pub id: String,
    pub dag_id: String,
    pub status: String,
    pub trigger_type: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub total_nodes: i32,
    pub completed_nodes: i32,
    pub failed_nodes: i32,
    pub created_at: String,
}

impl DagRun {
    pub fn new(dag_id: &str, trigger_type: &str, total_nodes: i32) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            dag_id: dag_id.to_string(),
            status: "pending".to_string(),
            trigger_type: trigger_type.to_string(),
            started_at: None,
            completed_at: None,
            total_nodes,
            completed_nodes: 0,
            failed_nodes: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Individual node execution within a DAG run
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct DagNodeExecution {
    pub id: String,
    pub dag_run_id: String,
    pub job_id: String,
    pub execution_id: Option<String>,
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
}

impl DagNodeExecution {
    pub fn new(dag_run_id: &str, job_id: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            dag_run_id: dag_run_id.to_string(),
            job_id: job_id.to_string(),
            execution_id: None,
            status: "waiting".to_string(),
            started_at: None,
            completed_at: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Request to create a DAG
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateDagRequest {
    pub name: String,
    pub description: Option<String>,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_nodes: i32,
    #[serde(default = "default_on_failure")]
    pub on_failure: String,
}

fn default_max_concurrent() -> i32 { 4 }
fn default_on_failure() -> String { "stop".to_string() }

/// Request to update a DAG
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateDagRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub max_concurrent_nodes: Option<i32>,
    pub on_failure: Option<String>,
}

/// Request to add an edge to a DAG
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct AddEdgeRequest {
    pub upstream_job_id: String,
    pub downstream_job_id: String,
    #[serde(default = "default_condition")]
    pub condition: String,
}

fn default_condition() -> String { "success".to_string() }

/// DAG detail response (includes edges)
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DagDetailResponse {
    #[serde(flatten)]
    pub dag: Dag,
    pub edges: Vec<DagEdge>,
}

/// DAG list response
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DagListResponse {
    pub dags: Vec<Dag>,
    pub total: i64,
}

/// DAG run detail response (includes node executions)
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DagRunDetailResponse {
    #[serde(flatten)]
    pub run: DagRun,
    pub nodes: Vec<DagNodeExecution>,
}

/// DAG run list response
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DagRunListResponse {
    pub runs: Vec<DagRun>,
    pub total: i64,
}

/// Topology response showing execution levels
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TopologyResponse {
    pub levels: Vec<Vec<String>>,
    pub total_nodes: usize,
}

/// DAG validation response
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DagValidationResponse {
    pub valid: bool,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub levels: Vec<Vec<String>>,
    pub errors: Vec<String>,
}
