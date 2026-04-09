/// Framework API — Claim/Reason CRUD + Gate-enforced accept.
/// Claim이 Knowledge로 격상되려면 Gate를 통과해야 한다.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::knowledge::framework::{self, *};
use crate::server::llm_provider::{LlmRouting, LlmConfig};
use crate::server::routes::AppState;

/// UTF-8 safe truncation for Korean/mixed text
fn truncate_str(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

// ── Claim ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateClaim {
    pub statement: String,
    pub trigger: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

pub async fn create_claim(
    State(state): State<AppState>,
    Json(req): Json<CreateClaim>,
) -> Json<serde_json::Value> {
    // Gate: claim 생성 시점부터 강제
    let gate = claim_gate(&req.statement, &req.trigger);
    if !gate.passed {
        return Json(serde_json::json!({
            "error": "CLAIM_GATE 실패",
            "gate": gate,
        }));
    }

    let (id, stmt) = {
        let mut kb = state.knowledge.write().await;
        let stmt = req.statement.clone();
        let claim = Claim::new(req.statement, req.trigger).with_keywords(req.keywords);
        let id = kb.add_claim(claim);
        let _ = kb.save();
        (id, stmt)
    };

    { let mut log = state.activity.write().await;
      log.push("claim", "created", &format!("Claim 생성: {}...", &truncate_str(&stmt, 50)), None); }

    Json(serde_json::json!({
        "id": id,
        "status": "draft",
        "gate": gate,
    }))
}

pub async fn list_claims(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;

    let drafts: Vec<serde_json::Value> = kb.draft_claims().iter().map(|c| {
        let reasons = kb.reasons_for_claim(&c.id);
        let n_support = reasons.iter().filter(|r| r.reason_type == ReasonType::Support).count();
        let n_rebut = reasons.iter().filter(|r| r.reason_type == ReasonType::Rebut).count();

        // 작업 상태: 각 reason의 experiment 상태 집계
        let all_exps: Vec<_> = reasons.iter()
            .flat_map(|r| kb.experiments_for_reason(&r.id))
            .collect();
        let n_designed = all_exps.iter().filter(|e| e.status == ExperimentStatus::Designed).count();
        let n_running = all_exps.iter().filter(|e| e.status == ExperimentStatus::Running).count();
        let n_completed = all_exps.iter().filter(|e| e.status == ExperimentStatus::Completed).count();
        let n_interpreted = all_exps.iter().filter(|e| e.status == ExperimentStatus::Interpreted).count();

        let work_status = if n_running > 0 { "running" }
            else if n_completed > 0 { "needs_verdict" }
            else if n_designed > 0 { "designed" }
            else if n_interpreted > 0 && n_designed == 0 { "done" }
            else { "idle" };

        serde_json::json!({
            "id": c.id,
            "statement": c.statement,
            "trigger": c.trigger,
            "keywords": c.keywords,
            "status": c.status,
            "work_status": work_status,
            "n_support": n_support,
            "n_rebut": n_rebut,
            "n_reasons": reasons.len(),
            "n_experiments": all_exps.len(),
            "experiments_summary": {
                "designed": n_designed,
                "running": n_running,
                "completed": n_completed,
                "interpreted": n_interpreted,
            },
            "updated_at": c.updated_at.to_rfc3339(),
        })
    }).collect();

    let knowledge: Vec<serde_json::Value> = kb.knowledge().iter().map(|c| {
        serde_json::json!({
            "id": c.id,
            "statement": c.statement,
            "trigger": c.trigger,
            "status": c.status,
            "accept_reason": c.accept_reason,
            "accepted_at": c.accepted_at.map(|t| t.to_rfc3339()),
        })
    }).collect();

    Json(serde_json::json!({
        "drafts": drafts,
        "knowledge": knowledge,
        "n_drafts": drafts.len(),
        "n_knowledge": knowledge.len(),
    }))
}

pub async fn get_claim(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;
    match kb.get_claim(&id) {
        Some(claim) => {
            let reasons: Vec<serde_json::Value> = kb.reasons_for_claim(&id).iter().map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "reason_type": r.reason_type,
                    "statement": r.statement,
                    "bridge": r.bridge,
                    "evidences": r.evidences,
                    "relations": r.relations,
                    "sub_claim_id": r.sub_claim_id,
                    "created_at": r.created_at.to_rfc3339(),
                })
            }).collect();

            // Structural check
            let n_support = reasons.iter().filter(|r| r["reason_type"] == "Support").count();
            let n_rebut = reasons.iter().filter(|r| r["reason_type"] == "Rebut").count();
            let n_evidence: usize = kb.reasons_for_claim(&id).iter()
                .map(|r| r.evidences.len()).sum();
            let n_correlated = kb.reasons_for_claim(&id).iter()
                .flat_map(|r| &r.relations)
                .filter(|rel| rel.relation_type == RelationType::Correlated).count();
            let n_independent = kb.reasons_for_claim(&id).iter()
                .flat_map(|r| &r.relations)
                .filter(|rel| rel.relation_type == RelationType::Independent).count();

            let check = structural_check(
                n_support, n_rebut, n_evidence,
                !claim.trigger.is_empty(),
                n_correlated, n_independent,
            );

            Json(serde_json::json!({
                "claim": claim,
                "reasons": reasons,
                "structural_check": check,
                "ready_to_accept": check.passed,
            }))
        }
        None => Json(serde_json::json!({"error": "Claim not found"})),
    }
}

