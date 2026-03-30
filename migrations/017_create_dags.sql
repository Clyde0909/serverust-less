-- Create DAGs and DAG edges tables for job dependency graphs

CREATE TABLE IF NOT EXISTS dags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    enabled INTEGER DEFAULT 1,
    max_concurrent_nodes INTEGER DEFAULT 4,
    on_failure TEXT DEFAULT 'stop',
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS dag_edges (
    id TEXT PRIMARY KEY,
    dag_id TEXT NOT NULL,
    upstream_job_id TEXT NOT NULL,
    downstream_job_id TEXT NOT NULL,
    condition TEXT DEFAULT 'success',
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (dag_id) REFERENCES dags(id) ON DELETE CASCADE,
    FOREIGN KEY (upstream_job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    FOREIGN KEY (downstream_job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    UNIQUE(dag_id, upstream_job_id, downstream_job_id)
);

CREATE INDEX IF NOT EXISTS idx_dag_edges_dag_id ON dag_edges(dag_id);
CREATE INDEX IF NOT EXISTS idx_dag_edges_upstream ON dag_edges(upstream_job_id);
CREATE INDEX IF NOT EXISTS idx_dag_edges_downstream ON dag_edges(downstream_job_id);
