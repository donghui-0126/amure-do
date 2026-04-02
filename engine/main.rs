use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;

use amuredo::config::AmuConfig;
use amuredo::knowledge::db::KnowledgeDB;
use amuredo::server::backend::Backend;
use amuredo::server::routes::{build_router, AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Load configuration
    let amu_config = AmuConfig::load();

    println!("=== amuredo — Hypothesis Engine ===");
    println!("Project: {} ({})", amu_config.project.name, amu_config.project.domain);
    println!("Backend: {:?}", amu_config.backend.backend_type);
    println!("LLM: {}", amu_config.llm.default_provider);

    let t0 = Instant::now();

    let kb_dir = std::path::PathBuf::from("data/knowledge_db");

    // Load knowledge DB
    let kb = KnowledgeDB::open(&kb_dir).expect("Failed to open KnowledgeDB");
    let summary = kb.summary();
    println!(
        "Knowledge: {} hypotheses, {} experiments, {} insights ({} mature)",
        summary.n_hypotheses, summary.n_experiments, summary.n_insights, summary.n_mature,
    );

    // Load Lab conversations
    let chat_state = amuredo::server::handlers::chat::ChatState::load();
    let n_sessions = chat_state.sessions.len();
    let n_messages = chat_state.messages.len();
    println!("Lab: {} sessions, {} messages", n_sessions, n_messages);

    // Load canvas
    let canvas_state = amuredo::server::handlers::canvas::CanvasState::load();
    println!("Canvas: {} nodes", canvas_state.nodes.len());

    // Load user profile
    let profile = amuredo::server::handlers::adaptive::UserProfile::load();
    println!("Profile: adaptive={}, {} disagreements, {} hooks",
        profile.adaptive_mode, profile.disagreements.len(), profile.hooks.len());

    // Create backend from config
    let backend = Backend::new(amu_config.backend.clone());

    // Build LLM config from amuredo.toml (with fallback to saved llm_config.json)
    let llm_config = {
        let saved = amuredo::server::llm_provider::LlmConfig::load();
        if saved.provider != "claude_cli" || saved.api_key.is_some() {
            // Use saved config if it has been customized
            saved
        } else {
            // Use config from amuredo.toml
            amu_config.to_llm_config()
        }
    };

    println!("Ready in {:.2}s", t0.elapsed().as_secs_f64());

    // Activity log
    let activity = Arc::new(RwLock::new(
        amuredo::server::activity::ActivityState::new(),
    ));
    {
        let mut log = activity.write().await;
        log.push("system", "started", &format!(
            "Engine started: {} claims, {} knowledge, {} reasons",
            summary.n_claims, summary.n_knowledge, summary.n_reasons,
        ), None);
    }

    let state = AppState {
        knowledge: Arc::new(RwLock::new(kb)),
        chat: Arc::new(RwLock::new(chat_state)),
        canvas: Arc::new(RwLock::new(canvas_state)),
        profile: Arc::new(RwLock::new(profile)),
        llm_config: Arc::new(RwLock::new(llm_config)),
        call_log: Arc::new(RwLock::new(
            amuredo::server::handlers::monitor::CallLog::default(),
        )),
        activity,
        backend: Arc::new(RwLock::new(backend)),
        amu_config: Arc::new(RwLock::new(amu_config.clone())),
    };
    let app = build_router(state);

    let addr = format!("{}:{}", amu_config.server.host, amu_config.server.port);
    println!("\nServer listening on http://{}", addr);
    println!("  Dashboard: http://localhost:{}/", amu_config.server.port);
    println!("  Backend:   POST http://localhost:{}/api/backend/exec", amu_config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
