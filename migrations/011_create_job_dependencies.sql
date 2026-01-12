-- Create job_dependencies table for package dependencies per job
CREATE TABLE IF NOT EXISTS job_dependencies (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    package_name TEXT NOT NULL,
    version_constraint TEXT DEFAULT '*',
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    UNIQUE(job_id, package_name)
);

CREATE INDEX IF NOT EXISTS idx_job_dependencies_job_id ON job_dependencies(job_id);
CREATE INDEX IF NOT EXISTS idx_job_dependencies_package_name ON job_dependencies(package_name);
