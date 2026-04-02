/// LLM Provider abstraction — supports any AI provider with role-based routing.
///
/// Supported providers:
///   claude_cli        — Claude CLI (`claude -p`)
///   claude_api        — Anthropic Messages API
///   openai            — OpenAI Chat Completions API
///   openai_compatible — Any OpenAI-compatible API (vLLM, LiteLLM, LocalAI, etc.)
///   ollama            — Ollama REST API (/api/chat)
///   google            — Google AI / Gemini API (generateContent)
///   azure             — Azure OpenAI Service
///   groq              — Groq API (OpenAI-compatible)
///   together          — Together AI (OpenAI-compatible)
///   lmstudio          — LM Studio (OpenAI-compatible, local)
///   custom            — Arbitrary endpoint with flexible response parsing

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

// ── Config Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub api_key: Option<String>,
    pub api_url: Option<String>,
    pub max_tokens: usize,
    /// Sampling temperature (0.0–2.0). None = provider default.
    #[serde(default)]
    pub temperature: Option<f64>,
    /// System prompt prepended to all requests. None = no system prompt.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Azure-specific: resource name (e.g. "my-resource" → my-resource.openai.azure.com)
    #[serde(default)]
    pub azure_resource: Option<String>,
    /// Azure-specific: API version (default "2024-02-01")
    #[serde(default)]
    pub azure_api_version: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "claude_cli".into(),
            model: String::new(),
            api_key: None,
            api_url: None,
            max_tokens: 4096,
            temperature: None,
            system_prompt: None,
            azure_resource: None,
            azure_api_version: None,
        }
    }
}

/// Role-based LLM routing configuration.
/// When roles is empty, all roles use the default config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRouting {
    pub default: LlmConfig,
    /// Per-role overrides. Keys: "lab", "judge", "canvas", "experiment", "gate", etc.
    #[serde(default)]
    pub roles: HashMap<String, LlmConfig>,
}

impl Default for LlmRouting {
    fn default() -> Self {
        Self {
            default: LlmConfig::default(),
            roles: HashMap::new(),
        }
    }
}

// ── Persistence ──────────────────────────────────────────────────────────────

const CONFIG_FILE: &str = "data/knowledge_db/llm_config.json";

