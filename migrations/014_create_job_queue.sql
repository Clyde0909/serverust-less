-- Create job_queue table for persistent queue overflow
CREATE TABLE IF NOT EXISTS job_queue (
    id TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL UNIQUE,
    job_id TEXT NOT NULL,
    priority INTEGER DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'queued',
    queued_at TEXT DEFAULT (datetime('now')),
    started_at TEXT,
    completed_at TEXT,
    FOREIGN KEY (execution_id) REFERENCES executions(id) ON DELETE CASCADE,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_job_queue_status_priority ON job_queue(status, priority DESC);
CREATE INDEX IF NOT EXISTS idx_job_queue_queued_at ON job_queue(queued_at);
CREATE INDEX IF NOT EXISTS idx_job_queue_execution_id ON job_queue(execution_id);
