//! DAG API endpoints

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::{
    AddEdgeRequest, CreateDagRequest, Dag, DagDetailResponse, DagEdge, DagListResponse,
    DagRunDetailResponse, DagRunListResponse, DagValidationResponse, TopologyResponse,
    UpdateDagRequest,
};

/// Create the DAG router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/dags", post(create_dag).get(list_dags))
        .route("/dags/:id", get(get_dag).put(update_dag).delete(delete_dag))
        .route("/dags/:id/edges", post(add_edge))
        .route("/dags/:dag_id/edges/:edge_id", delete(delete_edge))
        .route("/dags/:id/topology", get(get_topology))
        .route("/dags/:id/validate", get(validate_dag))
        .route("/dags/:id/trigger", post(trigger_dag))
        .route("/dags/:id/runs", get(list_dag_runs))
        .route("/dags/:dag_id/runs/:run_id", get(get_dag_run))
        .route("/dags/:dag_id/runs/:run_id/cancel", post(cancel_dag_run))
}

/// Create a new DAG
#[utoipa::path(
    post,
    path = "/api/v1/dags",
    tag = "dags",
    request_body = CreateDagRequest,
    responses(
        (status = 201, description = "DAG created", body = Dag),
        (status = 400, description = "Invalid request")
    )
)]
pub async fn create_dag(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateDagRequest>,
) -> Result<(axum::http::StatusCode, Json<Dag>), AppError> {
    let dag = state.dag_service.create_dag(req).await?;
    Ok((axum::http::StatusCode::CREATED, Json(dag)))
}

/// List all DAGs
#[utoipa::path(
    get,
    path = "/api/v1/dags",
    tag = "dags",
    responses(
        (status = 200, description = "List of DAGs", body = DagListResponse)
    )
)]
pub async fn list_dags(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DagListResponse>, AppError> {
    let response = state.dag_service.list_dags().await?;
    Ok(Json(response))
}

/// Get a DAG with edges
#[utoipa::path(
    get,
    path = "/api/v1/dags/{id}",
    tag = "dags",
    params(("id" = String, Path, description = "DAG ID")),
    responses(
        (status = 200, description = "DAG detail", body = DagDetailResponse),
        (status = 404, description = "DAG not found")
    )
)]
pub async fn get_dag(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DagDetailResponse>, AppError> {
    let detail = state.dag_service.get_dag_detail(&id).await?;
    Ok(Json(detail))
}

/// Update a DAG
#[utoipa::path(
    put,
    path = "/api/v1/dags/{id}",
    tag = "dags",
    params(("id" = String, Path, description = "DAG ID")),
    request_body = UpdateDagRequest,
    responses(
        (status = 200, description = "DAG updated", body = Dag),
        (status = 404, description = "DAG not found")
    )
)]
pub async fn update_dag(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateDagRequest>,
) -> Result<Json<Dag>, AppError> {
    let dag = state.dag_service.update_dag(&id, req).await?;
    Ok(Json(dag))
}

