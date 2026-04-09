/// Claim lifecycle handlers — create, evaluate, and manage hypothesis claims.
///
/// All mutations persist via `graph.save()`. Gate checks are configurable
/// through `AmureConfig.gates.enabled`.

use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use amure_db::edge::{Edge, EdgeKind};
use amure_db::graph::{AmureGraph, Direction};
use amure_db::node::{Node, NodeKind, NodeStatus};

const GRAPH_DIR: &str = "data/amure_graph";

// ── Request types ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateClaimReq {
    statement: String,
    trigger: String,
    #[serde(default)]
    keywords: Vec<String>,
}

#[derive(Deserialize)]
pub struct AddReasonReq {
    reason_type: String,
    statement: String,
    bridge: String,
}

#[derive(Deserialize)]
pub struct AddEvidenceReq {
    tag: String,
    description: String,
}

#[derive(Deserialize)]
pub struct AddExperimentReq {
    description: String,
    method: String,
    expected_output: String,
}

#[derive(Deserialize)]
pub struct SubmitResultReq {
    result: serde_json::Value,
}

#[derive(Deserialize)]
pub struct VerdictReq {
    verdict: String,
    reason: String,
}

#[derive(Deserialize)]
pub struct AutoGenerateReq {
    idea: String,
}

// ── POST /api/claims ────────────────────────────────────────────────────────

pub async fn create_claim(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<CreateClaimReq>,
) -> Json<serde_json::Value> {
    if req.statement.len() < 10 {
        return Json(json!({"error": "Statement must be at least 10 characters"}));
    }
    if req.trigger.is_empty() {
        return Json(json!({"error": "Trigger must not be empty"}));
    }

    let node = Node::new(NodeKind::Claim, req.statement, req.keywords)
        .with_metadata(json!({"trigger": req.trigger}));

    let mut g = state.graph.write().await;
    let id = g.add_node(node);
    let _ = g.save(std::path::Path::new(GRAPH_DIR));

    Json(json!({"id": id, "status": "created"}))
}

// ── GET /api/claims ─────────────────────────────────────────────────────────

pub async fn list_claims(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;
    let claims = g.nodes_by_kind(NodeKind::Claim);

    let mut n_drafts = 0usize;
    let mut n_accepted = 0usize;

    let list: Vec<serde_json::Value> = claims
        .iter()
        .map(|c| {
            match c.status {
                NodeStatus::Draft => n_drafts += 1,
                NodeStatus::Accepted => n_accepted += 1,
                _ => {}
            }
            let (n_support, n_rebut) = count_reasons(&g, &c.id);
            let mut v = node_to_json(c);
            v.as_object_mut().unwrap().insert("n_support".into(), json!(n_support));
            v.as_object_mut().unwrap().insert("n_rebut".into(), json!(n_rebut));
            v
        })
        .collect();

    Json(json!({
        "claims": list,
        "n_drafts": n_drafts,
        "n_accepted": n_accepted,
    }))
}

// ── GET /api/claims/{id} ────────────────────────────────────────────────────

pub async fn get_claim(
    State(state): State<crate::server::routes::AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;

    let claim = match g.get_node(&id) {
        Some(n) if n.kind == NodeKind::Claim => n,
        _ => return Json(json!({"error": "Claim not found"})),
    };

    // Incoming Support/Rebut edges point Reason -> Claim
    let reason_edges = g.neighbors(
        &id,
        Direction::In,
        Some(&[EdgeKind::Support, EdgeKind::Rebut]),
    );

    let reasons: Vec<serde_json::Value> = reason_edges
        .iter()
        .filter_map(|(reason_id, edge)| {
            let reason = g.get_node(reason_id)?;
            if reason.kind != NodeKind::Reason {
                return None;
            }

            // Walk 1 hop from reason to find Evidence and Experiments
            let children = g.neighbors(
                reason_id,
                Direction::In,
                Some(&[EdgeKind::DerivedFrom]),
            );

            let mut evidences = Vec::new();
            let mut experiments = Vec::new();
            for (child_id, _) in &children {
                if let Some(child) = g.get_node(child_id) {
                    match child.kind {
                        NodeKind::Evidence => evidences.push(node_to_json(child)),
                        NodeKind::Experiment => experiments.push(node_to_json(child)),
                        _ => {}
                    }
                }
            }

            Some(json!({
                "reason": node_to_json(reason),
                "edge_kind": format!("{:?}", edge.kind),
                "evidences": evidences,
                "experiments": experiments,
            }))
        })
        .collect();

    let enabled = {
        let cfg = state.amure_config.read().await;
        cfg.gates.enabled.clone()
    };
    let check = gate_check(&g, &id, &enabled);

    Json(json!({
        "claim": node_to_json(claim),
        "reasons": reasons,
        "gate_check": check,
    }))
}

