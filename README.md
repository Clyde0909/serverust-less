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
- Virtual environment support — shared main venv, per-job isolated venvs, or named standalone venvs
- **Per-job venv selection** — assign any named venv to a job via dropdown; `venv_id` stored in DB
- Named venv creation with explicit Python version (resolved from `3.12.3` → `python3.12`)
- Package management per-venv: main venv via DB-backed pip cache, custom venvs via live `pip list`
- Package conflict detection with configurable resolution strategy
- **Scheduled jobs** — cron expressions with automatic execution via background scheduler
- **DAG workflows** — directed acyclic graph job orchestration with topological execution, cycle detection, and failure policies
- Resource limits (timeout, memory)
- Real-time execution log streaming via SSE
- Execution history and structured audit logging
- Background retention policy scheduler (auto-cleanup of old executions and logs)
- Bulk operations (create, delete jobs/executions)
- Job cloning (including `venv_id` propagation)
- Jobs created disabled by default; Execute action auto-enables them
- Web UI — single-page app with CodeMirror (Dracula/Python), modal dialogs, reusable Confirm/Toast/Modal system
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
    ├── DB updates ── sets running / success / failed / timeout status
    │       │
    │       ├── Retry? ── re-queue with delay
    │       └── Max retries exhausted? ── move to Dead Letter Queue
    └── DagEngine callback ── advances DAG runs when a node completes

SchedulerRunner (background)
    │  tick every N seconds
    ├── finds due cron schedules
    ├── creates executions
    └── enqueues via QueueManager

DagEngine
    │  triggered via API or scheduler
    ├── creates DagRun with DagNodeExecutions
    ├── finds root nodes (no upstream deps)
    ├── executes nodes in topological order
    └── respects max_concurrent_nodes and on_failure policy
```

**Key components:**

| Component | Responsibility |
|---|---|
| `QueueManager` | Shared `Arc<>` between API and workers; in-memory priority queue with SQLite overflow, crash-recovery, dead letter queue, and delayed retry |
| `WorkerPool` | Spawns `pool_size` async workers that each dequeue jobs, execute Python, and write results to DB |
| `ProcessManager` | Tracks running process PIDs; `cancel()` sends SIGTERM then escalates to SIGKILL after the grace period |
| `PythonRunner` | Spawns the Python interpreter inside the target venv with stdout/stderr capture and timeout enforcement |
| `PackageService` | Manages pip operations with file-based locking and conflict detection (fail / suggest custom venv / force upgrade) |
| `VenvManager` | Creates/deletes named venvs; resolves Python version strings (e.g. `3.12.3` → `python3.12`); exposes `python_executable()` and `list_packages()` |
| `VenvService` | DB-backed venv lifecycle: `ensure_main_venv()`, `mark_ready()`, `mark_failed()`, LRU eviction helpers |
| `RetentionScheduler` | Background task that periodically cleans old executions, orphaned logs, and stale queue entries |
| `SchedulerRunner` | Background cron tick loop — finds due schedules, creates executions, enqueues them |
| `DagEngine` | DAG workflow orchestrator — triggers runs, advances nodes in topological order, handles failure policies |
| `ScheduleService` | Cron expression validation, schedule CRUD, next-run calculation |
| `DagService` | DAG CRUD, edge management, cycle detection (Kahn's algorithm), topological level analysis |

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
host = "0.0.0.0"
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

[scheduler]
tick_interval_seconds = 10          # how often the scheduler checks for due cron jobs
enabled = true                      # set to false to disable the background scheduler

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
| `GET` | `/api/v1/jobs` | List all jobs (paginated, searchable) |
| `POST` | `/api/v1/jobs` | Create a job (disabled by default; `venv_id` optional) |
| `POST` | `/api/v1/jobs/bulk` | Bulk create multiple jobs |
| `DELETE` | `/api/v1/jobs/bulk` | Bulk delete multiple jobs |
| `GET` | `/api/v1/jobs/{id}` | Get job details |
| `PUT` | `/api/v1/jobs/{id}` | Update a job (including `venv_id`) |
| `DELETE` | `/api/v1/jobs/{id}` | Delete a job |
| `POST` | `/api/v1/jobs/{id}/execute` | Enqueue a job (auto-enables if disabled) |
| `POST` | `/api/v1/jobs/{id}/clone` | Clone a job (copies `venv_id`) |
| `POST` | `/api/v1/jobs/{id}/enable` | Enable a job |
| `POST` | `/api/v1/jobs/{id}/disable` | Disable a job |
| `GET` | `/api/v1/jobs/{id}/dependencies` | List job package dependencies |
| `POST` | `/api/v1/jobs/{id}/dependencies` | Add a package dependency to a job |
| `PUT` | `/api/v1/jobs/{id}/dependencies/{name}` | Update dependency version |
| `DELETE` | `/api/v1/jobs/{id}/dependencies/{name}` | Remove a dependency |
| `POST` | `/api/v1/jobs/{id}/dependencies/install` | Install all job dependencies |
| `GET` | `/api/v1/jobs/{id}/dependencies/status` | Check dependency installation status |
| `GET` | `/api/v1/jobs/{id}/executions` | List executions for a specific job |

### Executions

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/executions` | List executions (filterable by status, job, date range; paginated) |
| `GET` | `/api/v1/executions/{id}` | Get execution details |
| `DELETE` | `/api/v1/executions/{id}` | Delete an execution record |
| `DELETE` | `/api/v1/executions/bulk` | Bulk delete multiple executions |
| `POST` | `/api/v1/executions/{id}/cancel` | Cancel a running execution (SIGTERM → SIGKILL) |
| `POST` | `/api/v1/executions/{id}/retry` | Re-enqueue a failed/cancelled execution |
| `GET` | `/api/v1/executions/{id}/logs` | Get execution logs |
| `GET` | `/api/v1/executions/{id}/stream` | Stream logs in real-time via SSE |