// ── Accept / Reject ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct VerdictRequest {
    pub reason: String,
}

/// Gate를 모두 통과해야만 accept 가능
pub async fn accept_claim(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<VerdictRequest>,
) -> Json<serde_json::Value> {
    let mut kb = state.knowledge.write().await;

    let Some(claim) = kb.get_claim(&id) else {
        return Json(serde_json::json!({"error": "Claim not found"}));
    };

    // Gate 1: Claim itself
    let claim_g = claim_gate(&claim.statement, &claim.trigger);
    if !claim_g.passed {
        return Json(serde_json::json!({"error": "CLAIM_GATE 실패", "gate": claim_g}));
    }

    // Collect reason stats
    let reasons = kb.reasons_for_claim(&id);
    let n_support = reasons.iter().filter(|r| r.reason_type == ReasonType::Support).count();
    let n_rebut = reasons.iter().filter(|r| r.reason_type == ReasonType::Rebut).count();
    let n_evidence: usize = reasons.iter().map(|r| r.evidences.len()).sum();
    let n_correlated = reasons.iter()
        .flat_map(|r| &r.relations)
        .filter(|rel| rel.relation_type == RelationType::Correlated).count();
    let n_independent = reasons.iter()
        .flat_map(|r| &r.relations)
        .filter(|rel| rel.relation_type == RelationType::Independent).count();

    // Gate 2: Structural check
    let struct_g = structural_check(
        n_support, n_rebut, n_evidence,
        !claim.trigger.is_empty(),
        n_correlated, n_independent,
    );
    if !struct_g.passed {
        return Json(serde_json::json!({
            "error": "STRUCTURAL_CHECK 실패 — 근거 부족",
            "gate": struct_g,
            "hint": "support Reason + Evidence를 추가하세요",
        }));
    }

    // Gate 3: 각 Reason의 bridge 검증
    let mut reason_errors = Vec::new();
    for r in &reasons {
        let rtype = match r.reason_type {
            ReasonType::Support => "support",
            ReasonType::Rebut => "rebut",
        };
        let rg = reason_gate(&r.statement, &r.bridge, rtype);
        if !rg.passed {
            reason_errors.push(serde_json::json!({
                "reason_id": r.id,
                "gate": rg,
            }));
        }
    }
    if !reason_errors.is_empty() {
        return Json(serde_json::json!({
            "error": "REASON_GATE 실패 — bridge 누락",
            "failed_reasons": reason_errors,
        }));
    }

    // All gates passed → Knowledge로 격상
    let stmt = kb.get_claim(&id).map(|c| c.statement.clone()).unwrap_or_default();
    kb.accept_claim(&id, req.reason);
    let _ = kb.save();
    drop(kb);

    { let mut log = state.activity.write().await;
      log.push("claim", "accepted", &format!("Knowledge 격상: {}...", &truncate_str(&stmt, 50)), Some("3중 Gate 통과".into())); }

    Json(serde_json::json!({
        "status": "accepted",
        "id": id,
        "message": "Knowledge로 격상 완료",
        "gates_passed": ["CLAIM_GATE", "STRUCTURAL_CHECK", "REASON_GATE"],
        "warnings": struct_g.warnings,
    }))
}

pub async fn delete_claim(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let mut kb = state.knowledge.write().await;
    let stmt = kb.get_claim(&id).map(|c| truncate_str(&c.statement, 30)).unwrap_or_default();
    if kb.delete_claim(&id) {
        let _ = kb.save();
        drop(kb);
        { let mut log = state.activity.write().await;
          log.push("claim", "deleted", &format!("Claim 삭제: {}...", stmt), None); }
        Json(serde_json::json!({"status": "deleted", "id": id}))
    } else {
        Json(serde_json::json!({"error": "Claim not found"}))
    }
}

pub async fn reject_claim(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<VerdictRequest>,
) -> Json<serde_json::Value> {
    let mut kb = state.knowledge.write().await;
    if kb.reject_claim(&id, req.reason) {
        let _ = kb.save();
        Json(serde_json::json!({"status": "rejected", "id": id}))
    } else {
        Json(serde_json::json!({"error": "Claim not found"}))
    }
}

// ── Reason ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateReason {
    pub claim_id: Uuid,
    pub reason_type: String,  // "support" or "rebut"
    pub statement: String,
    pub bridge: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

pub async fn create_reason(
    State(state): State<AppState>,
    Json(req): Json<CreateReason>,
) -> Json<serde_json::Value> {
    // Gate: reason 생성 시점부터 강제
    let gate = reason_gate(&req.statement, &req.bridge, &req.reason_type);
    if !gate.passed {
        return Json(serde_json::json!({
            "error": "REASON_GATE 실패",
            "gate": gate,
        }));
    }

    let rtype = match req.reason_type.as_str() {
        "rebut" => ReasonType::Rebut,
        _ => ReasonType::Support,
    };

    let (id, rtype_str, stmt) = {
        let mut kb = state.knowledge.write().await;
        let rtype_str = req.reason_type.clone();
        let stmt = req.statement.clone();
        let reason = Reason::new(req.claim_id, rtype, req.statement, req.bridge).with_keywords(req.keywords);
        let id = kb.add_reason(reason);
        let _ = kb.save();
        (id, rtype_str, stmt)
    };

    { let mut log = state.activity.write().await;
      log.push("reason", "created", &format!("Reason [{}]: {}...", rtype_str, &stmt[..stmt.len().min(40)]), None); }

    Json(serde_json::json!({
        "id": id,
        "status": "created",
        "gate": gate,
    }))
}

