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
        cors_config: Default::default(),
        rate_limit_config: Default::default(),
        scheduler_enabled: true,
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
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn create_job(app: &Router, name: &str) -> Value {
    let response = send_json(
        app,
        Method::POST,
        "/api/v1/jobs",
        json!({
            "name": name,
            "python_code": "print('hello')",
            "timeout_seconds": 30,
            "memory_limit_mb": 128,
            "priority": 0,
            "max_retries": 1
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await
}

// ── Cancel pending execution ─────────────────────────────────────────────────

#[tokio::test]
async fn test_cancel_pending_execution() {
    let app = setup().await;
    let job = create_job(&app, "cancel-pending-job").await;
    let job_id = job["id"].as_str().unwrap();

    let exec_resp = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/execute", job_id),
        json!({"priority": 5}),
    )
    .await;
    assert_eq!(exec_resp.status(), StatusCode::CREATED);
    let exec = json_body(exec_resp).await;
    let exec_id = exec["id"].as_str().unwrap();

    let cancel_resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{}/cancel", exec_id)).await;
    assert_eq!(cancel_resp.status(), StatusCode::OK);
}

// ── Reject double cancel ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_reject_double_cancel() {
    let app = setup().await;
    let job = create_job(&app, "double-cancel-job").await;
    let job_id = job["id"].as_str().unwrap();

    let exec_resp = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/execute", job_id),
        json!({"priority": 5}),
    )
    .await;
    assert_eq!(exec_resp.status(), StatusCode::CREATED);
    let exec = json_body(exec_resp).await;
    let exec_id = exec["id"].as_str().unwrap();

    // First cancel should succeed
    let cancel1 = send_empty(&app, Method::POST, &format!("/api/v1/executions/{}/cancel", exec_id)).await;
    assert_eq!(cancel1.status(), StatusCode::OK);

    // Second cancel should be rejected (already cancelled)
    let cancel2 = send_empty(&app, Method::POST, &format!("/api/v1/executions/{}/cancel", exec_id)).await;
    assert!(cancel2.status() == StatusCode::CONFLICT || cancel2.status() == StatusCode::BAD_REQUEST);
}

// ── Retry cancelled execution ────────────────────────────────────────────────

#[tokio::test]
async fn test_retry_cancelled_execution() {
    let app = setup().await;
    let job = create_job(&app, "retry-cancelled-job").await;
    let job_id = job["id"].as_str().unwrap();

    let exec_resp = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/execute", job_id),
        json!({"priority": 5}),
    )
    .await;
    assert_eq!(exec_resp.status(), StatusCode::CREATED);
    let exec = json_body(exec_resp).await;
    let exec_id = exec["id"].as_str().unwrap();

    // Cancel first
    let cancel_resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{}/cancel", exec_id)).await;
    assert_eq!(cancel_resp.status(), StatusCode::OK);

    // Retry should create a new execution
    let retry_resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{}/retry", exec_id)).await;
    assert_eq!(retry_resp.status(), StatusCode::OK);
}

// ── Max retry enforcement ────────────────────────────────────────────────────

#[tokio::test]
async fn test_max_retry_enforcement() {
    let app = setup().await;
    let job = create_job(&app, "max-retry-job").await;
    let job_id = job["id"].as_str().unwrap();

    // Create execution with max_retries=1 on the job
    let exec_resp = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/execute", job_id),
        json!({"priority": 5}),
    )
    .await;
    assert_eq!(exec_resp.status(), StatusCode::CREATED);
    let exec = json_body(exec_resp).await;
    let exec_id = exec["id"].as_str().unwrap();

    // Cancel first so retry becomes valid (only terminal states can be retried)
    let cancel_resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{}/cancel", exec_id)).await;
    assert_eq!(cancel_resp.status(), StatusCode::OK);

    // First retry should work
    let retry1 = send_empty(&app, Method::POST, &format!("/api/v1/executions/{}/retry", exec_id)).await;
    assert_eq!(retry1.status(), StatusCode::OK);

    // Second retry on the same execution should be rejected (max_retries=1)
    let retry2 = send_empty(&app, Method::POST, &format!("/api/v1/executions/{}/retry", exec_id)).await;
    assert!(retry2.status() == StatusCode::CONFLICT || retry2.status() == StatusCode::BAD_REQUEST);
}

// ── Multi-execution cancel ───────────────────────────────────────────────────

#[tokio::test]
async fn test_multi_execution_cancel() {
    let app = setup().await;
    let job = create_job(&app, "multi-cancel-job").await;
    let job_id = job["id"].as_str().unwrap();

    // Create multiple executions
    let mut exec_ids = Vec::new();
    for _ in 0..3 {
        let exec_resp = send_json(
            &app,
            Method::POST,
            &format!("/api/v1/jobs/{}/execute", job_id),
            json!({"priority": 5}),
        )
        .await;
        assert_eq!(exec_resp.status(), StatusCode::CREATED);
        let exec = json_body(exec_resp).await;
        exec_ids.push(exec["id"].as_str().unwrap().to_string());
    }

    // Cancel all of them
    for exec_id in &exec_ids {
        let cancel_resp = send_empty(&app, Method::POST, &format!("/api/v1/executions/{}/cancel", exec_id)).await;
        assert_eq!(cancel_resp.status(), StatusCode::OK);
    }
}
