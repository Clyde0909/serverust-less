-- Create python_packages table for package registry
CREATE TABLE IF NOT EXISTS python_packages (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    description TEXT,
    pypi_url TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    UNIQUE(name, version)
);

CREATE INDEX IF NOT EXISTS idx_python_packages_name ON python_packages(name);
CREATE INDEX IF NOT EXISTS idx_python_packages_version ON python_packages(version);
