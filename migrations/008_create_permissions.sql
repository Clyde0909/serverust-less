-- Create permissions table for fine-grained RBAC
CREATE TABLE IF NOT EXISTS permissions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    resource TEXT NOT NULL,
    action TEXT NOT NULL,
    description TEXT,
    created_at TEXT DEFAULT (datetime('now'))
);

-- Insert default permissions
INSERT OR IGNORE INTO permissions (id, name, resource, action, description) VALUES 
    ('perm-job-create', 'job.create', 'job', 'create', 'Create new jobs'),
    ('perm-job-read', 'job.read', 'job', 'read', 'View jobs'),
    ('perm-job-update', 'job.update', 'job', 'update', 'Update jobs'),
    ('perm-job-delete', 'job.delete', 'job', 'delete', 'Delete jobs'),
    ('perm-job-execute', 'job.execute', 'job', 'execute', 'Execute jobs'),
    ('perm-execution-read', 'execution.read', 'execution', 'read', 'View executions'),
    ('perm-execution-cancel', 'execution.cancel', 'execution', 'cancel', 'Cancel executions'),
    ('perm-package-install', 'package.install', 'package', 'install', 'Install packages'),
    ('perm-package-delete', 'package.delete', 'package', 'delete', 'Delete packages'),
    ('perm-venv-manage', 'venv.manage', 'venv', 'manage', 'Manage virtual environments');
