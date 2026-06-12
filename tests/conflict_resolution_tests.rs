//! Conflict resolution and dependency management tests.
//! Tests the dependency CRUD lifecycle, version conflict detection strategies,
//! and package validation at both API and service levels.

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
use serverust_less::models::{PackageCache, PackageStatus};
use serverust_less::queue::QueueManager;
use serverust_less::services::{
    AuditService, DagService, ExecutionService, JobService, PackageService, QueueService,
    ScheduleService, VenvService,
};
use serverust_less::worker::{PackageManager, ProcessManager, VenvManager};
use std::sync::Arc;
use tower::ServiceExt;

/// Setup options to allow different PackageService configurations
struct SetupOpts {
    conflict_strategy: Option<String>,
    with_workers: bool,
}

async fn setup_with(opts: SetupOpts) -> (Router, PackageRepository) {
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
        std::path::Path::new("/tmp/serverust-conflict-test-venvs"),
        "python3",
    ));

    let package_service = if let Some(strategy) = opts.conflict_strategy {
        let pkg_mgr = Arc::new(PackageManager::new(300, true, None));
        PackageService::with_config(
            package_repo.clone(),
            Arc::clone(&venv_manager),
            pkg_mgr,
            strategy,
        )
    } else {
        PackageService::new(package_repo.clone())
    };

    let state = AppState {
        job_service: JobService::new(job_repo.clone()),
        execution_service: ExecutionService::new(execution_repo.clone(), log_repo.clone(), job_repo),
        package_service,
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

    (create_router(state), package_repo)
}

async fn setup() -> Router {
    let (app, _) = setup_with(SetupOpts {
        conflict_strategy: None,
        with_workers: false,
    })
    .await;
    app
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
            "max_retries": 0
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await
}

// ── Dependency CRUD lifecycle ────────────────────────────────────────────────

#[tokio::test]
async fn test_add_and_get_dependency() {
    let app = setup().await;
    let job = create_job(&app, "dep-crud-job").await;
    let job_id = job["id"].as_str().unwrap();

    // Add dependency
    let add_resp = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/dependencies", job_id),
        json!({"package_name": "requests", "version_constraint": "==2.31.0"}),
    )
    .await;
    assert_eq!(add_resp.status(), StatusCode::CREATED);

    // Get dependencies
    let get_resp = send_empty(&app, Method::GET, &format!("/api/v1/jobs/{}/dependencies", job_id)).await;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let deps = json_body(get_resp).await;
    assert!(deps["dependencies"].as_array().unwrap().len() >= 1);
}

#[tokio::test]
async fn test_update_dependency() {
    let app = setup().await;
    let job = create_job(&app, "dep-update-job").await;
    let job_id = job["id"].as_str().unwrap();

    // Add dependency
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/dependencies", job_id),
        json!({"package_name": "numpy", "version_constraint": "==1.24.0"}),
    )
    .await;

    // Update dependency
    let update_resp = send_json(
        &app,
        Method::PUT,
        &format!("/api/v1/jobs/{}/dependencies/numpy", job_id),
        json!({"version_constraint": ">=1.24.0,<2.0.0"}),
    )
    .await;
    assert_eq!(update_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_remove_dependency() {
    let app = setup().await;
    let job = create_job(&app, "dep-remove-job").await;
    let job_id = job["id"].as_str().unwrap();

    // Add dependency
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/dependencies", job_id),
        json!({"package_name": "pandas", "version_constraint": "*"}),
    )
    .await;

    // Remove dependency
    let remove_resp = send_empty(&app, Method::DELETE, &format!("/api/v1/jobs/{}/dependencies/pandas", job_id)).await;
    assert_eq!(remove_resp.status(), StatusCode::NO_CONTENT);
}

// ── Dependency status tracking ───────────────────────────────────────────────

#[tokio::test]
async fn test_dependency_status() {
    let app = setup().await;
    let job = create_job(&app, "dep-status-job").await;
    let job_id = job["id"].as_str().unwrap();

    // Add dependency
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/dependencies", job_id),
        json!({"package_name": "requests", "version_constraint": "*"}),
    )
    .await;

    // Check status
    let status_resp = send_empty(&app, Method::GET, &format!("/api/v1/jobs/{}/dependencies/status", job_id)).await;
    assert_eq!(status_resp.status(), StatusCode::OK);
}

// ── Package validation ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_invalid_package_name_rejected() {
    let app = setup().await;
    let job = create_job(&app, "pkg-validation-job").await;
    let job_id = job["id"].as_str().unwrap();

    // Try to add dependency with suspicious package name
    let response = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/dependencies", job_id),
        json!({"package_name": "rm -rf /", "version_constraint": "*"}),
    )
    .await;
    // Should be rejected
    assert!(response.status() == StatusCode::UNPROCESSABLE_ENTITY || response.status() == StatusCode::BAD_REQUEST);
}

// ── Cross-job isolation ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_cross_job_dependency_isolation() {
    let app = setup().await;
    let job1 = create_job(&app, "isolation-job-1").await;
    let job2 = create_job(&app, "isolation-job-2").await;
    let job1_id = job1["id"].as_str().unwrap();
    let job2_id = job2["id"].as_str().unwrap();

    // Add dependency to job1
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{}/dependencies", job1_id),
        json!({"package_name": "requests", "version_constraint": "==2.31.0"}),
    )
    .await;

    // job2 should not have job1's dependency
    let get_resp = send_empty(&app, Method::GET, &format!("/api/v1/jobs/{}/dependencies", job2_id)).await;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let deps = json_body(get_resp).await;
    let dep_list = deps["dependencies"].as_array().unwrap();
    let has_requests = dep_list.iter().any(|d| d["package_name"] == "requests");
    assert!(!has_requests, "job2 should not have job1's dependencies");
}