// ── Evidence ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddEvidence {
    pub reason_id: Uuid,
    pub tag: String,        // "backtest", "live", "intuition", "literature"
    pub description: String,
}

pub async fn add_evidence(
    State(state): State<AppState>,
    Json(req): Json<AddEvidence>,
) -> Json<serde_json::Value> {
    // Gate
    let gate = evidence_gate(&req.description, &req.tag);
    if !gate.passed {
        return Json(serde_json::json!({"error": "EVIDENCE_GATE 실패", "gate": gate}));
    }

    let tag = match req.tag.as_str() {
        "live" => EvidenceTag::Live,
        "intuition" => EvidenceTag::Intuition,
        "literature" => EvidenceTag::Literature,
        _ => EvidenceTag::Backtest,
    };

    let mut kb = state.knowledge.write().await;
    if let Some(reason) = kb.get_reason_mut(&req.reason_id) {
        let ev = Evidence::new(tag, req.description);
        let id = ev.id;
        reason.evidences.push(ev);
        let _ = kb.save();
        Json(serde_json::json!({"id": id, "status": "added", "gate": gate}))
    } else {
        Json(serde_json::json!({"error": "Reason not found"}))
    }
}

// ── Relation ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddRelation {
    pub reason_id: Uuid,
    pub other_reason_id: Uuid,
    pub relation_type: String, // "independent", "correlated", "conditional"
    pub note: String,
}

pub async fn add_relation(
    State(state): State<AppState>,
    Json(req): Json<AddRelation>,
) -> Json<serde_json::Value> {
    let gate = relation_gate(&req.relation_type, &req.note);
    if !gate.passed {
        return Json(serde_json::json!({"error": "RELATION_GATE 실패", "gate": gate}));
    }

    let rtype = match req.relation_type.as_str() {
        "correlated" => RelationType::Correlated,
        "conditional" => RelationType::Conditional,
        _ => RelationType::Independent,
    };

    let mut kb = state.knowledge.write().await;
    if let Some(reason) = kb.get_reason_mut(&req.reason_id) {
        reason.relations.push(Relation {
            other_reason_id: req.other_reason_id,
            relation_type: rtype,
            note: req.note,
        });
        let _ = kb.save();
        Json(serde_json::json!({"status": "added", "gate": gate}))
    } else {
        Json(serde_json::json!({"error": "Reason not found"}))
    }
}

// ── Experiment ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateExperiment {
    pub reason_id: Uuid,
    pub method: String,    // "distributional", "conditional", "cross_sectional", etc.
    pub description: String,
    pub if_true: String,
    pub if_false: String,
    pub expected_output: String,
    pub config: serde_json::Value,
}

pub async fn create_experiment(
    State(state): State<AppState>,
    Json(req): Json<CreateExperiment>,
) -> Json<serde_json::Value> {
    // 선행 분석 유무 확인
    let kb = state.knowledge.read().await;
    let has_prior = kb.experiments_for_reason(&req.reason_id).iter().any(|e| {
        !matches!(e.method, ExperimentMethod::Backtest | ExperimentMethod::EntryExit)
    });
    drop(kb);

    let gate = framework::experiment_gate(
        &req.method, &req.description, &req.if_true, &req.if_false,
        &req.expected_output, &req.config, has_prior,
    );
    if !gate.passed {
        return Json(serde_json::json!({"error": "EXPERIMENT_GATE 실패", "gate": gate}));
    }

    let method = match req.method.as_str() {
        "distributional" => ExperimentMethod::Distributional,
        "conditional" => ExperimentMethod::Conditional,
        "cross_sectional" => ExperimentMethod::CrossSectional,
        "dose_response" => ExperimentMethod::DoseResponse,
        "regime" => ExperimentMethod::Regime,
        "temporal" => ExperimentMethod::Temporal,
        "multi_horizon" => ExperimentMethod::MultiHorizon,
        "entry_exit" => ExperimentMethod::EntryExit,
        "backtest" => ExperimentMethod::Backtest,
        _ => ExperimentMethod::CrossSectional,
    };

    let (id, method_str, desc) = {
        let mut kb = state.knowledge.write().await;
        let method_str = req.method.clone();
        let desc = req.description.clone();
        let exp = Experiment::new(
            req.reason_id, method, req.description,
            req.if_true, req.if_false, req.expected_output, req.config,
        );
        let id = kb.add_fw_experiment(exp);
        let _ = kb.save();
        (id, method_str, desc)
    };

    { let mut log = state.activity.write().await;
      log.push("experiment", "created", &format!("[{}] {}...", method_str, &truncate_str(&desc, 40)), None); }

    Json(serde_json::json!({"id": id, "status": "designed", "method": method_str, "gate": gate}))
}

