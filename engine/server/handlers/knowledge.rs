use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::knowledge::types::*;
use crate::server::routes::AppState;

// ── Hypothesis ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateHypothesis {
    pub title: String,
    pub economic_rationale: String,
}

pub async fn create_hypothesis(
    State(state): State<AppState>,
    Json(req): Json<CreateHypothesis>,
) -> Json<serde_json::Value> {
    let mut kb = state.knowledge.write().await;
    let h = Hypothesis::new(req.title, req.economic_rationale);
    let id = kb.add_hypothesis(h);
    let _ = kb.save();
    Json(serde_json::json!({"id": id, "status": "created"}))
}

pub async fn list_hypotheses(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;
    let list: Vec<serde_json::Value> = kb
        .list_hypotheses()
        .iter()
        .map(|h| {
            serde_json::json!({
                "id": h.id,
                "title": h.title,
                "status": h.status,
                "maturity": h.maturity,
                "n_experiments": h.experiment_ids.len(),
                "created_at": h.created_at.to_rfc3339(),
            })
        })
        .collect();
    Json(serde_json::json!({"hypotheses": list}))
}

pub async fn get_hypothesis(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;
    match kb.get_hypothesis(&id) {
        Some(h) => {
            let experiments: Vec<serde_json::Value> = kb
                .experiments_for_hypothesis(&id)
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id,
                        "description": e.description,
                        "status": e.status,
                        "n_insights": e.insight_ids.len(),
                        "results": e.results,
                    })
                })
                .collect();
            Json(serde_json::json!({
                "hypothesis": h,
                "experiments": experiments,
            }))
        }
        None => Json(serde_json::json!({"error": "Not found"})),
    }
}

// ── Insight ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateInsight {
    pub experiment_id: Uuid,
    pub text: String,
    pub evidence: String,
    pub tags: Vec<String>,
}

pub async fn create_insight(
    State(state): State<AppState>,
    Json(req): Json<CreateInsight>,
) -> Json<serde_json::Value> {
    let mut kb = state.knowledge.write().await;
    let ins = Insight::new(req.experiment_id, req.text, req.evidence, req.tags);
    let id = kb.add_insight(ins);
    let _ = kb.save();
    Json(serde_json::json!({"id": id, "status": "pending"}))
}

/// Auto-suggest accept/reject reasons based on evidence + tags.
fn suggest_reasons(text: &str, evidence: &str, tags: &[String]) -> (Vec<String>, Vec<String>) {
    let mut accept = Vec::new();
    let mut reject = Vec::new();

    // Check if has economic mechanism
    let lower = text.to_lowercase();
    if lower.contains("때문") || lower.contains("구조") || lower.contains("mechanism") || lower.contains("cascade") {
        accept.push("경제적 메커니즘이 명시되어 있음".into());
    } else {
        reject.push("경제적 메커니즘 불분명 — 왜 이 현상이 발생하는지 설명 부족".into());
    }

    // Check validity
    if evidence.contains("regime") || evidence.contains("레짐") || evidence.contains("bull") {
        accept.push("레짐별 조건이 명시되어 있음".into());
    } else {
        reject.push("레짐별 유효성 미검증 — 어떤 시장 상황에서 통하는지 불분명".into());
    }

    // Check if numbers-only
    if (lower.contains("mean=") || lower.contains("bp")) && !lower.contains("때문") && !lower.contains("구조") {
        reject.push("숫자만 나열 — 경제적 논리 없이 통계 결과만으로는 채택 불가".into());
    }

    // Check for cross-asset consistency
    if tags.contains(&"cross-asset".to_string()) || tags.contains(&"structural".to_string()) {
        accept.push("cross-asset 비교를 통한 구조적 insight".into());
    }

    // Check for counter-argument
    if tags.contains(&"argument-against".to_string()) {
        accept.push("반론을 명시적으로 제시 — 균형잡힌 분석".into());
    }

    // Default fallbacks
    if accept.is_empty() { accept.push("추가 검토 후 판단 필요".into()); }
    if reject.is_empty() { reject.push("추가 실험/검증 필요".into()); }

    (accept, reject)
}

pub async fn list_pending_insights(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;
    let pending: Vec<serde_json::Value> = kb
        .pending_insights()
        .iter()
        .map(|i| {
            let (accept_reasons, reject_reasons) = suggest_reasons(&i.text, &i.evidence, &i.tags);
            serde_json::json!({
                "id": i.id,
                "text": i.text,
                "evidence": i.evidence,
                "tags": i.tags,
                "experiment_id": i.experiment_id,
                "created_at": i.created_at.to_rfc3339(),
                "suggested_accept_reasons": accept_reasons,
                "suggested_reject_reasons": reject_reasons,
            })
        })
        .collect();
    Json(serde_json::json!({"pending": pending, "count": pending.len()}))
}

#[derive(Deserialize)]
pub struct VerdictRequest {
    pub reason: String,
}

pub async fn accept_insight(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<VerdictRequest>,
) -> Json<serde_json::Value> {
    let mut kb = state.knowledge.write().await;
    if kb.accept_insight(&id, req.reason) {
        let _ = kb.save();
        Json(serde_json::json!({"status": "accepted", "id": id}))
    } else {
        Json(serde_json::json!({"error": "Insight not found"}))
    }
}

pub async fn reject_insight(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<VerdictRequest>,
) -> Json<serde_json::Value> {
    let mut kb = state.knowledge.write().await;
    if kb.reject_insight(&id, req.reason) {
        let _ = kb.save();
        Json(serde_json::json!({"status": "rejected", "id": id}))
    } else {
        Json(serde_json::json!({"error": "Insight not found"}))
    }
}

pub async fn promote_insight(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let mut kb = state.knowledge.write().await;
    if kb.promote_insight(&id) {
        let _ = kb.save();
        Json(serde_json::json!({"status": "promoted to mature", "id": id}))
    } else {
        Json(serde_json::json!({"error": "Insight not found"}))
    }
}

// ── Search ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub top_k: Option<usize>,
}

pub async fn search_insights(
    State(state): State<AppState>,
    Json(req): Json<SearchQuery>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;
    let results = kb.search_by_text(&req.text, req.top_k.unwrap_or(10));
    Json(serde_json::json!({"results": results, "count": results.len()}))
}
