# Serverust-Less

A self-hosted serverless platform for Python code execution, built with Rust. Similar to AWS Lambda but lightweight and easy to deploy.

## Features

- Execute Python code on-demand via REST API
- Concurrent worker pool with configurable parallelism
- Priority-based job queue with in-memory heap and SQLite overflow persistence
- Dead letter queue for jobs that exhaust retries
- Automatic re-queue with configurable delay on failure
- Automatic queue recovery on restart
- Process lifecycle management (graceful cancel / SIGKILL escalation)
- Virtual environment support — shared main venv or per-job isolated venvs
- Package dependency management with PyPI integration
- Package conflict detection with configurable resolution strategy
- Resource limits (timeout, memory)
- Real-time execution log streaming via SSE
- Execution history and structured audit logging
- Background retention policy scheduler (auto-cleanup of old executions and logs)
- Bulk operations (create, delete jobs/executions)
- Job cloning
- Web UI for job management, monitoring, and package management
- OpenAPI/Swagger documentation

## Architecture

```
HTTP Request
    │
    ▼
API Handler (axum)
    │  enqueue()
    ▼
QueueManager ──overflow──► SQLite (queue table)
    │  (in-memory BinaryHeap + SQLite overflow)
    │  dequeue()
    ▼
WorkerPool  (N concurrent tokio tasks)
    │
    ├── PythonRunner  ── executes job code inside venv
    ├── ProcessManager ── tracks PIDs, handles cancel/SIGTERM/SIGKILL
    └── DB updates ── sets running / success / failed / timeout status
            │
            ├── Retry? ── re-queue with delay
            └── Max retries exhausted? ── move to Dead Letter Queue
```

**Key components:**

| Component | Responsibility |
|---|---|
| `QueueManager` | Shared `Arc<>` between API and workers; in-memory priority queue with SQLite overflow, crash-recovery, dead letter queue, and delayed retry |
| `WorkerPool` | Spawns `pool_size` async workers that each dequeue jobs, execute Python, and write results to DB |
| `ProcessManager` | Tracks running process PIDs; `cancel()` sends SIGTERM then escalates to SIGKILL after the grace period |
| `PythonRunner` | Spawns the Python interpreter inside the target venv with stdout/stderr capture and timeout enforcement |
| `PackageService` | Manages pip operations with file-based locking and conflict detection (fail / suggest custom venv / force upgrade) |
| `RetentionScheduler` | Background task that periodically cleans old executions, orphaned logs, and stale queue entries |

## Requirements

- Rust 1.70+
- Python 3.8+
- SQLite 3
- OpenSSL development headers

### Ubuntu / Debian

```bash
sudo apt install build-essential libssl-dev pkg-config python3 python3-venv
```

### Windows

