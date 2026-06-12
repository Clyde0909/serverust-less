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
    /// Optional custom package index URL for pip installs.
    index_url: Option<String>,
    /// Trusted hosts to pass through to pip when using custom indexes.
    trusted_hosts: Vec<String>,
}

/// Builder for fluent package installation allowing optional version, extras, and custom venv.
#[derive(Debug, Clone)]
pub struct InstallBuilder {
    /// Package name (required)
    name: String,
    /// Optional version constraint (e.g., "==1.2.3" or ">=1.0,<2.0")
    version: Option<String>,
    /// Optional extras (e.g., "[extra1,extra2]") – currently not used in installation logic.
    extras: Vec<String>,
    /// Optional target venv path. If None, installs to the main venv.
    target_venv: Option<std::path::PathBuf>,
}

impl InstallBuilder {
    /// Start a new builder with the package name.
    pub fn new<N: Into<String>>(name: N) -> Self {
        Self {
            name: name.into(),
            version: None,
            extras: Vec::new(),
            target_venv: None,
        }
    }

    /// Set a version constraint.
    pub fn version<V: Into<String>>(mut self, version: V) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Add extras (comma‑separated or individual strings).
    pub fn extras<I, S>(mut self, extras: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.extras = extras.into_iter().map(|s| s.into()).collect();
        self
    }

    /// Target a specific virtual environment path.
    pub fn target_venv<P: Into<std::path::PathBuf>>(mut self, path: P) -> Self {
        self.target_venv = Some(path.into());
        self
    }

    /// Build the full package specification string, including extras and version constraint.
    pub fn build_spec(&self) -> String {
        let mut spec = self.name.clone();
        if !self.extras.is_empty() {
            let extras_joined = self.extras.join(",");
            spec = format!("{}[{}]", spec, extras_joined);
        }
        if let Some(ref ver) = self.version {
            // If version already contains an operator, keep it; otherwise prepend "=="
            let has_operator = ver.starts_with("==")
                || ver.starts_with(">=")
                || ver.starts_with("<=")
                || ver.starts_with('>')
                || ver.starts_with('<')
                || ver.starts_with("~=")
                || ver.starts_with("!=");
            if has_operator {
                spec.push_str(ver);
            } else {
                spec.push_str(&format!("=={}", ver));
            }
        }
        spec
    }

    /// Retrieve the optional version constraint (raw) for callers that need it.
    pub fn version_constraint(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

impl PackageManager {
    /// Create a new PackageManager
    pub fn new(pip_timeout: u64, use_cache: bool, cache_dir: Option<String>) -> Self {
        Self {
            main_venv_lock: Arc::new(Mutex::new(())),
            pip_timeout,
            use_cache,
            cache_dir,
            index_url: None,
            trusted_hosts: Vec::new(),
        }
    }

    /// Create a PackageManager with explicit index configuration.
    pub fn with_index_config(
        pip_timeout: u64,
        use_cache: bool,
        cache_dir: Option<String>,
        index_url: Option<String>,
        trusted_hosts: Vec<String>,
    ) -> Self {
        Self {
            main_venv_lock: Arc::new(Mutex::new(())),
            pip_timeout,
            use_cache,
            cache_dir,
            index_url,
            trusted_hosts,
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

        let args = self.build_install_args(&package_spec);

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

    /// Uninstall a package from the main venv (with locking)
    pub async fn uninstall_from_main_venv(&self, venv_path: &Path, package_name: &str) -> InstallResult {
        let _lock = self.main_venv_lock.lock().await;
        self.uninstall_package(venv_path, package_name).await
    }

    /// Uninstall a package (no locking — use `uninstall_from_main_venv` for shared venvs)
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

    fn build_install_args(&self, package_spec: &str) -> Vec<String> {
        let mut args = vec![
            "install".to_string(),
            package_spec.to_string(),
            "--no-input".to_string(),
            "--disable-pip-version-check".to_string(),
        ];

        if self.use_cache {
            if let Some(ref cache_dir) = self.cache_dir {
                args.push(format!("--cache-dir={}", cache_dir));
            }
        } else {
            args.push("--no-cache-dir".to_string());
        }

        if let Some(ref index_url) = self.index_url {
            if !index_url.trim().is_empty() {
                args.push("--index-url".to_string());
                args.push(index_url.clone());
            }
        }

        for trusted_host in &self.trusted_hosts {
            let host = trusted_host.trim();
            if !host.is_empty() {
                args.push("--trusted-host".to_string());
                args.push(host.to_string());
            }
        }

        args
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

    #[test]
    fn test_build_install_args_with_index_configuration() {
        let manager = PackageManager::with_index_config(
            120,
            true,
            Some("/tmp/cache".to_string()),
            Some("https://pypi.example.com/simple".to_string()),
            vec!["pypi.example.com".to_string(), "files.example.com".to_string()],
        );

        let args = manager.build_install_args("requests==2.31.0");

        assert!(args.contains(&"install".to_string()));
        assert!(args.contains(&"requests==2.31.0".to_string()));
        assert!(args.contains(&"--index-url".to_string()));
        assert!(args.contains(&"https://pypi.example.com/simple".to_string()));
        assert!(args.contains(&"--trusted-host".to_string()));
        assert!(args.contains(&"pypi.example.com".to_string()));
        assert!(args.contains(&"files.example.com".to_string()));
        assert!(args.iter().any(|arg| arg == "--cache-dir=/tmp/cache"));
    }

    #[test]
    fn test_build_install_args_without_cache() {
        let manager = PackageManager::new(60, false, None);
        let args = manager.build_install_args("numpy");

        assert!(args.contains(&"--no-cache-dir".to_string()));
        assert!(!args.contains(&"--index-url".to_string()));
    }
}
