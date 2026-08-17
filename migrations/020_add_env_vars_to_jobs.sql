-- Add env_vars column to jobs table for per-job environment variables
ALTER TABLE jobs ADD COLUMN env_vars TEXT;

-- Add env_vars column to job_versions table for immutable snapshots
ALTER TABLE job_versions ADD COLUMN env_vars TEXT;
