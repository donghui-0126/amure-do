/// Lab handler — LLM-assisted research conversations, optionally linked to a graph claim.

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use amure_db::edge::EdgeKind;
use amure_db::graph::AmureGraph;
use amure_db::node::NodeKind;

// ── Data Structures ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabState {
    pub sessions: Vec<LabSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabSession {
    pub id: String,
    pub title: String,
    pub claim_id: Option<String>,
    pub messages: Vec<LabMessage>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

// ── Persistence ──────────────────────────────────────────────────────────────

const LAB_DATA_DIR: &str = "data/lab";
const LAB_SESSIONS_FILE: &str = "data/lab/sessions.json";

impl LabState {
    pub fn load() -> Self {
        if let Ok(content) = std::fs::read_to_string(LAB_SESSIONS_FILE) {
            if let Ok(state) = serde_json::from_str::<LabState>(&content) {
                return state;
            }
        }
        Self { sessions: Vec::new() }
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(LAB_DATA_DIR);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(LAB_SESSIONS_FILE, json);
        }
    }
}

// ── Request/Response types ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateSessionReq {
    pub title: String,
    pub claim_id: Option<String>,
}

#[derive(Deserialize)]
pub struct SendMessageReq {
    pub session_id: String,
    pub content: String,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Build claim context by walking 1 hop from a claim node to collect support/rebut reasons.
async fn build_claim_context(graph: &AmureGraph, claim_id: &str) -> String {
    let id = match claim_id.parse::<Uuid>() {
        Ok(u) => u,
        Err(_) => return String::new(),
    };

    let claim = match graph.get_node(&id) {
        Some(n) if n.kind == NodeKind::Claim => n,
        _ => return String::new(),
    };

    let mut support_stmts: Vec<String> = Vec::new();
    let mut rebut_stmts: Vec<String> = Vec::new();

    for edge in graph.edges.values() {
        let neighbor_id = if edge.source == id {
            edge.target
        } else if edge.target == id {
            edge.source
        } else {
            continue;
        };

        let neighbor = match graph.get_node(&neighbor_id) {
            Some(n) if n.kind == NodeKind::Reason => n,
            _ => continue,
        };

        match edge.kind {
            EdgeKind::Support => support_stmts.push(neighbor.statement.clone()),
            EdgeKind::Rebut => rebut_stmts.push(neighbor.statement.clone()),
            _ => {}
        }
    }

    let mut ctx = format!(
        "Current claim: {}\nKeywords: {}",
        claim.statement,
        claim.keywords.join(", ")
    );

    if !support_stmts.is_empty() {
        ctx.push_str(&format!("\nSupport:\n- {}", support_stmts.join("\n- ")));
    }
    if !rebut_stmts.is_empty() {
        ctx.push_str(&format!("\nRebut:\n- {}", rebut_stmts.join("\n- ")));
    }

    ctx
}

/// Build the full prompt for the LLM from system instructions, optional claim context,
/// conversation history (last 20 messages), and the new user message.
fn build_prompt(session: &LabSession, claim_ctx: &str, user_content: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    parts.push(
        "You are a research assistant in amure-do. \
         Help the user formulate, test, and refine hypotheses. \
         Answer in the user's language."
            .to_string(),
    );

    if !claim_ctx.is_empty() {
        parts.push(format!("\n---\n{}", claim_ctx));
    }

    parts.push("\n---\nConversation:".to_string());

    let history: Vec<&LabMessage> = session
        .messages
        .iter()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    for msg in history {
        let role_label = if msg.role == "user" { "User" } else { "Assistant" };
        parts.push(format!("{}: {}", role_label, msg.content));
    }

    parts.push(format!("\nUser: {}", user_content));
    parts.push("Assistant:".to_string());

    parts.join("\n")
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// POST /api/lab/sessions — create a new session.
pub async fn create_session(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<CreateSessionReq>,
) -> Json<serde_json::Value> {
    let id = Uuid::new_v4().to_string();
    let now = now_iso();

    let session = LabSession {
        id: id.clone(),
        title: req.title.clone(),
        claim_id: req.claim_id,
        messages: Vec::new(),
        created_at: now,
    };

    let mut lab = state.lab.write().await;
    lab.sessions.push(session);
    lab.save();

    Json(serde_json::json!({ "id": id, "title": req.title }))
}

/// GET /api/lab/sessions — list all sessions sorted by created_at desc.
pub async fn list_sessions(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let lab = state.lab.read().await;

    let mut sessions: Vec<serde_json::Value> = lab
        .sessions
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "title": s.title,
                "claim_id": s.claim_id,
                "n_messages": s.messages.len(),
                "created_at": s.created_at,
            })
        })
        .collect();

    sessions.sort_by(|a, b| {
        b["created_at"]
            .as_str()
            .cmp(&a["created_at"].as_str())
    });

    Json(serde_json::json!({ "sessions": sessions }))
}

