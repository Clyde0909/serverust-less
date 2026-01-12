-- Create job_schedules table for cron-like scheduling (future feature)
CREATE TABLE IF NOT EXISTS job_schedules (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    cron_expression TEXT,
    next_run_at TEXT,
    last_run_at TEXT,
    enabled INTEGER DEFAULT 1,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_job_schedules_job_id ON job_schedules(job_id);
CREATE INDEX IF NOT EXISTS idx_job_schedules_next_run_at ON job_schedules(next_run_at);
CREATE INDEX IF NOT EXISTS idx_job_schedules_enabled ON job_schedules(enabled);
