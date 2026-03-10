//! Virtual environment manager

use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, error, info};

/// Virtual environment manager
pub struct VenvManager {
    base_path: PathBuf,
    python_executable: String,
}

impl VenvManager {
    /// Create a new VenvManager
    pub fn new(base_path: &Path, python_executable: &str) -> Self {
        Self {
            base_path: base_path.to_path_buf(),
            python_executable: python_executable.to_string(),
        }
    }

    /// Get the main venv path
    pub fn main_venv_path(&self) -> PathBuf {
        self.base_path.join("main")
    }

    /// Get the path for a job-specific venv
    pub fn job_venv_path(&self, job_id: &str) -> PathBuf {
        self.base_path.join(format!("job-{}", job_id))
    }

    /// Check if main venv exists
    pub fn main_venv_exists(&self) -> bool {
        let python_path = self.get_python_path(&self.main_venv_path());
        python_path.exists()
    }

    /// Check if a job venv exists
    pub fn job_venv_exists(&self, job_id: &str) -> bool {
        let venv_path = self.job_venv_path(job_id);
        let python_path = self.get_python_path(&venv_path);
        python_path.exists()
    }

    /// Create the main virtual environment
    pub async fn create_main_venv(&self) -> Result<PathBuf, String> {
        let venv_path = self.main_venv_path();
        self.create_venv(&venv_path).await?;
        Ok(venv_path)
    }

    /// Create a job-specific virtual environment
    pub async fn create_job_venv(&self, job_id: &str) -> Result<PathBuf, String> {
        let venv_path = self.job_venv_path(job_id);
        self.create_venv(&venv_path).await?;
        Ok(venv_path)
    }

    /// Create a virtual environment at the specified path
    async fn create_venv(&self, path: &Path) -> Result<(), String> {
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        info!("Creating virtual environment at {:?}", path);

        let output = Command::new(&self.python_executable)
            .args(["-m", "venv", path.to_str().unwrap()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("Failed to create venv: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("Failed to create venv: {}", stderr);
            return Err(format!("Failed to create venv: {}", stderr));
        }

        debug!("Virtual environment created successfully");
        Ok(())
    }

    /// Delete a virtual environment
    pub async fn delete_venv(&self, path: &Path) -> Result<(), String> {
        if path.exists() {
            tokio::fs::remove_dir_all(path)
                .await
                .map_err(|e| format!("Failed to delete venv: {}", e))?;
            info!("Deleted virtual environment at {:?}", path);
        }
        Ok(())
    }

    /// Delete a job-specific venv
    pub async fn delete_job_venv(&self, job_id: &str) -> Result<(), String> {
        let path = self.job_venv_path(job_id);
        self.delete_venv(&path).await
    }

    /// Get the Python executable path for a venv
    pub fn get_python_path(&self, venv_path: &Path) -> PathBuf {
        if cfg!(windows) {
            venv_path.join("Scripts").join("python.exe")
        } else {
            venv_path.join("bin").join("python")
        }
    }

    /// Get the pip executable path for a venv
    pub fn get_pip_path(&self, venv_path: &Path) -> PathBuf {
        if cfg!(windows) {
            venv_path.join("Scripts").join("pip.exe")
        } else {
            venv_path.join("bin").join("pip")
        }
    }

    /// Get venv size in bytes (runs blocking FS walk on a dedicated thread)
    pub async fn get_venv_size(&self, venv_path: &Path) -> Result<u64, String> {
        let path = venv_path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            fn dir_size(path: &Path) -> std::io::Result<u64> {
                let mut size = 0;
                if path.is_dir() {
                    for entry in std::fs::read_dir(path)? {
                        let entry = entry?;
                        let p = entry.path();
                        if p.is_dir() {
                            size += dir_size(&p)?;
                        } else {
                            size += entry.metadata()?.len();
                        }
                    }
                }
                Ok(size)
            }

            if path.exists() {
                dir_size(&path)
                    .map_err(|e| format!("Failed to calculate venv size: {}", e))
            } else {
                Ok(0)
            }
        })
        .await
        .map_err(|e| format!("Blocking task failed: {}", e))?
    }

    /// Get Python version in a venv
    pub async fn get_python_version(&self, venv_path: &Path) -> Result<String, String> {
        let python_path = self.get_python_path(venv_path);

        let output = Command::new(&python_path)
            .arg("--version")
            .output()
            .await
            .map_err(|e| format!("Failed to get Python version: {}", e))?;

        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(version)
        } else {
            Err("Failed to get Python version".to_string())
        }
    }

    /// List installed packages in a venv
    pub async fn list_packages(&self, venv_path: &Path) -> Result<Vec<(String, String)>, String> {
        let pip_path = self.get_pip_path(venv_path);

        let output = Command::new(&pip_path)
            .args(["list", "--format", "json"])
            .output()
            .await
            .map_err(|e| format!("Failed to list packages: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to list packages: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let packages: Vec<serde_json::Value> = serde_json::from_str(&stdout)
            .map_err(|e| format!("Failed to parse package list: {}", e))?;

        let result = packages
            .into_iter()
            .filter_map(|p| {
                let name = p.get("name")?.as_str()?.to_string();
                let version = p.get("version")?.as_str()?.to_string();
                Some((name, version))
            })
            .collect();

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_venv_manager_paths() {
        let manager = VenvManager::new(Path::new("/tmp/venvs"), "python3");

        assert_eq!(manager.main_venv_path(), PathBuf::from("/tmp/venvs/main"));
        assert_eq!(
            manager.job_venv_path("test-job-123"),
            PathBuf::from("/tmp/venvs/job-test-job-123")
        );
    }

    #[test]
    fn test_get_python_path() {
        let manager = VenvManager::new(Path::new("/tmp/venvs"), "python3");
        let venv_path = Path::new("/tmp/venvs/main");
        let python_path = manager.get_python_path(venv_path);

        if cfg!(windows) {
            assert_eq!(python_path, PathBuf::from("/tmp/venvs/main/Scripts/python.exe"));
        } else {
            assert_eq!(python_path, PathBuf::from("/tmp/venvs/main/bin/python"));
        }
    }

    #[test]
    fn test_get_pip_path() {
        let manager = VenvManager::new(Path::new("/tmp/venvs"), "python3");
        let venv_path = Path::new("/tmp/venvs/main");
        let pip_path = manager.get_pip_path(venv_path);

        if cfg!(windows) {
            assert_eq!(pip_path, PathBuf::from("/tmp/venvs/main/Scripts/pip.exe"));
        } else {
            assert_eq!(pip_path, PathBuf::from("/tmp/venvs/main/bin/pip"));
        }
    }

    #[test]
    fn test_main_venv_exists_false() {
        let manager = VenvManager::new(Path::new("/nonexistent/path"), "python3");
        assert!(!manager.main_venv_exists());
    }

    #[test]
    fn test_job_venv_exists_false() {
        let manager = VenvManager::new(Path::new("/nonexistent/path"), "python3");
        assert!(!manager.job_venv_exists("some-job-id"));
    }
}