impl LlmConfig {
    pub fn load() -> Self {
        if let Ok(content) = std::fs::read_to_string(CONFIG_FILE) {
            if let Ok(routing) = serde_json::from_str::<LlmRouting>(&content) {
                return routing.default;
            }
            if let Ok(config) = serde_json::from_str::<LlmConfig>(&content) {
                return config;
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(CONFIG_FILE, json);
        }
    }
}

impl LlmRouting {
    pub fn load() -> Self {
        if let Ok(content) = std::fs::read_to_string(CONFIG_FILE) {
            if let Ok(routing) = serde_json::from_str::<LlmRouting>(&content) {
                return routing;
            }
            if let Ok(config) = serde_json::from_str::<LlmConfig>(&content) {
                return Self { default: config, roles: HashMap::new() };
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(CONFIG_FILE, json);
        }
    }

    /// Get config for a specific role. Falls back to default.
    pub fn config_for(&self, role: &str) -> &LlmConfig {
        self.roles.get(role).unwrap_or(&self.default)
    }
}

// ── Provider Info & Presets ──────────────────────────────────────────────────

/// Metadata about an available provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    pub display_name: String,
    pub needs_api_key: bool,
    pub needs_url: bool,
    pub default_url: Option<String>,
    pub default_model: String,
    pub is_local: bool,
}

/// List all available providers with their metadata.
pub fn available_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            name: "claude_cli".into(),
            display_name: "Claude CLI".into(),
            needs_api_key: false,
            needs_url: false,
            default_url: None,
            default_model: String::new(),
            is_local: true,
        },
        ProviderInfo {
            name: "claude_api".into(),
            display_name: "Anthropic Claude API".into(),
            needs_api_key: true,
            needs_url: false,
            default_url: Some("https://api.anthropic.com/v1/messages".into()),
            default_model: "claude-sonnet-4-20250514".into(),
            is_local: false,
        },
        ProviderInfo {
            name: "openai".into(),
            display_name: "OpenAI".into(),
            needs_api_key: true,
            needs_url: false,
            default_url: Some("https://api.openai.com/v1".into()),
            default_model: "gpt-4o".into(),
            is_local: false,
        },
        ProviderInfo {
            name: "openai_compatible".into(),
            display_name: "OpenAI-Compatible API".into(),
            needs_api_key: false,
            needs_url: true,
            default_url: None,
            default_model: "default".into(),
            is_local: false,
        },
        ProviderInfo {
            name: "ollama".into(),
            display_name: "Ollama".into(),
            needs_api_key: false,
            needs_url: false,
            default_url: Some("http://localhost:11434".into()),
            default_model: "llama3.1".into(),
            is_local: true,
        },
        ProviderInfo {
            name: "google".into(),
            display_name: "Google AI (Gemini)".into(),
            needs_api_key: true,
            needs_url: false,
            default_url: Some("https://generativelanguage.googleapis.com/v1beta".into()),
            default_model: "gemini-2.0-flash".into(),
            is_local: false,
        },
        ProviderInfo {
            name: "azure".into(),
            display_name: "Azure OpenAI".into(),
            needs_api_key: true,
            needs_url: false,
            default_url: None,
            default_model: "gpt-4o".into(),
            is_local: false,
        },
        ProviderInfo {
            name: "groq".into(),
            display_name: "Groq".into(),
            needs_api_key: true,
            needs_url: false,
            default_url: Some("https://api.groq.com/openai/v1".into()),
            default_model: "llama-3.1-70b-versatile".into(),
            is_local: false,
        },
        ProviderInfo {
            name: "together".into(),
            display_name: "Together AI".into(),
            needs_api_key: true,
            needs_url: false,
            default_url: Some("https://api.together.xyz/v1".into()),
            default_model: "meta-llama/Llama-3-70b-chat-hf".into(),
            is_local: false,
        },
        ProviderInfo {
            name: "lmstudio".into(),
            display_name: "LM Studio".into(),
            needs_api_key: false,
            needs_url: false,
            default_url: Some("http://localhost:1234".into()),
            default_model: "default".into(),
            is_local: true,
        },
        ProviderInfo {
            name: "custom".into(),
            display_name: "Custom Endpoint".into(),
            needs_api_key: false,
            needs_url: true,
            default_url: None,
            default_model: String::new(),
            is_local: false,
        },
    ]
}

/// Return a sensible default config for a given provider name.
pub fn provider_preset(provider: &str) -> LlmConfig {
    match provider {
        "claude_cli" => LlmConfig {
            provider: "claude_cli".into(),
            ..Default::default()
        },
        "claude_api" => LlmConfig {
            provider: "claude_api".into(),
            model: "claude-sonnet-4-20250514".into(),
            ..Default::default()
        },
        "openai" => LlmConfig {
            provider: "openai".into(),
            model: "gpt-4o".into(),
            ..Default::default()
        },
        "openai_compatible" => LlmConfig {
            provider: "openai_compatible".into(),
            model: "default".into(),
            ..Default::default()
        },
        "ollama" => LlmConfig {
            provider: "ollama".into(),
            model: "llama3.1".into(),
            api_url: Some("http://localhost:11434".into()),
            ..Default::default()
        },
        "google" => LlmConfig {
            provider: "google".into(),
            model: "gemini-2.0-flash".into(),
            ..Default::default()
        },
        "azure" => LlmConfig {
            provider: "azure".into(),
            model: "gpt-4o".into(),
            azure_api_version: Some("2024-02-01".into()),
            ..Default::default()
        },
        "groq" => LlmConfig {
            provider: "groq".into(),
            model: "llama-3.1-70b-versatile".into(),
            api_url: Some("https://api.groq.com/openai/v1".into()),
            ..Default::default()
        },
        "together" => LlmConfig {
            provider: "together".into(),
            model: "meta-llama/Llama-3-70b-chat-hf".into(),
            api_url: Some("https://api.together.xyz/v1".into()),
            ..Default::default()
        },
        "lmstudio" => LlmConfig {
            provider: "lmstudio".into(),
            model: "default".into(),
            api_url: Some("http://localhost:1234".into()),
            ..Default::default()
        },
        _ => LlmConfig::default(),
    }
}

// ── LLM Call Dispatch ────────────────────────────────────────────────────────

