//! Cancellation scenario tests — validates execution lifecycle transitions:
//! cancel pending/running, reject cancel on terminal states, retry logic, max-retry limits.

use axum::{
    body::Body,
    http::{header, Method, Request, Response, StatusCode},
    Router,
};
use serde_json::{json, Value};
use serverust_less::api::{create_router, AppState};
use serverust_less::db::{
    init_pool, run_migrations, AuditRepository, DagRepository, ExecutionLogRepository,
    ExecutionRepository, JobRepository, PackageRepository, QueueRepository, ScheduleRepository,
    VenvRepository,
};
use serverust_less::queue::QueueManager;
use serverust_less::services::{
    AuditService, DagService, ExecutionService, JobService, PackageService, QueueService,
    ScheduleService, VenvService,
};
use serverust_less::worker::{ProcessManager, VenvManager};
use std::sync::Arc;
use tower::ServiceExt;

async fn setup() -> Router {
    let pool = init_pool("sqlite::memory:").await.unwrap();
    run_migrations(&pool).await.unwrap();

    let job_repo = JobRepository::new(pool.clone());
    let execution_repo = ExecutionRepository::new(pool.clone());
    let log_repo = ExecutionLogRepository::new(pool.clone());
    let package_repo = PackageRepository::new(pool.clone());
    let venv_repo = VenvRepository::new(pool.clone());
    let queue_repo = QueueRepository::new(pool.clone());
    let audit_repo = AuditRepository::new(pool.clone());
    let schedule_repo = ScheduleRepository::new(pool.clone());
    let dag_repo = DagRepository::new(pool.clone());

    let queue_manager = Arc::new(QueueManager::with_config(
        queue_repo.clone(),
        execution_repo.clone(),
        job_repo.clone(),
        1000,
        3,
        5,
    ));
    let process_manager = Arc::new(ProcessManager::new(5));
    let venv_manager = Arc::new(VenvManager::new(
        std::path::Path::new("/tmp/serverust-cancel-test-venvs"),
        "python3",
    ));

    let state = AppState {
        job_service: JobService::new(job_repo.clone()),
        execution_service: ExecutionService::new(execution_repo.clone(), log_repo.clone(), job_repo),
        package_service: PackageService::new(package_repo),
        venv_service: VenvService::new(venv_repo),
        queue_service: QueueService::new(queue_repo),
        audit_service: AuditService::new(audit_repo, true),
        schedule_service: ScheduleService::new(schedule_repo),
        dag_service: DagService::new(dag_repo),
        queue_manager,
        process_manager,
        worker_pool_size: 2,
        venv_manager,
        dag_engine: None,
    };

    create_router(state)
}

