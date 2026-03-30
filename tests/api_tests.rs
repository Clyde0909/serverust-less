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
        1000, // max memory queue size
        3,    // max retries
        5,    // retry delay seconds
    ));
    let process_manager = Arc::new(ProcessManager::new(30)); // 30 seconds graceful shutdown
    let venv_manager = Arc::new(VenvManager::new(
        std::path::Path::new("/tmp/serverust-test-venvs"),
        "python3",
    ));

    // 4. Initialize Services
    let audit_service = AuditService::new(audit_repo, true);
    let job_service = JobService::new(job_repo.clone());
    let execution_service = ExecutionService::new(execution_repo.clone(), log_repo.clone(), job_repo.clone());
    let package_service = PackageService::new(package_repo.clone());
    let venv_service = VenvService::new(venv_repo.clone());
    let queue_service = QueueService::new(queue_repo.clone());

    // 5. Create AppState
    let state = AppState {
        job_service,
        execution_service,
        package_service,
        venv_service,
        queue_service,
        audit_service,
        schedule_service: ScheduleService::new(schedule_repo),
        dag_service: DagService::new(dag_repo),
        queue_manager,
        process_manager,
        worker_pool_size: 2,
        venv_manager,
        dag_engine: None,
    };

    // 6. Return router
    create_router(state)
}

#[tokio::test]
async fn test_health_api() {
    let app = setup_app().await;

    // Test /api/v1/health
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_workers_status_api() {
    let app = setup_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/workers/status")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_queue_status_api() {
    let app = setup_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/queue/status")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_jobs_api_crud() {
    let app = setup_app().await;

    // 1. Array of jobs should be empty initially
    let response = app.clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/jobs")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 2. Create a Job
    let create_body = serde_json::json!({
        "name": "test_job",
        "description": "Integration Test Job",
        "python_code": "print('hello')",
        "timeout_seconds": 30,
        "memory_limit_mb": 128,
        "use_custom_venv": false,
        "priority": 1,
        "max_retries": 0,
        "enabled": true
    });

    let response = app.clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/jobs")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(create_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let created_job: Value = serde_json::from_slice(&body_bytes).unwrap();
    let job_id = created_job["id"].as_str().unwrap().to_string();

    // 3. Get the Job
    let response = app.clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/jobs/{}", job_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 4. Update the Job
    let update_body = serde_json::json!({
        "name": "updated_test_job",
        "python_code": "print('hello world')",
        "priority": 10
    });

    let response = app.clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/jobs/{}", job_id))
                .method("PUT")
                .header("Content-Type", "application/json")
                .body(Body::from(update_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 5. Delete the Job
    let response = app.clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/jobs/{}", job_id))
                .method("DELETE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // 6. Get deleted job - Should be 404
    let response = app.clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/jobs/{}", job_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_executions_api() {
    let app = setup_app().await;

    // 1. Create a job first
    let create_body = serde_json::json!({
        "name": "exec_test",
        "python_code": "print('running')"
    });

    let resp = app.clone().oneshot(
        Request::builder()
            .uri("/api/v1/jobs")
            .method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(create_body.to_string()))
            .unwrap(),
    ).await.unwrap();
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let job: Value = serde_json::from_slice(&body_bytes).unwrap();
    let job_id = job["id"].as_str().unwrap();

    // 2. Execute the job
    let exec_body = serde_json::json!({
        "priority": 5
    });

    let response = app.clone().oneshot(
        Request::builder()
            .uri(format!("/api/v1/jobs/{}/execute", job_id))
            .method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(exec_body.to_string()))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let execution: Value = serde_json::from_slice(&body_bytes).unwrap();
    let exec_id = execution["id"].as_str().unwrap();

    // 3. Get the Execution
    let response = app.clone().oneshot(
        Request::builder()
            .uri(format!("/api/v1/executions/{}", exec_id))
            .method("GET")
            .body(Body::empty())
            .unwrap(),
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 4. Cancel the execution
    let response = app.clone().oneshot(
        Request::builder()
            .uri(format!("/api/v1/executions/{}/cancel", exec_id))
            .method("POST")
            .body(Body::empty())
            .unwrap(),
    ).await.unwrap();
    // Usually ok or conflict if already done.
    assert!(response.status() == StatusCode::OK || response.status() == StatusCode::CONFLICT);
    
    // 5. List Executions
    let response = app.clone().oneshot(
        Request::builder()
            .uri("/api/v1/executions")
            .method("GET")
            .body(Body::empty())
            .unwrap(),
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_job_dependencies_api() {
    let app = setup_app().await;

    // Create a Job to attach dependencies to
    let create_body = serde_json::json!({
        "name": "dep_test",
        "python_code": "import requests",
        "use_custom_venv": true
    });
    
    let resp = app.clone().oneshot(
        Request::builder()
            .uri("/api/v1/jobs")
            .method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(create_body.to_string()))
            .unwrap(),
    ).await.unwrap();
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let job: Value = serde_json::from_slice(&body_bytes).unwrap();
    let job_id = job["id"].as_str().unwrap();

    // Add dependency
    let dep_body = serde_json::json!({
        "package_name": "requests",
        "version_constraint": "==2.31.0"
    });

    let resp = app.clone().oneshot(
        Request::builder()
            .uri(format!("/api/v1/jobs/{}/dependencies", job_id))
            .method("POST")
            .header("Content-Type", "application/json")
            .body(Body::from(dep_body.to_string()))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Get dependencies
    let resp = app.clone().oneshot(
        Request::builder()
            .uri(format!("/api/v1/jobs/{}/dependencies", job_id))
            .method("GET")
            .body(Body::empty())
            .unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_packages_api() {
    let app = setup_app().await;

    // Test /api/v1/packages
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/packages")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_venvs_api() {
    let app = setup_app().await;

    // Test /api/v1/venvs
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/venvs")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
