/// Setup wizard API — first-run configuration.

use axum::extract::State;
use axum::Json;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SetupInitRequest {
    pub project_name: Option<String>,
    pub domain: Option<String>,
    pub description: Option<String>,
    pub backend_type: Option<String>,
    pub backend_url: Option<String>,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub llm_api_key: Option<String>,
    pub gates: Option<Vec<String>>,
}

pub async fn setup_status(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let config = state.amure_config.read().await;
    let graph = state.graph.read().await;
    let needs_setup = graph.node_count() == 0 && config.project.name == "My Research";
    Json(serde_json::json!({
        "needs_setup": needs_setup,
        "current_config": {
            "project_name": config.project.name,
            "domain": config.project.domain,
            "backend_type": config.backend.backend_type,
            "llm_provider": config.llm.default_provider,
        }
    }))
}

pub async fn setup_init(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<SetupInitRequest>,
) -> Json<serde_json::Value> {
    let mut config = state.amure_config.write().await;

    if let Some(name) = req.project_name {
        config.project.name = name;
    }
    if let Some(domain) = req.domain {
        config.project.domain = domain;
    }
    if let Some(description) = req.description {
        config.project.description = description;
    }
    if let Some(backend_type) = req.backend_type {
        config.backend.backend_type = match backend_type.to_lowercase().as_str() {
            "http" => crate::server::backend::BackendType::Http,
            "file" => crate::server::backend::BackendType::File,
            "subprocess" => crate::server::backend::BackendType::Subprocess,
            _ => crate::server::backend::BackendType::None,
        };
    }
    if let Some(url) = req.backend_url {
        config.backend.url = if url.is_empty() { None } else { Some(url) };
    }
    if let Some(provider) = req.llm_provider {
        config.llm.default_provider = provider;
    }
    if let Some(model) = req.llm_model {
        config.llm.default_model = if model.is_empty() { None } else { Some(model) };
    }
    if let Some(api_key) = req.llm_api_key {
        config.llm.default_api_key = if api_key.is_empty() { None } else { Some(api_key) };
    }
    if let Some(gates) = req.gates {
        config.gates.enabled = gates;
    }

    if let Err(e) = config.save() {
        return Json(serde_json::json!({
            "status": "error",
            "error": format!("Failed to save config: {}", e),
        }));
    }

    let project_name = config.project.name.clone();
    Json(serde_json::json!({
        "status": "configured",
        "project_name": project_name,
    }))
}
