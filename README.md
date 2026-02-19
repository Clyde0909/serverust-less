# Serverust-Less

A self-hosted serverless platform for Python code execution, built with Rust. Similar to AWS Lambda but lightweight and easy to deploy.

## Features

- Execute Python code on-demand via REST API
- Concurrent worker pool with configurable parallelism
- Priority-based job queue with in-memory heap and SQLite overflow persistence
- Automatic queue recovery on restart
- Process lifecycle management (graceful cancel / SIGKILL escalation)
- Virtual environment support — shared main venv or per-job isolated venvs
- Package dependency management with PyPI integration
- Resource limits (timeout, memory)
- Real-time execution log streaming via SSE
- Execution history and structured audit logging
- Web UI for job management and monitoring
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
```

**Key components:**

| Component | Responsibility |
|---|---|
| `QueueManager` | Shared `Arc<>` between API and workers; in-memory priority queue with SQLite overflow and crash-recovery |
| `WorkerPool` | Spawns `pool_size` async workers that each dequeue jobs, execute Python, and write results to DB |
| `ProcessManager` | Tracks running process PIDs; `cancel()` sends SIGTERM then escalates to SIGKILL after the grace period |
| `PythonRunner` | Spawns the Python interpreter inside the target venv with stdout/stderr capture and timeout enforcement |

## Requirements

- Rust 1.70+
- Python 3.8+
- SQLite 3
- OpenSSL development headers

### Ubuntu / Debian

```bash
sudo apt install build-essential libssl-dev pkg-config python3 python3-venv
```

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

- Web UI: http://localhost:3000/
- API Docs: http://localhost:3000/swagger-ui/

The main Python venv (`venvs/main`) is created automatically on first run if it does not exist.

## Configuration

Configuration is loaded from `config/default.toml`. All values have built-in defaults and can be overridden via environment variables using the `APP__` prefix (double underscore as separator).

```toml
[server]
host = "127.0.0.1"
port = 3000
log_level = "info"

[database]
path = "./data/serverust.db"
max_connections = 10

[worker]
pool_size = 4                       # concurrent Python workers
default_timeout_seconds = 30
default_memory_limit_mb = 128
python_executable = "python3"
graceful_shutdown_seconds = 5       # SIGTERM → SIGKILL grace period

[queue]
max_size = 1000                     # in-memory queue capacity before overflow to SQLite

[packages]
main_venv_path = "venvs/main"
custom_venv_base_path = "venvs/jobs"
pip_timeout_seconds = 120
enable_pip_cache = true

[security]
enable_audit_log = true
```

## API Endpoints

### Jobs

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/jobs` | List all jobs |
| `POST` | `/api/v1/jobs` | Create a job |
| `GET` | `/api/v1/jobs/{id}` | Get job details |
| `PUT` | `/api/v1/jobs/{id}` | Update a job |
| `DELETE` | `/api/v1/jobs/{id}` | Delete a job |
| `POST` | `/api/v1/jobs/{id}/execute` | Enqueue a job for execution |

### Executions

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/executions` | List executions (filterable) |
| `GET` | `/api/v1/executions/{id}` | Get execution details |
| `POST` | `/api/v1/executions/{id}/cancel` | Cancel a running execution (sends SIGTERM) |
| `POST` | `/api/v1/executions/{id}/retry` | Re-enqueue a failed execution |
| `GET` | `/api/v1/executions/{id}/logs` | Get execution logs |
| `GET` | `/api/v1/executions/{id}/stream` | Stream logs in real-time via SSE |

### Packages

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/packages` | List installed packages |
| `POST` | `/api/v1/packages/install` | Install a package into the main venv |
| `POST` | `/api/v1/packages/uninstall` | Uninstall a package |

### Virtual Environments

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/venvs` | List virtual environments |
| `GET` | `/api/v1/venvs/{id}` | Get venv details |
| `DELETE` | `/api/v1/venvs/{id}` | Delete a venv |

### Queue

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/queue` | Current queue status and depth |

### Health

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/health` | Health check |
| `GET` | `/api/v1/stats` | System statistics |

## Project Structure

```
serverust-less/
├── src/
│   ├── api/          # REST API handlers (axum)
│   │   ├── executions.rs  # execution lifecycle + enqueue/cancel
│   │   ├── jobs.rs
│   │   ├── packages.rs
│   │   ├── queue.rs
│   │   ├── venvs.rs
│   │   └── mod.rs    # AppState, router
│   ├── db/           # SQLite repositories (sqlx)
│   ├── models/       # Data structs and DTOs
│   ├── queue/
│   │   └── manager.rs  # QueueManager: priority heap + SQLite overflow + recovery
│   ├── services/     # Business logic layer
│   ├── worker/
│   │   ├── pool.rs         # WorkerPool: concurrent job executors
│   │   ├── process_manager.rs  # PID tracking, graceful cancel
│   │   ├── python_runner.rs    # Python subprocess execution
│   │   ├── venv_manager.rs
│   │   └── package_manager.rs
│   ├── config.rs     # Configuration structs and loading
│   ├── error.rs      # Unified error type
│   ├── lib.rs        # Crate exports
│   └── main.rs       # Startup: DB, queue recovery, worker pool, HTTP server
├── migrations/       # SQLite schema migrations (15 files)
├── web/              # Web UI (HTML / CSS / vanilla JS)
├── config/           # default.toml
├── venvs/            # Python virtual environments (auto-created)
└── data/             # SQLite database file
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