/// Delete a DAG
#[utoipa::path(
    delete,
    path = "/api/v1/dags/{id}",
    tag = "dags",
    params(("id" = String, Path, description = "DAG ID")),
    responses(
        (status = 204, description = "DAG deleted"),
        (status = 404, description = "DAG not found")
    )
)]
pub async fn delete_dag(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, AppError> {
    state.dag_service.delete_dag(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Add an edge to a DAG
#[utoipa::path(
    post,
    path = "/api/v1/dags/{id}/edges",
    tag = "dags",
    params(("id" = String, Path, description = "DAG ID")),
    request_body = AddEdgeRequest,
    responses(
        (status = 201, description = "Edge added", body = DagEdge),
        (status = 400, description = "Invalid edge or would create cycle"),
        (status = 409, description = "Edge already exists")
    )
)]
pub async fn add_edge(
    State(state): State<Arc<AppState>>,
    Path(dag_id): Path<String>,
    Json(req): Json<AddEdgeRequest>,
) -> Result<(axum::http::StatusCode, Json<DagEdge>), AppError> {
    let edge = state.dag_service.add_edge(&dag_id, req).await?;
    Ok((axum::http::StatusCode::CREATED, Json(edge)))
}

/// Delete an edge from a DAG
#[utoipa::path(
    delete,
    path = "/api/v1/dags/{dag_id}/edges/{edge_id}",
    tag = "dags",
    params(
        ("dag_id" = String, Path, description = "DAG ID"),
        ("edge_id" = String, Path, description = "Edge ID")
    ),
    responses(
        (status = 204, description = "Edge deleted"),
        (status = 404, description = "Edge not found")
    )
)]
pub async fn delete_edge(
    State(state): State<Arc<AppState>>,
    Path((dag_id, edge_id)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, AppError> {
    state.dag_service.delete_edge(&dag_id, &edge_id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Get DAG topology (execution levels)
#[utoipa::path(
    get,
    path = "/api/v1/dags/{id}/topology",
    tag = "dags",
    params(("id" = String, Path, description = "DAG ID")),
    responses(
        (status = 200, description = "Topology levels", body = TopologyResponse),
        (status = 404, description = "DAG not found")
    )
)]
pub async fn get_topology(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<TopologyResponse>, AppError> {
    let topology = state.dag_service.get_topology(&id).await?;
    Ok(Json(topology))
}

/// Validate a DAG
#[utoipa::path(
    get,
    path = "/api/v1/dags/{id}/validate",
    tag = "dags",
    params(("id" = String, Path, description = "DAG ID")),
    responses(
        (status = 200, description = "Validation result", body = DagValidationResponse)
    )
)]
pub async fn validate_dag(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DagValidationResponse>, AppError> {
    let result = state.dag_service.validate_dag(&id).await?;
    Ok(Json(result))
}

/// Trigger a DAG run
#[utoipa::path(
    post,
    path = "/api/v1/dags/{id}/trigger",
    tag = "dags",
    params(("id" = String, Path, description = "DAG ID")),
    responses(
        (status = 201, description = "DAG run started", body = crate::models::DagRun),
        (status = 400, description = "DAG is disabled or invalid"),
        (status = 404, description = "DAG not found")
    )
)]
pub async fn trigger_dag(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<(axum::http::StatusCode, Json<crate::models::DagRun>), AppError> {
    let dag_engine = state.dag_engine.as_ref()
        .ok_or_else(|| AppError::Internal("DAG engine not available".to_string()))?;
    let run = dag_engine.trigger_dag(&id, "manual").await?;
    Ok((axum::http::StatusCode::CREATED, Json(run)))
}

/// List DAG runs
#[utoipa::path(
    get,
    path = "/api/v1/dags/{id}/runs",
    tag = "dags",
    params(("id" = String, Path, description = "DAG ID")),
    responses(
        (status = 200, description = "List of DAG runs", body = DagRunListResponse)
    )
)]
pub async fn list_dag_runs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DagRunListResponse>, AppError> {
    let dag_repo = state.dag_service.repo();
    let (runs, total) = dag_repo.list_runs_by_dag_id(&id).await?;
    Ok(Json(DagRunListResponse { runs, total }))
}

/// Get DAG run detail with node executions
#[utoipa::path(
    get,
    path = "/api/v1/dags/{dag_id}/runs/{run_id}",
    tag = "dags",
    params(
        ("dag_id" = String, Path, description = "DAG ID"),
        ("run_id" = String, Path, description = "Run ID")
    ),
    responses(
        (status = 200, description = "DAG run detail", body = DagRunDetailResponse),
        (status = 404, description = "DAG run not found")
    )
)]
pub async fn get_dag_run(
    State(state): State<Arc<AppState>>,
    Path((_dag_id, run_id)): Path<(String, String)>,
) -> Result<Json<DagRunDetailResponse>, AppError> {
    let dag_repo = state.dag_service.repo();
    let run = dag_repo.get_run_by_id(&run_id).await?;
    let nodes = dag_repo.get_node_executions_by_run_id(&run_id).await?;
    Ok(Json(DagRunDetailResponse { run, nodes }))
}

/// Cancel a DAG run
#[utoipa::path(
    post,
    path = "/api/v1/dags/{dag_id}/runs/{run_id}/cancel",
    tag = "dags",
    params(
        ("dag_id" = String, Path, description = "DAG ID"),
        ("run_id" = String, Path, description = "Run ID")
    ),
    responses(
        (status = 200, description = "DAG run cancelled", body = crate::models::DagRun),
        (status = 404, description = "DAG run not found")
    )
)]
pub async fn cancel_dag_run(
    State(state): State<Arc<AppState>>,
    Path((_dag_id, run_id)): Path<(String, String)>,
) -> Result<Json<crate::models::DagRun>, AppError> {
    let dag_engine = state.dag_engine.as_ref()
        .ok_or_else(|| AppError::Internal("DAG engine not available".to_string()))?;
    let run = dag_engine.cancel_dag_run(&run_id).await?;
    Ok(Json(run))
}
