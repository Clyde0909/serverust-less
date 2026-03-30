-- Create DAG runs and DAG node executions tables

CREATE TABLE IF NOT EXISTS dag_runs (
    id TEXT PRIMARY KEY,
    dag_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    trigger_type TEXT NOT NULL DEFAULT 'manual',
    started_at TEXT,
    completed_at TEXT,
    total_nodes INTEGER DEFAULT 0,
    completed_nodes INTEGER DEFAULT 0,
    failed_nodes INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (dag_id) REFERENCES dags(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS dag_node_executions (
    id TEXT PRIMARY KEY,
    dag_run_id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    execution_id TEXT,
    status TEXT NOT NULL DEFAULT 'waiting',
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (dag_run_id) REFERENCES dag_runs(id) ON DELETE CASCADE,
    FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    FOREIGN KEY (execution_id) REFERENCES executions(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_dag_runs_dag_id ON dag_runs(dag_id);
CREATE INDEX IF NOT EXISTS idx_dag_runs_status ON dag_runs(status);
CREATE INDEX IF NOT EXISTS idx_dag_node_executions_run_id ON dag_node_executions(dag_run_id);
CREATE INDEX IF NOT EXISTS idx_dag_node_executions_execution_id ON dag_node_executions(execution_id);
