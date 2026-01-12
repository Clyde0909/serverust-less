-- Create venvs table for virtual environment management
CREATE TABLE IF NOT EXISTS venvs (
    id TEXT PRIMARY KEY,
    venv_type TEXT NOT NULL,
    job_id TEXT,
    path TEXT NOT NULL,
    python_version TEXT,
    status TEXT NOT NULL DEFAULT 'creating',
    size_bytes INTEGER,
    package_count INTEGER DEFAULT 0,
    error_message TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now')),
    last_used_at TEXT,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_venvs_job_id ON venvs(job_id);
CREATE INDEX IF NOT EXISTS idx_venvs_status ON venvs(status);
CREATE INDEX IF NOT EXISTS idx_venvs_venv_type ON venvs(venv_type);
