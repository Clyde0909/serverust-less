# Serverust-Less

A self-hosted serverless platform for Python code execution, built with Rust. Similar to AWS Lambda but lightweight and easy to deploy.

## Features

- Execute Python code on-demand via REST API
- Web UI for job management and monitoring
- Virtual environment support (shared or per-job isolated)
- Package dependency management with PyPI integration
- Priority-based job queue with SQLite persistence
- Real-time execution streaming via SSE
- Resource limits (timeout, memory)
- Execution history and audit logging
- OpenAPI/Swagger documentation

## Requirements

- Rust 1.70+
- Python 3.8+
- SQLite 3
- OpenSSL development headers

### Ubuntu/Debian

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

## Configuration

Configuration is loaded from `config/default.toml`:

```toml
[server]
host = "0.0.0.0"
port = 3000

[database]
path = "data/serverust.db"

[packages]
main_venv_path = "venvs/main"
job_venvs_path = "venvs/jobs"

[worker]
pool_size = 4
default_timeout = 30
default_memory_limit = 128

[security]
enable_audit_log = true
```

Environment variables can override config values using the `SERVERUST_` prefix.

## API Endpoints

### Jobs

- `GET /api/v1/jobs` - List all jobs
- `POST /api/v1/jobs` - Create a new job
- `GET /api/v1/jobs/{id}` - Get job details
- `PUT /api/v1/jobs/{id}` - Update a job
- `DELETE /api/v1/jobs/{id}` - Delete a job
- `POST /api/v1/jobs/{id}/execute` - Execute a job

### Executions

- `GET /api/v1/executions` - List executions
- `GET /api/v1/executions/{id}` - Get execution details
- `POST /api/v1/executions/{id}/cancel` - Cancel execution
- `GET /api/v1/executions/{id}/logs` - Get execution logs
- `GET /api/v1/executions/{id}/stream` - Stream logs via SSE

### Packages

- `GET /api/v1/packages` - List installed packages
- `POST /api/v1/packages/install` - Install a package
- `POST /api/v1/packages/uninstall` - Uninstall a package

### Virtual Environments

- `GET /api/v1/venvs` - List virtual environments
- `GET /api/v1/venvs/{id}` - Get venv details
- `DELETE /api/v1/venvs/{id}` - Delete a venv

### Health

- `GET /api/v1/health` - Health check
- `GET /api/v1/stats` - System statistics

## Project Structure

```
serverust-less/
├── src/
│   ├── api/          # REST API handlers
│   ├── db/           # Database repositories
│   ├── models/       # Data models and DTOs
│   ├── queue/        # Job queue management
│   ├── services/     # Business logic layer
│   ├── worker/       # Python execution workers
│   ├── config.rs     # Configuration loading
│   ├── error.rs      # Error types
│   ├── lib.rs        # Library exports
│   └── main.rs       # Application entry point
├── migrations/       # SQLite migrations
├── web/              # Web UI (HTML/CSS/JS)
├── config/           # Configuration files
├── venvs/            # Virtual environments
└── data/             # SQLite database
```

## Development

Run tests:

```bash
cargo test
```

Check for issues:

```bash
cargo clippy
```

## Logging

### Backend Logging

The backend uses the `tracing` crate with configurable log levels via the `RUST_LOG` environment variable.

Default log levels:

```bash
# Default: info level with debug for application and tower_http
cargo run
```

Custom log levels:

```bash
# Debug all components
RUST_LOG=debug cargo run

# Trace SQL queries and execution details
RUST_LOG=trace cargo run

# Only show warnings and errors
RUST_LOG=warn cargo run

# Granular control
RUST_LOG=info,serverust_less::worker=trace,tower_http=debug cargo run
```

Log levels in order of verbosity: trace, debug, info, warn, error

### Frontend Logging

The web UI includes a Logger utility that writes to the browser console. It is disabled by default for performance.

Enable in browser console:

```javascript
Logger.enable();   // Turn on debug logging
Logger.disable();  // Turn off debug logging
```

When enabled, the frontend logs:
- API requests and response times
- Navigation events
- Component loading (jobs, executions, packages, venvs)
- Toast notifications
- Modal interactions
- Errors and warnings

The setting persists across page refreshes using localStorage.

## License

MIT License