// ── POST /api/claims/{id}/reason ────────────────────────────────────────────

pub async fn add_reason(
    State(state): State<crate::server::routes::AppState>,
    Path(claim_id): Path<Uuid>,
    Json(req): Json<AddReasonReq>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;

    match g.get_node(&claim_id) {
        Some(n) if n.kind == NodeKind::Claim => {}
        _ => return Json(json!({"error": "Claim not found"})),
    }

    let edge_kind = match req.reason_type.as_str() {
        "support" => EdgeKind::Support,
        "rebut" => EdgeKind::Rebut,
        _ => return Json(json!({"error": "reason_type must be 'support' or 'rebut'"})),
    };

    let reason = Node::new(NodeKind::Reason, req.statement, vec![])
        .with_metadata(json!({"bridge": req.bridge}));
    let reason_id = g.add_node(reason);

    let edge = Edge::new(reason_id, claim_id, edge_kind);
    g.add_edge(edge);

    let _ = g.save(std::path::Path::new(GRAPH_DIR));
    Json(json!({"id": reason_id, "status": "created"}))
}

// ── POST /api/reasons/{id}/evidence ─────────────────────────────────────────

pub async fn add_evidence(
    State(state): State<crate::server::routes::AppState>,
    Path(reason_id): Path<Uuid>,
    Json(req): Json<AddEvidenceReq>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;

    match g.get_node(&reason_id) {
        Some(n) if n.kind == NodeKind::Reason => {}
        _ => return Json(json!({"error": "Reason not found"})),
    }

    let evidence = Node::new(
        NodeKind::Evidence,
        req.description,
        vec![req.tag],
    );
    let evidence_id = g.add_node(evidence);

    let edge = Edge::new(evidence_id, reason_id, EdgeKind::DerivedFrom);
    g.add_edge(edge);

    let _ = g.save(std::path::Path::new(GRAPH_DIR));
    Json(json!({"id": evidence_id, "status": "created"}))
}

// ── POST /api/reasons/{id}/experiment ───────────────────────────────────────

pub async fn add_experiment(
    State(state): State<crate::server::routes::AppState>,
    Path(reason_id): Path<Uuid>,
    Json(req): Json<AddExperimentReq>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;

    match g.get_node(&reason_id) {
        Some(n) if n.kind == NodeKind::Reason => {}
        _ => return Json(json!({"error": "Reason not found"})),
    }

    let experiment = Node::new(NodeKind::Experiment, req.description, vec![])
        .with_metadata(json!({
            "method": req.method,
            "expected_output": req.expected_output,
        }));
    let experiment_id = g.add_node(experiment);

    let edge = Edge::new(experiment_id, reason_id, EdgeKind::DerivedFrom);
    g.add_edge(edge);

    let _ = g.save(std::path::Path::new(GRAPH_DIR));
    Json(json!({"id": experiment_id, "status": "created"}))
}

// ── POST /api/experiments/{id}/result ───────────────────────────────────────

pub async fn submit_experiment_result(
    State(state): State<crate::server::routes::AppState>,
    Path(experiment_id): Path<Uuid>,
    Json(req): Json<SubmitResultReq>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;

    let node = match g.get_node_mut(&experiment_id) {
        Some(n) if n.kind == NodeKind::Experiment => n,
        _ => return Json(json!({"error": "Experiment not found"})),
    };

    // Merge result into existing metadata
    if let Some(obj) = node.metadata.as_object_mut() {
        obj.insert("result".into(), req.result);
    } else {
        node.metadata = json!({"result": req.result});
    }
    node.status = NodeStatus::Active;
    node.updated_at = Utc::now();

    let _ = g.save(std::path::Path::new(GRAPH_DIR));
    Json(json!({"id": experiment_id, "status": "updated"}))
}

