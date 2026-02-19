//! Execution API endpoints

use axum::{
    extract::{Path, Query, State},
    response::sse::{Event, Sse},
    routing::{delete, get, post},
    Json, Router,
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::StreamExt;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::{
    ExecuteJobRequest, Execution, ExecutionListResponse, ExecutionLogsResponse,
    ListExecutionsQuery, ListLogsQuery, QueueItem,
};

/// Create the executions router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/executions", get(list_executions))
        .route("/executions/:id", get(get_execution).delete(delete_execution))
        .route("/executions/:id/logs", get(get_execution_logs))
        .route("/executions/:id/stream", get(stream_execution_logs))
        .route("/executions/:id/cancel", post(cancel_execution))
        .route("/executions/:id/retry", post(retry_execution))
        .route("/jobs/:id/execute", post(execute_job))
        .route("/jobs/:id/executions", get(list_job_executions))
}

/// List all executions
#[utoipa::path(
    get,
    path = "/api/v1/executions",
    tag = "executions",
    params(
        ("limit" = Option<i32>, Query, description = "Maximum number of results"),
        ("offset" = Option<i32>, Query, description = "Offset for pagination"),
        ("status" = Option<String>, Query, description = "Filter by status"),
        ("job_id" = Option<String>, Query, description = "Filter by job ID"),
        ("from" = Option<String>, Query, description = "Filter from date"),
        ("to" = Option<String>, Query, description = "Filter to date")
    ),
    responses(
        (status = 200, description = "List of executions", body = ExecutionListResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_executions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListExecutionsQuery>,
) -> Result<Json<ExecutionListResponse>, AppError> {
    let response = state.execution_service.list_executions(query).await?;
    Ok(Json(response))
}

/// Get execution by ID
#[utoipa::path(
    get,
    path = "/api/v1/executions/{id}",
    tag = "executions",
    params(
        ("id" = String, Path, description = "Execution ID")
    ),
    responses(
        (status = 200, description = "Execution details", body = Execution),
        (status = 404, description = "Execution not found")
    )
)]
pub async fn get_execution(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Execution>, AppError> {
    let execution = state.execution_service.get_execution(&id).await?;
    Ok(Json(execution))
}

/// Delete an execution
#[utoipa::path(
    delete,
    path = "/api/v1/executions/{id}",
    tag = "executions",
    params(
        ("id" = String, Path, description = "Execution ID")
    ),
    responses(
        (status = 204, description = "Execution deleted"),
        (status = 404, description = "Execution not found")
    )
)]
pub async fn delete_execution(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, AppError> {
    state.execution_service.delete_execution(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Get execution logs
#[utoipa::path(
    get,
    path = "/api/v1/executions/{id}/logs",
    tag = "executions",
    params(
        ("id" = String, Path, description = "Execution ID"),
        ("log_type" = Option<String>, Query, description = "Filter by log type (stdout/stderr)")
    ),
    responses(
        (status = 200, description = "Execution logs", body = ExecutionLogsResponse),
        (status = 404, description = "Execution not found")
    )
)]
pub async fn get_execution_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<ListLogsQuery>,
) -> Result<Json<ExecutionLogsResponse>, AppError> {
    let logs = state.execution_service.get_logs(&id, query).await?;
    Ok(Json(logs))
}

/// Cancel an execution
#[utoipa::path(
    post,
    path = "/api/v1/executions/{id}/cancel",
    tag = "executions",
    params(
        ("id" = String, Path, description = "Execution ID")
    ),
    responses(
        (status = 200, description = "Execution cancelled", body = Execution),
        (status = 400, description = "Cannot cancel execution"),
        (status = 404, description = "Execution not found")
    )
)]
pub async fn cancel_execution(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Execution>, AppError> {
    let execution = state.execution_service.cancel_execution(&id).await?;
    // Also attempt to kill the running process (may be no-op if already finished)
    let _ = state.process_manager.cancel(&id).await;
    Ok(Json(execution))
}

/// Retry a failed execution
#[utoipa::path(
    post,
    path = "/api/v1/executions/{id}/retry",
    tag = "executions",
    params(
        ("id" = String, Path, description = "Execution ID")
    ),
    responses(
        (status = 200, description = "New execution created", body = Execution),
        (status = 400, description = "Cannot retry execution"),
        (status = 404, description = "Execution not found")
    )
)]
pub async fn retry_execution(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Execution>, AppError> {
    let execution = state.execution_service.retry_execution(&id).await?;

    // Enqueue the newly created retry execution
    let job = state.job_service.get_job(&execution.job_id).await?;
    let item = QueueItem::new(
        &execution.id,
        &job.id,
        0, // retry uses default priority
        &job.python_code,
        job.timeout_seconds,
        job.memory_limit_mb,
        execution.input_data.clone(),
        job.use_custom_venv,
    );
    state
        .queue_manager
        .enqueue(item)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to enqueue retry: {}", e)))?;

    Ok(Json(execution))
}

/// Execute a job
#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/execute",
    tag = "executions",
    params(
        ("id" = String, Path, description = "Job ID")
    ),
    request_body = ExecuteJobRequest,
    responses(
        (status = 201, description = "Execution created", body = Execution),
        (status = 400, description = "Job is disabled"),
        (status = 404, description = "Job not found")
    )
)]
pub async fn execute_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Json(req): Json<Option<ExecuteJobRequest>>,
) -> Result<(axum::http::StatusCode, Json<Execution>), AppError> {
    let priority = req.as_ref().and_then(|r| r.priority).unwrap_or(0);
    let execution = state
        .execution_service
        .create_execution(&job_id, req)
        .await?;

    // Fetch job details needed to build the QueueItem
    let job = state.job_service.get_job(&job_id).await?;
    let item = QueueItem::new(
        &execution.id,
        &job.id,
        priority,
        &job.python_code,
        job.timeout_seconds,
        job.memory_limit_mb,
        execution.input_data.clone(),
        job.use_custom_venv,
    );
    state
        .queue_manager
        .enqueue(item)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to enqueue execution: {}", e)))?;

    Ok((axum::http::StatusCode::CREATED, Json(execution)))
}

