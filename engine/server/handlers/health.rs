/// Health check — returns system status.

use axum::extract::State;
use axum::Json;

pub async fn health(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let graph = state.graph.read().await;
    let summary = graph.summary();
    let backend = state.backend.read().await;
    let backend_alive = backend.health_check().await.unwrap_or(false);
    let config = state.amure_config.read().await;

    Json(serde_json::json!({
        "status": "ok",
        "project": config.project.name,
        "domain": config.project.domain,
        "graph": {
            "nodes": summary.n_nodes,
            "edges": summary.n_edges,
            "node_kinds": summary.node_kinds,
        },
        "backend": {
            "type": config.backend.backend_type,
            "alive": backend_alive,
        },
        "llm": {
            "provider": config.llm.default_provider,
        },
    }))
}
