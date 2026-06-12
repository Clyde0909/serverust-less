//! Performance and concurrency tests — validates behaviour under load:
//! concurrent job creation, bulk operations, pagination boundaries,
//! and simultaneous execution requests.

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
use std::time::Instant;
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
        std::path::Path::new("/tmp/serverust-perf-test-venvs"),
        "python3",
    ));

    let state = AppState {
        job_service: JobService::new(job_repo.clone()),
        execution_service: ExecutionService::new(execution_repo.clone(), log_repo.clone(), job_repo),
        package_service: PackageService::new(package_repo),
        venv_service: VenvService::new(venv_repo),
        queue_service: QueueService::new(queue_repo),
        audit_service: AuditService::new(audit_repo, true),
        queue_manager,
        process_manager,
        worker_pool_size: 4,
        venv_manager,
        schedule_service: ScheduleService::new(schedule_repo),
        dag_service: DagService::new(dag_repo),
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

// ── Concurrent job creation ──────────────────────────────────────────────────

#[tokio::test]
async fn test_concurrent_job_creation() {
    let app = setup().await;
    let start = Instant::now();
    let mut handles = Vec::new();

    for i in 0..20 {
        let app = app.clone();
        let handle = tokio::spawn(async move {
            let response = send_json(
                &app,
                Method::POST,
                "/api/v1/jobs",
                json!({
                    "name": format!("concurrent-job-{}", i),
                    "python_code": "print('hello')",
                    "timeout_seconds": 30,
                    "memory_limit_mb": 128,
                    "priority": 0,
                    "max_retries": 0
                }),
            )
            .await;
            response.status()
        });
        handles.push(handle);
    }

    for handle in handles {
        let status = handle.await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
    }

    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < 10, "Concurrent creation took too long: {:?}", elapsed);
}

// ── Bulk delete performance ──────────────────────────────────────────────────

#[tokio::test]
async fn test_bulk_delete_performance() {
    let app = setup().await;

    // Create 10 jobs
    let mut ids = Vec::new();
    for i in 0..10 {
        let response = send_json(
            &app,
            Method::POST,
            "/api/v1/jobs",
            json!({
                "name": format!("bulk-del-job-{}", i),
                "python_code": "print('hello')",
                "timeout_seconds": 30,
                "memory_limit_mb": 128,
                "priority": 0,
                "max_retries": 0
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let job = json_body(response).await;
        ids.push(job["id"].as_str().unwrap().to_string());
    }

    // Bulk delete
    let start = Instant::now();
    let response = send_json(
        &app,
        Method::DELETE,
        "/api/v1/jobs/bulk",
        json!({"ids": ids}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < 5, "Bulk delete took too long: {:?}", elapsed);
}

// ── Pagination walkthrough ───────────────────────────────────────────────────

#[tokio::test]
async fn test_pagination_walkthrough() {
    let app = setup().await;

    // Create 25 jobs
    for i in 0..25 {
        send_json(
            &app,
            Method::POST,
            "/api/v1/jobs",
            json!({
                "name": format!("page-job-{:02}", i),
                "python_code": "print('hello')",
                "timeout_seconds": 30,
                "memory_limit_mb": 128,
                "priority": 0,
                "max_retries": 0
            }),
        )
        .await;
    }

    // Walk through pages
    let mut total_seen = 0;
    let mut offset = 0;
    loop {
        let response = send_empty(&app, Method::GET, &format!("/api/v1/jobs?limit=10&offset={}", offset)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let jobs = body["jobs"].as_array().unwrap();
        if jobs.is_empty() {
            break;
        }
        total_seen += jobs.len();
        offset += 10;
    }
    assert!(total_seen >= 25, "Expected at least 25 jobs, saw {}", total_seen);
}

// ── Rapid CRUD cycles ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_rapid_crud_cycles() {
    let app = setup().await;

    for cycle in 0..5 {
        // Create
        let create_resp = send_json(
            &app,
            Method::POST,
            "/api/v1/jobs",
            json!({
                "name": format!("crud-cycle-{}", cycle),
                "python_code": "print('hello')",
                "timeout_seconds": 30,
                "memory_limit_mb": 128,
                "priority": 0,
                "max_retries": 0
            }),
        )
        .await;
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let job = json_body(create_resp).await;
        let job_id = job["id"].as_str().unwrap();

        // Read
        let get_resp = send_empty(&app, Method::GET, &format!("/api/v1/jobs/{}", job_id)).await;
        assert_eq!(get_resp.status(), StatusCode::OK);

        // Update
        let update_resp = send_json(
            &app,
            Method::PUT,
            &format!("/api/v1/jobs/{}", job_id),
            json!({"name": format!("crud-cycle-{}-updated", cycle)}),
        )
        .await;
        assert_eq!(update_resp.status(), StatusCode::OK);

        // Delete
        let delete_resp = send_empty(&app, Method::DELETE, &format!("/api/v1/jobs/{}", job_id)).await;
        assert_eq!(delete_resp.status(), StatusCode::NO_CONTENT);
    }
}

// ── Health under load ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_health_under_load() {
    let app = setup().await;

    // Create some jobs first
    for i in 0..10 {
        send_json(
            &app,
            Method::POST,
            "/api/v1/jobs",
            json!({
                "name": format!("health-load-job-{}", i),
                "python_code": "print('hello')",
                "timeout_seconds": 30,
                "memory_limit_mb": 128,
                "priority": 0,
                "max_retries": 0
            }),
        )
        .await;
    }

    // Health check should still respond quickly
    let start = Instant::now();
    let response = send_empty(&app, Method::GET, "/api/v1/health").await;
    assert_eq!(response.status(), StatusCode::OK);
    let elapsed = start.elapsed();
    assert!(elapsed.as_millis() < 500, "Health check too slow under load: {:?}", elapsed);
}