- Install [Rust](https://rustup.rs/)
- Ensure `python` is on your PATH (note: `python_executable` defaults to `python3.12`; on Windows you may need to set it to `python` or `py -3.12` in config)

## Quick Start

1. Clone the repository:

```bash
git clone https://github.com/Clyde0909/serverust-less.git
cd serverust-less
```

2. Build and run:

```bash
cargo build --release
cargo run --release
```

3. Access the application:

- Web UI: http://localhost:8080/
- Swagger UI: http://localhost:8080/swagger-ui/
- OpenAPI JSON: http://localhost:8080/api/openapi.json

The main Python venv (`venvs/main`) is created automatically on first run if it does not exist.

## Configuration

Configuration is loaded from `config/default.toml`. All values have built-in defaults and can be overridden via environment variables using the `SERVERUST__` prefix (double underscore as separator).

```toml
[server]
host = "127.0.0.1"
port = 8080
log_level = "info"

[database]
path = "./data/serverust.db"
max_connections = 10

[worker]
pool_size = 4                       # concurrent Python workers
default_timeout_seconds = 30
default_memory_limit_mb = 128
python_executable = "python3.12"    # use "python" or "py -3.12" on Windows if needed
graceful_shutdown_seconds = 30      # SIGTERM → SIGKILL grace period

[queue]
max_size = 1000                     # in-memory queue capacity before overflow to SQLite
persistence_enabled = true
retry_delay_seconds = 5             # delay before re-queuing a failed job
max_retries = 3                     # max retries before moving to dead letter queue

[packages]
main_venv_path = "./venvs/main"
custom_venv_base_path = "./venvs"
pip_cache_dir = "./cache/pip"
max_cache_size_mb = 5000
max_custom_venvs = 50
pip_timeout_seconds = 300

[retention]
execution_history_days = 30         # delete terminal executions older than N days
log_max_size_bytes = 1048576        # 1 MB max per execution log
cleanup_interval_hours = 24         # how often the retention scheduler runs

[security]
enable_auth = false
enable_multitenancy = false
enable_audit_log = true
```

## REST API Endpoints

All API endpoints are prefixed with `/api/v1/`.

### Jobs

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/jobs` | List all jobs (paginated) |
| `POST` | `/api/v1/jobs` | Create a job |
| `POST` | `/api/v1/jobs/bulk` | Bulk create multiple jobs |
| `GET` | `/api/v1/jobs/{id}` | Get job details |
| `PUT` | `/api/v1/jobs/{id}` | Update a job |
| `DELETE` | `/api/v1/jobs/{id}` | Delete a job |
| `POST` | `/api/v1/jobs/{id}/execute` | Enqueue a job for execution |
| `POST` | `/api/v1/jobs/{id}/clone` | Clone a job |
| `POST` | `/api/v1/jobs/{id}/enable` | Enable a job |
| `POST` | `/api/v1/jobs/{id}/disable` | Disable a job |
| `GET` | `/api/v1/jobs/{id}/dependencies` | Get job package dependencies |
| `PUT` | `/api/v1/jobs/{id}/dependencies` | Update job package dependencies |

### Executions

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/executions` | List executions (filterable, paginated) |
| `GET` | `/api/v1/executions/{id}` | Get execution details |
| `DELETE` | `/api/v1/executions/{id}` | Delete an execution |
| `POST` | `/api/v1/executions/{id}/cancel` | Cancel a running execution (SIGTERM → SIGKILL) |
| `POST` | `/api/v1/executions/{id}/retry` | Re-enqueue a failed/cancelled execution |
| `GET` | `/api/v1/executions/{id}/logs` | Get execution logs |
| `GET` | `/api/v1/executions/{id}/stream` | Stream logs in real-time via SSE |
| `POST` | `/api/v1/executions/bulk-delete` | Bulk delete multiple executions |

### Packages

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/packages` | List installed packages in main venv |
| `POST` | `/api/v1/packages/install` | Install a package (with conflict detection) |
| `POST` | `/api/v1/packages/uninstall` | Uninstall a package |
| `GET` | `/api/v1/packages/search` | Search PyPI for packages |
| `GET` | `/api/v1/packages/main-venv-status` | Get main venv status |

### Virtual Environments

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/venvs` | List virtual environments |
| `GET` | `/api/v1/venvs/{id}` | Get venv details (including installed packages) |
| `DELETE` | `/api/v1/venvs/{id}` | Delete a custom venv |
| `POST` | `/api/v1/venvs/{id}/toggle` | Toggle custom venv active/inactive |

### Queue

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/queue/status` | Current queue status, depth, and dead letter queue info |

### Health & Monitoring

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/health` | Health check |
| `GET` | `/api/v1/stats` | System statistics (job/execution counts) |
| `GET` | `/api/v1/workers/status` | Worker pool status |

### OpenAPI

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/openapi.json` | OpenAPI 3.0 spec (JSON) |
| `GET` | `/swagger-ui/` | Interactive Swagger UI |

## Project Structure

```
serverust-less/
├── src/
│   ├── api/                    # REST API handlers (axum)
│   │   ├── executions.rs       # execution lifecycle, cancel, retry, bulk delete
│   │   ├── health.rs           # health check, stats, worker status
│   │   ├── jobs.rs             # CRUD, execute, clone, enable/disable, dependencies
│   │   ├── packages.rs         # install, uninstall, search, venv status
│   │   ├── queue.rs            # queue status
│   │   ├── venvs.rs            # list, get, delete, toggle venvs
│   │   └── mod.rs              # AppState, router, OpenAPI setup
│   ├── db/                     # SQLite repositories (sqlx)
│   │   ├── executions.rs
│   │   ├── jobs.rs
│   │   ├── logs.rs
│   │   ├── venvs.rs
│   │   └── mod.rs
│   ├── models/                 # Data structs, DTOs, enums
│   ├── queue/
│   │   └── manager.rs          # QueueManager: priority heap + SQLite overflow + DLQ + recovery
│   ├── services/               # Business logic layer
│   │   ├── execution_service.rs
│   │   ├── job_service.rs
│   │   └── mod.rs
│   ├── worker/
│   │   ├── pool.rs             # WorkerPool: concurrent job executors
│   │   ├── executor.rs         # Job execution coordinator
│   │   ├── process_manager.rs  # PID tracking, graceful cancel
│   │   ├── python_runner.rs    # Python subprocess execution
│   │   ├── venv_manager.rs     # Venv creation and management
│   │   └── package_manager.rs  # pip operations and conflict detection
│   ├── config.rs               # Configuration structs and loading
│   ├── error.rs                # Unified error type
│   ├── lib.rs                  # Crate exports
│   └── main.rs                 # Startup: DB, migrations, queue recovery, retention scheduler, worker pool, HTTP server
├── migrations/                 # SQLite schema migrations (15 files)
├── web/                        # Web UI (HTML / CSS / vanilla JS SPA)
│   ├── index.html
│   ├── css/
│   └── js/
│       ├── api.js              # API client
│       ├── app.js              # SPA router and layout
│       └── components/         # UI components (job-list, execution-history, packages, venvs)
├── config/
│   └── default.toml            # Default configuration
├── venvs/                      # Python virtual environments (auto-created)
└── data/                       # SQLite database file (auto-created)
```

## Development

```bash
# Type-check without linking
cargo check

# Run with debug logging
RUST_LOG=debug cargo run

# Lint
cargo clippy

# Tests
cargo test
```

## Logging

### Backend

The backend uses the `tracing` crate. Log level is controlled by the `RUST_LOG` environment variable:

```bash
# Default (info + debug for app code and tower_http)
cargo run

# Verbose
RUST_LOG=debug cargo run

# Trace SQL and worker details
RUST_LOG=trace cargo run

# Quiet
RUST_LOG=warn cargo run

# Granular
RUST_LOG=info,serverust_less::worker=trace,tower_http=debug cargo run
```

Verbosity order: `trace` > `debug` > `info` > `warn` > `error`

### Frontend

The Web UI includes a `Logger` utility that writes to the browser console. It is disabled by default.

```javascript
Logger.enable();   // turn on (persists via localStorage)
Logger.disable();  // turn off
```

When enabled, the frontend logs API request timings, navigation events, component lifecycle, and errors.

## License

MIT License
