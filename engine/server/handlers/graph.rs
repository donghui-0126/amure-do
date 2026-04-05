/// Graph API handlers — ported from amure-db server.rs into amure-do handler style.
/// All graph CRUD, knowledge analysis, Yahoo Finance, and LLM endpoints.

use std::collections::{HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::response::Html;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use amure_db::edge::{Edge, EdgeKind};
use amure_db::graph::AmureGraph;
use amure_db::node::{Node, NodeKind, NodeStatus, tokenize};
use amure_db::search::{search, SearchOptions};

const GRAPH_DATA_DIR: &str = "data/amure_graph";
const GRAPH_DASHBOARD: &str = include_str!("../../dashboard/graph.html");

// ── Dashboard ───────────────────────────────────────────────────────────────

pub async fn serve_graph_dashboard() -> Html<&'static str> {
    Html(GRAPH_DASHBOARD)
}

// ── Graph API — Core CRUD ──────────────────────────────────────────────────

pub async fn graph_all(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;
    let nodes: Vec<serde_json::Value> = g.nodes.values().map(node_json).collect();
    let edges: Vec<serde_json::Value> = g.edges.values().map(edge_json).collect();
    Json(serde_json::json!({"nodes": nodes, "edges": edges, "n_nodes": nodes.len(), "n_edges": edges.len()}))
}

pub async fn graph_summary(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;
    Json(serde_json::json!(g.summary()))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
    top_k: Option<usize>,
    include_failed: Option<bool>,
}

pub async fn graph_search(
    State(state): State<crate::server::routes::AppState>,
    Query(q): Query<SearchQuery>,
) -> Json<serde_json::Value> {
    let query = q.q.unwrap_or_default();
    if query.is_empty() {
        let g = state.graph.read().await;
        let mut nodes: Vec<serde_json::Value> = g.nodes.values().map(node_json).collect();
        nodes.sort_by(|a, b| a["kind"].as_str().cmp(&b["kind"].as_str()));
        return Json(serde_json::json!({"results": nodes, "count": nodes.len()}));
    }
    let g = state.graph.read().await;
    let results = search(&g, &query, &state.synonyms, &SearchOptions {
        top_k: q.top_k.unwrap_or(10),
        include_failed: q.include_failed.unwrap_or(true),
        ..Default::default()
    });
    Json(serde_json::json!({"results": results, "count": results.len()}))
}

pub async fn graph_node(
    State(state): State<crate::server::routes::AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;
    match g.get_node(&id) {
        Some(n) => {
            let edges: Vec<serde_json::Value> = g.edges.values()
                .filter(|e| e.source == id || e.target == id)
                .map(|e| {
                    let other_id = if e.source == id { e.target } else { e.source };
                    let other = g.get_node(&other_id);
                    let mut ej = edge_json(e);
                    if let Some(o) = other {
                        ej.as_object_mut().unwrap().insert("other_statement".into(), serde_json::Value::String(o.statement.clone()));
                        ej.as_object_mut().unwrap().insert("other_kind".into(), serde_json::Value::String(format!("{:?}", o.kind)));
                    }
                    ej
                }).collect();
            Json(serde_json::json!({"node": node_json(n), "edges": edges}))
        }
        None => Json(serde_json::json!({"error": "Not found"})),
    }
}

pub async fn delete_node(
    State(state): State<crate::server::routes::AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;
    if g.remove_node(&id).is_some() {
        let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
        Json(serde_json::json!({"status": "deleted"}))
    } else {
        Json(serde_json::json!({"error": "Not found"}))
    }
}

// ── POST /api/graph/node — add any node ────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateNodeReq {
    kind: String,
    statement: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    metadata: serde_json::Value,
    #[serde(default)]
    status: Option<String>,
}

pub async fn create_node(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<CreateNodeReq>,
) -> Json<serde_json::Value> {
    let kind = parse_node_kind(&req.kind);
    let status = req.status.as_deref().map(parse_node_status).unwrap_or(NodeStatus::Draft);
    let mut node = Node::new(kind, req.statement, req.keywords)
        .with_status(status);
    if !req.metadata.is_null() {
        node = node.with_metadata(req.metadata);
    }
    let mut g = state.graph.write().await;
    let id = g.add_node(node);
    let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
    Json(serde_json::json!({"status": "created", "id": id}))
}

// ── PATCH /api/graph/node/{id} — update node ──────────────────────────────

#[derive(Deserialize)]
pub struct UpdateNodeReq {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    keywords: Option<Vec<String>>,
    #[serde(default)]
    statement: Option<String>,
}

pub async fn update_node(
    State(state): State<crate::server::routes::AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateNodeReq>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;
    match g.get_node_mut(&id) {
        Some(node) => {
            if let Some(ref status) = req.status {
                node.status = parse_node_status(status);
            }
            if let Some(ref meta) = req.metadata {
                if let (Some(existing), Some(incoming)) = (node.metadata.as_object_mut(), meta.as_object()) {
                    for (k, v) in incoming {
                        existing.insert(k.clone(), v.clone());
                    }
                } else {
                    node.metadata = meta.clone();
                }
            }
            if let Some(ref kws) = req.keywords {
                node.keywords = kws.clone();
            }
            if let Some(ref stmt) = req.statement {
                node.statement = stmt.clone();
            }
            node.updated_at = chrono::Utc::now();
            let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
            Json(serde_json::json!({"status": "updated", "id": id}))
        }
        None => Json(serde_json::json!({"error": "Not found"})),
    }
}