/// Call LLM with the configured provider (backward compatible).
pub async fn call_llm(prompt: &str, config: &LlmConfig) -> Result<String, String> {
    match config.provider.as_str() {
        "claude_cli" => call_claude_cli(prompt, config).await,
        "claude_api" => call_claude_api(prompt, config).await,
        "openai" => call_openai(prompt, config).await,
        "openai_compatible" => {
            let base = config.api_url.as_deref()
                .ok_or("openai_compatible requires api_url")?;
            call_openai_compatible(prompt, config, base).await
        }
        "ollama" => call_ollama(prompt, config).await,
        "google" => call_google(prompt, config).await,
        "azure" => call_azure(prompt, config).await,
        // groq, together, lmstudio are OpenAI-compatible with preset base URLs
        "groq" => {
            let base = config.api_url.as_deref()
                .unwrap_or("https://api.groq.com/openai/v1");
            call_openai_compatible(prompt, config, base).await
        }
        "together" => {
            let base = config.api_url.as_deref()
                .unwrap_or("https://api.together.xyz/v1");
            call_openai_compatible(prompt, config, base).await
        }
        "lmstudio" => {
            let base = config.api_url.as_deref()
                .unwrap_or("http://localhost:1234");
            call_openai_compatible(prompt, config, base).await
        }
        "custom" => call_custom(prompt, config).await,
        other => Err(format!(
            "Unknown LLM provider: '{}'. Available: claude_cli, claude_api, openai, \
             openai_compatible, ollama, google, azure, groq, together, lmstudio, custom",
            other
        )),
    }
}

/// Role-based LLM call. Uses routing to find the right config for the role.
pub async fn call_llm_for_role(prompt: &str, role: &str, routing: &LlmRouting) -> Result<String, String> {
    let config = routing.config_for(role);
    call_llm(prompt, config).await
}

// ── HTTP Client ──────────────────────────────────────────────────────────────

/// Shared reqwest client with 120s timeout.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap_or_default()
}

// ── Provider Implementations ─────────────────────────────────────────────────

/// Claude CLI — calls `claude -p` subprocess.
/// No API key needed; uses whatever Claude CLI auth is configured locally.
async fn call_claude_cli(prompt: &str, config: &LlmConfig) -> Result<String, String> {
    let mut args = vec!["-p".to_string(), prompt.to_string()];

    // Pass model if specified
    if !config.model.is_empty() {
        args.push("--model".into());
        args.push(config.model.clone());
    }

    let output = std::process::Command::new("claude")
        .args(&args)
        .output();

    match output {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).to_string()),
        Ok(o) => Err(format!("Claude CLI error: {}", String::from_utf8_lossy(&o.stderr))),
        Err(e) => Err(format!("Claude CLI not found: {}", e)),
    }
}

/// Anthropic Messages API.
/// Docs: https://docs.anthropic.com/en/api/messages
/// Request: POST with model, max_tokens, messages[]. Optional: system, temperature.
/// Response: { "content": [{ "text": "..." }] }
async fn call_claude_api(prompt: &str, config: &LlmConfig) -> Result<String, String> {
    let api_key = config.api_key.as_ref()
        .ok_or("claude_api requires api_key (Anthropic API key)")?;
    let model = if config.model.is_empty() { "claude-sonnet-4-20250514" } else { &config.model };
    let url = config.api_url.as_deref()
        .unwrap_or("https://api.anthropic.com/v1/messages");

    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": config.max_tokens,
        "messages": [{"role": "user", "content": prompt}]
    });

    // Anthropic uses a top-level "system" field, not a system message in messages[]
    if let Some(sys) = &config.system_prompt {
        body["system"] = serde_json::Value::String(sys.clone());
    }
    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    let resp = http_client()
        .post(url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("claude_api request failed: {}", e))?;

    let status = resp.status();
    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("claude_api response parse failed: {}", e))?;

    if !status.is_success() {
        return Err(format!("claude_api HTTP {}: {}", status, json));
    }

    json["content"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("claude_api unexpected response format: {}", json))
}

/// OpenAI Chat Completions API.
/// Docs: https://platform.openai.com/docs/api-reference/chat/create
/// Request: POST /v1/chat/completions with model, max_tokens, messages[].
/// Response: { "choices": [{ "message": { "content": "..." } }] }
async fn call_openai(prompt: &str, config: &LlmConfig) -> Result<String, String> {
    let api_key = config.api_key.as_ref()
        .ok_or("openai requires api_key (OpenAI API key)")?;
    let base = config.api_url.as_deref().unwrap_or("https://api.openai.com/v1");
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));

    let body = build_openai_body(prompt, config, "gpt-4o");

    let resp = http_client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("openai request failed: {}", e))?;

    parse_openai_response(resp, "openai").await
}

