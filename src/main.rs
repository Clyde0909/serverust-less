//! Serverust-Less - AWS Lambda-like serverless platform for Python REPL execution

use std::net::SocketAddr;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use serverust_less::api::{create_router, AppState};
use serverust_less::config::AppConfig;
use serverust_less::db::{init_pool, run_migrations, JobRepository};
use serverust_less::services::JobService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting Serverust-Less...");

    // Load configuration
    let config = AppConfig::load()?;
    info!("Configuration loaded");

    // Ensure database directory exists
    if let Some(parent) = config.database.path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
            info!("Created database directory: {}", parent.display());
        }
    }

    // Initialize database
    let db_url = format!("sqlite:{}?mode=rwc", config.database.path.display());
    let pool = init_pool(&db_url).await?;
    info!("Database connection established");

    // Run migrations
    run_migrations(&pool).await?;
    info!("Database migrations completed");

    // Initialize repositories
    let job_repo = JobRepository::new(pool.clone());

    // Initialize services
    let job_service = JobService::new(job_repo);

    // Create app state
    let state = AppState { job_service };

    // Create router
    let app = create_router(state);

    // Start server
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .expect("Invalid address");

    info!("Server listening on http://{}", addr);
    info!("Swagger UI available at http://{}/swagger-ui/", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