// ── POST /api/graph/edge — add edge ────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateEdgeReq {
    source: Uuid,
    target: Uuid,
    kind: String,
    #[serde(default)]
    note: Option<String>,
}

pub async fn create_edge(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<CreateEdgeReq>,
) -> Json<serde_json::Value> {
    let kind = parse_edge_kind(&req.kind);
    let mut g = state.graph.write().await;
    let edge = Edge::new(req.source, req.target, kind).with_note(req.note.unwrap_or_default());
    let id = g.add_edge(edge);
    let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
    Json(serde_json::json!({"status": "created", "id": id}))
}

// ── DELETE /api/graph/edge/{id} — remove edge ──────────────────────────────

pub async fn delete_edge(
    State(state): State<crate::server::routes::AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;
    if g.remove_edge(&id).is_some() {
        let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
        Json(serde_json::json!({"status": "deleted"}))
    } else {
        Json(serde_json::json!({"error": "Not found"}))
    }
}

// ── GET /api/graph/walk/{id}?hops=2 — BFS walk ────────────────────────────

#[derive(Deserialize, Default)]
pub struct WalkQuery { hops: Option<usize> }

pub async fn graph_walk(
    State(state): State<crate::server::routes::AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<WalkQuery>,
) -> Json<serde_json::Value> {
    let max_hops = q.hops.unwrap_or(2);
    let g = state.graph.read().await;
    if g.get_node(&id).is_none() {
        return Json(serde_json::json!({"error": "Node not found"}));
    }
    let walked = g.walk(&id, max_hops, None);
    let nodes: Vec<serde_json::Value> = walked.iter().filter_map(|(nid, depth)| {
        g.get_node(nid).map(|n| serde_json::json!({
            "id": n.id,
            "kind": format!("{:?}", n.kind),
            "statement": n.statement,
            "status": format!("{:?}", n.status),
            "depth": depth,
        }))
    }).collect();
    Json(serde_json::json!({"start": id, "max_hops": max_hops, "nodes": nodes, "count": nodes.len()}))
}

// ── GET /api/graph/subgraph/{id} — full subgraph ──────────────────────────

pub async fn graph_subgraph(
    State(state): State<crate::server::routes::AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;
    if g.get_node(&id).is_none() {
        return Json(serde_json::json!({"error": "Node not found"}));
    }
    let walked = g.walk(&id, 10, None);
    let node_ids: Vec<Uuid> = walked.iter().map(|(nid, _)| *nid).collect();
    let (nodes, edges) = g.subgraph(&node_ids);

    let node_list: Vec<serde_json::Value> = nodes.iter().map(|n| {
        serde_json::json!({
            "id": n.id,
            "kind": format!("{:?}", n.kind),
            "statement": n.statement,
            "keywords": n.keywords,
            "status": format!("{:?}", n.status),
            "metadata": n.metadata,
            "failed": n.is_failed(),
        })
    }).collect();

    let edge_list: Vec<serde_json::Value> = edges.iter().map(|e| {
        serde_json::json!({
            "id": e.id,
            "source": e.source,
            "target": e.target,
            "kind": format!("{:?}", e.kind),
            "weight": e.weight,
            "note": e.note,
        })
    }).collect();

    Json(serde_json::json!({
        "root": id,
        "nodes": node_list,
        "edges": edge_list,
        "n_nodes": node_list.len(),
        "n_edges": edge_list.len(),
    }))
}

// ══════════════════════════════════════════════════════════════════════════════
// Knowledge Analysis Endpoints
// ══════════════════════════════════════════════════════════════════════════════

// ── POST /api/knowledge-util/check-failures ───────────────────────────────

#[derive(Deserialize)]
pub struct FailureCheckReq {
    statement: String,
    #[serde(default)]
    keywords: Vec<String>,
}

#[derive(serde::Serialize)]
struct FailureWarning {
    failed_node_id: Uuid,
    failed_statement: String,
    status: String,
    overlap_keywords: Vec<String>,
    score: f64,
    failure_reason: String,
    experiments_done: Vec<String>,
    gaps_remaining: Vec<String>,
    methods_used: Vec<String>,
    methods_not_used: Vec<String>,
}

pub async fn check_failures(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<FailureCheckReq>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;
    let warnings = do_check_failure_patterns(&g, &req.statement, &req.keywords);
    Json(serde_json::json!({"warnings": warnings, "count": warnings.len()}))
}

