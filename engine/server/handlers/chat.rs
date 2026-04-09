/// Lab handler — collaborative experiment development via Claude Code.
/// Sessions can be linked to hypothesis, experiment, or insight.
/// Conversations are persisted to disk.

use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::server::routes::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub session_id: Uuid,
    pub role: String,
    pub content: String,
    pub status: ChatStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ChatStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LabTarget {
    Claim,       // Claim 기반 세션
    Reason,      // 특정 Reason 탐구
    Experiment,  // 실험 설계/분석
    Free,        // 자유 대화
    // Legacy (backward compat for saved sessions)
    Hypothesis,
    Insight,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: Uuid,
    pub name: String,
    pub target_type: LabTarget,
    pub target_id: Option<Uuid>,
    pub messages: Vec<Uuid>,
    pub created_at: String,
}

pub type ChatStore = Arc<RwLock<ChatState>>;

#[derive(Default, Serialize, Deserialize)]
pub struct ChatState {
    pub sessions: HashMap<Uuid, ChatSession>,
    pub messages: HashMap<Uuid, ChatMessage>,
}

const LAB_FILE: &str = "data/knowledge_db/lab.json";

impl ChatState {
    pub fn load() -> Self {
        std::fs::read_to_string(LAB_FILE)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(LAB_FILE, json);
        }
    }
}

#[derive(Deserialize)]
pub struct CreateSession {
    pub name: String,
    pub target_type: Option<String>,  // "hypothesis", "experiment", "insight", "free"
    pub target_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct SendMessage {
    pub session_id: Uuid,
    pub content: String,
}

pub async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSession>,
) -> Json<serde_json::Value> {
    let target_type = match req.target_type.as_deref() {
        Some("claim") => LabTarget::Claim,
        Some("reason") => LabTarget::Reason,
        Some("experiment") => LabTarget::Experiment,
        // Legacy
        Some("hypothesis") => LabTarget::Hypothesis,
        Some("insight") => LabTarget::Insight,
        _ => LabTarget::Free,
    };
    let session = ChatSession {
        id: Uuid::new_v4(),
        name: req.name,
        target_type,
        target_id: req.target_id,
        messages: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
    };
    let id = session.id;
    let mut chat = state.chat.write().await;
    chat.sessions.insert(id, session);
    chat.save();
    Json(serde_json::json!({"id": id}))
}

pub async fn list_sessions(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let chat = state.chat.read().await;
    let mut sessions: Vec<&ChatSession> = chat.sessions.values().collect();
    sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Json(serde_json::json!({"sessions": sessions}))
}

pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let chat = state.chat.read().await;
    let session = match chat.sessions.get(&id) {
        Some(s) => s,
        None => return Json(serde_json::json!({"error": "Session not found"})),
    };
    let messages: Vec<&ChatMessage> = session.messages.iter()
        .filter_map(|mid| chat.messages.get(mid))
        .collect();
    Json(serde_json::json!({"session": session, "messages": messages}))
}

pub async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let mut chat = state.chat.write().await;
    if let Some(session) = chat.sessions.remove(&id) {
        for mid in &session.messages {
            chat.messages.remove(mid);
        }
        chat.save();
        Json(serde_json::json!({"status": "deleted", "id": id}))
    } else {
        Json(serde_json::json!({"error": "Session not found"}))
    }
}

pub async fn send_message(
    State(state): State<AppState>,
    Json(req): Json<SendMessage>,
) -> Json<serde_json::Value> {
    let user_msg = ChatMessage {
        id: Uuid::new_v4(),
        session_id: req.session_id,
        role: "user".into(),
        content: req.content.clone(),
        status: ChatStatus::Completed,
        created_at: Utc::now().to_rfc3339(),
    };
    let user_msg_id = user_msg.id;

    let asst_msg = ChatMessage {
        id: Uuid::new_v4(),
        session_id: req.session_id,
        role: "assistant".into(),
        content: String::new(),
        status: ChatStatus::Processing,
        created_at: Utc::now().to_rfc3339(),
    };
    let asst_msg_id = asst_msg.id;

    {
        let mut chat = state.chat.write().await;
        chat.messages.insert(user_msg_id, user_msg);
        chat.messages.insert(asst_msg_id, asst_msg);
        if let Some(session) = chat.sessions.get_mut(&req.session_id) {
            session.messages.push(user_msg_id);
            session.messages.push(asst_msg_id);
        }
        chat.save();
    }

    let context = build_context(&state, req.session_id).await;

    // Message Hook: parse intent + enrich context
    let intent = super::message_hook::parse_intent(&req.content);
    let hook_context = super::message_hook::build_hook_context(&intent);

    // Log the hook
    {
        let mut log = state.call_log.write().await;
        log.log("HOOK", &format!("intent:{}", intent.action),
            &intent.symbol.clone().unwrap_or_default(), "parsed");
    }

    let state_clone = state.clone();
    let content = req.content.clone();
    tokio::spawn(async move {
        let prompt = format!("{}\n{}\n\n---\nUser: {}", context, hook_context, content);

        // Debug log: 실제 프롬프트 저장
        debug_log_prompt(&prompt, &content);

        let result = run_llm(&prompt, &state_clone).await;

        let mut chat = state_clone.chat.write().await;
        if let Some(msg) = chat.messages.get_mut(&asst_msg_id) {
            match result {
                Ok(output) => {
                    msg.content = output;
                    msg.status = ChatStatus::Completed;
                }
                Err(e) => {
                    msg.content = format!("Error: {}", e);
                    msg.status = ChatStatus::Failed;
                }
            }
        }
        chat.save();
    });

    Json(serde_json::json!({
        "user_msg_id": user_msg_id,
        "assistant_msg_id": asst_msg_id,
        "status": "processing",
    }))
}