/// GET /api/lab/sessions/{id} — get session detail with all messages.
pub async fn get_session(
    State(state): State<crate::server::routes::AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let lab = state.lab.read().await;

    match lab.sessions.iter().find(|s| s.id == id) {
        Some(session) => Json(serde_json::json!({
            "session": {
                "id": session.id,
                "title": session.title,
                "claim_id": session.claim_id,
                "created_at": session.created_at,
            },
            "messages": session.messages,
        })),
        None => Json(serde_json::json!({ "error": "session not found" })),
    }
}

/// DELETE /api/lab/sessions/{id} — delete session.
pub async fn delete_session(
    State(state): State<crate::server::routes::AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let mut lab = state.lab.write().await;
    let before = lab.sessions.len();
    lab.sessions.retain(|s| s.id != id);

    if lab.sessions.len() < before {
        lab.save();
        Json(serde_json::json!({ "status": "deleted" }))
    } else {
        Json(serde_json::json!({ "error": "session not found" }))
    }
}

/// POST /api/lab/send — send a user message and receive an LLM response.
pub async fn send_message(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<SendMessageReq>,
) -> Json<serde_json::Value> {
    // --- 1. Find session and add user message ---
    let user_msg_id = Uuid::new_v4().to_string();
    let user_msg = LabMessage {
        id: user_msg_id.clone(),
        role: "user".to_string(),
        content: req.content.clone(),
        timestamp: now_iso(),
    };

    let (claim_id, prompt) = {
        let mut lab = state.lab.write().await;

        let session = match lab.sessions.iter_mut().find(|s| s.id == req.session_id) {
            Some(s) => s,
            None => {
                return Json(serde_json::json!({ "error": "session not found" }));
            }
        };

        session.messages.push(user_msg);

        let claim_id = session.claim_id.clone();

        // Build claim context while we still hold the lab lock but before graph read.
        // We snapshot the session data needed for prompt building.
        let session_snapshot = session.clone();

        (claim_id, session_snapshot)
    };

    // --- 2. Build claim context (graph read, outside lab lock) ---
    let claim_ctx = if let Some(ref cid) = claim_id {
        let graph = state.graph.read().await;
        build_claim_context(&graph, cid).await
    } else {
        String::new()
    };

    let full_prompt = build_prompt(&prompt, &claim_ctx, &req.content);

    // --- 3. Call LLM ---
    let llm_cfg = state.llm_config.read().await.clone();
    let llm_result = crate::server::llm_provider::call_llm(&full_prompt, &llm_cfg).await;

    let assistant_content = match llm_result {
        Ok(text) => text.trim().to_string(),
        Err(e) => format!("[LLM error: {}]", e),
    };

    // --- 4. Add assistant message and save ---
    let assistant_msg_id = Uuid::new_v4().to_string();
    let assistant_msg = LabMessage {
        id: assistant_msg_id.clone(),
        role: "assistant".to_string(),
        content: assistant_content.clone(),
        timestamp: now_iso(),
    };

    {
        let mut lab = state.lab.write().await;
        if let Some(session) = lab.sessions.iter_mut().find(|s| s.id == req.session_id) {
            session.messages.push(assistant_msg);
        }
        lab.save();
    }

    Json(serde_json::json!({
        "user_msg_id": user_msg_id,
        "assistant_msg_id": assistant_msg_id,
        "content": assistant_content,
    }))
}