### Packages

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/packages` | List installed packages in main venv (DB-backed) |
| `POST` | `/api/v1/packages/install` | Install a package to main venv (with conflict detection) |
| `POST` | `/api/v1/packages/uninstall` | Uninstall a package from main venv |
| `GET` | `/api/v1/packages/search` | Search PyPI for packages |
| `GET` | `/api/v1/packages/main-venv` | Get main venv package list and status |
| `POST` | `/api/v1/packages/main-venv/update` | Update all packages in main venv |
| `DELETE` | `/api/v1/packages/main-venv` | Clear and recreate main venv |
| `DELETE` | `/api/v1/packages/{name}/{version}` | Remove a package from cache |

### Virtual Environments

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/venvs` | List all virtual environments |
| `POST` | `/api/v1/venvs` | Create a named standalone venv (specify Python version) |
| `GET` | `/api/v1/venvs/{id}` | Get venv details |
| `GET` | `/api/v1/venvs/{id}/packages` | List packages installed in a venv (live `pip list`) |
| `DELETE` | `/api/v1/venvs/{id}` | Delete a custom venv |
| `GET` | `/api/v1/jobs/{id}/venv/info` | Get the venv currently assigned to a job |
| `POST` | `/api/v1/jobs/{id}/venv/toggle` | Toggle job between main venv and its own custom venv |
| `DELETE` | `/api/v1/jobs/{id}/venv` | Delete job-associated custom venv |

### Queue

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/queue/status` | Current queue status, depth, and dead letter queue info |

### Schedules

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/v1/jobs/{id}/schedule` | Create a cron schedule for a job |
| `GET` | `/api/v1/jobs/{id}/schedule` | Get schedule for a job |
| `PUT` | `/api/v1/jobs/{id}/schedule` | Update schedule |
| `DELETE` | `/api/v1/jobs/{id}/schedule` | Remove schedule |
| `POST` | `/api/v1/jobs/{id}/schedule/toggle` | Enable/disable schedule |
| `GET` | `/api/v1/schedules` | List all schedules |

