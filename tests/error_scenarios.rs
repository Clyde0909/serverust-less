//! Error scenario tests — validates that the API returns correct error codes and messages
//! for invalid inputs, missing resources, and boundary conditions.

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
        std::path::Path::new("/tmp/serverust-error-test-venvs"),
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

// ── 404 Not Found ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_job_not_found_404() {
    let app = setup().await;
    let response = send_empty(&app, Method::GET, "/api/v1/jobs/nonexistent-id").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_execution_not_found_404() {
    let app = setup().await;
    let response = send_empty(&app, Method::GET, "/api/v1/executions/nonexistent-id").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ── 422 Validation Error ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_create_job_empty_name_422() {
    let app = setup().await;
    let response = send_json(
        &app,
        Method::POST,
        "/api/v1/jobs",
        json!({"name": "", "python_code": "print(1)", "timeout_seconds": 30, "memory_limit_mb": 128, "priority": 0, "max_retries": 0}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_create_job_empty_code_422() {
    let app = setup().await;
    let response = send_json(
        &app,
        Method::POST,
        "/api/v1/jobs",
        json!({"name": "test", "python_code": "", "timeout_seconds": 30, "memory_limit_mb": 128, "priority": 0, "max_retries": 0}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ── 400 Bad Request ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_invalid_json_400() {
    let app = setup().await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/jobs")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();
    // axum 0.7 returns 400 for malformed JSON body parsing errors.
    assert!(response.status() == StatusCode::BAD_REQUEST || response.status() == StatusCode::UNPROCESSABLE_ENTITY);
}

// ── Disabled job execution ───────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_disabled_job_auto_enables() {
    let app = setup().await;

    // Create a job
    let create_resp = send_json(
        &app,
        Method::POST,
        "/api/v1/jobs",
        json!({"name": "disabled-job", "python_code": "print(1)", "timeout_seconds": 30, "memory_limit_mb": 128, "priority": 0, "max_retries": 0}),
    )
    .await;
    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let job = axum::body::to_bytes(create_resp.into_body(), usize::MAX).await.unwrap();
    let job: Value = serde_json::from_slice(&job).unwrap();
    let job_id = job["id"].as_str().unwrap();

    // Disable it
    let disable_resp = send_empty(&app, Method::POST, &format!("/api/v1/jobs/{}/disable", job_id)).await;
    assert_eq!(disable_resp.status(), StatusCode::OK);

    // Execute should auto-enable and succeed
    let exec_resp = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/execute", job_id),
        json!({"priority": 5}),
    )
    .await;
    assert_eq!(exec_resp.status(), StatusCode::CREATED);
}

// ── Bulk operations edge cases ───────────────────────────────────────────────

#[tokio::test]
async fn test_bulk_delete_empty_ids() {
    let app = setup().await;
    let response = send_json(
        &app,
        Method::DELETE,
        "/api/v1/jobs/bulk",
        json!({"ids": []}),
    )
    .await;
    // Handler rejects empty ID lists as a bad request (defense against no-op deletes).
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ── Pagination boundaries ────────────────────────────────────────────────────

#[tokio::test]
async fn test_pagination_boundary_zero_limit() {
    let app = setup().await;
    let response = send_empty(&app, Method::GET, "/api/v1/jobs?limit=0&offset=0").await;
    // limit=0 should be clamped to 1
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_pagination_large_offset() {
    let app = setup().await;
    let response = send_empty(&app, Method::GET, "/api/v1/jobs?limit=10&offset=99999").await;
    assert_eq!(response.status(), StatusCode::OK);
}