/// OpenAI-compatible API — shared handler for groq, together, lmstudio, vLLM,
/// LiteLLM, LocalAI, and any service implementing the OpenAI chat completions format.
/// `base_url` is the base URL (e.g. "https://api.groq.com/openai/v1").
/// The endpoint appended is /chat/completions.
async fn call_openai_compatible(prompt: &str, config: &LlmConfig, base_url: &str) -> Result<String, String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = build_openai_body(prompt, config, "default");

    let mut req = http_client()
        .post(&url)
        .header("content-type", "application/json");

    // API key is optional — local providers (lmstudio, vLLM) often don't need one
    if let Some(key) = &config.api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    let resp = req
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("{} request failed: {}", config.provider, e))?;

    parse_openai_response(resp, &config.provider).await
}

/// Ollama REST API — uses /api/chat with messages format.
/// Docs: https://github.com/ollama/ollama/blob/main/docs/api.md
/// Default URL: http://localhost:11434, default model: "llama3.1".
/// Request: POST /api/chat with model, messages[], stream:false, options.
/// Response: { "message": { "content": "..." } }
async fn call_ollama(prompt: &str, config: &LlmConfig) -> Result<String, String> {
    let base = config.api_url.as_deref().unwrap_or("http://localhost:11434");
    let url = format!("{}/api/chat", base.trim_end_matches('/'));
    let model = if config.model.is_empty() { "llama3.1" } else { &config.model };

    let mut messages = Vec::new();
    if let Some(sys) = &config.system_prompt {
        messages.push(serde_json::json!({"role": "system", "content": sys}));
    }
    messages.push(serde_json::json!({"role": "user", "content": prompt}));

    let mut options = serde_json::json!({
        "num_predict": config.max_tokens
    });
    if let Some(temp) = config.temperature {
        options["temperature"] = serde_json::json!(temp);
    }

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "options": options
    });

    let resp = http_client()
        .post(&url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("ollama request failed: {}", e))?;

    let status = resp.status();
    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("ollama response parse failed: {}", e))?;

    if !status.is_success() {
        return Err(format!("ollama HTTP {}: {}", status, json));
    }

    // Ollama /api/chat response format
    json["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("ollama unexpected response format: {}", json))
}

/// Google AI / Gemini API — generateContent endpoint.
/// Docs: https://ai.google.dev/api/generate-content
/// API key goes in query parameter `?key=`.
/// Request: POST /v1beta/models/{model}:generateContent with contents[], generationConfig.
/// Response: { "candidates": [{ "content": { "parts": [{ "text": "..." }] } }] }
async fn call_google(prompt: &str, config: &LlmConfig) -> Result<String, String> {
    let api_key = config.api_key.as_ref()
        .ok_or("google requires api_key (Google AI API key)")?;
    let model = if config.model.is_empty() { "gemini-2.0-flash" } else { &config.model };
    let base = config.api_url.as_deref()
        .unwrap_or("https://generativelanguage.googleapis.com/v1beta");

    let url = format!(
        "{}/models/{}:generateContent?key={}",
        base.trim_end_matches('/'),
        model,
        api_key
    );

    let mut body = serde_json::json!({
        "contents": [{
            "parts": [{"text": prompt}]
        }]
    });

    // Gemini uses a separate systemInstruction field
    if let Some(sys) = &config.system_prompt {
        body["systemInstruction"] = serde_json::json!({
            "parts": [{"text": sys}]
        });
    }

    let mut gen_config = serde_json::json!({
        "maxOutputTokens": config.max_tokens
    });
    if let Some(temp) = config.temperature {
        gen_config["temperature"] = serde_json::json!(temp);
    }
    body["generationConfig"] = gen_config;

    let resp = http_client()
        .post(&url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("google request failed: {}", e))?;

    let status = resp.status();
    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("google response parse failed: {}", e))?;

    if !status.is_success() {
        return Err(format!("google HTTP {}: {}", status, json));
    }

    json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("google unexpected response format: {}", json))
}

