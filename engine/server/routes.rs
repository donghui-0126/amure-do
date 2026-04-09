use std::sync::Arc;

use axum::routing::{get, post, patch, delete};
use axum::Router;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use crate::config::AmureConfig;
use crate::server::backend::Backend;

use super::handlers::{health, graph, claims, lab, setup, dashboard, backend};

#[derive(Clone)]
pub struct AppState {
    pub graph: Arc<RwLock<amure_db::graph::AmureGraph>>,
    pub synonyms: Arc<amure_db::synonym::SynonymDict>,
    pub amure_config: Arc<RwLock<AmureConfig>>,
    pub llm_config: Arc<RwLock<crate::server::llm_provider::LlmConfig>>,
    pub backend: Arc<RwLock<Backend>>,
    pub lab: Arc<RwLock<lab::LabState>>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        // Health
        .route("/api/health", get(health::health))
        // Setup wizard
        .nest("/api/setup", Router::new()
            .route("/status", get(setup::setup_status))
            .route("/init", post(setup::setup_init))
        )
        // Claims lifecycle
        .nest("/api/claims", Router::new()
            .route("/", get(claims::list_claims).post(claims::create_claim))
            .route("/auto-generate", post(claims::auto_generate))
            .route("/{id}", get(claims::get_claim).delete(claims::delete_claim))
            .route("/{id}/reason", post(claims::add_reason))
            .route("/{id}/verdict", post(claims::verdict))
        )
        .route("/api/reasons/{id}/evidence", post(claims::add_evidence))
        .route("/api/reasons/{id}/experiment", post(claims::add_experiment))
        .route("/api/experiments/{id}/result", post(claims::submit_experiment_result))
        // Lab / Chat
        .nest("/api/lab", Router::new()
            .route("/sessions", get(lab::list_sessions).post(lab::create_session))
            .route("/sessions/{id}", get(lab::get_session).delete(lab::delete_session))
            .route("/send", post(lab::send_message))
        )
        // Backend proxy
        .nest("/api/backend", Router::new()
            .route("/status", get(backend::backend_status))
            .route("/exec", post(backend::backend_exec))
            .route("/start", post(backend::backend_start))
        )
        // Graph DB (amure-db)
        .nest("/api/graph", Router::new()
            .route("/all", get(graph::graph_all))
            .route("/summary", get(graph::graph_summary))
            .route("/search", get(graph::graph_search))
            .route("/node/{id}", get(graph::graph_node).patch(graph::update_node).delete(graph::delete_node))
            .route("/node", post(graph::create_node))
            .route("/edge", post(graph::create_edge))
            .route("/edge/{id}", delete(graph::delete_edge))
            .route("/walk/{id}", get(graph::graph_walk))
            .route("/subgraph/{id}", get(graph::graph_subgraph))
            .route("/claim", post(graph::create_claim))
            .route("/save", post(graph::save_graph))
        )
        // Knowledge utilization
        .nest("/api/knowledge-util", Router::new()
            .route("/check-failures", post(graph::check_failures))
            .route("/check-revalidation", get(graph::check_revalidation))
            .route("/detect-contradictions", post(graph::detect_contradictions))
            .route("/auto-gap-claims", post(graph::auto_gap_claims))
            .route("/suggest-combinations", get(graph::suggest_combinations))
        )
        // Yahoo Finance
        .nest("/api/yahoo", Router::new()
            .route("/fetch", post(graph::yahoo_fetch))
            .route("/batch", post(graph::yahoo_batch))
            .route("/auto-organize", post(graph::auto_organize))
        )
        // LLM operations
        .nest("/api/llm", Router::new()
            .route("/auto-tag", post(graph::llm_auto_tag))
            .route("/auto-tag-all", post(graph::llm_auto_tag_all))
            .route("/summarize", post(graph::llm_summarize_search))
            .route("/explain-groups", post(graph::llm_explain_groups))
            .route("/verify-claim", post(graph::llm_verify_claim))
        )
        // Dashboards
        .route("/graph", get(dashboard::serve_graph))
        .fallback(dashboard::serve_dashboard)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
