/// Canvas — tree-based idea notepad with knowledge references.

use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::server::routes::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub references: Vec<KnowledgeRef>,  // linked knowledge items
    pub status: NodeStatus,
    pub children: Vec<Uuid>,
    pub order: usize,                   // display order among siblings
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeRef {
    pub target_id: Uuid,
    pub target_type: String,  // "hypothesis", "experiment", "insight"
    pub label: String,        // display label
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum NodeStatus {
    Idea,       // just an idea
    Planned,    // planned for execution
    Running,    // assigned to Lab
    Done,       // completed
    Archived,   // no longer relevant
}

#[derive(Default, Serialize, Deserialize)]
pub struct CanvasState {
    pub nodes: HashMap<Uuid, CanvasNode>,
    pub root_ids: Vec<Uuid>,  // top-level nodes
}

const CANVAS_FILE: &str = "data/knowledge_db/canvas.json";

impl CanvasState {
    pub fn load() -> Self {
        std::fs::read_to_string(CANVAS_FILE)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(CANVAS_FILE, json);
        }
    }
}

// ── API Handlers ────────────────────────────────────────────────────────────

pub async fn get_canvas(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let canvas = state.canvas.read().await;
    // Build tree structure for frontend
    let tree = build_tree(&canvas);
    Json(serde_json::json!({"tree": tree, "total_nodes": canvas.nodes.len()}))
}

#[derive(Deserialize)]
pub struct CreateNode {
    pub title: String,
    pub description: Option<String>,
    pub parent_id: Option<Uuid>,
}

pub async fn create_node(
    State(state): State<AppState>,
    Json(req): Json<CreateNode>,
) -> Json<serde_json::Value> {
    let mut canvas = state.canvas.write().await;
    let now = Utc::now().to_rfc3339();
    let node = CanvasNode {
        id: Uuid::new_v4(),
        parent_id: req.parent_id,
        title: req.title,
        description: req.description.unwrap_or_default(),
        references: Vec::new(),
        status: NodeStatus::Idea,
        children: Vec::new(),
        order: 0,
        created_at: now.clone(),
        updated_at: now,
    };
    let id = node.id;

    if let Some(pid) = req.parent_id {
        if let Some(parent) = canvas.nodes.get_mut(&pid) {
            let order = parent.children.len();
            parent.children.push(id);
            canvas.nodes.insert(id, CanvasNode { order, ..node });
        } else {
            return Json(serde_json::json!({"error": "Parent not found"}));
        }
    } else {
        let order = canvas.root_ids.len();
        canvas.root_ids.push(id);
        canvas.nodes.insert(id, CanvasNode { order, ..node });
    }

    canvas.save();
    Json(serde_json::json!({"id": id, "status": "created"}))
}

#[derive(Deserialize)]
pub struct UpdateNode {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<NodeStatus>,
}

pub async fn update_node(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateNode>,
) -> Json<serde_json::Value> {
    let mut canvas = state.canvas.write().await;
    if let Some(node) = canvas.nodes.get_mut(&id) {
        if let Some(t) = req.title { node.title = t; }
        if let Some(d) = req.description { node.description = d; }
        if let Some(s) = req.status { node.status = s; }
        node.updated_at = Utc::now().to_rfc3339();
        canvas.save();
        Json(serde_json::json!({"status": "updated"}))
    } else {
        Json(serde_json::json!({"error": "Node not found"}))
    }
}

pub async fn delete_node(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let mut canvas = state.canvas.write().await;
    // Remove from parent's children or root_ids
    if let Some(node) = canvas.nodes.get(&id) {
        if let Some(pid) = node.parent_id {
            if let Some(parent) = canvas.nodes.get_mut(&pid) {
                parent.children.retain(|c| *c != id);
            }
        } else {
            canvas.root_ids.retain(|r| *r != id);
        }
    }
    // Recursively remove children
    remove_recursive(&mut canvas, id);
    canvas.save();
    Json(serde_json::json!({"status": "deleted"}))
}

#[derive(Deserialize)]
pub struct AddReference {
    pub target_id: Uuid,
    pub target_type: String,
    pub label: String,
}

pub async fn add_reference(
    State(state): State<AppState>,
    Path(node_id): Path<Uuid>,
    Json(req): Json<AddReference>,
) -> Json<serde_json::Value> {
    let mut canvas = state.canvas.write().await;
    if let Some(node) = canvas.nodes.get_mut(&node_id) {
        node.references.push(KnowledgeRef {
            target_id: req.target_id,
            target_type: req.target_type,
            label: req.label,
        });
        node.updated_at = Utc::now().to_rfc3339();
        canvas.save();
        Json(serde_json::json!({"status": "reference added"}))
    } else {
        Json(serde_json::json!({"error": "Node not found"}))
    }
}

