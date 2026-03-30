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

// ===== Sequential Bulk Job Creation =====

#[tokio::test]
async fn test_create_many_jobs_sequentially() {
    let app = setup().await;
    let count = 50;

    let start = Instant::now();
    for i in 0..count {
        let resp = send_json(
            &app,
            Method::POST,
            "/api/v1/jobs",
            json!({
                "name": format!("perf-seq-job-{i}"),
                "python_code": format!("print('job {i}')"),
                "timeout_seconds": 30,
                "memory_limit_mb": 128,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
    }
    let elapsed = start.elapsed();
    eprintln!("Created {count} jobs sequentially in {:?}", elapsed);

    // Verify list returns all
    let resp = send_empty(&app, Method::GET, &format!("/api/v1/jobs?limit=100&offset=0")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["total"].as_i64().unwrap(), count);
}

// ===== Concurrent Job Creation =====

#[tokio::test]
async fn test_create_jobs_concurrently() {
    let app = setup().await;
    let count = 20;

    let start = Instant::now();
    let mut handles = Vec::new();
    for i in 0..count {
        let app_clone = app.clone();
        handles.push(tokio::spawn(async move {
            let resp = app_clone
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/jobs")
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            json!({
                                "name": format!("perf-concurrent-{i}"),
                                "python_code": format!("print('concurrent {i}')"),
                                "timeout_seconds": 30,
                                "memory_limit_mb": 128,
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            resp.status()
        }));
    }

    let mut success = 0;
    for handle in handles {
        let status = handle.await.unwrap();
        if status == StatusCode::CREATED {
            success += 1;
        }
    }
    let elapsed = start.elapsed();
    eprintln!("Created {success}/{count} jobs concurrently in {:?}", elapsed);

    assert_eq!(success, count, "All concurrent creations should succeed");
}

// ===== Concurrent Execution Requests =====

#[tokio::test]
async fn test_execute_job_concurrently() {
    let app = setup().await;
    let resp = send_json(
        &app,
        Method::POST,
        "/api/v1/jobs",
        json!({
            "name": "perf-exec-target",
            "python_code": "print('hello')",
            "timeout_seconds": 30,
            "memory_limit_mb": 128,
        }),
    )
    .await;
    let job = json_body(resp).await;
    let job_id = job["id"].as_str().unwrap().to_string();

    let concurrent = 10;
    let mut handles = Vec::new();
    for _ in 0..concurrent {
        let app_clone = app.clone();
        let jid = job_id.clone();
        handles.push(tokio::spawn(async move {
            let resp = app_clone
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/jobs/{jid}/execute"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            resp.status()
        }));
    }

    let mut ok_count = 0;
    for handle in handles {
        if handle.await.unwrap() == StatusCode::CREATED {
            ok_count += 1;
        }
    }
    assert_eq!(ok_count, concurrent, "All concurrent executions should succeed");

    // Verify execution count matches
    let resp = send_empty(
        &app,
        Method::GET,
        &format!("/api/v1/jobs/{job_id}/executions?limit=100&offset=0"),
    )
    .await;
    let body = json_body(resp).await;
    assert_eq!(body["total"].as_i64().unwrap(), concurrent as i64);
}

// ===== Pagination: Full Walkthrough =====

#[tokio::test]
async fn test_pagination_walk_all_pages() {
    let app = setup().await;
    let total_count = 25;

    // Create jobs
    for i in 0..total_count {
        send_json(
            &app,
            Method::POST,
            "/api/v1/jobs",
            json!({
                "name": format!("page-job-{i}"),
                "python_code": "x = 1",
                "timeout_seconds": 10,
                "memory_limit_mb": 64,
            }),
        )
        .await;
    }

    let page_size = 10;
    let mut fetched_ids: Vec<String> = Vec::new();
    let mut offset = 0;

    loop {
        let resp = send_empty(
            &app,
            Method::GET,
            &format!("/api/v1/jobs?limit={page_size}&offset={offset}"),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;

        let jobs = body["jobs"].as_array().unwrap();
        if jobs.is_empty() {
            break;
        }
        assert_eq!(body["total"].as_i64().unwrap(), total_count);

        for j in jobs {
            fetched_ids.push(j["id"].as_str().unwrap().to_string());
        }
        offset += page_size;
    }

    assert_eq!(fetched_ids.len(), total_count as usize);
    // All IDs should be unique
    let unique: std::collections::HashSet<_> = fetched_ids.iter().collect();
    assert_eq!(unique.len(), total_count as usize, "All IDs must be unique across pages");
}

// ===== Bulk Delete Performance =====

#[tokio::test]
async fn test_bulk_delete_jobs() {
    let app = setup().await;

    let mut ids = Vec::new();
    for i in 0..15 {
        let resp = send_json(
            &app,
            Method::POST,
            "/api/v1/jobs",
            json!({
                "name": format!("bulk-del-{i}"),
                "python_code": "x = 1",
                "timeout_seconds": 10,
                "memory_limit_mb": 64,
            }),
        )
        .await;
        let job = json_body(resp).await;
        ids.push(job["id"].as_str().unwrap().to_string());
    }

    let start = Instant::now();
    let resp = send_json(
        &app,
        Method::DELETE,
        "/api/v1/jobs/bulk",
        json!({"ids": ids}),
    )
    .await;
    let elapsed = start.elapsed();
    eprintln!("Bulk deleted {} jobs in {:?}", ids.len(), elapsed);
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify empty
    let resp = send_empty(&app, Method::GET, "/api/v1/jobs?limit=100&offset=0").await;
    let body = json_body(resp).await;
    assert_eq!(body["total"].as_i64().unwrap(), 0);
}

// ===== Bulk Delete Executions =====

#[tokio::test]
async fn test_bulk_delete_executions() {
    let app = setup().await;
    let resp = send_json(
        &app,
        Method::POST,
        "/api/v1/jobs",
        json!({
            "name": "bulk-exec-del",
            "python_code": "x = 1",
            "timeout_seconds": 10,
            "memory_limit_mb": 64,
        }),
    )
    .await;
    let job = json_body(resp).await;
    let job_id = job["id"].as_str().unwrap();

    let mut exec_ids = Vec::new();
    for _ in 0..10 {
        let resp = send_json(
            &app,
            Method::POST,
            &format!("/api/v1/jobs/{job_id}/execute"),
            json!({}),
        )
        .await;
        let exec = json_body(resp).await;
        exec_ids.push(exec["id"].as_str().unwrap().to_string());
    }

    let resp = send_json(
        &app,
        Method::DELETE,
        "/api/v1/executions/bulk",
        json!({"ids": exec_ids}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = send_empty(
        &app,
        Method::GET,
        &format!("/api/v1/jobs/{job_id}/executions?limit=100&offset=0"),
    )
    .await;
    let body = json_body(resp).await;
    assert_eq!(body["total"].as_i64().unwrap(), 0);
}

// ===== Health Endpoint Under Load =====

#[tokio::test]
async fn test_health_endpoint_under_load() {
    let app = setup().await;
    let count = 50;

    let mut handles = Vec::new();
    for _ in 0..count {
        let app_clone = app.clone();
        handles.push(tokio::spawn(async move {
            let resp = app_clone
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/health")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            resp.status()
        }));
    }

    let mut ok = 0;
    for h in handles {
        if h.await.unwrap() == StatusCode::OK {
            ok += 1;
        }
    }
    assert_eq!(ok, count, "All health checks should succeed");
}

// ===== Rapid Create-Read-Delete Cycle =====

#[tokio::test]
async fn test_rapid_create_read_delete_cycle() {
    let app = setup().await;

    let start = Instant::now();
    for i in 0..20 {
        let name = format!("rapid-cycle-{i}");

        // Create
        let resp = send_json(
            &app,
            Method::POST,
            "/api/v1/jobs",
            json!({"name": name, "python_code": "x = 1", "timeout_seconds": 10, "memory_limit_mb": 64}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let job = json_body(resp).await;
        let id = job["id"].as_str().unwrap();

        // Read
        let resp = send_empty(&app, Method::GET, &format!("/api/v1/jobs/{id}")).await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Delete
        let resp = send_empty(&app, Method::DELETE, &format!("/api/v1/jobs/{id}")).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify 404
        let resp = send_empty(&app, Method::GET, &format!("/api/v1/jobs/{id}")).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
    let elapsed = start.elapsed();
    eprintln!("20 create-read-delete cycles in {:?}", elapsed);
}