/// List executions for a job
#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}/executions",
    tag = "executions",
    params(
        ("id" = String, Path, description = "Job ID"),
        ("limit" = Option<i32>, Query, description = "Maximum number of results"),
        ("offset" = Option<i32>, Query, description = "Offset for pagination")
    ),
    responses(
        (status = 200, description = "List of executions", body = ExecutionListResponse),
        (status = 404, description = "Job not found")
    )
)]
pub async fn list_job_executions(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Query(query): Query<ListExecutionsQuery>,
) -> Result<Json<ExecutionListResponse>, AppError> {
    let response = state
        .execution_service
        .list_job_executions(&job_id, query.limit, query.offset)
        .await?;
    Ok(Json(response))
}

/// Stream execution logs via Server-Sent Events (SSE)
#[utoipa::path(
    get,
    path = "/api/v1/executions/{id}/stream",
    tag = "executions",
    params(
        ("id" = String, Path, description = "Execution ID")
    ),
    responses(
        (status = 200, description = "SSE stream of execution logs"),
        (status = 404, description = "Execution not found")
    )
)]
pub async fn stream_execution_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    // Verify execution exists
    let execution = state.execution_service.get_execution(&id).await?;

    // Create the SSE stream
    let stream = create_log_stream(state, id, execution);

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

/// Create a stream that polls for new logs
fn create_log_stream(
    state: Arc<AppState>,
    execution_id: String,
    initial_execution: Execution,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let mut last_log_count = 0i64;
    let mut execution_complete = initial_execution.completed_at.is_some();

    async_stream::stream! {
        // Send initial status
        let status_event = Event::default()
            .event("status")
            .data(serde_json::json!({
                "execution_id": execution_id,
                "status": initial_execution.status,
                "started_at": initial_execution.started_at,
            }).to_string());
        yield Ok(status_event);

        loop {
            // Fetch new logs
            let logs_result = state.execution_service.get_logs(
                &execution_id,
                ListLogsQuery {
                    log_type: None,
                    offset: Some(last_log_count as i32),
                    limit: Some(100),
                },
            ).await;

            match logs_result {
                Ok(logs_response) => {
                    // Send each new log entry
                    for log in &logs_response.logs {
                        let log_event = Event::default()
                            .event("log")
                            .data(serde_json::json!({
                                "id": log.id,
                                "log_type": log.log_type,
                                "content": log.log_content,
                                "created_at": log.created_at,
                            }).to_string());
                        yield Ok(log_event);
                    }
                    last_log_count += logs_response.logs.len() as i64;
                }
                Err(e) => {
                    let error_event = Event::default()
                        .event("error")
                        .data(format!("Failed to fetch logs: {}", e));
                    yield Ok(error_event);
                }
            }

            // Check if execution is complete
            if !execution_complete {
                if let Ok(exec) = state.execution_service.get_execution(&execution_id).await {
                    if exec.completed_at.is_some() {
                        execution_complete = true;

                        // Send completion event
                        let complete_event = Event::default()
                            .event("complete")
                            .data(serde_json::json!({
                                "execution_id": execution_id,
                                "status": exec.status,
                                "duration_ms": exec.duration_ms,
                                "output_data": exec.output_data,
                                "error_message": exec.error_message,
                            }).to_string());
                        yield Ok(complete_event);

                        // End the stream
                        break;
                    }
                }
            } else {
                break;
            }

            // Wait before polling again
            tokio::time::sleep(Duration::from_millis(2000)).await;
        }
    }
}