pub async fn remove_reference(
    State(state): State<AppState>,
    Path((node_id, ref_target_id)): Path<(Uuid, Uuid)>,
) -> Json<serde_json::Value> {
    let mut canvas = state.canvas.write().await;
    if let Some(node) = canvas.nodes.get_mut(&node_id) {
        node.references.retain(|r| r.target_id != ref_target_id);
        canvas.save();
        Json(serde_json::json!({"status": "reference removed"}))
    } else {
        Json(serde_json::json!({"error": "Node not found"}))
    }
}

/// Send a canvas node to Lab — creates a session with all referenced knowledge as context.
pub async fn send_to_lab(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let (node_title, context_desc) = {
        let mut canvas = state.canvas.write().await;
        let node = match canvas.nodes.get_mut(&id) {
            Some(n) => n,
            None => return Json(serde_json::json!({"error": "Node not found"})),
        };

        node.status = NodeStatus::Running;
        node.updated_at = Utc::now().to_rfc3339();

        let title = node.title.clone();
        let desc = node.description.clone();
        let refs = node.references.clone();
        canvas.save();

        // Build context outside the mutable borrow
        let mut context_desc = format!("Canvas idea: {}\n{}\n\n", title, desc);
        if !refs.is_empty() {
            context_desc.push_str("Referenced knowledge:\n");
            let kb = state.knowledge.read().await;
            for r in &refs {
                match r.target_type.as_str() {
                    "insight" => {
                        if let Some(ins) = kb.get_insight(&r.target_id) {
                            context_desc.push_str(&format!("- [Insight] {}\n  Evidence: {}\n", ins.text, ins.evidence));
                        }
                    }
                    "experiment" => {
                        if let Some(exp) = kb.get_experiment(&r.target_id) {
                            context_desc.push_str(&format!("- [Experiment] {}\n", exp.description));
                            if let Some(res) = &exp.results {
                                context_desc.push_str(&format!("  N={}, mean={:.2}bp, net={:.0}bp\n", res.n_trades, res.mean_ret_bp, res.net_cum_bp));
                            }
                        }
                    }
                    "hypothesis" => {
                        if let Some(h) = kb.get_hypothesis(&r.target_id) {
                            context_desc.push_str(&format!("- [Hypothesis] {}: {}\n", h.title, h.economic_rationale));
                        }
                    }
                    _ => {}
                }
            }
        }
        (title, context_desc)
    };

    // Create Lab session
    let session = super::chat::ChatSession {
        id: Uuid::new_v4(),
        name: format!("Canvas: {}", node_title),
        target_type: super::chat::LabTarget::Free,
        target_id: Some(id),
        messages: Vec::new(),
        created_at: Utc::now().to_rfc3339(),
    };
    let session_id = session.id;

    let mut chat = state.chat.write().await;
    chat.sessions.insert(session_id, session);

    // Add initial system message with context
    let sys_msg = super::chat::ChatMessage {
        id: Uuid::new_v4(),
        session_id,
        role: "system".into(),
        content: context_desc,
        status: super::chat::ChatStatus::Completed,
        created_at: Utc::now().to_rfc3339(),
    };
    let msg_id = sys_msg.id;
    chat.messages.insert(msg_id, sys_msg);
    if let Some(s) = chat.sessions.get_mut(&session_id) {
        s.messages.push(msg_id);
    }
    chat.save();

    Json(serde_json::json!({
        "status": "sent to lab",
        "session_id": session_id,
        "node_id": id,
    }))
}

/// Recursively design sub-experiments under a node.
/// Takes experiment results + open questions → auto-creates child nodes.
#[derive(Deserialize)]
pub struct RecursiveDesign {
    pub node_id: Uuid,
    pub result_summary: String,
    pub open_questions: Vec<String>,
}

