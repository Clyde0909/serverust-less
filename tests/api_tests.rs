use axum::{
    body::Body,
    http::{Request, StatusCode},
};
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
use serde_json::Value;

async fn setup_app() -> axum::Router {
    setup_app_with_options(true, 2).await
}

async fn setup_app_with_options(seed_main_venv: bool, worker_pool_size: usize) -> axum::Router {
    // 1. Initialize an in-memory database
    let pool = init_pool("sqlite::memory:").await.expect("Failed to initialize pool");
    run_migrations(&pool).await.expect("Failed to run migrations");

    // 2. Initialize repositories
    let job_repo = JobRepository::new(pool.clone());
    let execution_repo = ExecutionRepository::new(pool.clone());
    let log_repo = ExecutionLogRepository::new(pool.clone());
    let package_repo = PackageRepository::new(pool.clone());
    let venv_repo = VenvRepository::new(pool.clone());
    let queue_repo = QueueRepository::new(pool.clone());
    let audit_repo = AuditRepository::new(pool.clone());
    let schedule_repo = ScheduleRepository::new(pool.clone());
    let dag_repo = DagRepository::new(pool.clone());

    // 3. Initialize Shared Managers
    let queue_manager = Arc::new(QueueManager::with_config(
        queue_repo.clone(),
        execution_repo.clone(),
        job_repo.clone(),
        1000,
        3,
        5,
    ));
    let process_manager = Arc::new(ProcessManager::new(30));

    // 4. Initialize Services
    let job_service = JobService::new(job_repo.clone());
    let execution_service = ExecutionService::new(
        execution_repo.clone(),
        log_repo.clone(),
        job_repo.clone(),
    );
    let package_service = PackageService::new(package_repo.clone());
    let venv_service = VenvService::new(venv_repo.clone());
    let queue_service = QueueService::new(queue_repo.clone());
    let audit_service = AuditService::new(audit_repo.clone(), true);
    let schedule_service = ScheduleService::new(schedule_repo.clone());
    let dag_service = DagService::new(dag_repo.clone());

    let venv_manager = Arc::new(VenvManager::new(
        std::path::Path::new("/tmp/serverust-api-test-venvs"),
        "python3",
    ));

    // 5. Optionally seed main venv
    if seed_main_venv {
        let main_venv_path = venv_manager.main_venv_path();
        let _ = venv_service
            .ensure_main_venv(main_venv_path.to_str().unwrap_or(""), None)
            .await;
    }

    // 6. Create AppState
    let state = AppState {
        job_service,
        execution_service,
        package_service,
        venv_service,
        queue_service,
        audit_service,
        schedule_service,
        dag_service,
        queue_manager,
        process_manager,
        worker_pool_size,
        venv_manager,
        dag_engine: None,
        cors_config: Default::default(),
        rate_limit_config: Default::default(),
        scheduler_enabled: true,
    };

    // 7. Return router
    create_router(state)
}

// ── Health & Monitoring ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_health_check_healthy() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_stats_endpoint() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_workers_status() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/workers/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_queue_status() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ── Job CRUD ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_create_and_get_job() {
    let app = setup_app().await;
    let body = serde_json::json!({
        "name": "test-job",
        "python_code": "print('hello')",
        "timeout_seconds": 30,
        "memory_limit_mb": 128,
        "priority": 0,
        "max_retries": 0
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/jobs")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_list_jobs() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/jobs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_job_not_found() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/jobs/nonexistent-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ── Execution ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_execute_job() {
    let app = setup_app().await;

    // Create a job first
    let body = serde_json::json!({
        "name": "exec-test-job",
        "python_code": "print('hello')",
        "timeout_seconds": 30,
        "memory_limit_mb": 128,
        "priority": 0,
        "max_retries": 0
    });

    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/jobs")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);

    let body_bytes = axum::body::to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let job: Value = serde_json::from_slice(&body_bytes).unwrap();
    let job_id = job["id"].as_str().unwrap();

    // Execute the job
    let exec_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/api/v1/jobs/{}/execute", job_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"priority": 5}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(exec_resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_list_executions() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/executions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ── Packages ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_packages() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/packages")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_search_packages() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/packages/search?q=requests")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ── Venvs ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_venvs() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/venvs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ── OpenAPI ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_openapi_json() {
    let app = setup_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
