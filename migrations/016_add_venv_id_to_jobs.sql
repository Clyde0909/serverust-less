-- Add venv_id column to jobs to allow selecting a specific virtual environment
ALTER TABLE jobs ADD COLUMN venv_id TEXT REFERENCES venvs(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_jobs_venv_id ON jobs(venv_id);
