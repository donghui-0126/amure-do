/// Backend abstraction — pluggable compute backend.
/// Supports: HTTP, File-based (original Julia protocol), Subprocess, None.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BackendType {
    Http,
    File,
    Subprocess,
    None,
}

impl Default for BackendType {
    fn default() -> Self { Self::None }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    #[serde(rename = "type", default)]
    pub backend_type: BackendType,
    pub url: Option<String>,
    pub dir: Option<String>,
    pub command: Option<String>,
    pub script: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    pub health_endpoint: Option<String>,
}

fn default_timeout() -> u64 { 300 }

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            backend_type: BackendType::None,
            url: None,
            dir: None,
            command: None,
            script: None,
            timeout_secs: 300,
            health_endpoint: None,
        }
    }
}

pub struct Backend {
    config: BackendConfig,
}

impl Backend {
    pub fn new(config: BackendConfig) -> Self { Self { config } }

    pub fn config(&self) -> &BackendConfig { &self.config }

    /// Check if backend is alive.
    pub async fn health_check(&self) -> Result<bool, String> {
        match self.config.backend_type {
            BackendType::Http => {
                let url = self.config.url.as_ref().ok_or("No URL configured")?;
                let endpoint = self.config.health_endpoint.as_deref().unwrap_or("/health");
                let full_url = format!("{}{}", url.trim_end_matches('/'), endpoint);
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build().map_err(|e| e.to_string())?;
                match client.get(&full_url).send().await {
                    Ok(r) => Ok(r.status().is_success()),
                    Err(_) => Ok(false),
                }
            }
            BackendType::File => {
                let dir = self.config.dir.as_deref().unwrap_or("backend");
                Ok(PathBuf::from(dir).join("_ready").exists())
            }
            BackendType::Subprocess => {
                let dir = self.config.dir.as_deref().unwrap_or("backend");
                let pid_file = PathBuf::from(dir).join("_server.pid");
                Ok(pid_file.exists())
            }
            BackendType::None => Ok(true),
        }
    }

    /// Execute code/command on the backend.
    pub async fn exec(&self, code: &str, timeout: Option<Duration>) -> Result<(String, String), String> {
        let timeout = timeout.unwrap_or(Duration::from_secs(self.config.timeout_secs));
        match self.config.backend_type {
            BackendType::Http => self.exec_http(code, timeout).await,
            BackendType::File => self.exec_file(code, timeout).await,
            BackendType::Subprocess => self.exec_subprocess(code, timeout).await,
            BackendType::None => Err("No backend configured. Set [backend] in amure-do.toml".into()),
        }
    }

    /// Start the backend server.
    pub async fn start(&self) -> Result<String, String> {
        match self.config.backend_type {
            BackendType::Http => Ok("HTTP backend is external — start it manually".into()),
            BackendType::File => self.start_file_backend().await,
            BackendType::Subprocess => self.start_subprocess_backend().await,
            BackendType::None => Ok("No backend configured".into()),
        }
    }

    // --- HTTP backend ---
    async fn exec_http(&self, code: &str, timeout: Duration) -> Result<(String, String), String> {
        let url = self.config.url.as_ref().ok_or("No URL for HTTP backend")?;
        let exec_url = format!("{}/exec", url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build().map_err(|e| e.to_string())?;
        let body = serde_json::json!({"code": code, "timeout_secs": timeout.as_secs()});
        let resp = client.post(&exec_url)
            .header("content-type", "application/json")
            .json(&body)
            .send().await.map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let status = json.get("status").and_then(|v| v.as_str()).unwrap_or("ok").to_string();
        let output = json.get("output").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if let Some(err) = json.get("error").and_then(|v| v.as_str()) {
            return Err(err.to_string());
        }
        Ok((status, output))
    }

    // --- File-based backend (original Julia protocol) ---
    async fn exec_file(&self, code: &str, timeout: Duration) -> Result<(String, String), String> {
        let dir = PathBuf::from(self.config.dir.as_deref().unwrap_or("backend"));
        let cmd_file = dir.join("_cmd.txt");
        let out_file = dir.join("_out.txt");
        let log_file = dir.join("_server.log");

        if !dir.join("_ready").exists() {
            return Err("Backend server not running. Start it first.".into());
        }

        let _ = std::fs::remove_file(&out_file);
        let log_before = std::fs::read_to_string(&log_file).unwrap_or_default().len();
        std::fs::write(&cmd_file, code).map_err(|e| format!("Failed to write cmd: {}", e))?;

        let start = Instant::now();
        loop {
            if start.elapsed() > timeout {
                return Err(format!("Timeout after {}s", timeout.as_secs()));
            }
            if out_file.exists() {
                let result = std::fs::read_to_string(&out_file).unwrap_or_default();
                let log_all = std::fs::read_to_string(&log_file).unwrap_or_default();
                let log_new = if log_all.len() > log_before { log_all[log_before..].to_string() } else { String::new() };
                return Ok((result, log_new));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    // --- Subprocess backend ---
    async fn exec_subprocess(&self, code: &str, timeout: Duration) -> Result<(String, String), String> {
        let command = self.config.command.as_deref().ok_or("No command configured for subprocess backend")?;
        let output = tokio::process::Command::new(command)
            .arg("-c")
            .arg(code)
            .output();
        let result = tokio::time::timeout(timeout, output).await
            .map_err(|_| format!("Timeout after {}s", timeout.as_secs()))?
            .map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).to_string();
        if result.status.success() {
            Ok(("ok".into(), stdout))
        } else {
            Err(format!("Exit {}: {}", result.status, stderr))
        }
    }

    // --- Start file-based backend ---
    async fn start_file_backend(&self) -> Result<String, String> {
        let dir = PathBuf::from(self.config.dir.as_deref().unwrap_or("backend"));
        if dir.join("_ready").exists() {
            return Ok("Already running".into());
        }
        let command = self.config.command.as_deref().ok_or("No command configured")?;
        let script = self.config.script.as_deref().ok_or("No script configured")?;

        let _ = std::fs::remove_file(dir.join("_cmd.txt"));
        let _ = std::fs::remove_file(dir.join("_out.txt"));
        let _ = std::fs::remove_file(dir.join("_ready"));

        let script_path = std::fs::canonicalize(dir.join(script)).map_err(|e| format!("Script not found: {}", e))?;
        let log_path = dir.join("_server.log");

        let child = std::process::Command::new("nohup")
            .args([command, &script_path.to_string_lossy()])
            .current_dir(&dir)
            .stdout(std::fs::File::create(&log_path).map_err(|e| e.to_string())?)
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to start: {}", e))?;

        let pid = child.id();
        let _ = std::fs::write(dir.join("_server.pid"), pid.to_string());

        // Wait for ready
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(90) {
            if dir.join("_ready").exists() {
                return Ok(format!("Started (pid {})", pid));
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        Ok(format!("Timeout waiting for ready (pid {})", pid))
    }

    // --- Start subprocess backend ---
    async fn start_subprocess_backend(&self) -> Result<String, String> {
        Ok("Subprocess backend runs on-demand".into())
    }
}