// ── POST /api/claims/{id}/verdict ───────────────────────────────────────────

pub async fn verdict(
    State(state): State<crate::server::routes::AppState>,
    Path(claim_id): Path<Uuid>,
    Json(req): Json<VerdictReq>,
) -> Json<serde_json::Value> {
    let new_status = match req.verdict.as_str() {
        "accept" => NodeStatus::Accepted,
        "reject" => NodeStatus::Rejected,
        _ => return Json(json!({"error": "verdict must be 'accept' or 'reject'"})),
    };

    let enabled = {
        let cfg = state.amure_config.read().await;
        cfg.gates.enabled.clone()
    };

    let mut g = state.graph.write().await;

    match g.get_node(&claim_id) {
        Some(n) if n.kind == NodeKind::Claim => {}
        _ => return Json(json!({"error": "Claim not found"})),
    }

    // Run gate checks for accept; reject always allowed
    if new_status == NodeStatus::Accepted {
        let check = gate_check(&g, &claim_id, &enabled);
        if !check["passed"].as_bool().unwrap_or(false) {
            return Json(json!({
                "error": "Gate checks failed",
                "gate_check": check,
            }));
        }
    }

    let node = g.get_node_mut(&claim_id).unwrap();
    if let Some(obj) = node.metadata.as_object_mut() {
        obj.insert("verdict_reason".into(), json!(req.reason));
    } else {
        node.metadata = json!({"verdict_reason": req.reason});
    }
    node.status = new_status;
    node.updated_at = Utc::now();

    let _ = g.save(std::path::Path::new(GRAPH_DIR));
    Json(json!({
        "id": claim_id,
        "status": format!("{:?}", new_status),
    }))
}

// ── POST /api/claims/auto-generate ──────────────────────────────────────────

pub async fn auto_generate(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<AutoGenerateReq>,
) -> Json<serde_json::Value> {
    if req.idea.is_empty() {
        return Json(json!({"error": "Idea must not be empty"}));
    }

    let prompt = format!(
        "Given the following research idea, generate a structured claim for hypothesis testing.\n\n\
        Idea: {}\n\n\
        Respond with ONLY valid JSON (no markdown, no explanation) in this exact format:\n\
        {{\n  \
          \"statement\": \"a clear falsifiable claim (at least 10 chars)\",\n  \
          \"trigger\": \"what observation or question prompted this claim\",\n  \
          \"keywords\": [\"keyword1\", \"keyword2\"],\n  \
          \"support_reasons\": [\n    {{\n      \
            \"statement\": \"reason supporting the claim\",\n      \
            \"bridge\": \"logical connection to the claim\"\n    \
          }}\n  ],\n  \
          \"rebut_reasons\": [\n    {{\n      \
            \"statement\": \"potential counterargument\",\n      \
            \"bridge\": \"why this challenges the claim\"\n    \
          }}\n  ]\n\
        }}",
        req.idea,
    );

    let config = state.llm_config.read().await;
    match crate::server::llm_provider::call_llm(&prompt, &config).await {
        Ok(raw) => {
            // Try to parse as JSON; return raw if parsing fails
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(parsed) => Json(json!({"suggestion": parsed})),
                Err(_) => {
                    // Try extracting JSON from markdown code blocks
                    let trimmed = raw.trim();
                    let json_str = if trimmed.starts_with("```") {
                        trimmed
                            .trim_start_matches("```json")
                            .trim_start_matches("```")
                            .trim_end_matches("```")
                            .trim()
                    } else {
                        trimmed
                    };
                    match serde_json::from_str::<serde_json::Value>(json_str) {
                        Ok(parsed) => Json(json!({"suggestion": parsed})),
                        Err(e) => Json(json!({
                            "error": format!("Failed to parse LLM response: {}", e),
                            "raw": raw.trim(),
                        })),
                    }
                }
            }
        }
        Err(e) => Json(json!({"error": e})),
    }
}

// ── DELETE /api/claims/{id} ─────────────────────────────────────────────────

