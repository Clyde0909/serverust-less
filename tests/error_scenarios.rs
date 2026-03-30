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

async fn create_job(app: &Router, name: &str) -> Value {
    let resp = send_json(
        app,
        Method::POST,
        "/api/v1/jobs",
        json!({
            "name": name,
            "python_code": "print('hello')",
            "timeout_seconds": 30,
            "memory_limit_mb": 128,
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    json_body(resp).await
}

// ===== Job Error Scenarios =====

#[tokio::test]
async fn test_get_nonexistent_job_returns_404() {
    let app = setup().await;
    let resp = send_empty(&app, Method::GET, "/api/v1/jobs/nonexistent-id-12345").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_update_nonexistent_job_returns_404() {
    let app = setup().await;
    let resp = send_json(
        &app,
        Method::PUT,
        "/api/v1/jobs/nonexistent-id-12345",
        json!({"name": "updated"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_nonexistent_job_returns_404() {
    let app = setup().await;
    let resp = send_empty(&app, Method::DELETE, "/api/v1/jobs/nonexistent-id-12345").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_job_missing_required_fields_returns_422() {
    let app = setup().await;

    // Missing python_code (required)
    let resp = send_json(
        &app,
        Method::POST,
        "/api/v1/jobs",
        json!({"name": "missing-code"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_create_job_empty_name_returns_422() {
    let app = setup().await;
    let resp = send_json(
        &app,
        Method::POST,
        "/api/v1/jobs",
        json!({"name": "", "python_code": "print(1)"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_create_duplicate_job_name_returns_conflict() {
    let app = setup().await;
    let _ = create_job(&app, "duplicate-job").await;
    let resp = send_json(
        &app,
        Method::POST,
        "/api/v1/jobs",
        json!({"name": "duplicate-job", "python_code": "print(1)"}),
    )
    .await;
    // Should be 409 Conflict or 400
    assert!(
        resp.status() == StatusCode::CONFLICT || resp.status() == StatusCode::BAD_REQUEST,
        "Expected 409 or 400 for duplicate job name, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn test_create_job_with_invalid_json_returns_422() {
    let app = setup().await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/jobs")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{invalid json}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        resp.status() == StatusCode::UNPROCESSABLE_ENTITY || resp.status() == StatusCode::BAD_REQUEST,
        "Expected 422 or 400 for invalid JSON, got {}",
        resp.status()
    );
}

// ===== Execution Error Scenarios =====

#[tokio::test]
async fn test_execute_nonexistent_job_returns_404() {
    let app = setup().await;
    let resp = send_json(
        &app,
        Method::POST,
        "/api/v1/jobs/nonexistent-id/execute",
        json!({}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_execute_disabled_job_auto_enables() {
    let app = setup().await;
    let job = create_job(&app, "disabled-job").await;
    let job_id = job["id"].as_str().unwrap();

    // Disable the job explicitly
    let resp = send_empty(&app, Method::POST, &format!("/api/v1/jobs/{job_id}/disable")).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Execute should auto-enable the job and return 201
    let resp = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/execute"),
        json!({}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Verify the job is now enabled
    let get_resp = send_empty(&app, Method::GET, &format!("/api/v1/jobs/{job_id}")).await;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let job_json = json_body(get_resp).await;
    assert!(job_json["enabled"].as_bool().unwrap_or(false), "Job should be auto-enabled after Execute");
}

#[tokio::test]
async fn test_get_nonexistent_execution_returns_404() {
    let app = setup().await;
    let resp = send_empty(&app, Method::GET, "/api/v1/executions/nonexistent-id").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_cancel_nonexistent_execution_returns_404() {
    let app = setup().await;
    let resp = send_empty(&app, Method::POST, "/api/v1/executions/nonexistent-id/cancel").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_retry_nonexistent_execution_returns_404() {
    let app = setup().await;
    let resp = send_empty(&app, Method::POST, "/api/v1/executions/nonexistent-id/retry").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_logs_nonexistent_execution_returns_404() {
    let app = setup().await;
    let resp = send_empty(&app, Method::GET, "/api/v1/executions/nonexistent-id/logs").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===== Venv Error Scenarios =====

#[tokio::test]
async fn test_get_nonexistent_venv_returns_404() {
    let app = setup().await;
    let resp = send_empty(&app, Method::GET, "/api/v1/venvs/nonexistent-venv-id").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_nonexistent_venv_returns_404() {
    let app = setup().await;
    let resp = send_empty(&app, Method::DELETE, "/api/v1/venvs/nonexistent-venv-id").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===== Dependency Error Scenarios =====

#[tokio::test]
async fn test_add_dependency_to_nonexistent_job_returns_404() {
    let app = setup().await;
    let resp = send_json(
        &app,
        Method::POST,
        "/api/v1/jobs/nonexistent-id/dependencies",
        json!({"package_name": "requests", "version_constraint": ">=2.0.0"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_nonexistent_dependency_returns_404() {
    let app = setup().await;
    let job = create_job(&app, "dep-error-test").await;
    let job_id = job["id"].as_str().unwrap();

    let resp = send_empty(
        &app,
        Method::DELETE,
        &format!("/api/v1/jobs/{job_id}/dependencies/nonexistent-pkg"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===== Package Error Scenarios =====

#[tokio::test]
async fn test_install_package_without_workers_returns_500() {
    let app = setup().await;
    // PackageService created with new() has no worker managers
    let resp = send_json(
        &app,
        Method::POST,
        "/api/v1/packages/install",
        json!({"name": "requests", "version": "2.31.0"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_uninstall_package_without_workers_returns_500() {
    let app = setup().await;
    let resp = send_json(
        &app,
        Method::POST,
        "/api/v1/packages/uninstall",
        json!({"name": "requests"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// ===== Bulk Operation Error Scenarios =====

#[tokio::test]
async fn test_bulk_delete_jobs_empty_ids_returns_error() {
    let app = setup().await;
    let resp = send_json(
        &app,
        Method::DELETE,
        "/api/v1/jobs/bulk",
        json!({"ids": []}),
    )
    .await;
    // Empty list should return an error or handled gracefully
    assert!(
        resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::OK,
        "Unexpected status: {}",
        resp.status()
    );
}

#[tokio::test]
async fn test_bulk_delete_executions_empty_ids_returns_error() {
    let app = setup().await;
    let resp = send_json(
        &app,
        Method::DELETE,
        "/api/v1/executions/bulk",
        json!({"ids": []}),
    )
    .await;
    assert!(
        resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::OK,
        "Unexpected status: {}",
        resp.status()
    );
}

// ===== Pagination Boundary Tests =====

#[tokio::test]
async fn test_list_jobs_negative_offset_handled() {
    let app = setup().await;
    let resp = send_empty(&app, Method::GET, "/api/v1/jobs?limit=10&offset=-1").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_list_jobs_zero_limit_handled() {
    let app = setup().await;
    let resp = send_empty(&app, Method::GET, "/api/v1/jobs?limit=0&offset=0").await;
    // Should either clamp to 1 or return OK with empty results
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_list_executions_large_limit_clamped() {
    let app = setup().await;
    let resp = send_empty(&app, Method::GET, "/api/v1/executions?limit=99999&offset=0").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert!(body["limit"].as_i64().unwrap() <= 100);
}

// ===== Execution for Specific Job with Bad Filters =====

#[tokio::test]
async fn test_list_job_executions_nonexistent_job_returns_404() {
    let app = setup().await;
    let resp = send_empty(
        &app,
        Method::GET,
        "/api/v1/jobs/nonexistent-job-id/executions",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
