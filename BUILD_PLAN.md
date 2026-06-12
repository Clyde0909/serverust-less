# Serverust-Less: AWS Lambda-like Service in Rust

## Project Overview
A serverless job execution platform built with Rust that executes Python REPL code on-demand, similar to AWS Lambda but self-hosted and lightweight.

## Architecture

### High-Level Components

```
┌─────────────────┐
│   Web UI        │
│  (HTML/JS/CSS)  │
└────────┬────────┘
         │ HTTP
         ▼
┌─────────────────────────────────┐
│    REST API Server (Rust)       │
│  - Job Management               │
│  - Authentication (optional)    │
│  - Swagger UI                   │
└────────┬────────────────────────┘
         │
         ▼
┌─────────────────────────────────┐
│    SQLite Database              │
│  - Jobs metadata                │
│  - Execution history            │
│  - User data (optional)         │
└─────────────────────────────────┘
         ▲
         │
┌────────┴────────────────────────┐
│   Job Worker Pool (Rust)        │
│  - Python REPL execution        │
│  - Process isolation            │
│  - Resource management          │
└─────────────────────────────────┘
```

## Technology Stack

### Backend
- **Language**: Rust (stable, edition 2021)
- **Web Framework**: `axum` 0.7 (async, ergonomic)
- **Database**: SQLite with `sqlx` 0.7
- **Python Integration**: Subprocess isolation (process spawn)
- **API Documentation**: `utoipa` 4 + `utoipa-swagger-ui` 6 (OpenAPI/Swagger)
- **Serialization**: `serde` with `serde_json`
- **Async Runtime**: `tokio` 1 (full)
- **HTTP Client**: `reqwest` 0.11 (for PyPI search)
- **Validation**: `validator` 0.18
- **Process Management**: `nix` 0.28 (Unix signals/process control)
- **SSE Streaming**: `futures` + `tokio-stream` + `async-stream`
- **Static File Serving**: `tower-http` 0.5 (cors, trace, fs)

### Frontend
- **Framework**: Vanilla JavaScript (lightweight SPA)
- **Code Editor**: CodeMirror (bundled in `web/lib/codemirror/`)
- **Styling**: Custom CSS with variables, flexbox/grid, Dracula theme
- **HTTP Client**: Fetch API

### DevOps
- **Build**: Cargo
- **Testing**: `cargo test`
- **Logging**: `tracing` + `tracing-subscriber` (env-filter)
- **Configuration**: `config` crate with TOML/ENV (`dotenvy`)

## Database Schema

### Table: jobs
```sql
CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    python_code TEXT NOT NULL,
    timeout_seconds INTEGER DEFAULT 30,
    memory_limit_mb INTEGER DEFAULT 128,
    use_custom_venv BOOLEAN DEFAULT 0, -- 0: use main-venv, 1: use job-specific venv
    venv_id TEXT REFERENCES venvs(id) ON DELETE SET NULL, -- selected venv for execution
    priority INTEGER DEFAULT 0, -- higher = more priority
    max_retries INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    enabled BOOLEAN DEFAULT 1
);

CREATE INDEX idx_jobs_enabled ON jobs(enabled);
CREATE INDEX idx_jobs_created_at ON jobs(created_at);
CREATE INDEX idx_jobs_venv_id ON jobs(venv_id);
```

### Table: executions
```sql
CREATE TABLE executions (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    status TEXT NOT NULL, -- 'pending', 'queued', 'running', 'success', 'failed', 'timeout', 'cancelled'
    input_data TEXT,
    output_data TEXT,
    error_message TEXT,
    retry_count INTEGER DEFAULT 0,
    worker_id TEXT, -- which worker is handling this
    started_at DATETIME,
    completed_at DATETIME,
    duration_ms INTEGER,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
);

CREATE INDEX idx_executions_job_id ON executions(job_id);
CREATE INDEX idx_executions_status ON executions(status);
CREATE INDEX idx_executions_started_at ON executions(started_at);
```

### Table: execution_logs
```sql
CREATE TABLE execution_logs (
    id TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL,
    log_type TEXT NOT NULL, -- 'stdout', 'stderr', 'system'
    log_content TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (execution_id) REFERENCES executions(id) ON DELETE CASCADE
);

CREATE INDEX idx_execution_logs_execution_id ON execution_logs(execution_id);
```

### Table: job_schedules (optional for future)
```sql
CREATE TABLE job_schedules (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    cron_expression TEXT,
    next_run_at DATETIME,
    enabled BOOLEAN DEFAULT 1,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
);
```