fn do_check_failure_patterns(g: &AmureGraph, statement: &str, keywords: &[String]) -> Vec<FailureWarning> {
    let mut warnings = Vec::new();

    let failed_nodes: Vec<&Node> = g.nodes.values()
        .filter(|n| n.is_failed())
        .collect();

    if failed_nodes.is_empty() { return warnings; }

    let new_kws: HashSet<String> = keywords.iter()
        .map(|k| k.to_lowercase()).collect();
    let new_tokens: HashSet<String> = tokenize(statement).into_iter().collect();

    for node in &failed_nodes {
        let node_kws: HashSet<String> = node.keywords.iter()
            .map(|k| k.to_lowercase()).collect();
        let node_tokens: HashSet<String> = node.tokens().into_iter().collect();

        let kw_overlap: Vec<String> = new_kws.intersection(&node_kws).cloned().collect();
        let token_overlap = new_tokens.intersection(&node_tokens).count();

        let score = kw_overlap.len() as f64 * 0.6 + token_overlap as f64 * 0.1;
        if score > 0.5 {
            let reason = node.metadata.get("reject_reason")
                .or(node.metadata.get("accept_reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let mut experiments_done = Vec::new();
            let mut gaps_remaining = Vec::new();
            let mut methods_used = HashSet::new();

            let walked = g.walk(&node.id, 3, None);
            for (nid, _hop) in &walked {
                if let Some(n) = g.get_node(nid) {
                    if n.kind == NodeKind::Experiment {
                        experiments_done.push(n.statement.clone());
                        if let Some(m) = n.metadata.get("method").and_then(|v| v.as_str()) {
                            methods_used.insert(m.to_string());
                        }
                        if let Some(gaps) = n.metadata.get("gaps") {
                            if let Some(arr) = gaps.as_array() {
                                for gap in arr {
                                    if let Some(s) = gap.as_str() {
                                        gaps_remaining.push(s.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let all_methods = ["CrossSectional", "Distributional", "Conditional", "DoseResponse",
                "Regime", "Temporal", "MultiHorizon", "EntryExit", "Backtest"];
            let methods_not_used: Vec<String> = all_methods.iter()
                .filter(|m| !methods_used.contains(**m))
                .map(|m| m.to_string())
                .collect();

            warnings.push(FailureWarning {
                failed_node_id: node.id,
                failed_statement: node.statement.clone(),
                status: format!("{:?}", node.status),
                overlap_keywords: kw_overlap,
                score,
                failure_reason: reason,
                experiments_done,
                gaps_remaining,
                methods_used: methods_used.into_iter().collect(),
                methods_not_used,
            });
        }
    }

    warnings.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    warnings.truncate(5);
    warnings
}

// ── GET /api/knowledge-util/check-revalidation ────────────────────────────

#[derive(serde::Serialize)]
struct RevalidationAlert {
    node_id: Uuid,
    statement: String,
    days_since_update: i64,
    trigger: String,
    reason: String,
}

pub async fn check_revalidation(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;
    let alerts = do_check_revalidation(&g);
    Json(serde_json::json!({"alerts": alerts, "count": alerts.len()}))
}

fn do_check_revalidation(g: &AmureGraph) -> Vec<RevalidationAlert> {
    let now = chrono::Utc::now();
    let mut alerts = Vec::new();

    for node in g.nodes_by_kind(NodeKind::Claim) {
        if node.status != NodeStatus::Accepted { continue; }

        let days_since = (now - node.updated_at).num_days();
        let needs_revalidation = days_since > 30;
        let has_decay_risk = node.statement.contains("decay")
            || node.metadata.get("alpha_decay").and_then(|v| v.as_bool()).unwrap_or(false);

        let trigger = node.metadata.get("trigger")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if needs_revalidation || has_decay_risk {
            alerts.push(RevalidationAlert {
                node_id: node.id,
                statement: node.statement.clone(),
                days_since_update: days_since,
                trigger: trigger.to_string(),
                reason: if has_decay_risk {
                    "Alpha decay risk — periodic revalidation needed".into()
                } else {
                    format!("No revalidation for {} days", days_since)
                },
            });
        }
    }

    alerts.sort_by(|a, b| b.days_since_update.cmp(&a.days_since_update));
    alerts
}

// ── POST /api/knowledge-util/detect-contradictions ────────────────────────

#[derive(serde::Serialize)]
struct ContradictionAlert {
    node_a_id: Uuid,
    node_a_statement: String,
    node_b_id: Uuid,
    node_b_statement: String,
    overlap_keywords: Vec<String>,
    reason: String,
}

pub async fn detect_contradictions(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;
    let alerts = do_detect_contradictions(&mut g);
    let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
    Json(serde_json::json!({"contradictions": alerts, "count": alerts.len()}))
}

fn do_detect_contradictions(g: &mut AmureGraph) -> Vec<ContradictionAlert> {
    let meta_kw = ["validated", "disproven", "gap_derived", "auto_generated"];

    let accepted: Vec<(Uuid, String, Vec<String>, bool)> = g
        .nodes_by_kind(NodeKind::Claim)
        .iter()
        .filter(|n| n.status == NodeStatus::Accepted)
        .map(|n| {
            let is_reversal = n.statement.contains("reversal") || n.statement.contains("반전") || n.statement.contains("회귀");
            let filtered_kw: Vec<String> = n.keywords.iter()
                .filter(|k| !meta_kw.contains(&k.as_str()))
                .cloned().collect();
            (n.id, n.statement.clone(), filtered_kw, is_reversal)
        })
        .collect();

    let mut alerts = Vec::new();

    for i in 0..accepted.len() {
        for j in (i+1)..accepted.len() {
            let (id_a, stmt_a, kw_a, rev_a) = &accepted[i];
            let (id_b, stmt_b, kw_b, rev_b) = &accepted[j];

            let kw_set_a: HashSet<String> = kw_a.iter().map(|k| k.to_lowercase()).collect();
            let kw_set_b: HashSet<String> = kw_b.iter().map(|k| k.to_lowercase()).collect();
            let overlap: Vec<String> = kw_set_a.intersection(&kw_set_b).cloned().collect();

            if overlap.len() >= 2 && rev_a != rev_b {
                let has_edge = g.edges.values().any(|e| {
                    e.kind == EdgeKind::Contradicts &&
                    ((e.source == *id_a && e.target == *id_b) || (e.source == *id_b && e.target == *id_a))
                });

                if !has_edge {
                    g.add_edge(
                        Edge::new(*id_a, *id_b, EdgeKind::Contradicts)
                            .with_note(format!("Auto-detected: keyword overlap({}) + direction conflict", overlap.join(",")))
                    );
                }

                alerts.push(ContradictionAlert {
                    node_a_id: *id_a,
                    node_a_statement: stmt_a.clone(),
                    node_b_id: *id_b,
                    node_b_statement: stmt_b.clone(),
                    overlap_keywords: overlap,
                    reason: format!("Direction conflict: {} vs {}", if *rev_a {"reversal"} else {"momentum"}, if *rev_b {"reversal"} else {"momentum"}),
                });
            }
        }
    }

    alerts
}

// ── POST /api/knowledge-util/auto-gap-claims ──────────────────────────────

#[derive(Deserialize)]
pub struct AutoGapReq {
    source_claim_id: Uuid,
    gaps: Vec<String>,
    #[serde(default)]
    keywords: Vec<String>,
}

pub async fn auto_gap_claims(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<AutoGapReq>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;
    let mut created = Vec::new();

    for gap in &req.gaps {
        if gap.len() < 5 { continue; }
        let mut kw = req.keywords.clone();
        kw.push("gap_derived".into());
        let node = Node::new(NodeKind::Claim, gap.clone(), kw)
            .with_metadata(serde_json::json!({
                "trigger": "Derived from source claim gap",
                "source_claim": req.source_claim_id.to_string(),
                "auto_generated": true,
            }));
        let gap_id = g.add_node(node);
        g.add_edge(
            Edge::new(gap_id, req.source_claim_id, EdgeKind::Refines)
                .with_note("Sub-hypothesis derived from gap".to_string())
        );
        created.push(gap_id);
    }

    let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
    Json(serde_json::json!({"created": created, "count": created.len()}))
}

// ── GET /api/knowledge-util/suggest-combinations ──────────────────────────

#[derive(serde::Serialize)]
struct CombinationSuggestion {
    failed_nodes: Vec<(Uuid, String)>,
    shared_keywords: Vec<String>,
    individual_irs: Vec<String>,
    combination_idea: String,
    untried_combination: bool,
}

pub async fn suggest_combinations(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;
    let suggestions = do_suggest_failure_combinations(&g);
    Json(serde_json::json!({"suggestions": suggestions, "count": suggestions.len()}))
}

fn do_suggest_failure_combinations(g: &AmureGraph) -> Vec<CombinationSuggestion> {
    let meta_kw = ["validated", "disproven", "gap_derived", "auto_generated"];

    let failed: Vec<(&Node, Vec<String>)> = g.nodes.values()
        .filter(|n| n.kind == NodeKind::Claim && (n.status == NodeStatus::Rejected || n.status == NodeStatus::Weakened))
        .map(|n| {
            let walked = g.walk(&n.id, 3, None);
            let mut methods = Vec::new();
            for (nid, _) in &walked {
                if let Some(exp) = g.get_node(nid) {
                    if exp.kind == NodeKind::Experiment {
                        let m = exp.metadata.get("method").and_then(|v| v.as_str()).unwrap_or("?");
                        let ir = exp.metadata.get("result")
                            .and_then(|r| r.get("ir"))
                            .and_then(|v| v.as_f64())
                            .map(|v| format!("IR={:.3}", v))
                            .unwrap_or_default();
                        methods.push(format!("{}({})", m, ir));
                    }
                }
            }
            (n, methods)
        })
        .collect();

    let mut suggestions = Vec::new();

    for i in 0..failed.len() {
        for j in (i+1)..failed.len() {
            let (a, a_methods) = &failed[i];
            let (b, b_methods) = &failed[j];

            let kw_a: HashSet<String> = a.keywords.iter()
                .filter(|k| !meta_kw.contains(&k.as_str()))
                .map(|k| k.to_lowercase()).collect();
            let kw_b: HashSet<String> = b.keywords.iter()
                .filter(|k| !meta_kw.contains(&k.as_str()))
                .map(|k| k.to_lowercase()).collect();

            let shared: Vec<String> = kw_a.intersection(&kw_b).cloned().collect();
            if shared.is_empty() { continue; }

            let unique_a: Vec<String> = kw_a.difference(&kw_b).cloned().collect();
            let unique_b: Vec<String> = kw_b.difference(&kw_a).cloned().collect();

            let idea = format!(
                "Based on common({}), combine [{}] and [{}]. Each individually insignificant but co-occurrence may amplify signal",
                shared.join(","),
                unique_a.join(","),
                unique_b.join(","),
            );

            let combined_kw: HashSet<String> = kw_a.union(&kw_b).cloned().collect();
            let already_tried = g.nodes.values().any(|n| {
                if n.kind != NodeKind::Claim { return false; }
                let nkw: HashSet<String> = n.keywords.iter().map(|k| k.to_lowercase()).collect();
                let overlap = combined_kw.intersection(&nkw).count();
                overlap >= combined_kw.len().saturating_sub(1) && n.status != NodeStatus::Rejected
            });

            suggestions.push(CombinationSuggestion {
                failed_nodes: vec![
                    (a.id, a.statement.chars().take(60).collect()),
                    (b.id, b.statement.chars().take(60).collect()),
                ],
                shared_keywords: shared,
                individual_irs: [a_methods.clone(), b_methods.clone()].concat(),
                combination_idea: idea,
                untried_combination: !already_tried,
            });
        }
    }

    suggestions.sort_by(|a, b| {
        b.shared_keywords.len().cmp(&a.shared_keywords.len())
            .then(b.untried_combination.cmp(&a.untried_combination))
    });
    suggestions.truncate(10);
    suggestions
}

// ══════════════════════════════════════════════════════════════════════════════
// Yahoo Finance
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
pub struct YahooReq { symbol: String }

#[derive(Deserialize)]
pub struct YahooBatchReq { symbols: Vec<String> }

pub async fn yahoo_fetch(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<YahooReq>,
) -> Json<serde_json::Value> {
    match fetch_yahoo_fact(&req.symbol).await {
        Ok((node, meta)) => {
            let stmt = node.statement.clone();
            let kw = node.keywords.clone();
            let mut g = state.graph.write().await;
            let id = g.add_node(node);
            let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
            Json(serde_json::json!({"status": "created", "id": id, "statement": stmt, "keywords": kw, "metadata": meta}))
        }
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

pub async fn yahoo_batch(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<YahooBatchReq>,
) -> Json<serde_json::Value> {
    let mut created = Vec::new();
    let mut errors = Vec::new();
    for sym in &req.symbols {
        match fetch_yahoo_fact(sym).await {
            Ok((node, _)) => {
                let stmt = node.statement.clone();
                let mut g = state.graph.write().await;
                let id = g.add_node(node);
                drop(g);
                created.push(serde_json::json!({"id": id, "symbol": sym, "statement": stmt}));
            }
            Err(e) => errors.push(serde_json::json!({"symbol": sym, "error": e})),
        }
    }
    let g = state.graph.read().await;
    let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
    Json(serde_json::json!({"created": created, "errors": errors, "n_created": created.len()}))
}

async fn fetch_yahoo_fact(symbol: &str) -> Result<(Node, serde_json::Value), String> {
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?period1={}&period2={}&interval=1d",
        symbol,
        (chrono::Utc::now() - chrono::Duration::days(90)).timestamp(),
        chrono::Utc::now().timestamp(),
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url).header("User-Agent", "Mozilla/5.0").send().await.map_err(|e| e.to_string())?;
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let result = &body["chart"]["result"];
    if result.is_null() || !result.is_array() || result.as_array().unwrap().is_empty() {
        return Err("No data from Yahoo".into());
    }

    let r0 = &result[0];
    let meta = &r0["meta"];
    let closes: Vec<f64> = r0["indicators"]["quote"][0]["close"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_f64()).collect()).unwrap_or_default();
    let volumes: Vec<f64> = r0["indicators"]["quote"][0]["volume"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_f64()).collect()).unwrap_or_default();

    if closes.len() < 2 { return Err("Insufficient data".into()); }

    let last = *closes.last().unwrap();
    let first = *closes.first().unwrap();
    let ret = ((last / first) - 1.0) * 100.0;
    let avg_vol = if volumes.is_empty() { 0.0 } else { volumes.iter().sum::<f64>() / volumes.len() as f64 };
    let currency = meta["currency"].as_str().unwrap_or("USD");
    let exchange = meta["exchangeName"].as_str().unwrap_or("?");

    let statement = format!("{}: price {:.2} {}, 3m {:+.1}%, vol {:.0}, {}", symbol, last, currency, ret, avg_vol, exchange);

    let mut keywords = vec![symbol.to_lowercase()];
    let sym = symbol.to_uppercase();

    let sector_kw: &[&str] = match sym.as_str() {
        "AAPL" => &["apple", "tech", "hardware"],
        "MSFT" => &["microsoft", "tech", "cloud", "ai", "azure", "software"],
        "GOOGL" | "GOOG" => &["google", "alphabet", "tech", "ai", "advertising", "search"],
        "AMZN" => &["amazon", "tech", "ecommerce", "aws", "cloud", "consumer"],
        "TSLA" => &["tesla", "ev", "auto", "consumer"],
        "NVDA" => &["nvidia", "ai", "gpu", "semiconductor", "tech"],
        "META" => &["facebook", "meta", "tech", "advertising", "metaverse", "social"],
        "JPM" => &["jpmorgan", "bank", "financial", "dividend", "value"],
        "BAC" => &["bank_of_america", "bank", "financial", "dividend"],
        "GS" => &["goldman_sachs", "financial", "investment_bank"],
        "KO" => &["coca-cola", "beverage", "defensive", "dividend", "value", "consumer_staples"],
        "PG" => &["procter_gamble", "consumer_staples", "defensive", "dividend"],
        "WMT" => &["walmart", "retail", "consumer", "dividend"],
        "XOM" => &["exxon", "energy", "oil", "dividend"],
        "CVX" => &["chevron", "energy", "oil", "dividend"],
        "SPY" => &["s&p500", "etf", "index", "us_equity"],
        "QQQ" => &["nasdaq", "etf", "tech", "growth"],
        "VOO" => &["vanguard", "s&p500", "etf", "index", "low_cost"],
        "SCHD" => &["schwab", "dividend", "etf", "value", "income"],
        "TLT" => &["treasury", "bond", "rate", "safe_haven", "etf", "long_duration"],
        _ => &[],
    };
    keywords.extend(sector_kw.iter().map(|s| s.to_string()));

    let sym_lower = symbol.to_lowercase();
    if sym_lower.contains("btc") || sym_lower.contains("bitcoin") { keywords.extend(["crypto".into(), "bitcoin".into()]); }
    else if sym_lower.contains("eth") && !sym_lower.contains("meth") { keywords.extend(["crypto".into(), "ethereum".into()]); }
    else if sym_lower.contains("sol") { keywords.extend(["crypto".into(), "solana".into(), "defi".into()]); }

    if ret > 10.0 { keywords.push("bullish".into()); }
    else if ret > 0.0 { keywords.push("up".into()); }
    if ret < -10.0 { keywords.push("bearish".into()); }
    else if ret < 0.0 { keywords.push("down".into()); }

    if closes.len() > 5 {
        let daily_rets: Vec<f64> = closes.windows(2).map(|w| ((w[1] / w[0]) - 1.0).abs()).collect();
        let avg_abs_ret = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
        if avg_abs_ret > 0.025 { keywords.push("high_volatility".into()); }
    }

    keywords.push(exchange.to_lowercase());

    let fact_meta = serde_json::json!({
        "symbol": symbol, "price": last, "currency": currency, "exchange": exchange,
        "return_3m": (ret * 100.0).round() / 100.0, "avg_volume": avg_vol.round(),
        "n_bars": closes.len(), "fetched_at": chrono::Utc::now().to_rfc3339(),
    });

    let node = Node::new(NodeKind::Fact, statement, keywords)
        .with_status(NodeStatus::Active)
        .with_metadata(fact_meta.clone());

    Ok((node, fact_meta))
}

// ── Auto Organize ───────────────────────────────────────────────────────────

pub async fn auto_organize(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;
    let mut claims_created = 0;
    let mut edges_created = 0;

    let facts: Vec<(Uuid, String, serde_json::Value)> = g.nodes.values()
        .filter(|n| n.kind == NodeKind::Fact)
        .map(|n| (n.id, n.statement.clone(), n.metadata.clone()))
        .collect();

    if facts.is_empty() {
        return Json(serde_json::json!({"error": "No facts to organize. Fetch Yahoo data first."}));
    }

    let mut bullish = Vec::new();
    let mut bearish = Vec::new();
    for (id, stmt, meta) in &facts {
        let ret = meta["return_3m"].as_f64().unwrap_or(0.0);
        if ret > 5.0 { bullish.push((*id, stmt.clone(), ret)); }
        else if ret < -10.0 { bearish.push((*id, stmt.clone(), ret)); }
    }

    if bullish.len() >= 2 {
        let symbols: Vec<String> = bullish.iter().map(|(_, s, _)| s.split(':').next().unwrap_or("?").to_string()).collect();
        let avg_ret: f64 = bullish.iter().map(|(_, _, r)| r).sum::<f64>() / bullish.len() as f64;
        let claim = Node::new(NodeKind::Claim,
            format!("Recent 3-month bullish group ({}) avg return {:.1}% — momentum persists", symbols.join(", "), avg_ret),
            vec!["bullish".into(), "momentum".into()])
            .with_metadata(serde_json::json!({"trigger": "Review on market regime change", "auto_generated": true}));
        let cid = g.add_node(claim);
        claims_created += 1;
        for (fid, _, _) in &bullish {
            g.add_edge(Edge::new(*fid, cid, EdgeKind::DerivedFrom));
            edges_created += 1;
        }
    }

    if bearish.len() >= 2 {
        let symbols: Vec<String> = bearish.iter().map(|(_, s, _)| s.split(':').next().unwrap_or("?").to_string()).collect();
        let avg_ret: f64 = bearish.iter().map(|(_, _, r)| r).sum::<f64>() / bearish.len() as f64;
        let claim = Node::new(NodeKind::Claim,
            format!("Recent 3-month bearish group ({}) avg return {:.1}% — correction phase", symbols.join(", "), avg_ret),
            vec!["bearish".into(), "correction".into()])
            .with_metadata(serde_json::json!({"trigger": "Review on bounce signals", "auto_generated": true}));
        let cid = g.add_node(claim);
        claims_created += 1;
        for (fid, _, _) in &bearish {
            g.add_edge(Edge::new(*fid, cid, EdgeKind::DerivedFrom));
            edges_created += 1;
        }
    }

    let mut sectors: HashMap<String, Vec<(Uuid, String)>> = HashMap::new();
    for (id, stmt, meta) in &facts {
        if let Some(sym) = meta["symbol"].as_str() {
            let sector = guess_sector(sym);
            sectors.entry(sector).or_default().push((*id, stmt.clone()));
        }
    }
    for (sector, members) in &sectors {
        if members.len() >= 2 {
            let symbols: Vec<String> = members.iter().map(|(_, s)| s.split(':').next().unwrap_or("?").to_string()).collect();
            let claim = Node::new(NodeKind::Claim,
                format!("{} sector group: {}", sector, symbols.join(", ")),
                vec![sector.to_lowercase(), "sector".into()])
                .with_metadata(serde_json::json!({"trigger": "Sector composition change", "auto_generated": true, "sector": sector}));
            let cid = g.add_node(claim);
            claims_created += 1;
            for (fid, _) in members {
                g.add_edge(Edge::new(*fid, cid, EdgeKind::DerivedFrom));
                edges_created += 1;
            }
        }
    }

    let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
    let summary = g.summary();

    Json(serde_json::json!({
        "status": "organized",
        "claims_created": claims_created,
        "edges_created": edges_created,
        "total_nodes": summary.n_nodes,
        "total_edges": summary.n_edges,
    }))
}

fn guess_sector(symbol: &str) -> String {
    match symbol {
        "AAPL" | "MSFT" | "GOOGL" | "META" | "NVDA" => "Tech".into(),
        "AMZN" | "TSLA" => "Consumer".into(),
        "JPM" | "BAC" | "GS" => "Finance".into(),
        "KO" | "PG" | "WMT" => "Defensive".into(),
        "XOM" | "CVX" => "Energy".into(),
        s if s.contains("BTC") || s.contains("ETH") || s.contains("SOL") => "Crypto".into(),
        s if s.starts_with("SPY") || s.starts_with("QQQ") || s.starts_with("VOO") || s.starts_with("SCHD") || s.starts_with("TLT") => "ETF".into(),
        _ => "Other".into(),
    }
}

// ── Legacy Claim creation ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateClaim { statement: String, keywords: Vec<String>, trigger: Option<String> }

pub async fn create_claim(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<CreateClaim>,
) -> Json<serde_json::Value> {
    let mut g = state.graph.write().await;
    let node = Node::new(NodeKind::Claim, req.statement, req.keywords)
        .with_metadata(serde_json::json!({"trigger": req.trigger.unwrap_or_default()}));
    let id = g.add_node(node);
    let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));
    Json(serde_json::json!({"status": "created", "id": id}))
}

pub async fn save_graph(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;
    match g.save(std::path::Path::new(GRAPH_DATA_DIR)) {
        Ok(_) => Json(serde_json::json!({"status": "saved"})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// LLM endpoints — use amure-do's LLM provider
// ══════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
pub struct AutoTagReq { node_id: Uuid }

pub async fn llm_auto_tag(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<AutoTagReq>,
) -> Json<serde_json::Value> {
    let stmt = {
        let g = state.graph.read().await;
        match g.get_node(&req.node_id) {
            Some(n) => n.statement.clone(),
            None => return Json(serde_json::json!({"error": "Node not found"})),
        }
    };

    let prompt = format!(
        "Extract key keywords from the following financial data. Mix Korean + English, comma separated, max 10.\n\
        Include sector, industry, investment characteristics, and themes.\n\
        Example: tech, ai, semiconductor, large_cap, growth, gpu\n\n\
        Data: {}\n\nKeywords only:",
        stmt
    );

    let config = state.llm_config.read().await;
    match crate::server::llm_provider::call_llm(&prompt, &config).await {
        Ok(resp) => {
            let keywords: Vec<String> = resp.split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| s.len() >= 2 && s.len() < 30)
                .collect();

            drop(config);
            let mut g = state.graph.write().await;
            if let Some(node) = g.get_node_mut(&req.node_id) {
                for kw in &keywords {
                    if !node.keywords.contains(kw) {
                        node.keywords.push(kw.clone());
                    }
                }
                node.updated_at = chrono::Utc::now();
            }
            let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));

            Json(serde_json::json!({"status": "tagged", "new_keywords": keywords}))
        }
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

pub async fn llm_auto_tag_all(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let fact_ids: Vec<(Uuid, String)> = {
        let g = state.graph.read().await;
        g.nodes.values()
            .filter(|n| n.kind == NodeKind::Fact)
            .map(|n| (n.id, n.statement.clone()))
            .collect()
    };

    let mut tagged = 0;
    for (id, stmt) in &fact_ids {
        let prompt = format!(
            "Extract key keywords from financial data. Korean+English, comma separated, max 10.\n\
            Include sector, industry, investment characteristics, themes.\nData: {}\nKeywords only:", stmt
        );
        let config = state.llm_config.read().await;
        if let Ok(resp) = crate::server::llm_provider::call_llm(&prompt, &config).await {
            let keywords: Vec<String> = resp.split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| s.len() >= 2 && s.len() < 30)
                .collect();
            drop(config);
            let mut g = state.graph.write().await;
            if let Some(node) = g.get_node_mut(id) {
                for kw in &keywords {
                    if !node.keywords.contains(kw) { node.keywords.push(kw.clone()); }
                }
            }
            drop(g);
            tagged += 1;
        }
    }

    let g = state.graph.read().await;
    let _ = g.save(std::path::Path::new(GRAPH_DATA_DIR));

    Json(serde_json::json!({"status": "done", "tagged": tagged, "total": fact_ids.len()}))
}

#[derive(Deserialize)]
pub struct SummarizeReq { query: String, top_k: Option<usize> }

pub async fn llm_summarize_search(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<SummarizeReq>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;
    let results = search(&g, &req.query, &state.synonyms, &SearchOptions {
        top_k: req.top_k.unwrap_or(5), include_failed: true, ..Default::default()
    });
    drop(g);

    if results.is_empty() {
        return Json(serde_json::json!({"error": "No search results"}));
    }

    let context: String = results.iter().enumerate().map(|(i, r)| {
        format!("{}. [{}] {} (score={:.2}, {})", i+1, r.kind, r.statement, r.score,
            if r.failed_path { "FAILED" } else { &r.status })
    }).collect::<Vec<_>>().join("\n");

    let prompt = format!(
        "Summarize the following search results for '{}' in one paragraph (3-5 sentences).\n\
        Include key numbers, trends, and caveats.\n\n{}\n\nSummary:",
        req.query, context
    );

    let config = state.llm_config.read().await;
    match crate::server::llm_provider::call_llm(&prompt, &config).await {
        Ok(summary) => Json(serde_json::json!({
            "query": req.query,
            "summary": summary.trim(),
            "results": results,
            "n_results": results.len(),
        })),
        Err(e) => Json(serde_json::json!({"error": e, "results": results})),
    }
}

pub async fn llm_explain_groups(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let g = state.graph.read().await;

    let claims: Vec<(String, Vec<String>)> = g.nodes.values()
        .filter(|n| n.kind == NodeKind::Claim)
        .map(|claim| {
            let fact_stmts: Vec<String> = g.edges.values()
                .filter(|e| e.target == claim.id)
                .filter_map(|e| g.get_node(&e.source))
                .filter(|n| n.kind == NodeKind::Fact)
                .map(|n| n.statement.clone())
                .collect();
            (claim.statement.clone(), fact_stmts)
        })
        .filter(|(_, facts)| !facts.is_empty())
        .collect();
    drop(g);

    if claims.is_empty() {
        return Json(serde_json::json!({"error": "No claims with facts to explain"}));
    }

    let mut explanations = Vec::new();
    for (claim_stmt, facts) in &claims {
        let facts_text = facts.iter().enumerate()
            .map(|(i, f)| format!("  {}. {}", i+1, f))
            .collect::<Vec<_>>().join("\n");

        let prompt = format!(
            "Explain why the following assets/data are grouped together in 2-3 sentences.\n\
            Focus on common economic mechanisms, industry trends, and macro factors.\n\n\
            Group claim: {}\nIncluded data:\n{}\n\nEconomic rationale:",
            claim_stmt, facts_text
        );

        let config = state.llm_config.read().await;
        match crate::server::llm_provider::call_llm(&prompt, &config).await {
            Ok(explanation) => {
                explanations.push(serde_json::json!({
                    "claim": claim_stmt,
                    "n_facts": facts.len(),
                    "explanation": explanation.trim(),
                }));
            }
            Err(e) => {
                explanations.push(serde_json::json!({
                    "claim": claim_stmt,
                    "error": e,
                }));
            }
        }
    }

    Json(serde_json::json!({"explanations": explanations, "n_groups": explanations.len()}))
}

#[derive(Deserialize)]
pub struct VerifyReq { claim_id: Uuid }

pub async fn llm_verify_claim(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<VerifyReq>,
) -> Json<serde_json::Value> {
    let (claim_stmt, facts, keywords) = {
        let g = state.graph.read().await;
        let claim = match g.get_node(&req.claim_id) {
            Some(n) if n.kind == NodeKind::Claim => n,
            _ => return Json(serde_json::json!({"error": "Claim not found"})),
        };
        let fact_stmts: Vec<String> = g.edges.values()
            .filter(|e| e.target == req.claim_id)
            .filter_map(|e| g.get_node(&e.source))
            .map(|n| n.statement.clone())
            .collect();
        (claim.statement.clone(), fact_stmts, claim.keywords.clone())
    };

    let prompt = format!(
        "Evaluate the logical validity of the following investment claim.\n\n\
        Claim: {}\nKeywords: {}\nSupporting data:\n{}\n\n\
        Answer in this format:\n\
        Validity: (High/Medium/Low)\n\
        Strengths: (1-2 lines)\n\
        Weaknesses: (1-2 lines)\n\
        Improvement suggestions: (1-2 lines)\n\
        Caveats: (1 line)",
        claim_stmt,
        keywords.join(", "),
        facts.iter().enumerate().map(|(i,f)| format!("  {}. {}", i+1, f)).collect::<Vec<_>>().join("\n")
    );

    let config = state.llm_config.read().await;
    match crate::server::llm_provider::call_llm(&prompt, &config).await {
        Ok(assessment) => Json(serde_json::json!({
            "claim": claim_stmt,
            "assessment": assessment.trim(),
            "n_supporting_facts": facts.len(),
        })),
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn node_json(n: &Node) -> serde_json::Value {
    serde_json::json!({
        "id": n.id, "kind": format!("{:?}", n.kind), "statement": n.statement,
        "keywords": n.keywords, "status": format!("{:?}", n.status),
        "failed": n.is_failed(), "metadata": n.metadata,
        "created_at": n.created_at.to_rfc3339(),
        "updated_at": n.updated_at.to_rfc3339(),
    })
}

fn edge_json(e: &Edge) -> serde_json::Value {
    serde_json::json!({
        "id": e.id, "source": e.source, "target": e.target,
        "kind": format!("{:?}", e.kind), "weight": e.weight, "note": e.note,
    })
}

fn parse_node_kind(s: &str) -> NodeKind {
    match s {
        "Claim" | "claim" => NodeKind::Claim,
        "Reason" | "reason" => NodeKind::Reason,
        "Evidence" | "evidence" => NodeKind::Evidence,
        "Experiment" | "experiment" => NodeKind::Experiment,
        "Fact" | "fact" => NodeKind::Fact,
        _ => NodeKind::Claim,
    }
}

fn parse_node_status(s: &str) -> NodeStatus {
    match s {
        "Draft" | "draft" => NodeStatus::Draft,
        "Active" | "active" => NodeStatus::Active,
        "Accepted" | "accepted" => NodeStatus::Accepted,
        "Rejected" | "rejected" => NodeStatus::Rejected,
        "Weakened" | "weakened" => NodeStatus::Weakened,
        _ => NodeStatus::Draft,
    }
}

fn parse_edge_kind(s: &str) -> EdgeKind {
    match s {
        "support" | "Support" => EdgeKind::Support,
        "rebut" | "Rebut" => EdgeKind::Rebut,
        "depends_on" | "DependsOn" => EdgeKind::DependsOn,
        "contradicts" | "Contradicts" => EdgeKind::Contradicts,
        "refines" | "Refines" => EdgeKind::Refines,
        _ => EdgeKind::DerivedFrom,
    }
}
