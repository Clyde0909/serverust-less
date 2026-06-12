//! Python code runner - executes Python code in a subprocess

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// Result of Python execution
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Whether execution succeeded
    pub success: bool,
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Exit code
    pub exit_code: Option<i32>,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Whether execution timed out
    pub timed_out: bool,
    /// Whether memory limit was exceeded
    pub memory_exceeded: bool,
}

/// Python runner for executing code
pub struct PythonRunner {
    python_executable: String,
}

impl PythonRunner {
    /// Create a new Python runner
    pub fn new(python_executable: &str) -> Self {
        Self {
            python_executable: python_executable.to_string(),
        }
    }

    /// Execute Python code with the given venv
    pub async fn execute(
        &self,
        venv_path: &Path,
        code: &str,
        input_data: Option<&str>,
        timeout_seconds: u64,
        memory_limit_mb: u64,
    ) -> ExecutionResult {
        let start = std::time::Instant::now();

        // Construct the Python executable path from venv
        let python_path = if cfg!(windows) {
            venv_path.join("Scripts").join("python.exe")
        } else {
            venv_path.join("bin").join("python")
        };

        // Prepare the code with optional input data handling
        let full_code = if let Some(input) = input_data {
            format!(
                r#"
import json
import sys

# Input data
INPUT_DATA = json.loads('''{}''')

# User code
{}
"#,
                input.replace("'''", r"\'\'\'"),
                code
            )
        } else {
            code.to_string()
        };

        debug!(
            venv = %venv_path.display(),
            timeout_seconds = timeout_seconds,
            memory_limit_mb = memory_limit_mb,
            code_length = code.len(),
            has_input = input_data.is_some(),
            "Executing Python code"
        );

        // Choose python executable: prefer venv's python, fall back only if no venv path given.
        // Using an absolute path avoids silent fallback to system Python when the relative
        // ./venvs/main/bin/python check fails due to a different process CWD.
        let chosen_python_path = if python_path.exists() {
            python_path
        } else {
            warn!(
                path = %python_path.display(),
                "Venv Python not found at expected path — falling back to configured executable. \
                 Packages installed in the venv will NOT be available."
            );
            std::path::PathBuf::from(&self.python_executable)
        };

        // Build the command with resource limits
        let result = self
            .spawn_with_limits(&chosen_python_path, &full_code, timeout_seconds, memory_limit_mb, None)
            .await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(exec_result) => {
                if exec_result.success {
                    info!(
                        duration_ms = duration_ms,
                        exit_code = ?exec_result.exit_code,
                        stdout_len = exec_result.stdout.len(),
                        "Python execution succeeded"
                    );
                } else {
                    warn!(
                        duration_ms = duration_ms,
                        exit_code = ?exec_result.exit_code,
                        timed_out = exec_result.timed_out,
                        memory_exceeded = exec_result.memory_exceeded,
                        stderr_len = exec_result.stderr.len(),
                        "Python execution failed"
                    );
                }
                ExecutionResult {
                    duration_ms,
                    ..exec_result
                }
            }
            Err(e) => {
                error!(error = %e, "Python execution error");
                ExecutionResult {
                    success: false,
                    stdout: String::new(),
                    stderr: e,
                    exit_code: None,
                    duration_ms,
                    timed_out: false,
                    memory_exceeded: false,
                }
            }
        }
    }

    /// Spawn Python process with resource limits.
    /// If `pid_tx` is provided the child's OS PID is sent through it immediately
    /// after a successful `spawn()`, before the process is awaited.  This lets
    /// the caller register the PID for cancellation while the job is running.
    async fn spawn_with_limits(
        &self,
        python_path: &Path,
        code: &str,
        timeout_seconds: u64,
        memory_limit_mb: u64,
        pid_tx: Option<tokio::sync::oneshot::Sender<u32>>,
    ) -> Result<ExecutionResult, String> {
        debug!(
            python = %python_path.display(),
            timeout = timeout_seconds,
            memory_mb = memory_limit_mb,
            "Spawning Python process with limits"
        );
        
        // Create wrapper script that sets resource limits on Unix
        #[cfg(unix)]
        let (cmd_path, cmd_args, temp_script) = self
            .create_limited_command_unix(python_path, code, memory_limit_mb)
            .await?;

        #[cfg(not(unix))]
        let (cmd_path, cmd_args, temp_script) = (
            python_path.to_path_buf(),
            vec!["-c".to_string(), code.to_string()],
            None::<tempfile::NamedTempFile>,
        );

        // Spawn the process
        let mut cmd = Command::new(&cmd_path);
        cmd.args(&cmd_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Set environment variables
        cmd.env("PYTHONUNBUFFERED", "1");

        let spawn_result = cmd.spawn();

        let mut child = match spawn_result {
            Ok(child) => child,
            Err(e) => {
                error!("Failed to spawn Python process: {}", e);
                return Err(format!("Failed to spawn Python process: {}", e));
            }
        };

        // Report PID immediately so ProcessManager can send SIGTERM/SIGKILL on cancel.
        if let Some(tx) = pid_tx {
            if let Some(pid) = child.id() {
                let _ = tx.send(pid);
            }
        }

        // Capture stdout and stderr
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        let stdout_task = tokio::spawn(async move {
            let mut output = String::new();
            if let Some(stdout) = stdout_handle {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    output.push_str(&line);
                    output.push('\n');
                }
            }
            output
        });

        let stderr_task = tokio::spawn(async move {
            let mut output = String::new();
            if let Some(stderr) = stderr_handle {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    output.push_str(&line);
                    output.push('\n');
                }
            }
            output
        });

        // Wait for completion with timeout
        let timeout_duration = Duration::from_secs(timeout_seconds);
        let wait_result = timeout(timeout_duration, child.wait()).await;

        let (timed_out, exit_code) = match wait_result {
            Ok(Ok(status)) => (false, status.code()),
            Ok(Err(e)) => {
                error!("Error waiting for Python process: {}", e);
                (false, None)
            }
            Err(_) => {
                // Timeout - kill the process
                info!(
                    "Python execution timed out after {} seconds",
                    timeout_seconds
                );
                let _ = child.kill().await;
                (true, None)
            }
        };

        // Collect output
        let stdout = stdout_task.await.unwrap_or_default();
        let stderr = stderr_task.await.unwrap_or_default();

        // Check for memory limit exceeded (killed by OOM or signal 9)
        let memory_exceeded = Self::check_memory_exceeded(exit_code, &stderr);

        // Clean up temp script if created
        drop(temp_script);

        let success = !timed_out && !memory_exceeded && exit_code == Some(0);

        Ok(ExecutionResult {
            success,
            stdout,
            stderr,
            exit_code,
            duration_ms: 0, // Will be set by caller
            timed_out,
            memory_exceeded,
        })
    }

    /// Create a command with resource limits on Unix
    #[cfg(unix)]
    async fn create_limited_command_unix(
        &self,
        python_path: &Path,
        code: &str,
        memory_limit_mb: u64,
    ) -> Result<(std::path::PathBuf, Vec<String>, Option<tempfile::NamedTempFile>), String> {
        use std::io::Write;

        // Create a temporary script that sets limits and runs Python
        let limit_bytes = memory_limit_mb * 1024 * 1024;

        // Create temp file for the Python code
        let mut code_file = tempfile::Builder::new()
            .prefix("pycode_")
            .suffix(".py")
            .tempfile()
            .map_err(|e| format!("Failed to create temp file: {}", e))?;

        code_file
            .write_all(code.as_bytes())
            .map_err(|e| format!("Failed to write code to temp file: {}", e))?;

        let code_path = code_file.path().to_string_lossy().to_string();
        let python_path_str = python_path.to_string_lossy().to_string();

        // Use bash to set ulimit before executing Python
        // This sets both virtual memory (RLIMIT_AS) and data segment (RLIMIT_DATA) limits
        let bash_cmd = format!(
            "ulimit -v {} 2>/dev/null; ulimit -d {} 2>/dev/null; exec \"{}\" \"{}\"",
            limit_bytes / 1024, // ulimit uses KB
            limit_bytes / 1024,
            python_path_str,
            code_path
        );

        Ok((
            std::path::PathBuf::from("/bin/bash"),
            vec!["-c".to_string(), bash_cmd],
            Some(code_file),
        ))
    }

    /// Check if the process was killed due to memory limit
    fn check_memory_exceeded(exit_code: Option<i32>, stderr: &str) -> bool {
        // On Unix, signal 9 (SIGKILL) often indicates OOM
        if exit_code == Some(-9) || exit_code == Some(137) {
            return true;
        }

        // Check stderr for memory-related errors
        let memory_indicators = [
            "MemoryError",
            "Cannot allocate memory",
            "Out of memory",
            "memory allocation failed",
            "std::bad_alloc",
        ];

        for indicator in &memory_indicators {
            if stderr.contains(indicator) {
                return true;
            }
        }

        false
    }

    /// Check if Python is available
    pub async fn check_python(&self) -> Result<String, String> {
        let output = Command::new(&self.python_executable)
            .arg("--version")
            .output()
            .await
            .map_err(|e| format!("Failed to run Python: {}", e))?;

        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(version)
        } else {
            Err("Python check failed".to_string())
        }
    }

    /// Execute Python code, sending the child process PID through `pid_tx`
    /// immediately after spawn so the caller can arrange cancellation.
    pub async fn execute_with_pid(
        &self,
        venv_path: &Path,
        code: &str,
        input_data: Option<&str>,
        timeout_seconds: u64,
        memory_limit_mb: u64,
        pid_tx: tokio::sync::oneshot::Sender<u32>,
    ) -> ExecutionResult {
        let start = std::time::Instant::now();

        let python_path = if cfg!(windows) {
            venv_path.join("Scripts").join("python.exe")
        } else {
            venv_path.join("bin").join("python")
        };

        let full_code = if let Some(input) = input_data {
            format!(
                r#"
import json
import sys

# Input data
INPUT_DATA = json.loads('''{inp}''')

# User code
{code}
"#,
                inp = input.replace("'''", r"\'\'\'" ),
                code = code
            )
        } else {
            code.to_string()
        };

        let chosen_python_path = if python_path.exists() {
            python_path
        } else {
            warn!(
                path = %python_path.display(),
                "Venv Python not found at expected path — falling back to configured executable. \
                 Packages installed in the venv will NOT be available."
            );
            std::path::PathBuf::from(&self.python_executable)
        };

        let result = self
            .spawn_with_limits(
                &chosen_python_path,
                &full_code,
                timeout_seconds,
                memory_limit_mb,
                Some(pid_tx),
            )
            .await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(exec_result) => {
                if exec_result.success {
                    info!(duration_ms, "Python execution (tracked) succeeded");
                } else {
                    warn!(
                        duration_ms,
                        timed_out = exec_result.timed_out,
                        memory_exceeded = exec_result.memory_exceeded,
                        "Python execution (tracked) failed"
                    );
                }
                ExecutionResult {
                    duration_ms,
                    ..exec_result
                }
            }
            Err(e) => {
                error!(error = %e, "Python execution error");
                ExecutionResult {
                    success: false,
                    stdout: String::new(),
                    stderr: e,
                    exit_code: None,
                    duration_ms,
                    timed_out: false,
                    memory_exceeded: false,
                }
            }
        }
    }

    /// Execute Python code and stream output in real-time
    pub async fn execute_streaming<F>(
        &self,
        venv_path: &Path,
        code: &str,
        input_data: Option<&str>,
        timeout_seconds: u64,
        memory_limit_mb: u64,
        mut on_output: F,
    ) -> ExecutionResult
    where
        F: FnMut(OutputLine) + Send + 'static,
    {
        let start = std::time::Instant::now();

        // Construct the Python executable path from venv
        let python_path = if cfg!(windows) {
            venv_path.join("Scripts").join("python.exe")
        } else {
            venv_path.join("bin").join("python")
        };

        // Prepare the code with optional input data handling
        let full_code = if let Some(input) = input_data {
            format!(
                r#"
import json
import sys

# Input data
INPUT_DATA = json.loads('''{}''')

# User code
{}
"#,
                input.replace("'''", r"\'\'\'"),
                code
            )
        } else {
            code.to_string()
        };

        let chosen_python_path = if python_path.exists() {
            python_path
        } else {
            warn!(
                path = %python_path.display(),
                "Venv Python not found — falling back to configured executable."
            );
            std::path::PathBuf::from(&self.python_executable)
        };

        // Build command with resource limits (same as spawn_with_limits)
        #[cfg(unix)]
        let cmd_setup = self
            .create_limited_command_unix(&chosen_python_path, &full_code, memory_limit_mb)
            .await;
        #[cfg(unix)]
        let (cmd_path, cmd_args, _temp_script) = match cmd_setup {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to build limited command: {}", e);
                return ExecutionResult {
                    success: false,
                    stdout: String::new(),
                    stderr: format!("Failed to build limited command: {}", e),
                    exit_code: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                    timed_out: false,
                    memory_exceeded: false,
                };
            }
        };

        #[cfg(not(unix))]
        let (cmd_path, cmd_args, _temp_script) = (
            chosen_python_path.clone(),
            vec!["-c".to_string(), full_code.clone()],
            None::<tempfile::NamedTempFile>,
        );

        // Spawn the Python process with memory limits applied
        let mut cmd = Command::new(&cmd_path);
        cmd.args(&cmd_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        cmd.env("PYTHONUNBUFFERED", "1");

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                error!("Failed to spawn Python process: {}", e);
                return ExecutionResult {
                    success: false,
                    stdout: String::new(),
                    stderr: format!("Failed to spawn Python process: {}", e),
                    exit_code: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                    timed_out: false,
                    memory_exceeded: false,
                };
            }
        };

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // Create channels for streaming
        let (tx, mut rx) = tokio::sync::mpsc::channel::<OutputLine>(100);

        let tx_stdout = tx.clone();
        let stdout_task = tokio::spawn(async move {
            let mut output = String::new();
            if let Some(stdout) = stdout_handle {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx_stdout.send(OutputLine::Stdout(line.clone())).await;
                    output.push_str(&line);
                    output.push('\n');
                }
            }
            output
        });

        let tx_stderr = tx;
        let stderr_task = tokio::spawn(async move {
            let mut output = String::new();
            if let Some(stderr) = stderr_handle {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx_stderr.send(OutputLine::Stderr(line.clone())).await;
                    output.push_str(&line);
                    output.push('\n');
                }
            }
            output
        });

        // Process output in real-time
        let output_task = tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                on_output(line);
            }
        });

        // Wait for completion with timeout
        let timeout_duration = Duration::from_secs(timeout_seconds);
        let wait_result = timeout(timeout_duration, child.wait()).await;

        let (timed_out, exit_code) = match wait_result {
            Ok(Ok(status)) => (false, status.code()),
            Ok(Err(e)) => {
                error!("Error waiting for Python process: {}", e);
                (false, None)
            }
            Err(_) => {
                info!(
                    "Python execution timed out after {} seconds",
                    timeout_seconds
                );
                let _ = child.kill().await;
                (true, None)
            }
        };

        // Collect final output
        let all_stdout = stdout_task.await.unwrap_or_default();
        let all_stderr = stderr_task.await.unwrap_or_default();
        let _ = output_task.await;

        let memory_exceeded = Self::check_memory_exceeded(exit_code, &all_stderr);
        let success = !timed_out && !memory_exceeded && exit_code == Some(0);

        ExecutionResult {
            success,
            stdout: all_stdout,
            stderr: all_stderr,
            exit_code,
            duration_ms: start.elapsed().as_millis() as u64,
            timed_out,
            memory_exceeded,
        }
    }
}

/// Output line type for streaming
#[derive(Debug, Clone)]
pub enum OutputLine {
    Stdout(String),
    Stderr(String),
}

impl Default for PythonRunner {
    fn default() -> Self {
        Self::new("python3")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_check_python() {
        let runner = PythonRunner::new("python3");
        let result = runner.check_python().await;
        // This might fail if python3 is not installed, but that's expected
        if result.is_ok() {
            assert!(result.unwrap().contains("Python"));
        }
    }

    #[test]
    fn test_check_memory_exceeded() {
        assert!(PythonRunner::check_memory_exceeded(Some(-9), ""));
        assert!(PythonRunner::check_memory_exceeded(Some(137), ""));
        assert!(PythonRunner::check_memory_exceeded(None, "MemoryError: out of memory"));
        assert!(!PythonRunner::check_memory_exceeded(Some(0), ""));
        assert!(!PythonRunner::check_memory_exceeded(Some(1), "Some other error"));
    }
}