/// Azure OpenAI Service.
/// Docs: https://learn.microsoft.com/en-us/azure/ai-services/openai/reference
/// URL: https://{resource}.openai.azure.com/openai/deployments/{model}/chat/completions?api-version={version}
/// API key goes in `api-key` header (not Bearer token).
/// Request/response format matches OpenAI chat completions.
async fn call_azure(prompt: &str, config: &LlmConfig) -> Result<String, String> {
    let api_key = config.api_key.as_ref()
        .ok_or("azure requires api_key (Azure OpenAI key)")?;
    let model = if config.model.is_empty() { "gpt-4o" } else { &config.model };
    let api_version = config.azure_api_version.as_deref().unwrap_or("2024-02-01");

    // URL can be fully overridden via api_url, or constructed from azure_resource + model
    let url = if let Some(custom_url) = &config.api_url {
        custom_url.clone()
    } else {
        let resource = config.azure_resource.as_ref()
            .ok_or("azure requires either api_url or azure_resource")?;
        format!(
            "https://{}.openai.azure.com/openai/deployments/{}/chat/completions?api-version={}",
            resource, model, api_version
        )
    };

    let body = build_openai_body(prompt, config, "gpt-4o");

    let resp = http_client()
        .post(&url)
        .header("api-key", api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("azure request failed: {}", e))?;

    parse_openai_response(resp, "azure").await
}

/// Custom endpoint — sends a flexible request and tries multiple response formats.
/// Both `prompt` (string) and `messages` (array) are included so the endpoint can
/// use whichever format it expects.
/// Response parsing tries common formats: OpenAI, Anthropic, Ollama, and flat fields.
async fn call_custom(prompt: &str, config: &LlmConfig) -> Result<String, String> {
    let url = config.api_url.as_ref()
        .ok_or("custom provider requires api_url")?;

    let mut messages = Vec::new();
    if let Some(sys) = &config.system_prompt {
        messages.push(serde_json::json!({"role": "system", "content": sys}));
    }
    messages.push(serde_json::json!({"role": "user", "content": prompt}));

    let mut body = serde_json::json!({
        "prompt": prompt,
        "messages": messages,
        "max_tokens": config.max_tokens,
        "model": config.model,
    });

    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    let mut req = http_client()
        .post(url)
        .header("content-type", "application/json");

    if let Some(key) = &config.api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    let resp = req
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("custom request failed: {}", e))?;

    let status = resp.status();
    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("custom response parse failed: {}", e))?;

    if !status.is_success() {
        return Err(format!("custom HTTP {}: {}", status, json));
    }

    // Try common response formats in order of popularity
    // OpenAI-style: choices[0].message.content
    if let Some(text) = json["choices"][0]["message"]["content"].as_str() {
        return Ok(text.to_string());
    }
    // Anthropic-style: content[0].text
    if let Some(text) = json["content"][0]["text"].as_str() {
        return Ok(text.to_string());
    }
    // Ollama-style: message.content
    if let Some(text) = json["message"]["content"].as_str() {
        return Ok(text.to_string());
    }
    // Simple flat fields
    for field in &["text", "content", "response", "output", "result", "message", "generated_text"] {
        if let Some(text) = json.get(field).and_then(|v| v.as_str()) {
            return Ok(text.to_string());
        }
    }

    Err(format!("custom: cannot extract text from response: {}", json))
}

// ── Shared Helpers ───────────────────────────────────────────────────────────

/// Build an OpenAI-compatible chat completions request body.
/// Used by: openai, openai_compatible, groq, together, lmstudio, azure.
fn build_openai_body(prompt: &str, config: &LlmConfig, default_model: &str) -> serde_json::Value {
    let model = if config.model.is_empty() { default_model } else { &config.model };

    let mut messages = Vec::new();
    if let Some(sys) = &config.system_prompt {
        messages.push(serde_json::json!({"role": "system", "content": sys}));
    }
    messages.push(serde_json::json!({"role": "user", "content": prompt}));

    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": config.max_tokens,
        "messages": messages,
    });

    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    body
}

/// Parse an OpenAI-compatible chat completions response.
/// Expected format: { "choices": [{ "message": { "content": "..." } }] }
async fn parse_openai_response(resp: reqwest::Response, provider: &str) -> Result<String, String> {
    let status = resp.status();
    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("{} response parse failed: {}", provider, e))?;

    if !status.is_success() {
        return Err(format!("{} HTTP {}: {}", provider, status, json));
    }

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("{} unexpected response format: {}", provider, json))
}
