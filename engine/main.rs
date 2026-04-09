use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;

use amure_do::config::AmureConfig;
use amure_do::server::backend::Backend;
use amure_do::server::routes::{build_router, AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let amure_config = AmureConfig::load();

    println!("=== amure-do — Hypothesis Engine ===");
    println!("Project: {} ({})", amure_config.project.name, amure_config.project.domain);
    println!("Backend: {:?}", amure_config.backend.backend_type);
    println!("LLM: {}", amure_config.llm.default_provider);

    let t0 = Instant::now();

    // Load graph DB
    let graph_dir = std::path::PathBuf::from("data/amure_graph");
    let graph = if graph_dir.join("nodes.json").exists() {
        amure_db::graph::AmureGraph::load(&graph_dir).unwrap_or_default()
    } else {
        amure_db::graph::AmureGraph::new()
    };
    let graph_summary = graph.summary();
    println!("Graph: {} nodes, {} edges", graph_summary.n_nodes, graph_summary.n_edges);

    // Create backend
    let backend = Backend::new(amure_config.backend.clone());

    // Load LLM config (saved config takes precedence if customized)
    let llm_config = {
        let saved = amure_do::server::llm_provider::LlmConfig::load();
        if saved.provider != "claude_cli" || saved.api_key.is_some() {
            saved
        } else {
            amure_config.to_llm_config()
        }
    };

    // Load Lab sessions
    let lab_state = amure_do::server::handlers::lab::LabState::load();
    println!("Lab: {} sessions", lab_state.sessions.len());

    // Detect first-run
    let is_first_run = graph_summary.n_nodes == 0 && amure_config.project.name == "My Research";
    if is_first_run {
        println!("\n  First run detected — setup wizard will appear in dashboard");
    }

    println!("Ready in {:.2}s", t0.elapsed().as_secs_f64());

    let state = AppState {
        graph: Arc::new(RwLock::new(graph)),
        synonyms: Arc::new(amure_db::synonym::SynonymDict::new()),
        amure_config: Arc::new(RwLock::new(amure_config.clone())),
        llm_config: Arc::new(RwLock::new(llm_config)),
        backend: Arc::new(RwLock::new(backend)),
        lab: Arc::new(RwLock::new(lab_state)),
    };
    let app = build_router(state);

    let addr = format!("{}:{}", amure_config.server.host, amure_config.server.port);
    println!("\nListening on http://{}", addr);
    println!("  Dashboard: http://localhost:{}/", amure_config.server.port);
    println!("  Graph:     http://localhost:{}/graph", amure_config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