pub async fn recursive_design(
    State(state): State<AppState>,
    Json(req): Json<RecursiveDesign>,
) -> Json<serde_json::Value> {
    let mut canvas = state.canvas.write().await;
    let now = chrono::Utc::now().to_rfc3339();

    let parent = match canvas.nodes.get(&req.node_id) {
        Some(n) => n.title.clone(),
        None => return Json(serde_json::json!({"error": "Node not found"})),
    };

    let mut created = Vec::new();

    for (i, q) in req.open_questions.iter().enumerate() {
        let node = CanvasNode {
            id: Uuid::new_v4(),
            parent_id: Some(req.node_id),
            title: format!("Sub-EXP: {}", q),
            description: format!("Parent result: {}\nOpen question: {}", req.result_summary, q),
            references: Vec::new(),
            status: NodeStatus::Planned,
            children: Vec::new(),
            order: i,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let nid = node.id;
        canvas.nodes.insert(nid, node);
        created.push(nid);
    }

    // Link to parent
    if let Some(parent_node) = canvas.nodes.get_mut(&req.node_id) {
        parent_node.children.extend(&created);
    }

    canvas.save();

    Json(serde_json::json!({
        "status": "designed",
        "parent": parent,
        "sub_experiments": created.len(),
        "node_ids": created,
    }))
}

/// Run a canvas node (+ all children recursively).
/// Each node is sent to Claude for execution, results become insights.
pub async fn run_node(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    // Collect node + all descendants
    let nodes_to_run: Vec<(Uuid, String, String)> = {
        let canvas = state.canvas.read().await;
        let mut collected = Vec::new();
        collect_descendants(&canvas, &id, &mut collected);
        collected
    };

    if nodes_to_run.is_empty() {
        return Json(serde_json::json!({"error": "Node not found"}));
    }

    // Mark all as Running
    {
        let mut canvas = state.canvas.write().await;
        for (nid, _, _) in &nodes_to_run {
            if let Some(node) = canvas.nodes.get_mut(nid) {
                node.status = NodeStatus::Running;
                node.updated_at = Utc::now().to_rfc3339();
            }
        }
        canvas.save();
    }

    let n_total = nodes_to_run.len();
    let state_clone = state.clone();

    // Spawn execution in background
    tokio::spawn(async move {
        for (nid, title, description) in &nodes_to_run {
            if description.is_empty() { continue; }

            let prompt = format!(
                "You are a quant research assistant. Execute this experiment concisely.\n\
                 Experiment: {}\nDescription: {}\n\n\
                 Instructions:\n\
                 1. If this needs data, use Yahoo Finance or the available crypto data\n\
                 2. Show key numerical results (returns, win rate, trade count)\n\
                 3. State one economic insight from the results\n\
                 4. Keep response under 300 words, plain text, no markdown headers\n\
                 5. Use Korean if the description is in Korean",
                title, description
            );

            let result = run_claude_for_run(&prompt).await;

            match result {
                Ok(output) => {
                    // Create insight from result
                    let insight = crate::knowledge::types::Insight::new(
                        Uuid::nil(),
                        format!("[Auto] {}: {}", title, first_line(&output)),
                        output.clone(),
                        vec!["auto-run".to_string(), "canvas".to_string()],
                    );
                    let mut kb = state_clone.knowledge.write().await;
                    kb.add_insight(insight);
                    let _ = kb.save();

                    // Mark Done
                    {
                        let mut canvas = state_clone.canvas.write().await;
                        if let Some(node) = canvas.nodes.get_mut(nid) {
                            node.status = NodeStatus::Done;
                            node.updated_at = Utc::now().to_rfc3339();
                        }
                        canvas.save();
                    }

                    // Auto drill-down: check reject reasons → create sub-experiments
                    auto_drill_down(&state_clone, *nid, &output).await;
                }
                Err(_) => {
                    let mut canvas = state_clone.canvas.write().await;
                    if let Some(node) = canvas.nodes.get_mut(nid) {
                        node.status = NodeStatus::Idea; // revert
                        node.updated_at = Utc::now().to_rfc3339();
                    }
                    canvas.save();
                }
            }
        }
    });

    Json(serde_json::json!({
        "status": "running",
        "nodes_queued": n_total,
    }))
}

/// After run completes, check if reject reasons suggest more experiments needed.
/// If so, auto-create sub-experiments from reject reasons.
async fn auto_drill_down(state: &AppState, node_id: Uuid, result_text: &str) {
    // Generate reject reasons from the result
    let reject_reasons = generate_reject_reasons(result_text);

    if reject_reasons.is_empty() { return; }

    // Create sub-experiment nodes for each reject reason
    let mut canvas = state.canvas.write().await;
    let now = chrono::Utc::now().to_rfc3339();

    let mut created = 0;
    for (i, reason) in reject_reasons.iter().enumerate() {
        let node = CanvasNode {
            id: Uuid::new_v4(),
            parent_id: Some(node_id),
            title: format!("Drill: {}", reason.chars().take(50).collect::<String>()),
            description: format!("Auto-generated from reject reason:\n{}\n\nParent result:\n{}", reason, result_text.chars().take(200).collect::<String>()),
            references: Vec::new(),
            status: NodeStatus::Planned,
            children: Vec::new(),
            order: 100 + i,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let nid = node.id;
        canvas.nodes.insert(nid, node);
        if let Some(parent) = canvas.nodes.get_mut(&node_id) {
            parent.children.push(nid);
        }
        created += 1;
    }

    if created > 0 {
        canvas.save();
        // Log
        let mut log = state.call_log.write().await;
        log.log("AUTO", "recursive_design", &format!("{} sub-experiments", created), "created");
    }
}

fn generate_reject_reasons(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut reasons = Vec::new();

    if !lower.contains("레짐") && !lower.contains("regime") {
        reasons.push("레짐별 유효성 미검증 — bull/bear/sideways/crash별 성과 분리 필요".into());
    }
    if !lower.contains("유니버스") && !lower.contains("universe") && !lower.contains("심볼") {
        reasons.push("유니버스별 안정성 미검증 — 다른 종목군에서도 동일한지 확인 필요".into());
    }
    if lower.contains("decaying") || lower.contains("감소") || lower.contains("악화") {
        reasons.push("시간적 안정성 의문 — 최근 성과 악화 원인 분석 필요".into());
    }
    if lower.contains("crash") && lower.contains("-") {
        reasons.push("Crash regime에서 큰 손실 — crash filter 또는 regime 감지 로직 필요".into());
    }
    if lower.contains("short") && lower.contains("-") && lower.contains("long") {
        reasons.push("L/S 비대칭 — 한쪽만 유효하면 왜 그런지 경제적 메커니즘 검증 필요".into());
    }

    reasons
}

fn collect_descendants(canvas: &CanvasState, id: &Uuid, out: &mut Vec<(Uuid, String, String)>) {
    if let Some(node) = canvas.nodes.get(id) {
        out.push((node.id, node.title.clone(), node.description.clone()));
        for child_id in &node.children {
            collect_descendants(canvas, child_id, out);
        }
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(80).collect()
}

async fn run_claude_for_run(prompt: &str) -> Result<String, String> {
    // Try Julia structured execution first
    let julia_dir = std::path::PathBuf::from("analysis");
    if julia_dir.join("_ready").exists() {
        let code = format!(
            "include(\"analysis/run_structured.jl\")\nrun_structured_experiment(symbol=\"BTCUSDT\", data_source=\"crypto\", fast_span=24, slow_span=120, fee_bp=9.0)"
        );
        let cmd_file = julia_dir.join("_cmd.jl");
        let out_file = julia_dir.join("_out.txt");
        let log_file = julia_dir.join("_server.log");
        let _ = std::fs::remove_file(&out_file);
        let log_before = std::fs::read_to_string(&log_file).unwrap_or_default().len();
        if std::fs::write(&cmd_file, &code).is_ok() {
            let start = std::time::Instant::now();
            loop {
                if start.elapsed() > std::time::Duration::from_secs(120) { break; }
                if out_file.exists() {
                    let log_all = std::fs::read_to_string(&log_file).unwrap_or_default();
                    if log_all.len() > log_before {
                        return Ok(log_all[log_before..].to_string());
                    }
                    return Ok("Completed".into());
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }

    // Fallback: Claude CLI
    let output = std::process::Command::new("claude")
        .args(["-p", prompt])
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).to_string()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Create canvas from a free-form idea — auto-decompose into thesis structure.
/// Uses Claude to break down the idea into premises + experiments.
#[derive(Deserialize)]
pub struct IdeaToCanvas {
    pub idea: String,
}

pub async fn idea_to_canvas(
    State(state): State<AppState>,
    Json(req): Json<IdeaToCanvas>,
) -> Json<serde_json::Value> {
    let idea = &req.idea;

    // Use Claude to decompose
    let prompt = format!(
        "다음 연구 아이디어를 구조화해줘. JSON으로만 답해, 다른 텍스트 없이.\n\n\
         아이디어: {}\n\n\
         Format:\n\
         {{\n\
           \"claim\": \"주장 (한 문장)\",\n\
           \"mechanism\": \"경제적 메커니즘 (왜 이것이 작동하는지)\",\n\
           \"premises\": [\"전제1\", \"전제2\", \"전제3\"],\n\
           \"experiments\": [\"실험1 설명\", \"실험2 설명\"],\n\
           \"counter\": \"반론 또는 반증 조건\"\n\
         }}",
        idea
    );

    let claude_result = std::process::Command::new("claude")
        .args(["-p", &prompt])
        .output();

    let parsed = match claude_result {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout);
            // Find JSON in output
            let json_start = out.find('{');
            let json_end = out.rfind('}');
            match (json_start, json_end) {
                (Some(s), Some(e)) if s < e => {
                    serde_json::from_str::<serde_json::Value>(&out[s..=e]).ok()
                }
                _ => None,
            }
        }
        _ => None,
    };

    let parsed = match parsed {
        Some(p) => p,
        None => {
            // Fallback: create simple canvas from raw idea
            let mut canvas = state.canvas.write().await;
            let now = chrono::Utc::now().to_rfc3339();
            let root = CanvasNode {
                id: Uuid::new_v4(), parent_id: None,
                title: idea.chars().take(60).collect::<String>(),
                description: idea.clone(),
                references: Vec::new(), status: NodeStatus::Idea,
                children: Vec::new(), order: canvas.root_ids.len(),
                created_at: now.clone(), updated_at: now,
            };
            let rid = root.id;
            canvas.root_ids.push(rid);
            canvas.nodes.insert(rid, root);
            canvas.save();
            return Json(serde_json::json!({"status": "created_simple", "root_id": rid, "note": "Claude 분해 실패 — 단순 노드로 생성됨"}));
        }
    };

    // Build canvas tree from parsed JSON
    let mut canvas = state.canvas.write().await;
    let now = chrono::Utc::now().to_rfc3339();

    let claim = parsed["claim"].as_str().unwrap_or(idea);
    let mechanism = parsed["mechanism"].as_str().unwrap_or("");

    // Root: thesis
    let root = CanvasNode {
        id: Uuid::new_v4(), parent_id: None,
        title: claim.to_string(),
        description: format!("Mechanism: {}\nCounter: {}", mechanism, parsed["counter"].as_str().unwrap_or("")),
        references: Vec::new(), status: NodeStatus::Idea,
        children: Vec::new(), order: canvas.root_ids.len(),
        created_at: now.clone(), updated_at: now.clone(),
    };
    let rid = root.id;
    canvas.root_ids.push(rid);
    canvas.nodes.insert(rid, root);

    let mut child_ids = Vec::new();

    // Premises
    if let Some(premises) = parsed["premises"].as_array() {
        for (i, p) in premises.iter().enumerate() {
            let ptext = p.as_str().unwrap_or("");
            if ptext.is_empty() { continue; }
            let node = CanvasNode {
                id: Uuid::new_v4(), parent_id: Some(rid),
                title: format!("P{}: {}", i+1, ptext),
                description: String::new(),
                references: Vec::new(), status: NodeStatus::Idea,
                children: Vec::new(), order: i,
                created_at: now.clone(), updated_at: now.clone(),
            };
            let nid = node.id;
            canvas.nodes.insert(nid, node);
            child_ids.push(nid);
        }
    }

    // Experiments
    if let Some(exps) = parsed["experiments"].as_array() {
        for (i, e) in exps.iter().enumerate() {
            let etext = e.as_str().unwrap_or("");
            if etext.is_empty() { continue; }
            let node = CanvasNode {
                id: Uuid::new_v4(), parent_id: Some(rid),
                title: format!("EXP: {}", etext),
                description: String::new(),
                references: Vec::new(), status: NodeStatus::Planned,
                children: Vec::new(), order: 100 + i,
                created_at: now.clone(), updated_at: now.clone(),
            };
            let nid = node.id;
            canvas.nodes.insert(nid, node);
            child_ids.push(nid);
        }
    }

    // Link children to root
    if let Some(root_node) = canvas.nodes.get_mut(&rid) {
        root_node.children = child_ids.clone();
    }

    canvas.save();

    Json(serde_json::json!({
        "status": "created",
        "root_id": rid,
        "children": child_ids.len(),
        "claim": claim,
        "mechanism": mechanism,
    }))
}

/// Import from Obsidian Markdown — heading structure → canvas tree.
/// # H1 → root, ## H2 → child of H1, ### H3 → child of H2, etc.
/// Paragraph text under a heading → description.
#[derive(Deserialize)]
pub struct ImportMarkdown {
    pub content: String,
    pub source: Option<String>,  // filename for reference
}

pub async fn import_markdown(
    State(state): State<AppState>,
    Json(req): Json<ImportMarkdown>,
) -> Json<serde_json::Value> {
    let mut canvas = state.canvas.write().await;
    let now = Utc::now().to_rfc3339();

    // Parse markdown headings into tree
    let mut stack: Vec<(usize, Uuid)> = Vec::new(); // (heading_level, node_id)
    let mut current_desc = String::new();
    let mut last_node_id: Option<Uuid> = None;
    let mut created = 0;

    let lines: Vec<&str> = req.content.lines().collect();

    for line in &lines {
        let trimmed = line.trim();

        // Check if heading
        if trimmed.starts_with('#') {
            // Flush description to previous node
            if let Some(nid) = last_node_id {
                if let Some(node) = canvas.nodes.get_mut(&nid) {
                    node.description = current_desc.trim().to_string();
                }
            }
            current_desc.clear();

            let level = trimmed.chars().take_while(|c| *c == '#').count();
            let title = trimmed[level..].trim().to_string();
            if title.is_empty() { continue; }

            // Find parent: pop stack until we find a level < current
            while let Some((sl, _)) = stack.last() {
                if *sl >= level { stack.pop(); } else { break; }
            }

            let parent_id = stack.last().map(|(_, id)| *id);

            let node = CanvasNode {
                id: Uuid::new_v4(),
                parent_id,
                title,
                description: String::new(),
                references: Vec::new(),
                status: NodeStatus::Idea,
                children: Vec::new(),
                order: 0,
                created_at: now.clone(),
                updated_at: now.clone(),
            };
            let nid = node.id;

            if let Some(pid) = parent_id {
                let order = canvas.nodes.get(&pid).map(|p| p.children.len()).unwrap_or(0);
                canvas.nodes.insert(nid, CanvasNode { order, ..node });
                if let Some(parent) = canvas.nodes.get_mut(&pid) {
                    parent.children.push(nid);
                } else {
                    canvas.root_ids.push(nid);
                }
            } else {
                let order = canvas.root_ids.len();
                canvas.nodes.insert(nid, CanvasNode { order, ..node });
                canvas.root_ids.push(nid);
            }

            stack.push((level, nid));
            last_node_id = Some(nid);
            created += 1;
        } else if !trimmed.is_empty() && last_node_id.is_some() {
            // Paragraph text → append to description
            current_desc.push_str(trimmed);
            current_desc.push('\n');
        }
    }

    // Flush last description
    if let Some(nid) = last_node_id {
        if let Some(node) = canvas.nodes.get_mut(&nid) {
            node.description = current_desc.trim().to_string();
        }
    }

    canvas.save();

    Json(serde_json::json!({
        "status": "imported",
        "nodes_created": created,
        "source": req.source,
    }))
}

/// Import from a file path (for Obsidian vault files).
#[derive(Deserialize)]
pub struct ImportFile {
    pub path: String,
}

pub async fn import_file(
    State(state): State<AppState>,
    Json(req): Json<ImportFile>,
) -> Json<serde_json::Value> {
    let content = match std::fs::read_to_string(&req.path) {
        Ok(c) => c,
        Err(e) => return Json(serde_json::json!({"error": format!("Cannot read file: {}", e)})),
    };

    let filename = std::path::Path::new(&req.path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    import_markdown(
        State(state),
        Json(ImportMarkdown { content, source: Some(filename) }),
    ).await
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn remove_recursive(canvas: &mut CanvasState, id: Uuid) {
    if let Some(node) = canvas.nodes.remove(&id) {
        for child_id in node.children {
            remove_recursive(canvas, child_id);
        }
    }
}

fn build_tree(canvas: &CanvasState) -> Vec<serde_json::Value> {
    canvas.root_ids.iter()
        .filter_map(|id| build_node_json(canvas, id))
        .collect()
}

fn build_node_json(canvas: &CanvasState, id: &Uuid) -> Option<serde_json::Value> {
    let node = canvas.nodes.get(id)?;
    let children: Vec<serde_json::Value> = node.children.iter()
        .filter_map(|cid| build_node_json(canvas, cid))
        .collect();

    Some(serde_json::json!({
        "id": node.id,
        "title": node.title,
        "description": node.description,
        "status": node.status,
        "references": node.references,
        "children": children,
        "created_at": node.created_at,
    }))
}