### DAGs (Directed Acyclic Graphs)

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/v1/dags` | Create a new DAG workflow |
| `GET` | `/api/v1/dags` | List all DAGs |
| `GET` | `/api/v1/dags/{id}` | Get DAG details (with edges) |
| `PUT` | `/api/v1/dags/{id}` | Update DAG metadata |
| `DELETE` | `/api/v1/dags/{id}` | Delete a DAG |
| `POST` | `/api/v1/dags/{id}/edges` | Add an edge between two jobs |
| `DELETE` | `/api/v1/dags/{dag_id}/edges/{edge_id}` | Remove an edge |
| `GET` | `/api/v1/dags/{id}/topology` | Get topological levels |
| `POST` | `/api/v1/dags/{id}/validate` | Validate DAG (cycles, job existence) |
| `POST` | `/api/v1/dags/{id}/trigger` | Trigger a DAG run |
| `GET` | `/api/v1/dags/{id}/runs` | List runs for a DAG |
| `GET` | `/api/v1/dags/{dag_id}/runs/{run_id}` | Get run details (with node statuses) |
| `POST` | `/api/v1/dags/{dag_id}/runs/{run_id}/cancel` | Cancel a running DAG |

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
│   │   ├── schedules.rs        # cron schedule CRUD, toggle
│   │   ├── dags.rs             # DAG CRUD, edges, topology, trigger, runs
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
│   ├── scheduler/
│   │   └── mod.rs              # SchedulerRunner: background cron tick loop
│   ├── dag/
│   │   ├── mod.rs
│   │   └── engine.rs           # DagEngine: trigger, advance, cancel DAG runs
│   ├── services/               # Business logic layer
│   │   ├── execution_service.rs
│   │   ├── job_service.rs
│   │   ├── schedule_service.rs # Cron validation, next-run calculation
│   │   ├── dag_service.rs      # Cycle detection, topology, DAG CRUD
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
├── migrations/                 # SQLite schema migrations (18 files)
├── web/                        # Web UI (HTML / CSS / vanilla JS SPA)
│   ├── index.html
│   ├── css/
│   └── js/
│       ├── api.js              # API client
│       ├── app.js              # SPA router and layout
│       └── components/         # UI components (job-list, job-form, execution-history, packages, venvs, queue)
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

## Testing

The project includes a comprehensive integration test suite organized into five test files. All tests use in-memory SQLite databases, so no external infrastructure is required.

```bash
# Run all tests
cargo test

# Run a specific test suite
cargo test --test error_scenarios
cargo test --test cancellation_tests
cargo test --test conflict_resolution_tests
cargo test --test performance_tests

# Run with output (see timing info from performance tests)
cargo test --test performance_tests -- --nocapture
```

### Test Suites

| File | Category | Tests | Description |
|---|---|---|---|
| `tests/api_tests.rs` | Integration | 5 | Core API smoke tests — health, workers, queue status, CRUD, executions |
| `tests/api_endpoint_coverage.rs` | Coverage | 4 | Full endpoint coverage for all route groups |
| `tests/error_scenarios.rs` | Error handling | 20+ | 404 on missing resources, 422 on invalid input, disabled-job execution, bulk edge cases, pagination boundaries |
| `tests/cancellation_tests.rs` | Lifecycle | 7 | Cancel pending, reject double-cancel, retry cancelled, max-retry enforcement, multi-execution cancel |
| `tests/conflict_resolution_tests.rs` | Dependencies | 10+ | Dependency CRUD, status tracking, "fail" conflict strategy, cross-job isolation, package validation |
| `tests/performance_tests.rs` | Performance | 8 | Concurrent creation, bulk delete, pagination walkthrough, rapid CRUD cycles, health under load |

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
