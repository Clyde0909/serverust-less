//! Process manager - tracks and manages child processes for cancellation

use std::collections::HashMap;
use std::process::ExitStatus;
use std::sync::Arc;
use tokio::process::Child;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Handle to a running process
pub struct ProcessHandle {
    /// Process ID
    pub pid: u32,
    /// Execution ID this process belongs to
    pub execution_id: String,
    /// Job ID
    pub job_id: String,
    /// Worker ID handling this process
    pub worker_id: usize,
    /// When the process started
    pub started_at: std::time::Instant,
}

/// Process manager for tracking and cancelling child processes
pub struct ProcessManager {
    /// Map of execution_id -> process info
    processes: Arc<RwLock<HashMap<String, ProcessInfo>>>,
    /// Graceful shutdown timeout in seconds
    graceful_shutdown_seconds: u64,
}

/// Internal process info
struct ProcessInfo {
    /// Process ID (if available)
    pid: Option<u32>,
    /// Job ID
    job_id: String,
    /// Worker ID
    worker_id: usize,
    /// Cancellation signal sender
    cancel_tx: tokio::sync::oneshot::Sender<()>,
    /// Started at
    started_at: std::time::Instant,
}

/// Result of process registration
pub struct ProcessRegistration {
    /// Cancellation receiver - process should check this for cancellation
    pub cancel_rx: tokio::sync::oneshot::Receiver<()>,
}

