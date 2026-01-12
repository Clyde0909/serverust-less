//! Database layer module

pub mod audit;
pub mod execution_logs;
pub mod executions;
pub mod jobs;
pub mod packages;
pub mod queue;
pub mod venvs;

pub use audit::AuditRepository;
pub use execution_logs::ExecutionLogRepository;
pub use executions::ExecutionRepository;
pub use jobs::JobRepository;
pub use packages::PackageRepository;
pub use queue::QueueRepository;
pub use venvs::VenvRepository;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::time::Duration;

/// Initialize the database connection pool
pub async fn init_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    SqlitePoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
}

/// Run database migrations
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