pub async fn delete_claim(
    State(state): State<crate::server::routes::AppState>,
    Path(claim_id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;

    match g.get_node(&claim_id) {
        Some(n) if n.kind == NodeKind::Claim => {}
        _ => return Json(json!({"error": "Claim not found"})),
    }

    // Walk from claim to collect all connected nodes (BFS, no edge filter)
    let connected: Vec<Uuid> = g
        .walk(&claim_id, 10, None)
        .into_iter()
        .map(|(id, _)| id)
        .collect();

    let n_removed = connected.len();
    for id in &connected {
        g.remove_node(id);
    }

    let _ = g.save(std::path::Path::new(GRAPH_DIR));
    Json(json!({"status": "deleted", "nodes_removed": n_removed}))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn node_to_json(n: &Node) -> serde_json::Value {
    json!({
        "id": n.id,
        "kind": format!("{:?}", n.kind),
        "statement": n.statement,
        "keywords": n.keywords,
        "status": format!("{:?}", n.status),
        "metadata": n.metadata,
        "created_at": n.created_at.to_rfc3339(),
        "updated_at": n.updated_at.to_rfc3339(),
    })
}

fn count_reasons(g: &AmureGraph, claim_id: &Uuid) -> (usize, usize) {
    let incoming = g.neighbors(claim_id, Direction::In, Some(&[EdgeKind::Support, EdgeKind::Rebut]));
    let mut n_support = 0usize;
    let mut n_rebut = 0usize;
    for (_, edge) in &incoming {
        match edge.kind {
            EdgeKind::Support => n_support += 1,
            EdgeKind::Rebut => n_rebut += 1,
            _ => {}
        }
    }
    (n_support, n_rebut)
}

/// Run configurable gate checks against a claim.
/// Returns `{ passed: bool, errors: [...] }`.
fn gate_check(g: &AmureGraph, claim_id: &Uuid, enabled_gates: &[String]) -> serde_json::Value {
    let mut errors: Vec<String> = Vec::new();

    let claim = match g.get_node(claim_id) {
        Some(n) => n,
        None => return json!({"passed": false, "errors": ["Claim not found"]}),
    };

    // claim_gate: statement >= 10 chars, trigger present in metadata
    if enabled_gates.iter().any(|g| g == "claim_gate") {
        if claim.statement.len() < 10 {
            errors.push("Statement must be at least 10 characters".into());
        }
        let has_trigger = claim.metadata.get("trigger")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if !has_trigger {
            errors.push("Claim must have a trigger".into());
        }
    }

    // Collect support reasons and their children for downstream gates
    let support_reasons: Vec<Uuid> = g
        .neighbors(claim_id, Direction::In, Some(&[EdgeKind::Support]))
        .into_iter()
        .filter_map(|(id, _)| {
            g.get_node(&id).and_then(|n| {
                if n.kind == NodeKind::Reason { Some(id) } else { None }
            })
        })
        .collect();

    // argument_gate: >= 1 support reason
    if enabled_gates.iter().any(|g| g == "argument_gate") {
        if support_reasons.is_empty() {
            errors.push("At least one support reason is required".into());
        }
    }

    // evidence_gate: >= 1 evidence on at least one support reason
    if enabled_gates.iter().any(|g| g == "evidence_gate") {
        let has_evidence = support_reasons.iter().any(|rid| {
            g.neighbors(rid, Direction::In, Some(&[EdgeKind::DerivedFrom]))
                .iter()
                .any(|(cid, _)| {
                    g.get_node(cid).map(|n| n.kind == NodeKind::Evidence).unwrap_or(false)
                })
        });
        if !has_evidence {
            errors.push("At least one evidence item on a support reason is required".into());
        }
    }

    // experiment_gate: >= 1 experiment with a result
    if enabled_gates.iter().any(|g| g == "experiment_gate") {
        let has_completed_experiment = support_reasons.iter().any(|rid| {
            g.neighbors(rid, Direction::In, Some(&[EdgeKind::DerivedFrom]))
                .iter()
                .any(|(cid, _)| {
                    g.get_node(cid)
                        .map(|n| {
                            n.kind == NodeKind::Experiment
                                && n.metadata.get("result").is_some()
                        })
                        .unwrap_or(false)
                })
        });
        if !has_completed_experiment {
            errors.push("At least one experiment with a result is required".into());
        }
    }

    json!({
        "passed": errors.is_empty(),
        "errors": errors,
    })
}