async fn send_json(app: &Router, method: Method, uri: &str, body: Value) -> Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn send_empty(app: &Router, method: Method, uri: &str) -> Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn json_body(response: Response<Body>) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Create a job with max_retries control
async fn create_job_with_retries(app: &Router, name: &str, max_retries: i32) -> Value {
    let resp = send_json(
        app,
        Method::POST,
        "/api/v1/jobs",
        json!({
            "name": name,
            "python_code": "print('hello')",
            "timeout_seconds": 30,
            "memory_limit_mb": 128,
            "max_retries": max_retries,
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    json_body(resp).await
}

/// Execute a job and return the execution
async fn execute_job(app: &Router, job_id: &str) -> Value {
    let resp = send_json(
        app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/execute"),
        json!({}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED, "Failed to execute job");
    json_body(resp).await
}

// ===== Cancel Pending Execution =====

#[tokio::test]
async fn test_cancel_pending_execution() {
    let app = setup().await;
    let job = create_job_with_retries(&app, "cancel-pending", 3).await;
    let job_id = job["id"].as_str().unwrap();

    let execution = execute_job(&app, job_id).await;
    let exec_id = execution["id"].as_str().unwrap();

    // New execution should be in pending status
    assert_eq!(execution["status"].as_str().unwrap(), "pending");

    // Cancel
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/cancel")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let cancelled = json_body(resp).await;
    assert_eq!(cancelled["status"].as_str().unwrap(), "cancelled");
}

// ===== Cancel Already Cancelled → 400 =====

#[tokio::test]
async fn test_cancel_already_cancelled_returns_400() {
    let app = setup().await;
    let job = create_job_with_retries(&app, "cancel-twice", 3).await;
    let job_id = job["id"].as_str().unwrap();

    let execution = execute_job(&app, job_id).await;
    let exec_id = execution["id"].as_str().unwrap();

    // Cancel first time
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/cancel")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Cancel second time — should fail since already in terminal state
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/cancel")).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===== Cancel Completed Execution → 400 =====
// We can't easily put an execution into "success" status via API only,
// but we can test that retry on a pending execution fails.

#[tokio::test]
async fn test_retry_pending_execution_returns_400() {
    let app = setup().await;
    let job = create_job_with_retries(&app, "retry-pending", 3).await;
    let job_id = job["id"].as_str().unwrap();

    let execution = execute_job(&app, job_id).await;
    let exec_id = execution["id"].as_str().unwrap();

    // Retry on pending should fail — only failed/timeout/cancelled can be retried
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/retry")).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===== Retry Cancelled Execution → New Execution Created =====

#[tokio::test]
async fn test_retry_cancelled_execution() {
    let app = setup().await;
    let job = create_job_with_retries(&app, "retry-cancelled", 3).await;
    let job_id = job["id"].as_str().unwrap();

    let execution = execute_job(&app, job_id).await;
    let exec_id = execution["id"].as_str().unwrap();

    // Cancel it first
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/cancel")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Now retry
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/retry")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let retried = json_body(resp).await;
    // Retry creates a new execution
    assert_ne!(retried["id"].as_str().unwrap(), exec_id);
    assert_eq!(retried["status"].as_str().unwrap(), "pending");
    assert_eq!(retried["retry_count"].as_i64().unwrap(), 1);
    assert_eq!(retried["job_id"].as_str().unwrap(), job_id);
}

// ===== Retry With Max Retries Exceeded → 400 =====

#[tokio::test]
async fn test_retry_exceeds_max_retries() {
    let app = setup().await;
    // Create job with max_retries = 1
    let job = create_job_with_retries(&app, "retry-exceeded", 1).await;
    let job_id = job["id"].as_str().unwrap();

    // Execute and cancel
    let execution = execute_job(&app, job_id).await;
    let exec_id = execution["id"].as_str().unwrap();
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/cancel")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // First retry → succeeds, retry_count goes from 0 to 1
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/retry")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let retried = json_body(resp).await;
    let retried_id = retried["id"].as_str().unwrap();
    assert_eq!(retried["retry_count"].as_i64().unwrap(), 1);

    // Cancel the retried execution so we can retry again
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{retried_id}/cancel")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Second retry → should fail, retry_count(1) >= max_retries(1)
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{retried_id}/retry")).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===== Verify Execution Count After Cancel-Retry Cycle =====

#[tokio::test]
async fn test_cancel_retry_creates_new_execution_in_job() {
    let app = setup().await;
    let job = create_job_with_retries(&app, "cancel-retry-count", 3).await;
    let job_id = job["id"].as_str().unwrap();

    // Execute
    let execution = execute_job(&app, job_id).await;
    let exec_id = execution["id"].as_str().unwrap();

    // Cancel and retry
    send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/cancel")).await;
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/retry")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // List executions for this job — should have 2 (original cancelled + retry)
    let resp = send_empty(
        &app,
        Method::GET,
        &format!("/api/v1/jobs/{job_id}/executions?limit=10&offset=0"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["total"].as_i64().unwrap(), 2);
}

// ===== Execute Multiple Times and Cancel All =====

#[tokio::test]
async fn test_cancel_multiple_executions() {
    let app = setup().await;
    let job = create_job_with_retries(&app, "cancel-multi", 0).await;
    let job_id = job["id"].as_str().unwrap();

    let mut exec_ids = Vec::new();
    for _ in 0..3 {
        let exec = execute_job(&app, job_id).await;
        exec_ids.push(exec["id"].as_str().unwrap().to_string());
    }

    // Cancel all
    for exec_id in &exec_ids {
        let resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{exec_id}/cancel")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Verify all cancelled
    for exec_id in &exec_ids {
        let resp = send_empty(&app, Method::GET, &format!("/api/v1/executions/{exec_id}")).await;
        let body = json_body(resp).await;
        assert_eq!(body["status"].as_str().unwrap(), "cancelled");
    }
}
