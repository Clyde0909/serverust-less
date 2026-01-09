//! Application configuration

use serde::Deserialize;
use std::path::PathBuf;

/// Main application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub worker: WorkerConfig,
}

/// Server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

/// Database configuration
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_path")]
    pub path: PathBuf,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

/// Worker configuration
#[derive(Debug, Clone, Deserialize)]
pub struct WorkerConfig {
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,
    #[serde(default = "default_timeout")]
    pub default_timeout_seconds: u64,
    #[serde(default = "default_memory_limit")]
    pub default_memory_limit_mb: u64,
}

// Default value functions
fn default_host() -> String { "127.0.0.1".to_string() }
fn default_port() -> u16 { 8080 }
fn default_log_level() -> String { "info".to_string() }
fn default_db_path() -> PathBuf { PathBuf::from("./data/serverust.db") }
fn default_max_connections() -> u32 { 10 }
fn default_pool_size() -> usize { 4 }
fn default_timeout() -> u64 { 30 }
fn default_memory_limit() -> u64 { 128 }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: default_host(),
                port: default_port(),
                log_level: default_log_level(),
            },
            database: DatabaseConfig {
                path: default_db_path(),
                max_connections: default_max_connections(),
            },
            worker: WorkerConfig {
                pool_size: default_pool_size(),
                default_timeout_seconds: default_timeout(),
                default_memory_limit_mb: default_memory_limit(),
            },
        }
    }
}

impl AppConfig {
    /// Load configuration from file and environment
    pub fn load() -> anyhow::Result<Self> {
        let config = config::Config::builder()
            .add_source(config::File::with_name("config/default").required(false))
            .add_source(config::Environment::with_prefix("SERVERUST").separator("__"))
            .build()?;

        let app_config: AppConfig = config.try_deserialize().unwrap_or_default();
        Ok(app_config)
    }
}
