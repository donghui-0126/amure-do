/// Configuration system — loads from amure-do.toml, falls back to defaults.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::server::backend::BackendConfig;

const CONFIG_FILE: &str = "amure-do.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmureConfig {
    #[serde(default)]
    pub project: ProjectConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub backend: BackendConfig,
    #[serde(default)]
    pub llm: LlmSection,
    #[serde(default)]
    pub gates: GatesConfig,
    #[serde(default)]
    pub dashboard: DashboardConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default = "default_project_name")]
    pub name: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default = "default_description")]
    pub description: String,
}

fn default_project_name() -> String { "My Research".into() }
fn default_domain() -> String { "general".into() }
fn default_description() -> String { "Hypothesis-driven research project".into() }

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: default_project_name(),
            domain: default_domain(),
            description: default_description(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_host() -> String { "0.0.0.0".into() }
fn default_port() -> u16 { 8080 }

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSection {
    #[serde(default = "default_provider")]
    pub default_provider: String,
    pub default_model: Option<String>,
    pub default_api_key: Option<String>,
    pub default_api_url: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
}

fn default_provider() -> String { "claude_cli".into() }
fn default_max_tokens() -> usize { 4096 }

impl Default for LlmSection {
    fn default() -> Self {
        Self {
            default_provider: default_provider(),
            default_model: None,
            default_api_key: None,
            default_api_url: None,
            max_tokens: default_max_tokens(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatesConfig {
    #[serde(default = "default_gates")]
    pub enabled: Vec<String>,
}

fn default_gates() -> Vec<String> {
    vec!["claim_gate".into(), "argument_gate".into()]
}

impl Default for GatesConfig {
    fn default() -> Self {
        Self { enabled: default_gates() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default = "default_accent")]
    pub accent_color: String,
}

fn default_title() -> String { "amure-do".into() }
fn default_accent() -> String { "#58a6ff".into() }

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            title: default_title(),
            accent_color: default_accent(),
        }
    }
}

impl Default for AmureConfig {
    fn default() -> Self {
        Self {
            project: ProjectConfig::default(),
            server: ServerConfig::default(),
            backend: BackendConfig::default(),
            llm: LlmSection::default(),
            gates: GatesConfig::default(),
            dashboard: DashboardConfig::default(),
        }
    }
}

impl AmureConfig {
    /// Load configuration from amure-do.toml, falling back to defaults.
    pub fn load() -> Self {
        let path = Path::new(CONFIG_FILE);
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => match toml::from_str::<AmureConfig>(&content) {
                    Ok(config) => return config,
                    Err(e) => {
                        tracing::warn!("Failed to parse {}: {} — using defaults", CONFIG_FILE, e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read {}: {} — using defaults", CONFIG_FILE, e);
                }
            }
        }
        Self::default()
    }

    /// Save current configuration back to amure-do.toml.
    pub fn save(&self) -> Result<(), String> {
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(CONFIG_FILE, content).map_err(|e| e.to_string())
    }

    /// Build LlmConfig from the [llm] section for backward compatibility.
    pub fn to_llm_config(&self) -> crate::server::llm_provider::LlmConfig {
        crate::server::llm_provider::LlmConfig {
            provider: self.llm.default_provider.clone(),
            model: self.llm.default_model.clone().unwrap_or_default(),
            api_key: self.llm.default_api_key.clone(),
            api_url: self.llm.default_api_url.clone(),
            max_tokens: self.llm.max_tokens,
            ..Default::default()
        }
    }
}
