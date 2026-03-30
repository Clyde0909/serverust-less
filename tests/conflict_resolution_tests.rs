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

    let package_service = if opts.with_workers {
        let pkg_mgr = Arc::new(PackageManager::new(120, true, None));
        let strategy = opts.conflict_strategy.unwrap_or_else(|| "suggest_custom_venv".to_string());
        PackageService::with_config(package_repo.clone(), venv_manager.clone(), pkg_mgr, strategy)
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

// ===== Dependency CRUD =====

#[tokio::test]
async fn test_add_dependency_to_job() {
    let app = setup().await;
    let job = create_job(&app, "dep-add-test").await;
    let job_id = job["id"].as_str().unwrap();

    let resp = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
        json!({"package_name": "requests", "version_constraint": ">=2.28.0"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let dep = json_body(resp).await;
    assert_eq!(dep["package_name"].as_str().unwrap(), "requests");
    assert_eq!(dep["version_constraint"].as_str().unwrap(), ">=2.28.0");
    assert_eq!(dep["job_id"].as_str().unwrap(), job_id);
}

#[tokio::test]
async fn test_list_job_dependencies() {
    let app = setup().await;
    let job = create_job(&app, "dep-list-test").await;
    let job_id = job["id"].as_str().unwrap();

    // Add two dependencies
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
        json!({"package_name": "requests"}),
    )
    .await;
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
        json!({"package_name": "numpy", "version_constraint": ">=1.24.0"}),
    )
    .await;

    let resp = send_empty(
        &app,
        Method::GET,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    let deps = body["dependencies"].as_array().unwrap();
    assert_eq!(deps.len(), 2);
}

#[tokio::test]
async fn test_update_dependency_version() {
    let app = setup().await;
    let job = create_job(&app, "dep-update-test").await;
    let job_id = job["id"].as_str().unwrap();

    // Add
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
        json!({"package_name": "requests", "version_constraint": ">=2.28.0"}),
    )
    .await;

    // Update
    let resp = send_json(
        &app,
        Method::PUT,
        &format!("/api/v1/jobs/{job_id}/dependencies/requests"),
        json!({"version_constraint": ">=2.31.0,<3.0.0"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let updated = json_body(resp).await;
    assert_eq!(
        updated["version_constraint"].as_str().unwrap(),
        ">=2.31.0,<3.0.0"
    );
}

#[tokio::test]
async fn test_remove_dependency() {
    let app = setup().await;
    let job = create_job(&app, "dep-remove-test").await;
    let job_id = job["id"].as_str().unwrap();

    // Add
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
        json!({"package_name": "requests"}),
    )
    .await;

    // Remove
    let resp = send_empty(
        &app,
        Method::DELETE,
        &format!("/api/v1/jobs/{job_id}/dependencies/requests"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify gone
    let resp = send_empty(
        &app,
        Method::GET,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
    )
    .await;
    let body = json_body(resp).await;
    let deps = body["dependencies"].as_array().unwrap();
    assert_eq!(deps.len(), 0);
}

#[tokio::test]
async fn test_add_dependency_empty_package_name_returns_422() {
    let app = setup().await;
    let job = create_job(&app, "dep-empty-name").await;
    let job_id = job["id"].as_str().unwrap();

    let resp = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
        json!({"package_name": ""}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ===== Dependency Status =====

#[tokio::test]
async fn test_dependency_status_no_deps_returns_no_dependencies() {
    let app = setup().await;
    let job = create_job(&app, "dep-status-none").await;
    let job_id = job["id"].as_str().unwrap();

    let resp = send_empty(
        &app,
        Method::GET,
        &format!("/api/v1/jobs/{job_id}/dependencies/status"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["status"].as_str().unwrap(), "no_dependencies");
}

#[tokio::test]
async fn test_dependency_status_with_uninstalled_deps_returns_pending() {
    let app = setup().await;
    let job = create_job(&app, "dep-status-pending").await;
    let job_id = job["id"].as_str().unwrap();

    // Add dependency but don't install it
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
        json!({"package_name": "requests", "version_constraint": ">=2.28.0"}),
    )
    .await;

    let resp = send_empty(
        &app,
        Method::GET,
        &format!("/api/v1/jobs/{job_id}/dependencies/status"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["status"].as_str().unwrap(), "pending");
    let packages = body["packages"].as_array().unwrap();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0]["status"].as_str().unwrap(), "not_installed");
}

#[tokio::test]
async fn test_dependency_status_with_cached_ready_package() {
    let (app, repo) = setup_with(SetupOpts {
        conflict_strategy: None,
        with_workers: false,
    })
    .await;

    let job = create_job(&app, "dep-status-ready").await;
    let job_id = job["id"].as_str().unwrap();

    // Add dependency
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
        json!({"package_name": "requests", "version_constraint": ">=2.28.0"}),
    )
    .await;

    // Seed a ready cache entry via repo
    let cache = PackageCache {
        id: uuid::Uuid::new_v4().to_string(),
        venv_type: "main".to_string(),
        venv_id: None,
        package_name: "requests".to_string(),
        version: "2.31.0".to_string(),
        installation_path: "/tmp/test".to_string(),
        size_bytes: None,
        status: PackageStatus::Ready.as_str().to_string(),
        error_message: None,
        installed_at: chrono::Utc::now().to_rfc3339(),
        last_used_at: None,
        use_count: 0,
    };
    repo.upsert_cache(&cache).await.unwrap();

    // Check status → should be ready
    let resp = send_empty(
        &app,
        Method::GET,
        &format!("/api/v1/jobs/{job_id}/dependencies/status"),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["status"].as_str().unwrap(), "ready");
    let packages = body["packages"].as_array().unwrap();
    assert_eq!(packages[0]["status"].as_str().unwrap(), "ready");
    assert_eq!(
        packages[0]["installed"].as_str().unwrap(),
        "2.31.0"
    );
}

// ===== Conflict Detection via Service =====

#[tokio::test]
async fn test_conflict_fail_strategy_blocks_version_mismatch() {
    let pool = init_pool("sqlite::memory:").await.unwrap();
    run_migrations(&pool).await.unwrap();

    let package_repo = PackageRepository::new(pool.clone());
    let venv_manager = Arc::new(VenvManager::new(
        std::path::Path::new("/tmp/serverust-conflict-fail-venvs"),
        "python3",
    ));
    let pkg_mgr = Arc::new(PackageManager::new(120, true, None));

    let service = PackageService::with_config(
        package_repo.clone(),
        venv_manager,
        pkg_mgr,
        "fail".to_string(),
    );

    // Seed an existing package as "ready"
    let cache = PackageCache {
        id: uuid::Uuid::new_v4().to_string(),
        venv_type: "main".to_string(),
        venv_id: None,
        package_name: "requests".to_string(),
        version: "2.28.0".to_string(),
        installation_path: "/tmp/test".to_string(),
        size_bytes: None,
        status: PackageStatus::Ready.as_str().to_string(),
        error_message: None,
        installed_at: chrono::Utc::now().to_rfc3339(),
        last_used_at: None,
        use_count: 0,
    };
    package_repo.upsert_cache(&cache).await.unwrap();

    // Try to install a different version — should fail with Validation error
    let req = serverust_less::models::InstallPackageRequest {
        name: "requests".to_string(),
        version: Some("2.31.0".to_string()),
    };

    let result = service.install_package(req).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("conflict") || err_msg.contains("Conflict") || err_msg.contains("already installed"),
        "Expected conflict error, got: {}",
        err_msg
    );
}

// ===== Package Validation =====

#[tokio::test]
async fn test_install_package_empty_name_returns_422() {
    let app = setup().await;
    let resp = send_json(
        &app,
        Method::POST,
        "/api/v1/packages/install",
        json!({"name": ""}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_list_packages_empty() {
    let app = setup().await;
    let resp = send_empty(&app, Method::GET, "/api/v1/packages").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["total"].as_i64().unwrap(), 0);
}

#[tokio::test]
async fn test_main_venv_packages_empty() {
    let app = setup().await;
    let resp = send_empty(&app, Method::GET, "/api/v1/packages/main-venv").await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===== Cross-Job Dependency Isolation =====

#[tokio::test]
async fn test_dependencies_are_isolated_per_job() {
    let app = setup().await;
    let job_a = create_job(&app, "dep-isolated-a").await;
    let job_b = create_job(&app, "dep-isolated-b").await;
    let id_a = job_a["id"].as_str().unwrap();
    let id_b = job_b["id"].as_str().unwrap();

    // Add deps to job A only
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{id_a}/dependencies"),
        json!({"package_name": "requests"}),
    )
    .await;
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{id_a}/dependencies"),
        json!({"package_name": "numpy"}),
    )
    .await;

    // Add one dep to job B
    send_json(
        &app,
        Method::POST,
        &format!("/api/v1/jobs/{id_b}/dependencies"),
        json!({"package_name": "pandas"}),
    )
    .await;

    // Verify counts
    let resp = send_empty(&app, Method::GET, &format!("/api/v1/jobs/{id_a}/dependencies")).await;
    let body_a = json_body(resp).await;
    assert_eq!(body_a["dependencies"].as_array().unwrap().len(), 2);

    let resp = send_empty(&app, Method::GET, &format!("/api/v1/jobs/{id_b}/dependencies")).await;
    let body_b = json_body(resp).await;
    assert_eq!(body_b["dependencies"].as_array().unwrap().len(), 1);
    assert_eq!(
        body_b["dependencies"][0]["package_name"].as_str().unwrap(),
        "pandas"
    );
}


