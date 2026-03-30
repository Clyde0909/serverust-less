use axum::{
    body::{to_bytes, Body},
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
use serverust_less::models::{Execution, ExecutionLog, PackageCache, Venv};
use serverust_less::queue::QueueManager;
use serverust_less::services::{
    AuditService, DagService, ExecutionService, JobService, PackageService, QueueService,
    ScheduleService, VenvService,
};
use serverust_less::worker::{ProcessManager, VenvManager};
use std::sync::Arc;
use tower::ServiceExt;

#[derive(Clone)]
struct TestContext {
    app: Router,
    package_repo: PackageRepository,
    venv_repo: VenvRepository,
    log_repo: ExecutionLogRepository,
    execution_repo: ExecutionRepository,
}

async fn setup_context() -> TestContext {
    let pool = init_pool("sqlite::memory:")
        .await
        .expect("failed to initialize pool");
    run_migrations(&pool)
        .await
        .expect("failed to run migrations");

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
    let process_manager = Arc::new(ProcessManager::new(30));
    let venv_manager = Arc::new(VenvManager::new(
        std::path::Path::new("/tmp/serverust-test-venvs"),
        "python3",
    ));

    let state = AppState {
        job_service: JobService::new(job_repo.clone()),
        execution_service: ExecutionService::new(
            execution_repo.clone(),
            log_repo.clone(),
            job_repo,
        ),
        package_service: PackageService::new(package_repo.clone()),
        venv_service: VenvService::new(venv_repo.clone()),
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

    TestContext {
        app: create_router(state),
        package_repo,
        venv_repo,
        log_repo,
        execution_repo,
    }
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
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn create_job(app: &Router, name: &str) -> Value {
    let response = send_json(
        app,
        Method::POST,
        "/api/v1/jobs",
        json!({
            "name": name,
            "description": format!("job-{name}"),
            "python_code": "print('hello')",
            "timeout_seconds": 30,
            "memory_limit_mb": 128,
            "use_custom_venv": false,
            "priority": 1,
            "max_retries": 1
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await
}

async fn create_execution_for_job(app: &Router, job_id: &str) -> Value {
    let response = send_json(
        app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/execute"),
        json!({"priority": 5, "input_data": {"hello": "world"}}),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await
}

async fn seed_ready_package(ctx: &TestContext, name: &str, version: &str) {
    let mut cache = PackageCache::new("main", None, name, version, "/tmp/main-venv");
    cache.mark_ready(None);
    ctx.package_repo.upsert_cache(&cache).await.unwrap();
}

async fn seed_main_venv(ctx: &TestContext) -> Venv {
    let mut venv = Venv::new_main("/tmp/venvs/main", Some("3.11".to_string()));
    venv.mark_ready();
    ctx.venv_repo.create(&venv).await.unwrap()
}

async fn seed_custom_venv(ctx: &TestContext, job_id: &str, path: &str) -> Venv {
    let mut venv = Venv::new_custom(job_id, path, Some("3.11".to_string()));
    venv.mark_ready();
    ctx.venv_repo.create(&venv).await.unwrap()
}

#[tokio::test]
async fn test_job_routes_cover_all_endpoints() {
    let ctx = setup_context().await;

    let list_response = send_empty(&ctx.app, Method::GET, "/api/v1/jobs?limit=5&offset=0").await;
    assert_eq!(list_response.status(), StatusCode::OK);

    let job = create_job(&ctx.app, "job-routes-primary").await;
    let job_id = job["id"].as_str().unwrap();

    let get_response = send_empty(&ctx.app, Method::GET, &format!("/api/v1/jobs/{job_id}")).await;
    assert_eq!(get_response.status(), StatusCode::OK);

    let update_response = send_json(
        &ctx.app,
        Method::PUT,
        &format!("/api/v1/jobs/{job_id}"),
        json!({"name": "job-routes-updated", "priority": 7}),
    )
    .await;
    assert_eq!(update_response.status(), StatusCode::OK);

    let disable_response = send_empty(&ctx.app, Method::POST, &format!("/api/v1/jobs/{job_id}/disable")).await;
    assert_eq!(disable_response.status(), StatusCode::OK);

    let enable_response = send_empty(&ctx.app, Method::POST, &format!("/api/v1/jobs/{job_id}/enable")).await;
    assert_eq!(enable_response.status(), StatusCode::OK);

    let clone_response = send_json(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/clone"),
        json!({"name": "job-routes-clone"}),
    )
    .await;
    assert_eq!(clone_response.status(), StatusCode::CREATED);
    let cloned_job = json_body(clone_response).await;
    let cloned_job_id = cloned_job["id"].as_str().unwrap();

    let bulk_create_response = send_json(
        &ctx.app,
        Method::POST,
        "/api/v1/jobs/bulk",
        json!([
            {"name": "bulk-job-1", "python_code": "print(1)", "timeout_seconds": 30, "memory_limit_mb": 128, "priority": 0, "max_retries": 0},
            {"name": "bulk-job-2", "python_code": "print(2)", "timeout_seconds": 30, "memory_limit_mb": 128, "priority": 0, "max_retries": 0}
        ]),
    )
    .await;
    assert_eq!(bulk_create_response.status(), StatusCode::OK);

    let bulk_jobs = send_empty(&ctx.app, Method::GET, "/api/v1/jobs?search=bulk-job").await;
    let bulk_jobs_json = json_body(bulk_jobs).await;
    let bulk_ids: Vec<String> = bulk_jobs_json["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|job| job["id"].as_str().unwrap().to_string())
        .collect();

    let bulk_delete_response = send_json(
        &ctx.app,
        Method::DELETE,
        "/api/v1/jobs/bulk",
        json!({"ids": bulk_ids}),
    )
    .await;
    assert_eq!(bulk_delete_response.status(), StatusCode::OK);

    let delete_clone_response = send_empty(&ctx.app, Method::DELETE, &format!("/api/v1/jobs/{cloned_job_id}")).await;
    assert_eq!(delete_clone_response.status(), StatusCode::NO_CONTENT);

    let delete_primary_response = send_empty(&ctx.app, Method::DELETE, &format!("/api/v1/jobs/{job_id}")).await;
    assert_eq!(delete_primary_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_execution_routes_cover_all_endpoints() {
    let ctx = setup_context().await;
    let job = create_job(&ctx.app, "execution-routes-job").await;
    let job_id = job["id"].as_str().unwrap();

    let execution = create_execution_for_job(&ctx.app, job_id).await;
    let execution_id = execution["id"].as_str().unwrap();

    let get_response = send_empty(&ctx.app, Method::GET, &format!("/api/v1/executions/{execution_id}")).await;
    assert_eq!(get_response.status(), StatusCode::OK);

    let list_response = send_empty(&ctx.app, Method::GET, &format!("/api/v1/executions?job_id={job_id}&status=pending")).await;
    assert_eq!(list_response.status(), StatusCode::OK);

    let list_for_job = send_empty(&ctx.app, Method::GET, &format!("/api/v1/jobs/{job_id}/executions?limit=10&offset=0")).await;
    assert_eq!(list_for_job.status(), StatusCode::OK);

    ctx.log_repo
        .create(&ExecutionLog::stdout(execution_id, "hello from stdout"))
        .await
        .unwrap();
    let logs_response = send_empty(&ctx.app, Method::GET, &format!("/api/v1/executions/{execution_id}/logs?log_type=stdout")).await;
    assert_eq!(logs_response.status(), StatusCode::OK);

    let mut completed_execution = Execution::new(job_id, None);
    completed_execution.mark_running("worker-1");
    completed_execution.mark_success("done".to_string());
    let completed_execution = ctx.execution_repo.create(&completed_execution).await.unwrap();
    let completed_execution_id = completed_execution.id.clone();
    ctx.log_repo
        .create(&ExecutionLog::stdout(&completed_execution_id, "stream me"))
        .await
        .unwrap();

    let stream_response = send_empty(&ctx.app, Method::GET, &format!("/api/v1/executions/{completed_execution_id}/stream")).await;
    assert_eq!(stream_response.status(), StatusCode::OK);

    let cancel_response = send_empty(&ctx.app, Method::POST, &format!("/api/v1/executions/{execution_id}/cancel")).await;
    assert_eq!(cancel_response.status(), StatusCode::OK);

    let retry_response = send_empty(&ctx.app, Method::POST, &format!("/api/v1/executions/{execution_id}/retry")).await;
    assert_eq!(retry_response.status(), StatusCode::OK);
    let retry_json = json_body(retry_response).await;
    let retry_execution_id = retry_json["id"].as_str().unwrap();

    let delete_response = send_empty(&ctx.app, Method::DELETE, &format!("/api/v1/executions/{completed_execution_id}")).await;
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let bulk_delete_response = send_json(
        &ctx.app,
        Method::DELETE,
        "/api/v1/executions/bulk",
        json!({"ids": [execution_id, retry_execution_id]}),
    )
    .await;
    assert_eq!(bulk_delete_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_package_routes_cover_all_endpoints() {
    let ctx = setup_context().await;
    let job = create_job(&ctx.app, "package-routes-job").await;
    let job_id = job["id"].as_str().unwrap();

    seed_ready_package(&ctx, "demo-package", "1.0.0").await;

    let list_response = send_empty(&ctx.app, Method::GET, "/api/v1/packages").await;
    assert_eq!(list_response.status(), StatusCode::OK);

    let search_response = send_empty(&ctx.app, Method::GET, "/api/v1/packages/search?q=definitely-not-real").await;
    assert_eq!(search_response.status(), StatusCode::OK);

    let install_response = send_json(
        &ctx.app,
        Method::POST,
        "/api/v1/packages/install",
        json!({"name": "requests", "version": "2.31.0"}),
    )
    .await;
    assert_eq!(install_response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let uninstall_response = send_json(
        &ctx.app,
        Method::POST,
        "/api/v1/packages/uninstall",
        json!({"name": "demo-package"}),
    )
    .await;
    assert_eq!(uninstall_response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let main_venv_response = send_empty(&ctx.app, Method::GET, "/api/v1/packages/main-venv").await;
    assert_eq!(main_venv_response.status(), StatusCode::OK);

    let update_main_venv_response = send_empty(&ctx.app, Method::POST, "/api/v1/packages/main-venv/update").await;
    assert_eq!(update_main_venv_response.status(), StatusCode::OK);

    let delete_package_response = send_empty(&ctx.app, Method::DELETE, "/api/v1/packages/demo-package/1.0.0").await;
    assert_eq!(delete_package_response.status(), StatusCode::NO_CONTENT);

    let clear_main_venv_response = send_empty(&ctx.app, Method::DELETE, "/api/v1/packages/main-venv").await;
    assert_eq!(clear_main_venv_response.status(), StatusCode::OK);

    let add_dependency_response = send_json(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/jobs/{job_id}/dependencies"),
        json!({"package_name": "requests", "version_constraint": "==2.31.0"}),
    )
    .await;
    assert_eq!(add_dependency_response.status(), StatusCode::CREATED);

    let get_dependencies_response = send_empty(&ctx.app, Method::GET, &format!("/api/v1/jobs/{job_id}/dependencies")).await;
    assert_eq!(get_dependencies_response.status(), StatusCode::OK);

    let update_dependency_response = send_json(
        &ctx.app,
        Method::PUT,
        &format!("/api/v1/jobs/{job_id}/dependencies/requests"),
        json!({"version_constraint": ">=2.0.0,<3.0.0"}),
    )
    .await;
    assert_eq!(update_dependency_response.status(), StatusCode::OK);

    let dependency_status_response = send_empty(&ctx.app, Method::GET, &format!("/api/v1/jobs/{job_id}/dependencies/status")).await;
    assert_eq!(dependency_status_response.status(), StatusCode::OK);

    let install_dependencies_response = send_empty(&ctx.app, Method::POST, &format!("/api/v1/jobs/{job_id}/dependencies/install")).await;
    assert_eq!(install_dependencies_response.status(), StatusCode::OK);

    let remove_dependency_response = send_empty(&ctx.app, Method::DELETE, &format!("/api/v1/jobs/{job_id}/dependencies/requests")).await;
    assert_eq!(remove_dependency_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_venv_monitoring_and_docs_routes_cover_all_endpoints() {
    let ctx = setup_context().await;
    let main_venv = seed_main_venv(&ctx).await;

    let custom_job = create_job(&ctx.app, "venv-job-primary").await;
    let custom_job_id = custom_job["id"].as_str().unwrap();
    let toggle_job = create_job(&ctx.app, "venv-job-toggle").await;
    let toggle_job_id = toggle_job["id"].as_str().unwrap();

    let custom_venv = seed_custom_venv(&ctx, custom_job_id, "/tmp/venvs/custom-a").await;
    let deletable_venv = seed_custom_venv(&ctx, toggle_job_id, "/tmp/venvs/custom-b").await;

    assert_eq!(main_venv.venv_type, "main");
    assert_eq!(custom_venv.venv_type, "custom");

    let list_venvs_response = send_empty(&ctx.app, Method::GET, "/api/v1/venvs").await;
    assert_eq!(list_venvs_response.status(), StatusCode::OK);

    let get_venv_response = send_empty(&ctx.app, Method::GET, &format!("/api/v1/venvs/{}", custom_venv.id)).await;
    assert_eq!(get_venv_response.status(), StatusCode::OK);

    let get_job_venv_info_response = send_empty(&ctx.app, Method::GET, &format!("/api/v1/jobs/{custom_job_id}/venv/info")).await;
    assert_eq!(get_job_venv_info_response.status(), StatusCode::OK);

    let toggle_response = send_empty(&ctx.app, Method::POST, &format!("/api/v1/jobs/{toggle_job_id}/venv/toggle")).await;
    assert_eq!(toggle_response.status(), StatusCode::OK);

    let delete_job_venv_response = send_empty(&ctx.app, Method::DELETE, &format!("/api/v1/jobs/{custom_job_id}/venv")).await;
    assert_eq!(delete_job_venv_response.status(), StatusCode::NO_CONTENT);

    let delete_venv_response = send_empty(&ctx.app, Method::DELETE, &format!("/api/v1/venvs/{}", deletable_venv.id)).await;
    assert_eq!(delete_venv_response.status(), StatusCode::NO_CONTENT);

    let stats_response = send_empty(&ctx.app, Method::GET, "/api/v1/stats").await;
    assert_eq!(stats_response.status(), StatusCode::OK);

    let health_response = send_empty(&ctx.app, Method::GET, "/api/v1/health").await;
    assert_eq!(health_response.status(), StatusCode::OK);

    let workers_response = send_empty(&ctx.app, Method::GET, "/api/v1/workers/status").await;
    assert_eq!(workers_response.status(), StatusCode::OK);

    let queue_response = send_empty(&ctx.app, Method::GET, "/api/v1/queue/status").await;
    assert_eq!(queue_response.status(), StatusCode::OK);

    let openapi_response = send_empty(&ctx.app, Method::GET, "/api/openapi.json").await;
    assert_eq!(openapi_response.status(), StatusCode::OK);

    let swagger_response = send_empty(&ctx.app, Method::GET, "/swagger-ui").await;
    assert!(swagger_response.status().is_success() || swagger_response.status().is_redirection());
}