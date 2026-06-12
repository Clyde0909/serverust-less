-- Add immutable job version snapshots and execution lineage tracking

ALTER TABLE jobs ADD COLUMN current_version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE executions ADD COLUMN job_version INTEGER NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS job_versions (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    version_number INTEGER NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    python_code TEXT NOT NULL,
    timeout_seconds INTEGER NOT NULL,
    memory_limit_mb INTEGER NOT NULL,
    use_custom_venv BOOLEAN NOT NULL DEFAULT 0,
    venv_id TEXT,
    priority INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 0,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    change_summary TEXT,
    source TEXT NOT NULL DEFAULT 'update',
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    FOREIGN KEY (venv_id) REFERENCES venvs(id) ON DELETE SET NULL,
    UNIQUE(job_id, version_number)
);

CREATE INDEX IF NOT EXISTS idx_job_versions_job_id ON job_versions(job_id);
CREATE INDEX IF NOT EXISTS idx_job_versions_job_id_version ON job_versions(job_id, version_number DESC);
CREATE INDEX IF NOT EXISTS idx_executions_job_version ON executions(job_id, job_version);

-- Backfill a version-1 snapshot for all existing jobs.
INSERT INTO job_versions (
    id,
    job_id,
    version_number,
    name,
    description,
    python_code,
    timeout_seconds,
    memory_limit_mb,
    use_custom_venv,
    venv_id,
    priority,
    max_retries,
    enabled,
    created_at,
    change_summary,
    source
)
SELECT
    lower(hex(randomblob(4))) || '-' ||
    lower(hex(randomblob(2))) || '-' ||
    '4' || substr(lower(hex(randomblob(2))), 2) || '-' ||
    substr('89ab', abs(random()) % 4 + 1, 1) || substr(lower(hex(randomblob(2))), 2) || '-' ||
    lower(hex(randomblob(6))) AS id,
    j.id,
    1,
    j.name,
    j.description,
    j.python_code,
    j.timeout_seconds,
    j.memory_limit_mb,
    j.use_custom_venv,
    j.venv_id,
    j.priority,
    j.max_retries,
    j.enabled,
    COALESCE(j.updated_at, j.created_at, datetime('now')),
    'Backfilled initial version',
    'migration'
FROM jobs j
WHERE NOT EXISTS (
    SELECT 1
    FROM job_versions jv
    WHERE jv.job_id = j.id AND jv.version_number = 1
);

-- Existing executions predate version tracking, so attach them to version 1.
UPDATE executions
SET job_version = 1
WHERE job_version IS NULL OR job_version < 1;