### Table: dags
```sql
CREATE TABLE dags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    enabled BOOLEAN DEFAULT 1,
    max_concurrent_nodes INTEGER DEFAULT 2,
    on_failure TEXT NOT NULL DEFAULT 'stop', -- 'stop' or 'continue'
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

### Table: dag_edges
```sql
CREATE TABLE dag_edges (
    id TEXT PRIMARY KEY,
    dag_id TEXT NOT NULL,
    upstream_job_id TEXT NOT NULL,
    downstream_job_id TEXT NOT NULL,
    condition TEXT NOT NULL DEFAULT 'success', -- 'success', 'failure', 'always'
    FOREIGN KEY (dag_id) REFERENCES dags(id) ON DELETE CASCADE,
    FOREIGN KEY (upstream_job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    FOREIGN KEY (downstream_job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    UNIQUE(dag_id, upstream_job_id, downstream_job_id)
);
```

### Table: dag_runs
```sql
CREATE TABLE dag_runs (
    id TEXT PRIMARY KEY,
    dag_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'running', -- 'running', 'success', 'failed', 'cancelled'
    trigger_type TEXT NOT NULL DEFAULT 'manual', -- 'manual', 'schedule', 'api'
    total_nodes INTEGER DEFAULT 0,
    completed_nodes INTEGER DEFAULT 0,
    failed_nodes INTEGER DEFAULT 0,
    started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    FOREIGN KEY (dag_id) REFERENCES dags(id) ON DELETE CASCADE
);
```

### Table: dag_node_executions
```sql
CREATE TABLE dag_node_executions (
    id TEXT PRIMARY KEY,
    dag_run_id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    execution_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending', -- 'pending', 'running', 'success', 'failed', 'skipped'
    started_at DATETIME,
    completed_at DATETIME,
    FOREIGN KEY (dag_run_id) REFERENCES dag_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    FOREIGN KEY (execution_id) REFERENCES executions(id) ON DELETE SET NULL
);
```

### Table: tenants (optional for multi-tenancy)
```sql
CREATE TABLE tenants (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    api_key TEXT NOT NULL UNIQUE,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

### Table: roles (optional for RBAC)
```sql
CREATE TABLE roles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT
);
```

### Table: user_roles (optional for RBAC)
```sql
CREATE TABLE user_roles (
    user_id TEXT NOT NULL,
    role_id TEXT NOT NULL,
    PRIMARY KEY (user_id, role_id),
    FOREIGN KEY (user_id) REFERENCES tenants(id) ON DELETE CASCADE,
    FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE
);
```

### Table: permissions (optional for fine-grained RBAC)
```sql
CREATE TABLE permissions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    resource TEXT NOT NULL, -- 'job', 'execution', 'schedule'
    action TEXT NOT NULL, -- 'create', 'read', 'update', 'delete', 'execute'
    description TEXT
);
```

### Table: role_permissions (optional for fine-grained RBAC)
```sql
CREATE TABLE role_permissions (
    role_id TEXT NOT NULL,
    permission_id TEXT NOT NULL,
    PRIMARY KEY (role_id, permission_id),
    FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE,
    FOREIGN KEY (permission_id) REFERENCES permissions(id) ON DELETE CASCADE
);
```

### Table: python_packages
```sql
CREATE TABLE python_packages (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    description TEXT,
    pypi_url TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(name, version)
);
```

### Table: job_dependencies
```sql
CREATE TABLE job_dependencies (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    package_name TEXT NOT NULL,
    version_constraint TEXT, -- e.g., '>=1.0.0,<2.0.0', '==1.5.0', '*' for latest
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    UNIQUE(job_id, package_name)
);
```

### Table: package_cache
```sql
CREATE TABLE package_cache (
    id TEXT PRIMARY KEY,
    venv_type TEXT NOT NULL, -- 'main' or 'custom'
    venv_id TEXT, -- NULL for main-venv, job_id for custom venv
    package_name TEXT NOT NULL,
    version TEXT NOT NULL,
    installation_path TEXT NOT NULL,
    size_bytes INTEGER,
    status TEXT NOT NULL, -- 'installing', 'ready', 'failed'
    error_message TEXT,
    installed_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_used_at DATETIME,
    use_count INTEGER DEFAULT 0,
    UNIQUE(package_name, version, venv_type, venv_id)
);

CREATE INDEX idx_package_cache_venv ON package_cache(venv_type, venv_id);
CREATE INDEX idx_package_cache_status ON package_cache(status);
```

### Table: venvs
```sql
CREATE TABLE venvs (
    id TEXT PRIMARY KEY,
    venv_type TEXT NOT NULL, -- 'main' or 'custom'
    job_id TEXT, -- NULL for main-venv
    path TEXT NOT NULL,
    python_version TEXT,
    status TEXT NOT NULL, -- 'creating', 'ready', 'updating', 'failed', 'deleted'
    size_bytes INTEGER,
    package_count INTEGER DEFAULT 0,
    error_message TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_used_at DATETIME,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
);

CREATE INDEX idx_venvs_job_id ON venvs(job_id);
CREATE INDEX idx_venvs_status ON venvs(status);
```

### Table: job_queue
```sql
CREATE TABLE job_queue (
    id TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL UNIQUE,
    job_id TEXT NOT NULL,
    priority INTEGER DEFAULT 0,
    status TEXT NOT NULL, -- 'queued', 'processing', 'completed', 'failed', 'dead_letter'
    queued_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    started_at DATETIME,
    completed_at DATETIME,
    FOREIGN KEY (execution_id) REFERENCES executions(id) ON DELETE CASCADE,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
);

CREATE INDEX idx_job_queue_status_priority ON job_queue(status, priority DESC);
CREATE INDEX idx_job_queue_queued_at ON job_queue(queued_at);
```

### Table: audit_logs
```sql
CREATE TABLE audit_logs (
    id TEXT PRIMARY KEY,
    action TEXT NOT NULL, -- 'job.create', 'job.execute', 'package.install', etc.
    resource_type TEXT NOT NULL, -- 'job', 'execution', 'package', 'venv'
    resource_id TEXT,
    user_id TEXT, -- tenant id if auth enabled
    details TEXT, -- JSON with action details
    ip_address TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_audit_logs_action ON audit_logs(action);
CREATE INDEX idx_audit_logs_resource ON audit_logs(resource_type, resource_id);
CREATE INDEX idx_audit_logs_created_at ON audit_logs(created_at);
```

## REST API Endpoints

All endpoints use the `/api/v1` prefix.

### Job Management
- `GET /api/v1/jobs` - List all jobs
  - Query params: `?limit=20&offset=0&enabled=true&search=name`
- `GET /api/v1/jobs/:id` - Get job details
- `GET /api/v1/jobs/:id/executions` - Get executions for specific job
  - Query params: `?limit=20&offset=0&status=success`
- `POST /api/v1/jobs` - Create new job
- `POST /api/v1/jobs/bulk` - Create multiple jobs
- `PUT /api/v1/jobs/:id` - Update job
- `DELETE /api/v1/jobs/:id` - Delete job
- `DELETE /api/v1/jobs/bulk` - Delete multiple jobs (body: `{ids: [...]}`)
- `POST /api/v1/jobs/:id/enable` - Enable job
- `POST /api/v1/jobs/:id/disable` - Disable job
- `POST /api/v1/jobs/:id/clone` - Clone job with new name

### Job Execution
- `POST /api/v1/jobs/:id/execute` - Execute job immediately
  - Body: `{input_data?: object, priority?: number}`
- `POST /api/v1/executions/:id/cancel` - Cancel running execution
- `POST /api/v1/executions/:id/retry` - Retry failed execution
- `GET /api/v1/executions` - List all executions
  - Query params: `?limit=20&offset=0&status=running&job_id=xxx&from=date&to=date`
- `GET /api/v1/executions/:id` - Get execution details
- `GET /api/v1/executions/:id/logs` - Get execution logs
  - Query params: `?type=stdout|stderr`
- `GET /api/v1/executions/:id/stream` - SSE stream for real-time logs
- `DELETE /api/v1/executions/:id` - Delete execution record
- `DELETE /api/v1/executions/bulk` - Delete multiple executions

### Python Package Management
- `GET /api/v1/packages` - List all available/cached packages
- `GET /api/v1/packages/search?q=:query` - Search PyPI for packages
- `POST /api/v1/packages/install` - Install a package to main-venv
- `POST /api/v1/packages/uninstall` - Uninstall a package from main-venv
- `DELETE /api/v1/packages/:name/:version` - Remove package from cache
- `GET /api/v1/packages/main-venv` - Get main-venv package list and status
- `POST /api/v1/packages/main-venv/update` - Update all packages in main-venv
- `DELETE /api/v1/packages/main-venv` - Clear and recreate main-venv
- `GET /api/v1/jobs/:id/dependencies` - Get job dependencies
- `POST /api/v1/jobs/:id/dependencies` - Add dependency to job
- `PUT /api/v1/jobs/:id/dependencies/:name` - Update dependency version
- `DELETE /api/v1/jobs/:id/dependencies/:name` - Remove job dependency
- `POST /api/v1/jobs/:id/dependencies/install` - Install all job dependencies
- `GET /api/v1/jobs/:id/dependencies/status` - Check installation status
- `GET /api/v1/jobs/:id/venv/info` - Get venv type and status for job
- `POST /api/v1/jobs/:id/venv/toggle` - Toggle between main-venv and custom venv
- `DELETE /api/v1/jobs/:id/venv` - Delete custom venv for job

### Health & Monitoring
- `GET /api/v1/health` - Health check
- `GET /api/v1/stats` - System statistics
- `GET /api/v1/workers/status` - Worker pool status
- `GET /api/v1/queue/status` - Job queue status and depth
- `GET /api/v1/venvs` - List all virtual environments
- `POST /api/v1/venvs` - Create standalone virtual environment
- `GET /api/v1/venvs/:id` - Get venv details
- `GET /api/v1/venvs/:id/packages` - List packages in a specific venv
- `DELETE /api/v1/venvs/:id` - Delete a venv

### Schedules
- `POST /api/v1/jobs/:id/schedule` - Create or update cron schedule for a job
- `GET /api/v1/jobs/:id/schedule` - Get schedule for a job
- `PUT /api/v1/jobs/:id/schedule` - Update schedule
- `DELETE /api/v1/jobs/:id/schedule` - Remove schedule
- `POST /api/v1/jobs/:id/schedule/toggle` - Enable/disable schedule
- `GET /api/v1/schedules` - List all schedules

### DAGs (Directed Acyclic Graphs)
- `POST /api/v1/dags` - Create a new DAG workflow
- `GET /api/v1/dags` - List all DAGs
- `GET /api/v1/dags/:id` - Get DAG details (with edges)
- `PUT /api/v1/dags/:id` - Update DAG metadata
- `DELETE /api/v1/dags/:id` - Delete a DAG
- `POST /api/v1/dags/:id/edges` - Add an edge between two jobs
- `DELETE /api/v1/dags/:dag_id/edges/:edge_id` - Remove an edge
- `GET /api/v1/dags/:id/topology` - Get topological sort / level analysis
- `POST /api/v1/dags/:id/validate` - Validate DAG (cycle detection, job existence)
- `POST /api/v1/dags/:id/trigger` - Trigger a DAG run
- `GET /api/v1/dags/:id/runs` - List all runs for a DAG
- `GET /api/v1/dags/:dag_id/runs/:run_id` - Get run details (with node statuses)
- `POST /api/v1/dags/:dag_id/runs/:run_id/cancel` - Cancel a running DAG

### Documentation
- `GET /swagger-ui` - Swagger UI
- `GET /api-docs/openapi.json` - OpenAPI specification
- `GET /api/openapi.json` - OpenAPI specification (alias)

## Project Structure

```
serverust-less/
├── Cargo.toml
├── Cargo.lock
├── BUILD_PLAN.md
├── README.md
├── .gitignore
├── config/
│   └── default.toml
├── migrations/
│   ├── 001_create_jobs.sql
│   ├── 002_create_executions.sql
│   ├── 003_create_execution_logs.sql
│   ├── 004_create_schedules.sql           # (reserved for Phase 6)
│   ├── 005_create_tenants.sql             # (reserved for Phase 6)
│   ├── 006_create_roles.sql               # (reserved for Phase 6)
│   ├── 007_create_user_roles.sql          # (reserved for Phase 6)
│   ├── 008_create_permissions.sql         # (reserved for Phase 6)
│   ├── 009_create_role_permissions.sql    # (reserved for Phase 6)
│   ├── 010_create_python_packages.sql
│   ├── 011_create_job_dependencies.sql
│   ├── 012_create_package_cache.sql
│   ├── 013_create_venvs.sql
│   ├── 014_create_job_queue.sql
│   ├── 015_create_audit_logs.sql
│   └── 016_add_venv_id_to_jobs.sql
│   └── 017_create_dags.sql
│   └── 018_create_dag_runs.sql
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── config.rs
│   ├── error.rs
│   ├── api/
│   │   ├── mod.rs
│   │   ├── jobs.rs
│   │   ├── executions.rs
│   │   ├── packages.rs
│   │   ├── venvs.rs
│   │   ├── queue.rs
│   │   ├── health.rs
│   │   ├── schedules.rs
│   │   └── dags.rs
│   ├── services/
│   │   ├── mod.rs
│   │   ├── job_service.rs
│   │   ├── execution_service.rs
│   │   ├── package_service.rs
│   │   ├── venv_service.rs
│   │   ├── queue_service.rs
│   │   ├── audit_service.rs
│   │   ├── schedule_service.rs
│   │   └── dag_service.rs
│   ├── models/
│   │   ├── mod.rs
│   │   ├── job.rs
│   │   ├── execution.rs
│   │   ├── execution_log.rs
│   │   ├── package.rs          # includes job dependency models
│   │   ├── venv.rs
│   │   ├── queue.rs
│   │   ├── audit.rs
│   │   ├── schedule.rs
│   │   └── dag.rs
│   ├── db/
│   │   ├── mod.rs              # includes connection pool init + migrations
│   │   ├── jobs.rs
│   │   ├── executions.rs
│   │   ├── execution_logs.rs
│   │   ├── packages.rs         # includes dependency queries
│   │   ├── venvs.rs
│   │   ├── queue.rs
│   │   ├── audit.rs
│   │   ├── schedules.rs
│   │   └── dags.rs
│   ├── queue/
│   │   ├── mod.rs
│   │   └── manager.rs          # includes priority queue + persistence
│   ├── scheduler/
│   │   └── mod.rs              # SchedulerRunner: background cron tick loop
│   ├── dag/
│   │   ├── mod.rs
│   │   └── engine.rs           # DagEngine: trigger, advance, cancel DAG runs
│   └── worker/
│       ├── mod.rs
│       ├── pool.rs
│       ├── executor.rs
│       ├── python_runner.rs
│       ├── package_manager.rs
│       ├── venv_manager.rs
│       └── process_manager.rs
├── web/
│   ├── index.html
│   ├── css/
│   │   └── styles.css
│   ├── js/
│   │   ├── app.js
│   │   ├── api.js
│   │   └── components/
│   │       ├── job-list.js
│   │       ├── job-form.js
│   │       ├── execution-history.js
│   │       ├── packages.js
│   │       └── venvs.js
│   └── lib/
│       └── codemirror/          # bundled CodeMirror editor + Python mode
├── tests/
│   ├── api_tests.rs
│   ├── api_endpoint_coverage.rs
│   ├── integration/
│   └── fixtures/
├── test/
│   ├── api_endpoint_coverage.rs
│   └── README.md
├── data/                        # SQLite database (auto-created)
└── venvs/                       # virtual environments (auto-created)
```

## Core Components Detail

### 1. API Server (src/api/)
**Responsibilities:**
- Handle HTTP requests
- Validate input
- Coordinate with database and worker pool
- Serve Swagger UI
- Serve static web files

**Key Features:**
- CORS support for web UI
- Request/response logging
- Error handling middleware
- Rate limiting (optional)

### 2. Database Layer (src/db/)
**Responsibilities:**
- SQLite connection pool management
- CRUD operations for jobs and executions
- Transaction support
- Migration management

**Key Features:**
- Prepared statements
- Connection pooling with `sqlx`
- Type-safe queries
- Automatic timestamps

### 3. Services Layer (src/services/)
**Responsibilities:**
- Business logic between API and DB layers
- Transaction coordination
- Validation and authorization checks
- Event emission for audit logging

**Key Services:**
- `JobService` - Job lifecycle management
- `ExecutionService` - Execution creation, cancellation, retry logic
- `PackageService` - Package installation coordination
- `VenvService` - Virtual environment management
- `QueueService` - Job queue operations
- `AuditService` - Audit log recording
- `ScheduleService` - Cron schedule CRUD and validation
- `DagService` - DAG CRUD, edge management, cycle detection, topology

### 4. Job Queue (src/queue/)
**Responsibilities:**
- Priority-based job scheduling
- Queue persistence (SQLite-backed)
- Overflow handling
- Dead letter queue for failed jobs

**Queue Architecture:**
```
┌─────────────────────────────────────────────────────────┐
│                    Queue Manager                        │
├─────────────────────────────────────────────────────────┤
│  In-Memory Priority Queue (bounded, fast)               │
│  ├── High Priority Jobs                                 │
│  ├── Normal Priority Jobs                               │
│  └── Low Priority Jobs                                  │
├─────────────────────────────────────────────────────────┤
│  SQLite Overflow (unbounded, persistent)                │
│  └── Jobs exceeding in-memory limit                     │
├─────────────────────────────────────────────────────────┤
│  Dead Letter Queue                                      │
│  └── Jobs that failed max_retries times                 │
└─────────────────────────────────────────────────────────┘
```

**Queue Flow:**
```
Execute Request → Check Queue Capacity
  ├─ Under limit → Add to in-memory queue
  └─ Over limit → Persist to SQLite overflow
        ↓
Worker Available → Dequeue (priority order)
        ↓
Execute → Success/Failure
  ├─ Success → Remove from queue, record result
  └─ Failure → Check retry count
        ├─ Retries remaining → Re-queue with delay
        └─ Max retries reached → Move to dead letter queue
```

### 5. Worker Pool (src/worker/)
**Responsibilities:**
- Manage Python execution processes
- Queue job executions
- Resource limitation (CPU, memory, timeout)
- Concurrent execution management
- Python package installation and management
- Main-venv maintenance and updates
- User-defined venv creation and caching

**Key Features:**
- Configurable pool size
- Process isolation per execution
- Timeout enforcement
- Memory limit enforcement (Unix: resource limit wrapper script via `nix` crate)
- Capture stdout/stderr
- Return structured results via `WorkerResult`
- Shared main-venv for efficiency
- Per-job custom venvs when needed
- Standalone named venvs with custom Python version
- Automatic dependency installation
- Dependency resolution
- Cancellation via `tokio::select!` racing execution against cancel signal
- PID tracking via oneshot channel for SIGTERM/SIGKILL support
- `kill_on_drop(true)` for automatic child cleanup

**Python Package Management Strategy:**

1. **Two-Tier Virtual Environment System**
   - **Main-venv** (default): Shared virtual environment for all jobs at `./venvs/main/`
     - Contains commonly used packages
     - Default choice for jobs without version conflicts
     - Faster execution (no venv creation overhead)
     - Easier management and updates
   - **User-defined-venv** (per-job): Isolated environment at `./venvs/job-{job_id}/`
     - Only created when job has `use_custom_venv = true`
     - Used when specific package versions are required
     - Complete isolation from other jobs

2. **Virtual Environment Selection Logic**
   ```
   Job Execution → Check use_custom_venv flag?
     ├─ No (default) → Use main-venv → Install missing packages to main-venv
     └─ Yes → Check job-specific venv exists?
         ├─ Yes → Use existing venv
         └─ No → Create new venv → Install job dependencies
   ```

3. **Package Installation Flow**
   
   **For Main-venv (default):**
   ```
   Job Created → Parse Dependencies → Check main-venv → 
   Missing packages? → pip install to main-venv → Ready for Execution
   ```
   
   **For User-defined-venv:**
   ```
   Job Created (use_custom_venv=true) → Create job venv → 
   Install ALL dependencies → Cache venv → Ready for Execution
   ```

4. **Dependency Management**
   - Support version constraints (==, >=, <=, ~=, *)
   - Main-venv: Install latest compatible versions
   - User-defined-venv: Install exact versions specified
   - Parse requirements.txt format
   - Detect conflicts and suggest custom venv if needed

5. **Caching Strategy**
   - Package wheels cached in `./cache/pip/` (shared)
   - Main-venv: Single shared environment, incrementally updated
   - User-defined venvs: Cached per job, recreated only when dependencies change
   - LRU eviction for unused custom venvs when disk space limit reached

6. **Installation Process**
   
   **Main-venv execution:**
   ```bash
   # One-time setup
   python -m venv ./venvs/main
   
   # Install missing packages (incremental)
   ./venvs/main/bin/pip install package
   
   # Execute job
   ./venvs/main/bin/python -c "user_code"
   ```
   
   **User-defined-venv execution:**
   ```bash
   # Create job-specific venv
   python -m venv ./venvs/job-{job_id}
   
   # Install exact dependencies
   ./venvs/job-{job_id}/bin/pip install package==version
   
   # Execute job
   ./venvs/job-{job_id}/bin/python -c "user_code"
   ```

7. **When to Use Custom Venv**
   - Job requires specific package version (e.g., `pandas==1.3.0`)
   - Version conflicts with main-venv packages
   - Testing different package versions
   - Security isolation requirements
   - User explicitly enables custom venv

8. **Security Considerations**
   - Validate package names (no shell injection)
   - Use pip with `--no-input` and `--require-hashes` (optional)
   - Disk quota for custom venvs
   - Timeout for pip operations
   - Blacklist malicious packages
   - Main-venv protected from accidental deletions

9. **Main-venv Conflict Resolution**
   When a job requests a package version that conflicts with main-venv:
   ```
   Conflict Detected → Check conflict_resolution strategy
     ├─ 'suggest_custom_venv' → Return warning, suggest enabling custom venv
     ├─ 'force_upgrade' → Upgrade package (may break other jobs)
     └─ 'fail' → Return error, refuse to install
   ```
   
   **Conflict Detection:**
   - Before installing, check if package exists with different version
   - If conflict, apply configured strategy
   - Log conflict for admin review

10. **Concurrent pip Install Handling**
    - Use `tokio::sync::Mutex` lock for main-venv pip operations
    - Queue concurrent install requests
    - Only one pip process per venv at a time
    - Custom venvs: No locking needed (single owner)

### 6. Execution Cancellation

**Cancellation Mechanism:**
```
Cancel Request (execution_id)
  ↓
Check Status
  ├─ 'pending'/'queued' → Remove from queue, set status='cancelled'
  ├─ 'running' → Send SIGTERM to Python process
  │     ├─ Process exits → Set status='cancelled'
  │     └─ Timeout (5s) → Send SIGKILL → Set status='cancelled'
  └─ 'completed' → Return error (already finished)
```

**Process Tracking:**
- Store `worker_id` and process PID in execution record
- Worker maintains map of execution_id → child process handle
- On cancellation, look up process handle and terminate

**Graceful Shutdown:**
- On SIGTERM, Python code can catch `KeyboardInterrupt`
- 5 second grace period before SIGKILL
- Cleanup handlers can run during grace period

### 7. Resource Limits

**Memory Limit Enforcement:**
- **Linux**: Python subprocess spawned via a wrapper shell script that sets `ulimit -v` before exec
- Memory limit set before spawning Python process
- Process killed if limit exceeded
- Uses `nix` crate for Unix signal handling (SIGTERM/SIGKILL)

**Implementation:**
The `PythonRunner` generates a temporary shell script that applies resource limits:
```bash
#!/bin/bash
ulimit -v $((memory_limit_mb * 1024))
exec python3 -c "user_code"
```
This approach avoids requiring the `rlimit` crate and works with subprocess isolation.

### 8. Models (src/models/)
**Responsibilities:**
- Data structures for Jobs, Executions, ExecutionLogs, Packages, Dependencies, Venvs, Queue, Audit
- Validation logic (including version constraint parsing, package name validation)
- Serialization/deserialization
- Business logic and constraints (status transitions, ordering)
- Request/response types with OpenAPI schema annotations (`ToSchema`)

### 9. Configuration (src/config.rs)
**Responsibilities:**
- Load configuration from files and environment
- Validate configuration
- Provide typed access to settings

**Configuration Options:**
```toml
[server]
host = "0.0.0.0"
port = 8080
log_level = "info"

[server.cors]
enabled = true
allowed_origins = ["http://localhost:3000", "http://localhost:8080"]
allowed_methods = ["GET", "POST", "PUT", "DELETE", "OPTIONS"]
max_age_seconds = 3600

[database]
path = "./data/serverust.db"
max_connections = 10

[worker]
pool_size = 4
default_timeout_seconds = 30
default_memory_limit_mb = 128
python_executable = "python3.12"
graceful_shutdown_seconds = 30

[queue]
max_size = 1000
persistence_enabled = true
retry_delay_seconds = 5
max_retries = 3

[packages]
main_venv_path = "./venvs/main"
custom_venv_base_path = "./venvs"
pip_cache_dir = "./cache/pip"
max_cache_size_mb = 5000
max_custom_venvs = 50
pip_timeout_seconds = 300
enable_pip_cache = true
auto_install_dependencies = true
allow_prerelease = false
auto_suggest_custom_venv = true
pip_index_url = "https://pypi.org/simple"
pip_trusted_hosts = []

[packages.conflict_resolution]
strategy = "suggest_custom_venv"  # 'suggest_custom_venv', 'force_upgrade', 'fail'

[retention]
execution_history_days = 30
log_max_size_bytes = 1048576  # 1MB per execution
cleanup_interval_hours = 24

[scheduler]
tick_interval_seconds = 10    # how often the scheduler checks for due cron jobs
enabled = true                # set to false to disable the background scheduler

[security]
enable_auth = false
enable_multitenancy = false
api_key = "optional-api-key"
enable_audit_log = true
blocked_packages = ["os-sys", "malicious-pkg"]  # Package blacklist
```

Configuration is loaded from `config/default.toml` and can be overridden via environment variables with the `SERVERUST__` prefix (e.g., `SERVERUST__SERVER__PORT=9090`).

## Implementation Phases

### Phase 1: Foundation (Week 1) ✅ COMPLETE
- [x] Project setup with Cargo
- [x] Database schema design and migration scripts (16 migrations)
- [x] Core models (Job, Execution, ExecutionLog, Venv, Queue, Audit)
- [x] SQLite integration with sqlx
- [x] Configuration management (with all new options)
- [x] Logging setup with tracing
- [x] Services layer scaffolding

**Deliverables:**
- ✅ Database can be initialized with all tables and indexes
- ✅ Core models defined with proper types
- ✅ Configuration loaded from file/env
- ✅ Migration system working
- ✅ Services layer structure in place

### Phase 2: Worker Implementation (Week 1-2) ✅ COMPLETE
- [x] Python process executor
- [x] Timeout enforcement
- [x] stdout/stderr capture
- [x] Error handling
- [x] Worker pool management
- [x] Virtual environment manager
- [x] Package manager (pip wrapper) with locking
- [x] Dependency resolver
- [x] Package cache implementation
- [x] Job queue manager (in-memory + SQLite overflow)
- [x] Execution cancellation mechanism
- [x] Process manager for tracking child processes
- [x] Resource limits (memory) for Linux/Windows
- [x] Unit tests for worker

**Deliverables:**
- ✅ Can execute Python code and capture output
- ✅ Resource limits enforced (memory, timeout)
- ✅ Worker pool can handle concurrent jobs
- ✅ Main-venv created and managed automatically
- ✅ Can create custom venvs for specific jobs
- ✅ Can install packages from PyPI to main-venv
- ✅ Package caching working
- ✅ Venv selection logic implemented
- ✅ Job queue with priority support + SQLite overflow recovery
- ✅ Cancellation working for running jobs (process kill via ProcessManager)

### Phase 3: REST API (Week 2-3) ✅ COMPLETE
- [x] Axum server setup with CORS
- [x] Job CRUD endpoints with pagination
- [x] Execution endpoints (including cancel, retry)
- [x] Package management endpoints
- [x] Dependency management endpoints
- [x] Venv management endpoints
- [x] Queue status endpoint
- [x] Health check endpoint
- [x] Error handling middleware
- [x] Request validation
- [x] Audit logging middleware
- [x] OpenAPI/Swagger integration
- [x] SSE for execution streaming

**Deliverables:**
- ✅ All REST endpoints functional (including package APIs)
- ✅ API documented with Swagger
- ✅ Integration tests passing
- ✅ Can manage packages and dependencies via API
- ✅ Audit logs being recorded
- ✅ Real-time log streaming working

### Phase 4: Web UI (Week 3-4) ✅ COMPLETE
- ✅ HTML/CSS layout (`web/index.html`, `web/css/styles.css`)
- ✅ Job list view (show venv type: main/custom)
- ✅ Job creation form (with dependencies field and custom venv toggle)
- ✅ Job editing
- ✅ Package dependency manager UI
- ✅ Venv type selector (main-venv vs custom venv)
- ✅ Main-venv package list and management
- ✅ Package search integration (PyPI)
- ✅ Execute job button
- ✅ Execution history view
- ✅ Package cache status view
- ✅ Real-time execution status (SSE streaming)

**Implementation Details:**
- `web/index.html` - Single-page application with navigation, modals, and responsive layout
- `web/css/styles.css` - Modern CSS with variables, flexbox/grid, dark code editor theme
- `web/js/api.js` - Fetch-based API client wrapper for all endpoints
- `web/js/app.js` - Main application entry point with routing and state management
- `web/js/components/job-list.js` - Job list rendering with search/filter
- `web/js/components/job-form.js` - Job creation/editing with dependency management
- `web/js/components/execution-history.js` - Execution list with SSE streaming
- `web/js/components/packages.js` - Package management for main venv
- `web/js/components/venvs.js` - Virtual environment management
- Static file serving via tower-http ServeDir

**Deliverables:**
- ✅ Functional web interface
- ✅ Can create, edit, execute jobs
- ✅ Can toggle between main-venv and custom venv
- ✅ Can add/remove package dependencies
- ✅ View and manage main-venv packages
- ✅ View execution history
- ✅ View installed packages by venv type

### Phase 5: Testing & Polish (Week 4)
- [x] Integration tests (API CRUD, health, queue, workers)
- [x] Error scenario testing (tests/error_scenarios.rs — 404, 422, 400, invalid JSON, disabled job, bulk edge cases, pagination boundaries)
- [x] Performance testing (tests/performance_tests.rs — concurrent creation, bulk delete, pagination walkthrough, rapid CRUD cycles, health under load)
- [x] Cancellation scenario testing (tests/cancellation_tests.rs — cancel pending, reject double-cancel, retry cancelled, max-retry enforcement, multi-execution cancel)
- [x] Conflict resolution testing (tests/conflict_resolution_tests.rs — dependency CRUD, status tracking, "fail" strategy, cross-job isolation, package validation)
- [x] Documentation
- [x] Retention policy scheduler (inline in main.rs)
- [x] README with setup instructions

**Deliverables:**
- Comprehensive test suite: 4 test files, ~50 test cases covering error, cancellation, conflict, and performance scenarios
- Complete documentation

### Phase 6: Scheduled Jobs & DAG Workflows ✅ COMPLETE
- [x] Scheduled jobs with cron expressions (`cron` 0.12 crate)
- [x] Schedule CRUD API (create, get, update, delete, toggle, list)
- [x] Background scheduler runner (configurable tick interval)
- [x] Automatic next-run calculation from cron expressions
- [x] Job DAG (Directed Acyclic Graph) workflows
- [x] DAG CRUD API (create, get, update, delete, list)
- [x] DAG edge management with cycle detection (Kahn's algorithm)
- [x] DAG topology and validation endpoints
- [x] DAG run triggering with topological execution order
- [x] DAG node execution tracking (per-job status within a run)
- [x] Configurable `on_failure` policy per DAG (`continue` / `stop`)
- [x] Configurable `max_concurrent_nodes` per DAG
- [x] DAG run cancellation
- [x] Worker pool integration — `DagEngine.on_execution_complete()` callback advances DAG runs
- [x] Scheduler config section in `default.toml`
- [x] Database migrations for dags, dag_edges, dag_runs, dag_node_executions (migrations 017–018)
- [x] OpenAPI/Swagger documentation for all new endpoints (19 paths, 16 schemas, 2 tags)
- [x] All existing tests updated to work with new AppState fields

**New modules:**
- `src/scheduler/mod.rs` — SchedulerRunner (cron tick loop)
- `src/dag/engine.rs` — DagEngine (trigger, advance, cancel)
- `src/dag/mod.rs`
- `src/models/schedule.rs`, `src/models/dag.rs`
- `src/db/schedules.rs`, `src/db/dags.rs`
- `src/services/schedule_service.rs`, `src/services/dag_service.rs`
- `src/api/schedules.rs`, `src/api/dags.rs`

**Deliverables:**
- ✅ Cron-based job scheduling with background tick loop
- ✅ Full DAG workflow engine with topological execution
- ✅ Cycle detection prevents invalid DAG definitions
- ✅ Failure propagation policies (stop entire DAG or continue)
- ✅ Concurrency control per DAG run
- ✅ All endpoints documented in Swagger UI

## Current Build Status (Verified 2026-06-09)

- `Cargo.toml` is at version `0.3.0`, and the repository structure includes the Phase 6 scheduler + DAG implementation that the older footer status line no longer reflected.
- The latest compiler/flycheck artifacts show a successful build for the library, binary, and test targets.
- The current workspace diagnostics are clean after the robustness pass: no active compiler errors or warnings are being reported for the edited source and test files.
- The repository contains broad automated coverage across API, endpoint coverage, error scenarios, cancellation, conflict-resolution, and performance test targets, and those test targets compile successfully.
- The full runtime test suite should still be re-run from a normal terminal session once the workspace terminal provider issue is resolved; the direct terminal runner in this session could not attach to the workspace path.

## Improvement Plan — Phase 6.1: Robustness & Operational Hardening

- [x] Align build/documentation metadata with the verified repository state.
- [x] Establish a warning-free build baseline for the currently implemented code.
- [x] Harden `/api/v1/health` so it reports real subsystem state (database, queue, workers, main venv) instead of a static "healthy" response.
- [x] Add API tests that cover healthy, degraded, and unhealthy monitoring paths.
- [ ] Extend operational checks with scheduler state, disk-space visibility, and main-venv integrity follow-ups.
- [ ] Add CI-style validation gates once the workspace terminal provider issue is resolved.

### Phase 7: Future Enhancements

- [ ] Authentication & authorization (RBAC)
- [ ] Multi-tenancy support
- [ ] WebSocket for real-time updates
- [ ] Metrics & monitoring (Prometheus)
- [ ] Job versioning
- [ ] Environment variables per job
- [ ] Private PyPI repository support
- [ ] Conda package manager support
- [ ] Requirements.txt import/export
- [ ] Automatic security scanning of packages
- [ ] Package vulnerability checking
- [ ] Docker containerization
- [ ] MCP Server for Agent integration
- [ ] Multi-language support (JavaScript, Perl, etc.)
- [ ] Distributed execution across multiple worker nodes
- [ ] Job marketplace for sharing templates
- [ ] Web UI improvements (drag-and-drop DAG editor, real-time log streaming, dark mode, etc.)

## Key Design Decisions

### 1. Why Axum?
- Built on tokio, excellent async performance
- Type-safe extractors
- Easy middleware
- Great community support
- Perfect for REST APIs

### 2. Why SQLite?
- Embedded, no separate database server
- Perfect for local/small deployments
- ACID compliant
- Easy backup (single file)
- Good performance for this use case

### 3. Python Execution Strategy
- Subprocess isolation via `tokio::process::Command`
- Each execution gets fresh Python process with `kill_on_drop(true)`
- Memory limits enforced via wrapper shell script (`ulimit -v`)
- Timeout enforced via `tokio::time::timeout`
- `PYTHONUNBUFFERED=1` for real-time output capture
- PID tracked for cancellation support (SIGTERM → SIGKILL)

### 4. Job Queue Strategy
- Hybrid approach: In-memory priority queue + SQLite overflow
- In-memory queue for fast access (bounded size)
- SQLite persistence for overflow and crash recovery
- Priority-based scheduling (higher priority first)
- Dead letter queue for failed jobs after max retries

### 5. Cancellation Strategy
- Track child process handles in worker
- Send SIGTERM first, wait 5s, then SIGKILL
- Queue jobs can be cancelled immediately
- Execution status updated to 'cancelled'

### 6. Conflict Resolution Strategy
- Configurable: suggest_custom_venv, force_upgrade, or fail
- Default: suggest custom venv (safest)
- Log all conflicts for admin review
- API returns conflict details for UI display

## Security Considerations

1. **Code Execution Safety**
   - Run Python in isolated processes
   - Enforce strict timeouts
   - Limit memory usage
   - Consider sandboxing (Docker containers per job)
   - Validate Python code syntax before execution

2. **Package Installation Safety**
   - Validate package names against injection attacks
   - Use official PyPI repository (or approved mirrors)
   - Implement package blacklist for known malicious packages
   - Enforce disk quotas per job
   - Timeout pip install operations
   - Consider using `--require-hashes` for production
   - Scan packages for known vulnerabilities

3. **API Security**
   - Optional API key authentication
   - Rate limiting to prevent abuse
   - Input validation on all endpoints
   - SQL injection prevention (parameterized queries)
   - CORS properly configured

4. **Data Protection**
   - Sanitize error messages (don't leak system info)
   - Limit execution log size (configurable max)
   - Consider encryption for sensitive job data
   - Audit logging for all mutations

5. **Input Sanitization for Python Code**
   - Syntax validation before execution
   - No filesystem access validation (handled by process isolation)
   - Optional: restricted import whitelist
   - Log suspicious patterns for review

## Monitoring & Observability

1. **Logging**
   - Structured logging with `tracing`
   - Log levels: ERROR, WARN, INFO, DEBUG, TRACE
   - Log job execution start/end
   - Log API requests

2. **Metrics** (Future)
   - Execution count, success/failure rate
   - Execution duration histogram
   - Worker pool utilization
   - API response times

3. **Health Checks**
   - Database connectivity
   - Worker pool status
   - Queue depth and health
   - Disk space for SQLite and venvs
   - Main-venv status

4. **Audit Trail**
   - All job mutations logged
   - Package installations logged
   - Execution starts/completions logged
   - Failed operations logged with details

## Testing Strategy

### Unit Tests
- Models validation
- Database operations
- Worker executor logic
- Configuration parsing
- Queue priority ordering
- Conflict resolution logic

### Integration Tests
- End-to-end API workflows
- Job creation → execution → results
- Error scenarios
- Concurrent executions
- Cancellation flows
- Package conflict handling
- Queue overflow scenarios

### Performance Tests
- Load testing with many concurrent executions
- Database query performance
- Memory usage under load

## Deployment

### Development
```bash
cargo run
# Access at http://localhost:8080
```

### Production
```bash
cargo build --release
./target/release/serverust-less
```

### Docker
```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y python3.12 python3.12-venv && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/serverust-less /usr/local/bin/
COPY --from=builder /app/web /app/web
WORKDIR /app
CMD ["serverust-less"]
```

## Success Metrics

- [ ] Can create and manage 100+ jobs
- [ ] Can execute 10+ concurrent Python jobs
- [ ] Average execution latency < 500ms (excluding Python runtime)
- [ ] API response time < 100ms for CRUD operations
- [ ] Zero data loss on crashes (SQLite durability)
- [ ] Web UI responsive on all major browsers

## Future Enhancements

1. **Multi-language support** - Node.js, Ruby, Bash scripts
2. **Distributed execution** - Multiple worker nodes
3. **Job marketplace** - Share/import job templates
4. **IDE integration** - VS Code extension
5. **CLI tool** - Command-line job management
6. **Monitoring dashboard** - Real-time system monitoring
7. **Job templates** - Predefined job patterns
8. **Version control integration** - Git-backed job storage
9. **Notification system** - Email/Slack on job completion
10. **Resource quotas** - Per-job resource limits

## Getting Started

### Prerequisites
- Rust 1.75+ installed
- Python 3.12+ installed
- SQLite 3.35+

### Initial Setup
```bash
# Clone repository
git clone <repo-url>
cd serverust-less

# Install dependencies
cargo build

# Start server (migrations run automatically)
cargo run

# Access UI
open http://localhost:8080
```

## References

- [Axum Documentation](https://docs.rs/axum)
- [SQLx Documentation](https://docs.rs/sqlx)
- [Tokio Documentation](https://tokio.rs)
- [OpenAPI Specification](https://swagger.io/specification/)
- [nix crate](https://docs.rs/nix)

---

**Last Updated**: June 9, 2026
**Version**: 0.3.0
**Status**: Phase 6 Complete — Phase 6.1 Operational Hardening In Progress
