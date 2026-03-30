//! Job executor - coordinates job execution

use std::path::PathBuf;
use std::sync::Arc;

use crate::models::{Execution, QueueItem};
use crate::worker::python_runner::{ExecutionResult, PythonRunner};

/// Job executor handles the execution of a single job
pub struct JobExecutor {
    runner: Arc<PythonRunner>,
    main_venv_path: PathBuf,
    custom_venv_base_path: PathBuf,
}

impl JobExecutor {
    /// Create a new JobExecutor
    pub fn new(
        runner: Arc<PythonRunner>,
        main_venv_path: PathBuf,
        custom_venv_base_path: PathBuf,
    ) -> Self {
        Self {
            runner,
            main_venv_path,
            custom_venv_base_path,
        }
    }

    /// Get the venv path for a queue item
    pub fn get_venv_path(&self, item: &QueueItem) -> PathBuf {
        if item.use_custom_venv {
            self.custom_venv_base_path.join(format!("job-{}", item.job_id))
        } else {
            self.main_venv_path.clone()
        }
    }

    /// Execute a queued item
    pub async fn execute(&self, item: &QueueItem) -> ExecutionResult {
        let venv_path = self.get_venv_path(item);
        self.runner
            .execute(
                &venv_path,
                &item.python_code,
                item.input_data.as_deref(),
                item.timeout_seconds as u64,
                item.memory_limit_mb as u64,
            )
            .await
    }

    /// Execute a queued item, sending the child PID via `pid_tx` right after spawn.
    /// Use this variant when cancellation support is required.
    pub async fn execute_with_pid(
        &self,
        item: &QueueItem,
        pid_tx: tokio::sync::oneshot::Sender<u32>,
    ) -> ExecutionResult {
        let venv_path = self.get_venv_path(item);
        self.runner
            .execute_with_pid(
                &venv_path,
                &item.python_code,
                item.input_data.as_deref(),
                item.timeout_seconds as u64,
                item.memory_limit_mb as u64,
                pid_tx,
            )
            .await
    }

    /// Create execution result from runner result (test-only helper)
    #[cfg(test)]
    pub fn create_execution_result(
        &self,
        mut execution: Execution,
        result: &ExecutionResult,
    ) -> Execution {
        if result.timed_out {
            execution.mark_timeout();
        } else if result.memory_exceeded {
            execution.mark_failed("Memory limit exceeded".to_string());
        } else if result.success {
            execution.mark_success(result.stdout.clone());
        } else {
            let error = if result.stderr.is_empty() {
                format!("Execution failed with exit code {:?}", result.exit_code)
            } else {
                result.stderr.clone()
            };
            execution.mark_failed(error);
        }

        execution
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Execution;

    fn create_test_queue_item(use_custom_venv: bool) -> QueueItem {
        QueueItem::new(
            "exec-123",
            "job-456",
            0,
            "print('hello')",
            30,
            128,
            None,
            use_custom_venv,
        )
    }

    #[test]
    fn test_get_venv_path_main() {
        let runner = Arc::new(PythonRunner::new("python3"));
        let executor = JobExecutor::new(
            runner,
            PathBuf::from("/venvs/main"),
            PathBuf::from("/venvs"),
        );

        let item = create_test_queue_item(false);
        let path = executor.get_venv_path(&item);
        assert_eq!(path, PathBuf::from("/venvs/main"));
    }

    #[test]
    fn test_get_venv_path_custom() {
        let runner = Arc::new(PythonRunner::new("python3"));
        let executor = JobExecutor::new(
            runner,
            PathBuf::from("/venvs/main"),
            PathBuf::from("/venvs"),
        );

        let item = create_test_queue_item(true);
        let path = executor.get_venv_path(&item);
        assert_eq!(path, PathBuf::from("/venvs/job-job-456"));
    }

    #[test]
    fn test_create_execution_result_success() {
        let runner = Arc::new(PythonRunner::new("python3"));
        let executor = JobExecutor::new(
            runner,
            PathBuf::from("/venvs/main"),
            PathBuf::from("/venvs"),
        );

        let execution = Execution::new("job-123", None);
        let result = ExecutionResult {
            success: true,
            stdout: "Hello, World!".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            duration_ms: 100,
            timed_out: false,
            memory_exceeded: false,
        };

        let updated = executor.create_execution_result(execution, &result);
        assert_eq!(updated.status, "success");
        assert_eq!(updated.output_data, Some("Hello, World!".to_string()));
    }

    #[test]
    fn test_create_execution_result_timeout() {
        let runner = Arc::new(PythonRunner::new("python3"));
        let executor = JobExecutor::new(
            runner,
            PathBuf::from("/venvs/main"),
            PathBuf::from("/venvs"),
        );

        let execution = Execution::new("job-123", None);
        let result = ExecutionResult {
            success: false,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            duration_ms: 30000,
            timed_out: true,
            memory_exceeded: false,
        };

        let updated = executor.create_execution_result(execution, &result);
        assert_eq!(updated.status, "timeout");
    }

    #[test]
    fn test_create_execution_result_memory_exceeded() {
        let runner = Arc::new(PythonRunner::new("python3"));
        let executor = JobExecutor::new(
            runner,
            PathBuf::from("/venvs/main"),
            PathBuf::from("/venvs"),
        );

        let execution = Execution::new("job-123", None);
        let result = ExecutionResult {
            success: false,
            stdout: String::new(),
            stderr: "MemoryError".to_string(),
            exit_code: Some(-9),
            duration_ms: 1000,
            timed_out: false,
            memory_exceeded: true,
        };

        let updated = executor.create_execution_result(execution, &result);
        assert_eq!(updated.status, "failed");
        assert!(updated.error_message.unwrap().contains("Memory limit exceeded"));
    }

    #[test]
    fn test_create_execution_result_failed() {
        let runner = Arc::new(PythonRunner::new("python3"));
        let executor = JobExecutor::new(
            runner,
            PathBuf::from("/venvs/main"),
            PathBuf::from("/venvs"),
        );

        let execution = Execution::new("job-123", None);
        let result = ExecutionResult {
            success: false,
            stdout: String::new(),
            stderr: "NameError: name 'x' is not defined".to_string(),
            exit_code: Some(1),
            duration_ms: 50,
            timed_out: false,
            memory_exceeded: false,
        };

        let updated = executor.create_execution_result(execution, &result);
        assert_eq!(updated.status, "failed");
        assert!(updated.error_message.unwrap().contains("NameError"));
    }
}
