//! Application configuration

use serde::Deserialize;
use std::path::PathBuf;

/// Main application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub worker: WorkerConfig,
    #[serde(default)]
    pub queue: QueueConfig,
    #[serde(default)]
    pub packages: PackagesConfig,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
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
    #[serde(default)]
    pub cors: CorsConfig,
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
}

/// Rate limiting configuration
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    /// Whether rate limiting is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum requests per second (global, approximate)
    #[serde(default = "default_rate_limit_rps")]
    pub requests_per_second: u64,
    /// Maximum burst size
    #[serde(default = "default_rate_limit_burst")]
    pub burst_size: u64,
}

/// CORS configuration
#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub allowed_methods: Vec<String>,
    #[serde(default = "default_cors_max_age")]
    pub max_age_seconds: u64,
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
    #[serde(default = "default_python_executable")]
    pub python_executable: String,
    #[serde(default)]
    pub python_version: Option<String>,
    #[serde(default = "default_true")]
    pub enable_process_isolation: bool,
    #[serde(default = "default_graceful_shutdown")]
    pub graceful_shutdown_seconds: u64,
}

/// Queue configuration
#[derive(Debug, Clone, Deserialize)]
pub struct QueueConfig {
    #[serde(default = "default_queue_max_size")]
    pub max_size: usize,
    #[serde(default = "default_true")]
    pub persistence_enabled: bool,
    #[serde(default = "default_retry_delay")]
    pub retry_delay_seconds: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
}

/// Packages configuration
#[derive(Debug, Clone, Deserialize)]
pub struct PackagesConfig {
    #[serde(default = "default_main_venv_path")]
    pub main_venv_path: PathBuf,
    #[serde(default = "default_custom_venv_base_path")]
    pub custom_venv_base_path: PathBuf,
    #[serde(default = "default_pip_cache_dir")]
    pub pip_cache_dir: PathBuf,
    #[serde(default = "default_max_cache_size")]
    pub max_cache_size_mb: u64,
    #[serde(default = "default_max_custom_venvs")]
    pub max_custom_venvs: usize,
    #[serde(default = "default_pip_timeout")]
    pub pip_timeout_seconds: u64,
    #[serde(default = "default_true")]
    pub enable_pip_cache: bool,
    #[serde(default = "default_true")]
    pub auto_install_dependencies: bool,
    #[serde(default)]
    pub allow_prerelease: bool,
    #[serde(default = "default_true")]
    pub auto_suggest_custom_venv: bool,
    #[serde(default = "default_pip_index_url")]
    pub pip_index_url: String,
    #[serde(default)]
    pub pip_trusted_hosts: Vec<String>,
    #[serde(default)]
    pub conflict_resolution: ConflictResolutionConfig,
}

/// Conflict resolution configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ConflictResolutionConfig {
    #[serde(default = "default_conflict_strategy")]
    pub strategy: String,
}

/// Retention configuration
#[derive(Debug, Clone, Deserialize)]
pub struct RetentionConfig {
    #[serde(default = "default_execution_history_days")]
    pub execution_history_days: u32,
    #[serde(default = "default_log_max_size")]
    pub log_max_size_bytes: usize,
    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval_hours: u32,
}

/// Security configuration
#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub enable_auth: bool,
    #[serde(default)]
    pub enable_multitenancy: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_true")]
    pub enable_audit_log: bool,
    #[serde(default)]
    pub blocked_packages: Vec<String>,
}

/// Scheduler configuration
#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerConfig {
    #[serde(default = "default_tick_interval")]
    pub tick_interval_seconds: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
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
fn default_python_executable() -> String { "python3".to_string() }
fn default_graceful_shutdown() -> u64 { 30 }
fn default_queue_max_size() -> usize { 1000 }
fn default_retry_delay() -> u64 { 5 }
fn default_max_retries() -> i32 { 3 }
fn default_main_venv_path() -> PathBuf { PathBuf::from("./venvs/main") }
fn default_custom_venv_base_path() -> PathBuf { PathBuf::from("./venvs") }
fn default_pip_cache_dir() -> PathBuf { PathBuf::from("./cache/pip") }
fn default_max_cache_size() -> u64 { 5000 }
fn default_max_custom_venvs() -> usize { 50 }
fn default_pip_timeout() -> u64 { 300 }
fn default_pip_index_url() -> String { "https://pypi.org/simple".to_string() }
fn default_conflict_strategy() -> String { "suggest_custom_venv".to_string() }
fn default_execution_history_days() -> u32 { 30 }
fn default_log_max_size() -> usize { 1048576 }
fn default_cleanup_interval() -> u32 { 24 }
fn default_cors_max_age() -> u64 { 3600 }
fn default_true() -> bool { true }
fn default_tick_interval() -> u64 { 10 }
fn default_rate_limit_rps() -> u64 { 100 }
fn default_rate_limit_burst() -> u64 { 200 }

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_second: default_rate_limit_rps(),
            burst_size: default_rate_limit_burst(),
        }
    }
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_origins: vec![],
            allowed_methods: vec![],
            max_age_seconds: default_cors_max_age(),
        }
    }
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_size: default_queue_max_size(),
            persistence_enabled: true,
            retry_delay_seconds: default_retry_delay(),
            max_retries: default_max_retries(),
        }
    }
}

impl Default for PackagesConfig {
    fn default() -> Self {
        Self {
            main_venv_path: default_main_venv_path(),
            custom_venv_base_path: default_custom_venv_base_path(),
            pip_cache_dir: default_pip_cache_dir(),
            max_cache_size_mb: default_max_cache_size(),
            max_custom_venvs: default_max_custom_venvs(),
            pip_timeout_seconds: default_pip_timeout(),
            enable_pip_cache: true,
            auto_install_dependencies: true,
            allow_prerelease: false,
            auto_suggest_custom_venv: true,
            pip_index_url: default_pip_index_url(),
            pip_trusted_hosts: vec![],
            conflict_resolution: ConflictResolutionConfig::default(),
        }
    }
}

impl Default for ConflictResolutionConfig {
    fn default() -> Self {
        Self {
            strategy: default_conflict_strategy(),
        }
    }
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            execution_history_days: default_execution_history_days(),
            log_max_size_bytes: default_log_max_size(),
            cleanup_interval_hours: default_cleanup_interval(),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enable_auth: false,
            enable_multitenancy: false,
            api_key: None,
            enable_audit_log: true,
            blocked_packages: vec![],
        }
    }
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval_seconds: default_tick_interval(),
            enabled: true,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: default_host(),
                port: default_port(),
                log_level: default_log_level(),
                cors: CorsConfig::default(),
                rate_limit: RateLimitConfig::default(),
            },
            database: DatabaseConfig {
                path: default_db_path(),
                max_connections: default_max_connections(),
            },
            worker: WorkerConfig {
                pool_size: default_pool_size(),
                default_timeout_seconds: default_timeout(),
                default_memory_limit_mb: default_memory_limit(),
                python_executable: default_python_executable(),
                python_version: None,
                enable_process_isolation: true,
                graceful_shutdown_seconds: default_graceful_shutdown(),
            },
            queue: QueueConfig::default(),
            packages: PackagesConfig::default(),
            retention: RetentionConfig::default(),
            security: SecurityConfig::default(),
            scheduler: SchedulerConfig::default(),
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
