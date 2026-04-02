use std::sync::Arc;

use axum::routing::{get, post, patch, delete};
use axum::Router;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use crate::knowledge::db::KnowledgeDB;
use crate::server::backend::Backend;
use crate::config::AmureConfig;
use super::activity::ActivityLog;

use super::handlers::{query, knowledge, dashboard, external, chat, backend, canvas, monitor, evaluator, manager, judge, thesis_api, judge_v4, adaptive, strategy, framework_api};

#[derive(Clone)]
pub struct AppState {
    pub knowledge: Arc<RwLock<KnowledgeDB>>,
    pub chat: chat::ChatStore,
    pub canvas: Arc<RwLock<canvas::CanvasState>>,
    pub call_log: monitor::CallLogStore,
    pub profile: Arc<RwLock<adaptive::UserProfile>>,
    pub llm_config: Arc<RwLock<crate::server::llm_provider::LlmConfig>>,
    pub activity: ActivityLog,
    pub backend: Arc<RwLock<Backend>>,
    pub amure_config: Arc<RwLock<AmureConfig>>,
}

pub fn build_router(state: AppState) -> Router {
    let query_routes = Router::new()
        .route("/health", get(query::health));

    let knowledge_routes = Router::new()
        .route("/hypotheses", get(knowledge::list_hypotheses))
        .route("/hypotheses", post(knowledge::create_hypothesis))
        .route("/hypotheses/{id}", get(knowledge::get_hypothesis))
        .route("/insights", post(knowledge::create_insight))
        .route("/insights/pending", get(knowledge::list_pending_insights))
        .route("/insights/{id}/accept", post(knowledge::accept_insight))
        .route("/insights/{id}/reject", post(knowledge::reject_insight))
        .route("/insights/{id}/promote", patch(knowledge::promote_insight))
        .route("/insights/search", post(knowledge::search_insights));

    let dashboard_routes = Router::new()
        .route("/overview", get(dashboard::overview))
        .route("/tree", get(dashboard::knowledge_tree));

    let external_routes = Router::new()
        .route("/yahoo", post(external::fetch_yahoo))
        .route("/yahoo/save", post(external::fetch_and_save));

    let chat_routes = Router::new()
        .route("/sessions", get(chat::list_sessions))
        .route("/sessions", post(chat::create_session))
        .route("/sessions/{id}", get(chat::get_session))
        .route("/sessions/{id}", delete(chat::delete_session))
        .route("/send", post(chat::send_message))
        .route("/messages/{id}", get(chat::poll_message));

    let backend_routes = Router::new()
        .route("/status", get(backend::backend_status))
        .route("/exec", post(backend::backend_exec))
        .route("/start", post(backend::backend_start));

    let canvas_routes = Router::new()
        .route("/", get(canvas::get_canvas))
        .route("/nodes", post(canvas::create_node))
        .route("/nodes/{id}", patch(canvas::update_node))
        .route("/nodes/{id}", delete(canvas::delete_node))
        .route("/nodes/{id}/refs", post(canvas::add_reference))
        .route("/nodes/{id}/refs/{ref_id}", delete(canvas::remove_reference))
        .route("/nodes/{id}/to-lab", post(canvas::send_to_lab))
        .route("/nodes/{id}/run", post(canvas::run_node))
        .route("/nodes/recursive-design", post(canvas::recursive_design))
        .route("/from-idea", post(canvas::idea_to_canvas))
        .route("/import/markdown", post(canvas::import_markdown))
        .route("/import/file", post(canvas::import_file));

    let monitor_routes = Router::new()
        .route("/julia-log", get(monitor::julia_log))
        .route("/rust-log", get(monitor::rust_log))
        .route("/calls", get(monitor::call_log));

    Router::new()
        .nest("/api/query", query_routes)
        .nest("/api/knowledge", knowledge_routes)
        .nest("/api/dashboard", dashboard_routes)
        .nest("/api/external", external_routes)
        .nest("/api/chat", chat_routes)
        .nest("/api/backend", backend_routes)
        .nest("/api/canvas", canvas_routes)
        .nest("/api/adaptive", Router::new()
            .route("/profile", get(adaptive::get_profile))
            .route("/toggle", post(adaptive::toggle_adaptive))
            .route("/disagreement", post(adaptive::record_disagreement))
            .route("/tendency", post(adaptive::update_tendency))
            .route("/hook", post(adaptive::update_hook))
            .route("/hooks-context", get(adaptive::get_active_hooks_context))
        )
        .nest("/api/strategy", Router::new().route("/generate", get(strategy::generate_strategies)).route("/to-julia", post(strategy::strategy_to_julia)))
        .nest("/api/debug", Router::new()
            .route("/llm-log", get(chat::debug_log))
        )
        .nest("/api/monitor", monitor_routes)
        .nest("/api/evaluate", Router::new().route("/", post(evaluator::evaluate)))
        .nest("/api/thesis", Router::new()
            .route("/", post(thesis_api::create_thesis))
            .route("/experiment", post(thesis_api::design_experiment))
            .route("/argument", post(thesis_api::add_argument))
        )
        .nest("/api/framework", Router::new()
            .route("/claims", get(framework_api::list_claims).post(framework_api::create_claim))
            .route("/claims/{id}", get(framework_api::get_claim).delete(framework_api::delete_claim))
            .route("/claims/auto-complete", post(framework_api::auto_complete_claim))
            .route("/claims/apply-suggestion", post(framework_api::apply_suggestion))
            .route("/claims/{id}/run-all", post(framework_api::run_all_experiments))
            .route("/claims/{id}/accept", post(framework_api::accept_claim))
            .route("/claims/{id}/reject", post(framework_api::reject_claim))
            .route("/reasons", post(framework_api::create_reason))
            .route("/evidence", post(framework_api::add_evidence))
            .route("/relations", post(framework_api::add_relation))
            .route("/experiments", post(framework_api::create_experiment))
            .route("/experiments/reason/{reason_id}", get(framework_api::list_experiments))
            .route("/experiments/{id}/result", post(framework_api::submit_result))
            .route("/experiments/{id}/verdict", post(framework_api::submit_verdict))
            .route("/llm/routing", get(framework_api::get_llm_routing).post(framework_api::set_llm_role))
            .route("/llm/routing/{role}", delete(framework_api::delete_llm_role))
            .route("/activity", get(framework_api::get_activity))
            .route("/julia-log", get(framework_api::julia_log_tail))
        )
        .nest("/api/manager", Router::new().route("/run", post(manager::run_managed_experiment)))
        .nest("/api/judge", Router::new()
            .route("/hypotheses/{id}", post(judge::judge_hypothesis))
            .route("/v4", post(judge_v4::judge_v4))
        )
        .fallback(dashboard::serve_dashboard)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
