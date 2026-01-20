//! Package manager for pip operations

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

/// Result of a package installation
#[derive(Debug, Clone)]
pub struct InstallResult {
    pub success: bool,
    pub package_name: String,
    pub version: Option<String>,
    pub error: Option<String>,
}

/// Package manager for pip operations
pub struct PackageManager {
    /// Lock for main venv pip operations
    main_venv_lock: Arc<Mutex<()>>,
    /// Pip timeout in seconds
    pip_timeout: u64,
    /// Whether to use pip cache
    use_cache: bool,
    /// Cache directory
    cache_dir: Option<String>,
}

impl PackageManager {
    /// Create a new PackageManager
    pub fn new(pip_timeout: u64, use_cache: bool, cache_dir: Option<String>) -> Self {
        Self {
            main_venv_lock: Arc::new(Mutex::new(())),
            pip_timeout,
            use_cache,
            cache_dir,
        }
    }

    /// Install a package into the main venv (with locking)
    pub async fn install_to_main_venv(
        &self,
        venv_path: &Path,
        package_name: &str,
        version_constraint: Option<&str>,
    ) -> InstallResult {
        // Lock for main venv operations
        let _lock = self.main_venv_lock.lock().await;
        self.install_package(venv_path, package_name, version_constraint)
            .await
    }

    /// Install a package into a custom venv (no locking needed)
    pub async fn install_to_custom_venv(
        &self,
        venv_path: &Path,
        package_name: &str,
        version_constraint: Option<&str>,
    ) -> InstallResult {
        self.install_package(venv_path, package_name, version_constraint)
            .await
    }

    /// Install a package
    async fn install_package(
        &self,
        venv_path: &Path,
        package_name: &str,
        version_constraint: Option<&str>,
    ) -> InstallResult {
        let pip_path = self.get_pip_path(venv_path);

        // Build the package spec
        let package_spec = match version_constraint {
            Some(constraint) if constraint != "*" => {
                // If constraint doesn't start with an operator, default to ==
                if constraint.starts_with("==") || constraint.starts_with(">=") 
                   || constraint.starts_with("<=") || constraint.starts_with(">") 
                   || constraint.starts_with("<") || constraint.starts_with("~=") 
                   || constraint.starts_with("!=") {
                    format!("{}{}", package_name, constraint)
                } else {
                    format!("{}=={}", package_name, constraint)
                }
            }
            _ => package_name.to_string(),
        };

        info!("Installing package: {}", package_spec);

        let mut args = vec![
            "install".to_string(),
            package_spec.clone(),
            "--no-input".to_string(),
        ];

        // Add cache directory if configured
        if self.use_cache {
            if let Some(ref cache_dir) = self.cache_dir {
                args.push(format!("--cache-dir={}", cache_dir));
            }
        } else {
            args.push("--no-cache-dir".to_string());
        }

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(self.pip_timeout),
            Command::new(&pip_path)
                .args(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    // Try to get installed version
                    let version = self.get_installed_version(venv_path, package_name).await;
                    debug!("Package {} installed successfully: {:?}", package_name, version);

                    InstallResult {
                        success: true,
                        package_name: package_name.to_string(),
                        version,
                        error: None,
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Failed to install {}: {}", package_name, stderr);

                    InstallResult {
                        success: false,
                        package_name: package_name.to_string(),
                        version: None,
                        error: Some(stderr.to_string()),
                    }
                }
            }
            Ok(Err(e)) => {
                error!("Failed to run pip: {}", e);
                InstallResult {
                    success: false,
                    package_name: package_name.to_string(),
                    version: None,
                    error: Some(format!("Failed to run pip: {}", e)),
                }
            }
            Err(_) => {
                error!("Pip install timed out for {}", package_name);
                InstallResult {
                    success: false,
                    package_name: package_name.to_string(),
                    version: None,
                    error: Some("Installation timed out".to_string()),
                }
            }
        }
    }

    /// Uninstall a package
    pub async fn uninstall_package(&self, venv_path: &Path, package_name: &str) -> InstallResult {
        let pip_path = self.get_pip_path(venv_path);

        let output = Command::new(&pip_path)
            .args(["uninstall", package_name, "-y"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(output) => {
                if output.status.success() {
                    info!("Package {} uninstalled successfully", package_name);
                    InstallResult {
                        success: true,
                        package_name: package_name.to_string(),
                        version: None,
                        error: None,
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    InstallResult {
                        success: false,
                        package_name: package_name.to_string(),
                        version: None,
                        error: Some(stderr.to_string()),
                    }
                }
            }
            Err(e) => InstallResult {
                success: false,
                package_name: package_name.to_string(),
                version: None,
                error: Some(format!("Failed to run pip: {}", e)),
            },
        }
    }

    /// Get the installed version of a package
    async fn get_installed_version(&self, venv_path: &Path, package_name: &str) -> Option<String> {
        let pip_path = self.get_pip_path(venv_path);

        let output = Command::new(&pip_path)
            .args(["show", package_name])
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.starts_with("Version:") {
                return Some(line.replace("Version:", "").trim().to_string());
            }
        }

        None
    }

    /// Check if a package is installed
    pub async fn is_installed(&self, venv_path: &Path, package_name: &str) -> bool {
        self.get_installed_version(venv_path, package_name)
            .await
            .is_some()
    }

    /// Upgrade pip in a venv
    pub async fn upgrade_pip(&self, venv_path: &Path) -> Result<(), String> {
        let pip_path = self.get_pip_path(venv_path);

        let output = Command::new(&pip_path)
            .args(["install", "--upgrade", "pip"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("Failed to upgrade pip: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Failed to upgrade pip: {}", stderr))
        }
    }

    /// Install multiple packages
    pub async fn install_packages(
        &self,
        venv_path: &Path,
        packages: &[(String, Option<String>)],
        is_main_venv: bool,
    ) -> Vec<InstallResult> {
        let mut results = Vec::new();

        for (package_name, version_constraint) in packages {
            let result = if is_main_venv {
                self.install_to_main_venv(venv_path, package_name, version_constraint.as_deref())
                    .await
            } else {
                self.install_to_custom_venv(venv_path, package_name, version_constraint.as_deref())
                    .await
            };
            results.push(result);
        }

        results
    }

    /// Get pip path for a venv
    fn get_pip_path(&self, venv_path: &Path) -> std::path::PathBuf {
        if cfg!(windows) {
            venv_path.join("Scripts").join("pip.exe")
        } else {
            venv_path.join("bin").join("pip")
        }
    }
}