pub async fn poll_message(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let chat = state.chat.read().await;
    match chat.messages.get(&id) {
        Some(msg) => Json(serde_json::json!({
            "id": msg.id,
            "status": msg.status,
            "content": msg.content,
            "role": msg.role,
        })),
        None => Json(serde_json::json!({"error": "Message not found"})),
    }
}

async fn build_context(state: &AppState, session_id: Uuid) -> String {
    let chat = state.chat.read().await;
    let session = match chat.sessions.get(&session_id) {
        Some(s) => s,
        None => return String::new(),
    };

    let mut ctx = String::from(r#"# amure-do Lab Context

You are a research assistant in the amure-do hypothesis engine.
- Answer concisely in plain text. Use Korean if the user writes in Korean.
- Help the user formulate, test, and refine hypotheses.
- When the user has a compute backend configured, you can execute code via POST /api/backend/exec.
- Focus on structured reasoning: Claims, Reasons (Support/Rebut), Evidence, and Experiments.
- Always consider validity conditions: when/where does a hypothesis hold or fail?

"#);

    let kb = state.knowledge.read().await;

    match session.target_type {
        LabTarget::Claim => {
            if let Some(tid) = &session.target_id {
                if let Some(claim) = kb.get_claim(tid) {
                    ctx.push_str(&format!("## Claim: {}\nTrigger: {}\nStatus: {:?}\n\n", claim.statement, claim.trigger, claim.status));
                    // Include all reasons
                    let reasons = kb.reasons_for_claim(tid);
                    for r in &reasons {
                        let rtype = match r.reason_type {
                            crate::knowledge::framework::ReasonType::Support => "Support",
                            crate::knowledge::framework::ReasonType::Rebut => "Rebut",
                        };
                        ctx.push_str(&format!("[{}] {}\n  Bridge: {}\n", rtype, r.statement, r.bridge));
                        for ev in &r.evidences {
                            ctx.push_str(&format!("  Evidence [{:?}]: {}\n", ev.tag, ev.description));
                        }
                        // Include experiments for this reason
                        let exps = kb.experiments_for_reason(&r.id);
                        for exp in &exps {
                            ctx.push_str(&format!("  Experiment: {} [{:?}]\n", exp.description, exp.status));
                            if let Some(v) = &exp.verdict {
                                ctx.push_str(&format!("    Verdict: {} — {}\n", if v.supports_reason {"지지"} else {"약화"}, v.explanation));
                                if !v.gaps.is_empty() {
                                    ctx.push_str(&format!("    Gaps: {}\n", v.gaps.join(", ")));
                                }
                            }
                        }
                    }
                    ctx.push('\n');
                }
            }
        }
        LabTarget::Reason => {
            if let Some(tid) = &session.target_id {
                if let Some(reason) = kb.get_reason(tid) {
                    let rtype = match reason.reason_type {
                        crate::knowledge::framework::ReasonType::Support => "Support",
                        crate::knowledge::framework::ReasonType::Rebut => "Rebut",
                    };
                    ctx.push_str(&format!("## Reason [{}]: {}\nBridge: {}\n\n", rtype, reason.statement, reason.bridge));
                    // Parent claim
                    if let Some(claim) = kb.get_claim(&reason.claim_id) {
                        ctx.push_str(&format!("Parent Claim: {}\n\n", claim.statement));
                    }
                    // Experiments
                    let exps = kb.experiments_for_reason(tid);
                    for exp in &exps {
                        ctx.push_str(&format!("Experiment: {} [{:?}]\n", exp.description, exp.status));
                        if let Some(v) = &exp.verdict {
                            ctx.push_str(&format!("  Verdict: {} — {}\n", if v.supports_reason {"지지"} else {"약화"}, v.explanation));
                        }
                    }
                    ctx.push('\n');
                }
            }
        }
        LabTarget::Experiment => {
            if let Some(tid) = &session.target_id {
                if let Some(exp) = kb.get_fw_experiment(tid) {
                    ctx.push_str(&format!("## Experiment: {}\nif_true: {}\nif_false: {}\nStatus: {:?}\n", exp.description, exp.if_true, exp.if_false, exp.status));
                    if let Some(result) = &exp.result {
                        ctx.push_str(&format!("Result: {}\n", result));
                    }
                    if let Some(v) = &exp.verdict {
                        ctx.push_str(&format!("Verdict: {} — {}\n", if v.supports_reason {"지지"} else {"약화"}, v.explanation));
                        if !v.gaps.is_empty() {
                            ctx.push_str(&format!("Gaps: {}\n", v.gaps.join(", ")));
                        }
                    }
                    // Parent reason + claim
                    if let Some(reason) = kb.get_reason(&exp.reason_id) {
                        ctx.push_str(&format!("\nParent Reason: {}\nBridge: {}\n", reason.statement, reason.bridge));
                        if let Some(claim) = kb.get_claim(&reason.claim_id) {
                            ctx.push_str(&format!("Parent Claim: {}\n", claim.statement));
                        }
                    }
                    ctx.push('\n');
                }
                // Fallback: try old experiment type
                else if let Some(e) = kb.get_experiment(tid) {
                    ctx.push_str(&format!("## Legacy Experiment: {}\n", e.description));
                    if let Some(r) = &e.results {
                        ctx.push_str(&format!("Results: N={}, mean={:.2}bp, net={:.0}bp\n\n", r.n_trades, r.mean_ret_bp, r.net_cum_bp));
                    }
                }
            }
        }
        // Legacy targets
        LabTarget::Hypothesis => {
            if let Some(tid) = &session.target_id {
                if let Some(h) = kb.get_hypothesis(tid) {
                    ctx.push_str(&format!("## Hypothesis: {}\nRationale: {}\n\n", h.title, h.economic_rationale));
                }
            }
        }
        LabTarget::Insight => {
            if let Some(tid) = &session.target_id {
                if let Some(ins) = kb.get_insight(tid) {
                    ctx.push_str(&format!("## Insight: {}\nEvidence: {}\n\n", ins.text, ins.evidence));
                }
            }
        }
        LabTarget::Free => {}
    }

    // Recent conversation history
    let recent: Vec<&ChatMessage> = session.messages.iter()
        .rev().take(20).rev()
        .filter_map(|mid| chat.messages.get(mid))
        .filter(|m| m.status == ChatStatus::Completed)
        .collect();

    if !recent.is_empty() {
        ctx.push_str("## Conversation so far:\n");
        for msg in recent {
            ctx.push_str(&format!("{}: {}\n\n", msg.role, msg.content));
        }
    }

    ctx
}

async fn run_llm(prompt: &str, state: &AppState) -> Result<String, String> {
    let config = state.llm_config.read().await;
    crate::server::llm_provider::call_llm(prompt, &config).await
}

const DEBUG_LOG_FILE: &str = "data/knowledge_db/debug_llm.log";

fn debug_log_prompt(prompt: &str, user_msg: &str) {
    use std::io::Write;
    let timestamp = Utc::now().to_rfc3339();
    let separator = "═".repeat(80);
    let entry = format!(
        "\n{separator}\n[{timestamp}] User: {user_msg}\n{separator}\n\
         --- FULL PROMPT ({len} chars) ---\n{prompt}\n--- END PROMPT ---\n",
        separator = separator,
        timestamp = timestamp,
        user_msg = &user_msg[..user_msg.len().min(100)],
        len = prompt.len(),
        prompt = prompt,
    );

    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true).open(DEBUG_LOG_FILE)
    {
        let _ = f.write_all(entry.as_bytes());
    }
    tracing::debug!("LLM prompt: {} chars, user: {}", prompt.len(), &user_msg[..user_msg.len().min(50)]);
}

/// Debug endpoint: 최근 LLM 호출 로그 조회
pub async fn debug_log() -> Json<serde_json::Value> {
    let log = std::fs::read_to_string(DEBUG_LOG_FILE).unwrap_or_default();
    // 마지막 5000자만
    let tail = if log.len() > 5000 { &log[log.len()-5000..] } else { &log };
    Json(serde_json::json!({"log": tail, "total_bytes": log.len()}))
}
