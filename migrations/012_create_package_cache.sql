-- Create package_cache table for tracking installed packages
CREATE TABLE IF NOT EXISTS package_cache (
    id TEXT PRIMARY KEY,
    venv_type TEXT NOT NULL,
    venv_id TEXT,
    package_name TEXT NOT NULL,
    version TEXT NOT NULL,
    installation_path TEXT NOT NULL,
    size_bytes INTEGER,
    status TEXT NOT NULL DEFAULT 'installing',
    error_message TEXT,
    installed_at TEXT DEFAULT (datetime('now')),
    last_used_at TEXT,
    use_count INTEGER DEFAULT 0,
    UNIQUE(package_name, version, venv_type, venv_id)
);

CREATE INDEX IF NOT EXISTS idx_package_cache_venv ON package_cache(venv_type, venv_id);
CREATE INDEX IF NOT EXISTS idx_package_cache_status ON package_cache(status);
CREATE INDEX IF NOT EXISTS idx_package_cache_package ON package_cache(package_name, version);
