-- Create roles table for RBAC (optional feature)
CREATE TABLE IF NOT EXISTS roles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at TEXT DEFAULT (datetime('now'))
);

-- Insert default roles
INSERT OR IGNORE INTO roles (id, name, description) VALUES 
    ('role-admin', 'admin', 'Full system access'),
    ('role-user', 'user', 'Standard user access'),
    ('role-viewer', 'viewer', 'Read-only access');
