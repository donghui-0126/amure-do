/// Backend proxy handlers — replaces julia.rs with pluggable backend support.

use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::server::routes::AppState;

#[derive(Deserialize)]
pub struct BackendExec {
    pub code: String,
    pub timeout_secs: Option<u64>,
}

pub async fn backend_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let backend = state.backend.read().await;
    let config = backend.config();
    let alive = backend.health_check().await.unwrap_or(false);
    Json(serde_json::json!({
        "alive": alive,
        "backend_type": config.backend_type,
        "url": config.url,
        "dir": config.dir,
        "command": config.command,
    }))
}

pub async fn backend_exec(
    State(state): State<AppState>,
    Json(req): Json<BackendExec>,
) -> Json<serde_json::Value> {
    let backend = state.backend.read().await;
    let timeout = req.timeout_secs.map(|s| std::time::Duration::from_secs(s));
    match backend.exec(&req.code, timeout).await {
        Ok((status, output)) => Json(serde_json::json!({"status": status, "output": output})),
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

pub async fn backend_start(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let backend = state.backend.read().await;
    match backend.start().await {
        Ok(msg) => Json(serde_json::json!({"status": msg})),
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}