pub async fn list_experiments(
    State(state): State<AppState>,
    Path(reason_id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;
    let exps: Vec<serde_json::Value> = kb.experiments_for_reason(&reason_id).iter().map(|e| {
        serde_json::json!({
            "id": e.id,
            "method": e.method,
            "description": e.description,
            "if_true": e.if_true,
            "if_false": e.if_false,
            "expected_output": e.expected_output,
            "status": e.status,
            "result": e.result,
            "verdict": e.verdict,
            "evidence_id": e.evidence_id,
        })
    }).collect();
    Json(serde_json::json!({"experiments": exps, "count": exps.len()}))
}

#[derive(Deserialize)]
pub struct SubmitResult {
    pub result: serde_json::Value,
}

/// 실험 결과 제출 (Julia에서 실행 후)
pub async fn submit_result(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<SubmitResult>,
) -> Json<serde_json::Value> {
    let mut kb = state.knowledge.write().await;
    if let Some(exp) = kb.get_fw_experiment_mut(&id) {
        exp.result = Some(req.result);
        exp.status = ExperimentStatus::Completed;
        let _ = kb.save();
        Json(serde_json::json!({"status": "completed", "id": id, "message": "verdict를 제출하세요"}))
    } else {
        Json(serde_json::json!({"error": "Experiment not found"}))
    }
}

#[derive(Deserialize)]
pub struct SubmitVerdict {
    pub supports_reason: bool,
    pub explanation: String,
    pub gaps: Vec<String>,
    pub validity: framework::VerdictValidity,
}

/// Verdict 제출 → Evidence 자동 생성 → Reason에 첨부
pub async fn submit_verdict(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<SubmitVerdict>,
) -> Json<serde_json::Value> {
    let mut kb = state.knowledge.write().await;

    // DSR check: 같은 Reason에 실험이 몇 개인지
    let n_on_reason = if let Some(exp) = kb.get_fw_experiment(&id) {
        kb.experiments_for_reason(&exp.reason_id).len()
    } else {
        return Json(serde_json::json!({"error": "Experiment not found"}));
    };

    let gate = framework::verdict_gate(&req.explanation, req.supports_reason, &req.gaps, &req.validity, n_on_reason);
    if !gate.passed {
        return Json(serde_json::json!({"error": "VERDICT_GATE 실패", "gate": gate}));
    }

    let verdict = ExperimentVerdict {
        supports_reason: req.supports_reason,
        explanation: req.explanation,
        gaps: req.gaps,
        validity: req.validity,
    };

    let supports = req.supports_reason;
    let n_gaps = verdict.gaps.len();
    let result = kb.verdict_experiment(&id, verdict);
    let _ = kb.save();
    drop(kb);

    if let Some(ev_id) = result {
        { let mut log = state.activity.write().await;
          log.push("verdict", if supports {"supported"} else {"weakened"},
            &format!("Verdict: {} → Evidence 생성", if supports {"지지"} else {"약화"}),
            Some(format!("gaps: {}", n_gaps))); }

        Json(serde_json::json!({
            "status": "interpreted",
            "evidence_id": ev_id,
            "message": "Evidence가 Reason에 자동 첨부됨",
            "gate": gate,
        }))
    } else {
        Json(serde_json::json!({"error": "Experiment not found"}))
    }
}

// ── Batch Run ──────────────────────────────────────────────────────────────

/// Claim 내 모든 Designed 실험을 Running으로 전환 + Julia 실행
pub async fn run_all_experiments(
    State(state): State<AppState>,
    Path(claim_id): Path<Uuid>,
) -> Json<serde_json::Value> {
    // 1. 실험 목록 수집 + Running 전환
    let experiments: Vec<(Uuid, String, String, String)> = {
        let mut kb = state.knowledge.write().await;
        let reasons = kb.reasons_for_claim(&claim_id);
        let reason_ids: Vec<Uuid> = reasons.iter().map(|r| r.id).collect();

        let mut exps = Vec::new();
        for rid in &reason_ids {
            let exp_ids: Vec<Uuid> = kb.experiments_for_reason(rid)
                .iter()
                .filter(|e| e.status == ExperimentStatus::Designed)
                .map(|e| e.id)
                .collect();
            for eid in exp_ids {
                if let Some(exp) = kb.get_fw_experiment_mut(&eid) {
                    exp.status = ExperimentStatus::Running;
                    let method = format!("{:?}", exp.method);
                    let desc = exp.description.clone();
                    let config = serde_json::to_string(&exp.config).unwrap_or_default();
                    exps.push((eid, method, desc, config));
                }
            }
        }
        let _ = kb.save();
        exps
    };

    let n = experiments.len();
    if n == 0 {
        return Json(serde_json::json!({"status": "no_experiments", "n_started": 0}));
    }

    { let mut log = state.activity.write().await;
      log.push("experiment", "batch_run", &format!("{}개 실험 일괄 실행 시작", n), None); }

    // 2. 비동기로 각 실험을 Julia에서 실행
    let state_clone = state.clone();
    let started_ids: Vec<Uuid> = experiments.iter().map(|(id, _, _, _)| *id).collect();

    tokio::spawn(async move {
        for (i, (exp_id, method, desc, _config)) in experiments.iter().enumerate() {
            // Activity: 실험 시작
            { let mut log = state_clone.activity.write().await;
              log.push("experiment", "running",
                &format!("[{}/{}] {} — {}...", i+1, n, method, &truncate_str(desc, 30)),
                Some(format!("id: {}", &exp_id.to_string()[..8]))); }

            // Julia 코드 생성 — 실험 description을 LLM에 보내서 코드 생성하거나,
            // 간단한 분석은 직접 Julia 코드 생성
            let julia_code = build_experiment_julia_code(desc, method);

            // Julia 실행
            let result = run_julia_for_experiment(&julia_code).await;

            // 결과 저장 — __RESULT_JSON__ 마커에서 구조화된 결과 추출
            let (status_str, output) = match &result {
                Ok(output) => ("completed", output.clone()),
                Err(e) => ("failed", e.clone()),
            };

            { let mut kb = state_clone.knowledge.write().await;
              if let Some(exp) = kb.get_fw_experiment_mut(exp_id) {
                  match &result {
                      Ok(out) => {
                          // _result.json에서 직접 파싱, 실패하면 raw
                          let structured: Option<serde_json::Value> = serde_json::from_str(out).ok();
                          exp.result = Some(structured.unwrap_or(serde_json::json!({"raw_output": out})));
                          exp.status = ExperimentStatus::Completed;
                      }
                      Err(e) => {
                          exp.result = Some(serde_json::json!({"error": e}));
                          exp.status = ExperimentStatus::Designed;
                      }
                  }
              }
              let _ = kb.save();
            }

            // Activity: 완료/실패
            { let mut log = state_clone.activity.write().await;
              log.push("experiment", status_str,
                &format!("[{}/{}] {} — {}",
                    i+1, n, method,
                    &truncate_str(&output, 60)),
                Some(format!("id: {}", &exp_id.to_string()[..8]))); }
        }

        // 전체 완료
        { let mut log = state_clone.activity.write().await;
          log.push("experiment", "batch_done", &format!("{}개 실험 일괄 실행 완료", n), None); }
    });

    Json(serde_json::json!({
        "status": "running",
        "started": started_ids,
        "n_started": n,
        "message": "실험이 백그라운드에서 실행 중 — Activity에서 진행 확인",
    }))
}

/// 실험 method에서 최적화된 Julia 코드 생성.
/// experiment_ops.jl의 함수를 호출 + emit_* 으로 표준 JSON 결과 출력.
fn build_experiment_julia_code(description: &str, method: &str) -> String {
    let desc = description.replace('"', "\\\"").replace('\n', " ");
    match method {
        "CrossSectional" => format!(r#"
println("=== Experiment: Cross-sectional ===")
println("{desc}")
let T = size(fut_close.data, 1), S = size(fut_close.data, 2)
    signal = Matrix{{Float64}}(undef, T, S)
    fwd = Matrix{{Float64}}(undef, T, S)
    calc_delta!(signal, fut_oi.data, 288)
    calc_fwd_return!(fwd, fut_close.data, 12)
    result = cs_ic_series(signal, fwd; step=12, min_symbols=20)
    s = ic_summary(result.ic_values)
    emit_ic_result("cross_sectional", s; details="{desc}")
end
"#),
        "Distributional" => format!(r#"
println("=== Experiment: Distributional ===")
println("{desc}")
let s = distributional_summary(fut_oi.data; lookback=288)
    emit_distributional_result(s)
end
"#),
        "MultiHorizon" => format!(r#"
println("=== Experiment: Multi-horizon IC decay ===")
println("{desc}")
let T = size(fut_close.data, 1), S = size(fut_close.data, 2)
    signal = Matrix{{Float64}}(undef, T, S)
    calc_delta!(signal, fut_oi.data, 288)
    horizons = [1, 3, 6, 12, 24, 48, 96, 288]
    decay = ic_decay(signal, fut_close.data, horizons; step=12, min_symbols=20)
    emit_decay_result(decay)
end
"#),
        "Regime" => format!(r#"
println("=== Experiment: Regime-conditional IC ===")
println("{desc}")
let T = size(fut_close.data, 1), S = size(fut_close.data, 2)
    signal = Matrix{{Float64}}(undef, T, S)
    fwd = Matrix{{Float64}}(undef, T, S)
    calc_delta!(signal, fut_oi.data, 288)
    calc_fwd_return!(fwd, fut_close.data, 12)
    btc = @view fut_close.data[:, BTC_IDX]
    results = regime_ic(signal, fwd, btc; step=12, min_symbols=20)
    emit_regime_result(results)
end
"#),
        "Conditional" => format!(r#"
println("=== Experiment: Conditional ===")
println("{desc}")
let T = size(fut_close.data, 1), S = size(fut_close.data, 2)
    signal = Matrix{{Float64}}(undef, T, S)
    fwd = Matrix{{Float64}}(undef, T, S)
    cond = Matrix{{Float64}}(undef, T, S)
    calc_delta!(signal, fut_oi.data, 288)
    calc_fwd_return!(fwd, fut_close.data, 12)
    calc_delta!(cond, fut_vol.data, 288)  # volume delta as condition
    results = conditional_ic(signal, fwd, cond; step=12, min_symbols=20)
    emit_conditional_result(results)
end
"#),
        "DoseResponse" => format!(r#"
println("=== Experiment: Dose-response ===")
println("{desc}")
let T = size(fut_close.data, 1), S = size(fut_close.data, 2)
    signal = Matrix{{Float64}}(undef, T, S)
    fwd = Matrix{{Float64}}(undef, T, S)
    calc_delta!(signal, fut_oi.data, 288)
    calc_fwd_return!(fwd, fut_close.data, 12)
    results = dose_response(signal, fwd; n_bins=5, min_symbols=20)
    emit_dose_response_result(results)
end
"#),
        "Temporal" => format!(r#"
println("=== Experiment: Temporal stability ===")
println("{desc}")
let T = size(fut_close.data, 1), S = size(fut_close.data, 2)
    signal = Matrix{{Float64}}(undef, T, S)
    fwd = Matrix{{Float64}}(undef, T, S)
    calc_delta!(signal, fut_oi.data, 288)
    calc_fwd_return!(fwd, fut_close.data, 12)
    results = temporal_stability(signal, fwd, fut_close.timestamps; window_days=90, step_days=30, bar_step=12, min_symbols=20)
    emit_temporal_result(results)
end
"#),
        _ => format!(r#"
println("=== Experiment: {method} ===")
println("{desc}")
println("이 method에 대한 자동 코드가 없습니다. Lab에서 수동 실행하세요.")
emit_result("{method}", (;status="manual_required"), "수동 실행 필요", "Lab에서 직접 코드 작성")
"#),
    }
}

/// Julia 서버에 코드를 보내고 결과를 받음.
/// 결과는 analysis/_result.json 파일에서 읽음 (log binary 깨짐 방지).
async fn run_julia_for_experiment(code: &str) -> Result<String, String> {
    let dir = std::path::PathBuf::from("analysis");
    let cmd_file = dir.join("_cmd.jl");
    let out_file = dir.join("_out.txt");
    let result_file = dir.join("_result.json");

    if !dir.join("_ready").exists() {
        return Err("Julia server not running".into());
    }

    let _ = std::fs::remove_file(&out_file);
    let _ = std::fs::remove_file(&result_file);

    std::fs::write(&cmd_file, code).map_err(|e| format!("Failed to write cmd: {}", e))?;

    let timeout = std::time::Duration::from_secs(300);
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            return Err("Timeout after 300s".into());
        }
        if out_file.exists() {
            let status = std::fs::read_to_string(&out_file).unwrap_or_default();
            if status.starts_with("OK") {
                // _result.json이 쓰여질 시간 확보
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                let result_json = std::fs::read_to_string(&result_file).unwrap_or_default();
                if result_json.is_empty() {
                    return Ok(serde_json::json!({"raw_output": status}).to_string());
                }
                return Ok(result_json);
            } else {
                return Err(status);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

// ── Julia Log Tail ────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct JuliaLogQuery {
    pub since_bytes: Option<usize>,
}

/// Julia 서버 로그 실시간 tail (incremental)
pub async fn julia_log_tail(
    axum::extract::Query(query): axum::extract::Query<JuliaLogQuery>,
) -> Json<serde_json::Value> {
    let log_path = "analysis/_server.log";
    let content = std::fs::read_to_string(log_path).unwrap_or_default();
    let since = query.since_bytes.unwrap_or(content.len().saturating_sub(2000));
    let new_content = if content.len() > since { &content[since..] } else { "" };

    Json(serde_json::json!({
        "log": new_content,
        "total_bytes": content.len(),
        "since_bytes": since,
    }))
}

// ── Claim Auto-complete ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AutoCompleteClaim {
    pub idea: String,
}

#[derive(Deserialize, Default)]
pub struct AutoCompleteQuery {
    pub force: Option<String>,
}

/// 아이디어 → RAG 검색 → 유사 없으면 LLM이 Claim 구조 자동 생성
pub async fn auto_complete_claim(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<AutoCompleteQuery>,
    Json(req): Json<AutoCompleteClaim>,
) -> Json<serde_json::Value> {
    let force = query.force.is_some();
    { let mut log = state.activity.write().await;
      log.push("search", "started", &format!("Auto-generate: \"{}\"", &truncate_str(&req.idea, 30)), None); }

    // 1. RAG: 기존 Claims/Reasons에서 유사한 것 검색
    let (rag_results, rag_context) = {
        let kb = state.knowledge.read().await;
        let results = kb.search_claims(&req.idea, 5);

        // 컨텍스트 미리 구성
        let mut ctx_part = String::new();
        if !results.is_empty() {
            ctx_part.push_str("## 관련 기존 Claims (참고, 중복 방지):\n");
            for r in &results {
                ctx_part.push_str(&format!("- [{}] {} (score: {:.2})\n",
                    if r.status == ClaimStatus::Accepted { "Knowledge" } else { "Draft" },
                    r.statement, r.score));
                for mr in &r.matched_reasons {
                    ctx_part.push_str(&format!("    └── Reason: {}\n", mr));
                }
            }
            ctx_part.push('\n');
        }
        (results, ctx_part)
    }; // kb lock dropped here

    // 유사도 높은 결과가 있으면 먼저 반환 (force면 skip)
    let has_overlap = rag_results.iter().any(|r| r.score > 0.8);
    if has_overlap && !force {
        { let mut log = state.activity.write().await;
          log.push("search", "rag_hit", &format!("RAG hit: \"{}\" — 유사 Claim 발견 (score={:.2})",
            &truncate_str(&req.idea, 25),
            rag_results.first().map(|r| r.score).unwrap_or(0.0)), None); }
        return Json(serde_json::json!({
            "status": "rag_hit",
            "message": "유사한 기존 Claim이 발견됨 — 진행할지 확인하세요",
            "rag_results": rag_results,
            "idea": req.idea,
        }));
    }

    // 2. 프롬프트 구성
    let mut ctx = String::from("# amure-do Knowledge Framework\n\n");
    ctx.push_str("너는 가설 검증 프레임워크의 Claim 설계자다.\n");
    ctx.push_str("사용자가 아이디어를 주면, 아래 구조에 맞춰 JSON으로 응답해라.\n\n");
    ctx.push_str(&rag_context);

    // 2. 프롬프트 구성
    ctx.push_str(&format!(r#"## 규칙:
- Claim: 참이라고 믿는 핵심 명제. 구체적이고 검증 가능해야 함
- Trigger: 이 Claim을 재검토해야 하는 조건 (시장 구조 변화, 데이터 소스 변경 등)
- Support Reasons: Claim을 지지하는 논리 단위. 각각 bridge(왜 이것이 Claim을 지지하는지) 포함
- Rebut Reasons: Claim이 항상 참은 아닐 수 있다는 반론. bridge 포함
- 숫자보다 경제적 메커니즘 중시
- 기존 Knowledge와 중복되지 않게
- 한국어로 응답

## 응답 형식 (JSON만, 다른 텍스트 없이):
```json
{{
  "statement": "구체적인 Claim 명제",
  "trigger": "재검토 조건",
  "keywords": ["핵심키워드1", "핵심키워드2", "핵심키워드3"],
  "support_reasons": [
    {{"statement": "지지 논리", "bridge": "왜 이것이 Claim을 지지하는지", "keywords": ["키워드"]}}
  ],
  "rebut_reasons": [
    {{"statement": "반론", "bridge": "왜 이것이 Claim을 약화시킬 수 있는지", "keywords": ["키워드"]}}
  ],
  "suggested_experiments": [
    {{
      "reason_index": 0,
      "reason_type": "support",
      "method": "distributional|conditional|cross_sectional|dose_response|regime|temporal|multi_horizon|entry_exit",
      "description": "구체적 실험 설명: 어떤 데이터를, 어떤 기간에, 어떤 유니버스에서, 어떤 방법으로 분석하는지",
      "if_true": "이 Reason이 참이면 구체적으로 어떤 통계량이 어떤 값이어야 하는지 (예: IC > 0.02, p < 0.05)",
      "if_false": "이 Reason이 거짓이면 어떤 결과가 나오는지 (예: IC가 0에 가깝거나 음수)",
      "expected_output": "구체적 산출물: 어떤 통계량, 어떤 차트, 어떤 테이블을 기대하는지"
    }}
  ]

주의사항 — suggested_experiments 설계 규칙:
1. 각 Reason마다 최소 1개 실험. backtest보다 데이터 분석(distributional/conditional/cross_sectional) 우선
2. description은 반드시 구체적: 데이터 소스, 기간, 유니버스, 분석 방법, 파라미터를 명시
3. if_true/if_false는 구체적 수치 기준 포함 (예: "IC > 0.02 and t-stat > 2.0")
4. expected_output은 산출물 명시 (예: "시점별 Spearman IC 시계열, 평균 IC, NW t-stat, hit ratio, IC decay curve")
5. method는 메커니즘 검증 순서: distributional → conditional → cross_sectional → regime → multi_horizon
6. 가능한 한 여러 method를 조합해서 설계 (분포 먼저, 횡단면 다음, 레짐 안정성 마지막)
}}
```

## 사용자 아이디어:
{idea}
"#, idea = req.idea));

    // 3. LLM 호출
    { let mut log = state.activity.write().await;
      log.push("llm", "started", &format!("LLM 호출 중... ({}자 프롬프트)", ctx.len()), None); }

    let config = state.llm_config.read().await;
    let result = crate::server::llm_provider::call_llm(&ctx, &config).await;

    match result {
        Ok(output) => {
            { let mut log = state.activity.write().await;
              log.push("llm", "completed", &format!("LLM 응답 수신 ({}자)", output.len()), None); }
            // JSON 파싱 시도 — LLM 응답에서 JSON 블록 추출
            let json_str = extract_json(&output);
            match serde_json::from_str::<serde_json::Value>(&json_str) {
                Ok(parsed) => Json(serde_json::json!({
                    "status": "ok",
                    "suggestion": parsed,
                    "raw": output,
                })),
                Err(_) => Json(serde_json::json!({
                    "status": "ok",
                    "suggestion": null,
                    "raw": output,
                    "parse_error": "JSON 파싱 실패 — raw 응답 확인",
                })),
            }
        }
        Err(e) => {
            { let mut log = state.activity.write().await;
              log.push("llm", "failed", &format!("LLM 호출 실패: {}", &truncate_str(&e, 50)), None); }
            Json(serde_json::json!({"error": e}))
        }
    }
}

/// Julia 실험 출력에서 __RESULT_JSON__ 마커 간 JSON 추출
fn extract_result_json(output: &str) -> Option<serde_json::Value> {
    let start_marker = "__RESULT_JSON__";
    let end_marker = "__RESULT_JSON_END__";
    let start = output.find(start_marker)?;
    let after = &output[start + start_marker.len()..];
    let end = after.find(end_marker)?;
    let json_str = after[..end].trim();
    serde_json::from_str(json_str).ok()
}

/// LLM 응답에서 JSON 블록 추출
fn extract_json(text: &str) -> String {
    // ```json ... ``` 블록 찾기
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    // ``` ... ``` 블록 찾기
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    // { 로 시작하는 부분 찾기
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }
    text.trim().to_string()
}

/// 자동 생성된 suggestion을 한번에 DB에 저장
#[derive(Deserialize)]
pub struct ApplySuggestion {
    pub statement: String,
    pub trigger: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub support_reasons: Vec<SuggestionReason>,
    pub rebut_reasons: Vec<SuggestionReason>,
    #[serde(default)]
    pub suggested_experiments: Vec<SuggestedExperiment>,
}

#[derive(Deserialize)]
pub struct SuggestionReason {
    pub statement: String,
    pub bridge: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Deserialize)]
pub struct SuggestedExperiment {
    pub reason_index: usize,
    pub reason_type: String,  // "support" or "rebut"
    pub description: String,
    pub if_true: String,
    pub if_false: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub expected_output: Option<String>,
}

pub async fn apply_suggestion(
    State(state): State<AppState>,
    Json(req): Json<ApplySuggestion>,
) -> Json<serde_json::Value> {
    // Gate check
    let gate = claim_gate(&req.statement, &req.trigger);
    if !gate.passed {
        return Json(serde_json::json!({"error": "CLAIM_GATE 실패", "gate": gate}));
    }

    let mut kb = state.knowledge.write().await;
    let claim = Claim::new(req.statement, req.trigger).with_keywords(req.keywords);
    let claim_id = kb.add_claim(claim);

    let mut reason_ids = Vec::new();

    for r in &req.support_reasons {
        let rg = reason_gate(&r.statement, &r.bridge, "support");
        if rg.passed {
            let reason = Reason::new(claim_id, ReasonType::Support, r.statement.clone(), r.bridge.clone())
                .with_keywords(r.keywords.clone());
            reason_ids.push(kb.add_reason(reason));
        }
    }

    for r in &req.rebut_reasons {
        let rg = reason_gate(&r.statement, &r.bridge, "rebut");
        if rg.passed {
            let reason = Reason::new(claim_id, ReasonType::Rebut, r.statement.clone(), r.bridge.clone())
                .with_keywords(r.keywords.clone());
            reason_ids.push(kb.add_reason(reason));
        }
    }

    // Create experiments linked to the appropriate reasons
    let mut exp_ids = Vec::new();
    let support_ids: Vec<Uuid> = reason_ids.iter().copied().collect();
    // Build reason index mapping: support reasons first, then rebut
    let support_count = req.support_reasons.len();
    for se in &req.suggested_experiments {
        let reason_id = if se.reason_type == "support" {
            support_ids.get(se.reason_index).copied()
        } else {
            // rebut reasons start after support reasons in reason_ids
            support_ids.get(support_count + se.reason_index).copied()
        };
        if let Some(rid) = reason_id {
            let method = match se.method.as_deref().unwrap_or("cross_sectional") {
                "distributional" => ExperimentMethod::Distributional,
                "conditional" => ExperimentMethod::Conditional,
                "dose_response" => ExperimentMethod::DoseResponse,
                "regime" => ExperimentMethod::Regime,
                "temporal" => ExperimentMethod::Temporal,
                "multi_horizon" => ExperimentMethod::MultiHorizon,
                "entry_exit" => ExperimentMethod::EntryExit,
                "backtest" => ExperimentMethod::Backtest,
                _ => ExperimentMethod::CrossSectional,
            };
            let exp = Experiment::new(
                rid, method,
                se.description.clone(),
                se.if_true.clone(),
                se.if_false.clone(),
                se.expected_output.clone().unwrap_or_else(|| "IC, t-stat, 분포".into()),
                serde_json::json!({"source": "auto_generated"}),
            );
            exp_ids.push(kb.add_fw_experiment(exp));
        }
    }

    let _ = kb.save();
    let n = reason_ids.len();
    let ne = exp_ids.len();
    drop(kb);

    { let mut log = state.activity.write().await;
      log.push("claim", "auto_created", &format!("Auto-generate 적용: Claim + {} reasons + {} experiments", n, ne), None); }

    Json(serde_json::json!({
        "status": "created",
        "claim_id": claim_id,
        "reason_ids": reason_ids,
        "experiment_ids": exp_ids,
        "n_reasons": n,
        "n_experiments": ne,
    }))
}

// ── LLM Routing Config ────────────────────────────────────────────────────

pub async fn get_llm_routing() -> Json<serde_json::Value> {
    let routing = LlmRouting::load();
    Json(serde_json::json!(routing))
}

#[derive(Deserialize)]
pub struct SetLlmRole {
    pub role: String,           // "default", "lab", "judge", "experiment", "gate"
    pub provider: String,       // "claude_cli", "claude_api", "openai", "custom"
    pub model: String,
    pub api_key: Option<String>,
    pub api_url: Option<String>,
    pub max_tokens: Option<usize>,
}

pub async fn set_llm_role(
    Json(req): Json<SetLlmRole>,
) -> Json<serde_json::Value> {
    let mut routing = LlmRouting::load();
    let config = LlmConfig {
        provider: req.provider,
        model: req.model,
        api_key: req.api_key,
        api_url: req.api_url,
        max_tokens: req.max_tokens.unwrap_or(4096),
        ..Default::default()
    };

    if req.role == "default" {
        routing.default = config;
    } else {
        routing.roles.insert(req.role.clone(), config);
    }

    routing.save();
    Json(serde_json::json!({"status": "updated", "role": req.role}))
}

// ── Activity Feed ──────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct ActivityQuery {
    pub since: Option<u64>,
    pub limit: Option<usize>,
}

pub async fn get_activity(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ActivityQuery>,
) -> Json<serde_json::Value> {
    let log = state.activity.read().await;
    let events = if let Some(since) = query.since {
        log.since(since)
    } else {
        log.recent(query.limit.unwrap_or(50))
    };
    Json(serde_json::json!({"events": events, "count": events.len()}))
}

pub async fn delete_llm_role(
    Path(role): Path<String>,
) -> Json<serde_json::Value> {
    let mut routing = LlmRouting::load();
    routing.roles.remove(&role);
    routing.save();
    Json(serde_json::json!({"status": "deleted", "role": role}))
}