impl Default for PackageManager {
    fn default() -> Self {
        Self::new(300, true, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_manager_new() {
        let manager = PackageManager::new(120, true, Some("/tmp/cache".to_string()));
        assert_eq!(manager.pip_timeout, 120);
        assert!(manager.use_cache);
        assert_eq!(manager.cache_dir, Some("/tmp/cache".to_string()));
    }

    #[test]
    fn test_package_manager_default() {
        let manager = PackageManager::default();
        assert_eq!(manager.pip_timeout, 300);
        assert!(manager.use_cache);
        assert!(manager.cache_dir.is_none());
    }

    #[test]
    fn test_install_result() {
        let result = InstallResult {
            success: true,
            package_name: "requests".to_string(),
            version: Some("2.28.0".to_string()),
            error: None,
        };
        assert!(result.success);
        assert_eq!(result.package_name, "requests");
        assert_eq!(result.version, Some("2.28.0".to_string()));
        assert!(result.error.is_none());
    }

    #[test]
    fn test_install_result_failure() {
        let result = InstallResult {
            success: false,
            package_name: "nonexistent".to_string(),
            version: None,
            error: Some("Package not found".to_string()),
        };
        assert!(!result.success);
        assert!(result.version.is_none());
        assert!(result.error.is_some());
    }

    #[test]
    fn test_get_pip_path() {
        let manager = PackageManager::default();
        let venv_path = std::path::Path::new("/tmp/venv");
        let pip_path = manager.get_pip_path(venv_path);

        if cfg!(windows) {
            assert_eq!(pip_path, std::path::PathBuf::from("/tmp/venv/Scripts/pip.exe"));
        } else {
            assert_eq!(pip_path, std::path::PathBuf::from("/tmp/venv/bin/pip"));
        }
    }
}
