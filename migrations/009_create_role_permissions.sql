-- Create role_permissions junction table
CREATE TABLE IF NOT EXISTS role_permissions (
    role_id TEXT NOT NULL,
    permission_id TEXT NOT NULL,
    created_at TEXT DEFAULT (datetime('now')),
    PRIMARY KEY (role_id, permission_id),
    FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE,
    FOREIGN KEY (permission_id) REFERENCES permissions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_role_permissions_role_id ON role_permissions(role_id);

-- Assign all permissions to admin role
INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT 'role-admin', id FROM permissions;

-- Assign basic permissions to user role
INSERT OR IGNORE INTO role_permissions (role_id, permission_id) VALUES
    ('role-user', 'perm-job-create'),
    ('role-user', 'perm-job-read'),
    ('role-user', 'perm-job-update'),
    ('role-user', 'perm-job-delete'),
    ('role-user', 'perm-job-execute'),
    ('role-user', 'perm-execution-read'),
    ('role-user', 'perm-execution-cancel');

-- Assign read-only permissions to viewer role
INSERT OR IGNORE INTO role_permissions (role_id, permission_id) VALUES
    ('role-viewer', 'perm-job-read'),
    ('role-viewer', 'perm-execution-read');