impl ProcessManager {
    /// Create a new ProcessManager
    pub fn new(graceful_shutdown_seconds: u64) -> Self {
        Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            graceful_shutdown_seconds,
        }
    }

    /// Register a process for tracking
    pub async fn register(
        &self,
        execution_id: &str,
        job_id: &str,
        pid: Option<u32>,
        worker_id: usize,
    ) -> ProcessRegistration {
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();

        let info = ProcessInfo {
            pid,
            job_id: job_id.to_string(),
            worker_id,
            cancel_tx,
            started_at: std::time::Instant::now(),
        };

        let mut processes = self.processes.write().await;
        processes.insert(execution_id.to_string(), info);

        debug!(
            "Registered process for execution {} (pid: {:?}, worker: {})",
            execution_id, pid, worker_id
        );

        ProcessRegistration { cancel_rx }
    }

    /// Update the PID for a registered process
    pub async fn update_pid(&self, execution_id: &str, pid: u32) {
        let mut processes = self.processes.write().await;
        if let Some(info) = processes.get_mut(execution_id) {
            info.pid = Some(pid);
            debug!("Updated PID for execution {}: {}", execution_id, pid);
        }
    }

    /// Unregister a process (called when execution completes)
    pub async fn unregister(&self, execution_id: &str) {
        let mut processes = self.processes.write().await;
        if processes.remove(execution_id).is_some() {
            debug!("Unregistered process for execution {}", execution_id);
        }
    }

    /// Cancel a running process
    pub async fn cancel(&self, execution_id: &str) -> Result<bool, String> {
        let mut processes = self.processes.write().await;

        if let Some(info) = processes.remove(execution_id) {
            info!("Cancelling execution {} (pid: {:?})", execution_id, info.pid);

            // Send cancellation signal
            let _ = info.cancel_tx.send(());

            // If we have a PID, also try to kill the process directly
            #[cfg(unix)]
            if let Some(pid) = info.pid {
                self.kill_process_unix(pid).await;
            }

            #[cfg(windows)]
            if let Some(pid) = info.pid {
                self.kill_process_windows(pid).await;
            }

            Ok(true)
        } else {
            debug!("No process found for execution {}", execution_id);
            Ok(false)
        }
    }

    /// Kill a process on Unix using signals
    #[cfg(unix)]
    async fn kill_process_unix(&self, pid: u32) {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        let pid = Pid::from_raw(pid as i32);

        // First try SIGTERM for graceful shutdown
        info!("Sending SIGTERM to process {}", pid);
        if let Err(e) = kill(pid, Signal::SIGTERM) {
            warn!("Failed to send SIGTERM to {}: {}", pid, e);
            return;
        }

        // Wait for graceful shutdown
        let timeout = tokio::time::Duration::from_secs(self.graceful_shutdown_seconds);
        tokio::time::sleep(timeout).await;

        // Check if process is still running and send SIGKILL
        if kill(pid, None).is_ok() {
            info!("Process {} still running, sending SIGKILL", pid);
            if let Err(e) = kill(pid, Signal::SIGKILL) {
                warn!("Failed to send SIGKILL to {}: {}", pid, e);
            }
        }
    }

    /// Kill a process on Windows with graceful shutdown
    #[cfg(windows)]
    async fn kill_process_windows(&self, pid: u32) {
        // First try graceful termination (without /F)
        info!("Sending graceful termination to process {}", pid);
        let output = tokio::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                info!("Process {} terminated gracefully", pid);
                return;
            }
            _ => {
                debug!("Graceful termination failed for process {}, waiting before force kill", pid);
            }
        }

        // Wait for graceful shutdown period
        let timeout = tokio::time::Duration::from_secs(self.graceful_shutdown_seconds);
        tokio::time::sleep(timeout).await;

        // Force kill with /F
        info!("Force killing process {}", pid);
        let output = tokio::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                info!("Successfully force-killed process {}", pid);
            }
            Ok(out) => {
                warn!(
                    "Failed to force-kill process {}: {}",
                    pid,
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            Err(e) => {
                warn!("Failed to run taskkill for process {}: {}", pid, e);
            }
        }
    }

    /// Fallback kill for non-unix/windows systems
    #[cfg(not(any(unix, windows)))]
    async fn kill_process_unix(&self, _pid: u32) {
        warn!("Process killing not implemented for this platform");
    }

    #[cfg(not(any(unix, windows)))]
    async fn kill_process_windows(&self, _pid: u32) {
        warn!("Process killing not implemented for this platform");
    }

    /// Get the number of running processes
    pub async fn running_count(&self) -> usize {
        let processes = self.processes.read().await;
        processes.len()
    }

    /// Get list of running execution IDs
    pub async fn running_executions(&self) -> Vec<String> {
        let processes = self.processes.read().await;
        processes.keys().cloned().collect()
    }

    /// Get process info for an execution
    pub async fn get_info(&self, execution_id: &str) -> Option<ProcessHandle> {
        let processes = self.processes.read().await;
        processes.get(execution_id).map(|info| ProcessHandle {
            pid: info.pid.unwrap_or(0),
            execution_id: execution_id.to_string(),
            job_id: info.job_id.clone(),
            worker_id: info.worker_id,
            started_at: info.started_at,
        })
    }

    /// Cancel all running processes (for shutdown)
    pub async fn cancel_all(&self) {
        let execution_ids: Vec<String> = {
            let processes = self.processes.read().await;
            processes.keys().cloned().collect()
        };

        for execution_id in execution_ids {
            let _ = self.cancel(&execution_id).await;
        }
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new(5) // 5 second default graceful shutdown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_unregister() {
        let manager = ProcessManager::new(5);

        // Register a process
        let _reg = manager.register("exec-1", "job-1", Some(1234), 0).await;
        assert_eq!(manager.running_count().await, 1);

        // Unregister
        manager.unregister("exec-1").await;
        assert_eq!(manager.running_count().await, 0);
    }

    #[tokio::test]
    async fn test_cancel_nonexistent() {
        let manager = ProcessManager::new(5);

        // Cancel non-existent process should return Ok(false)
        let result = manager.cancel("nonexistent").await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_running_executions() {
        let manager = ProcessManager::new(5);

        let _reg1 = manager.register("exec-1", "job-1", Some(1234), 0).await;
        let _reg2 = manager.register("exec-2", "job-2", Some(5678), 1).await;

        let running = manager.running_executions().await;
        assert_eq!(running.len(), 2);
        assert!(running.contains(&"exec-1".to_string()));
        assert!(running.contains(&"exec-2".to_string()));
    }

    #[tokio::test]
    async fn test_update_pid() {
        let manager = ProcessManager::new(5);

        let _reg = manager.register("exec-1", "job-1", None, 0).await;
        manager.update_pid("exec-1", 9999).await;

        let info = manager.get_info("exec-1").await;
        assert!(info.is_some());
        assert_eq!(info.unwrap().pid, 9999);
    }
}
