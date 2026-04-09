/// Thesis API — CRUD with gate enforcement.

use axum::extract::State;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::knowledge::thesis::*;
use crate::server::routes::AppState;

// ── Create Thesis (THESIS_GATE enforced) ────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateThesis {
    pub claim: String,
    pub mechanism: String,
    pub falsification: String,
    pub premises: Vec<String>,
    pub counter_theses: Option<Vec<String>>,
}

pub async fn create_thesis(
    State(state): State<AppState>,
    Json(req): Json<CreateThesis>,
) -> Json<serde_json::Value> {
    // THESIS_GATE
    let gate = thesis_gate(&req.claim, &req.mechanism, &req.falsification, &req.premises);
    if !gate.passed {
        return Json(serde_json::json!({
            "gate": gate.gate,
            "passed": false,
            "errors": gate.errors,
            "warnings": gate.warnings,
        }));
    }

    let thesis_id = Uuid::new_v4();

    // Create premises
    let mut premise_ids = Vec::new();
    let mut premises_data = Vec::new();
    for stmt in &req.premises {
        let p = Premise {
            id: Uuid::new_v4(),
            thesis_id,
            statement: stmt.clone(),
            argument_ids: Vec::new(),
            experiment_ids: Vec::new(),
            status: PremiseStatus::Untested,
            validity: Validity::default(),
            created_at: Utc::now(),
        };
        premise_ids.push(p.id);
        premises_data.push(p);
    }

    let _thesis = Thesis {
        id: thesis_id,
        claim: req.claim.clone(),
        mechanism: req.mechanism.clone(),
        falsification: req.falsification.clone(),
        premise_ids: premise_ids.clone(),
        counter_theses: req.counter_theses.unwrap_or_default(),
        status: ThesisStatus::Active,
        meta: ThesisMeta::default(),
        created_at: Utc::now(),
    };

    // Store
    let mut kb = state.knowledge.write().await;
    // Save thesis as JSON in a separate structure
    // For now, use the existing insight system with special tags
    // TODO: dedicated thesis storage

    // Store thesis as a special insight for now
    let thesis_insight = crate::knowledge::types::Insight::new(
        Uuid::nil(),
        format!("[THESIS] {}", req.claim),
        format!("mechanism: {}\nfalsification: {}\npremises: {:?}",
            req.mechanism, req.falsification, req.premises),
        vec!["thesis".into(), "active".into()],
    );
    kb.add_insight(thesis_insight);

    // Store each premise
    for p in &premises_data {
        let premise_insight = crate::knowledge::types::Insight::new(
            Uuid::nil(),
            format!("[PREMISE:{}] {}", thesis_id.to_string()[..8].to_string(), p.statement),
            format!("thesis: {}\nstatus: untested", req.claim),
            vec!["premise".into(), "untested".into(), thesis_id.to_string()],
        );
        kb.add_insight(premise_insight);
    }

    let _ = kb.save();

    // Log
    {
        let mut log = state.call_log.write().await;
        log.log("POST", "/api/thesis", &req.claim[..req.claim.len().min(30)], "created");
    }

    Json(serde_json::json!({
        "gate": gate.gate,
        "passed": true,
        "warnings": gate.warnings,
        "thesis_id": thesis_id,
        "premise_ids": premise_ids,
        "premises": premises_data.iter().map(|p| serde_json::json!({
            "id": p.id,
            "statement": p.statement,
            "status": "untested",
        })).collect::<Vec<_>>(),
    }))
}

// ── Design Experiment (EXPERIMENT_GATE enforced) ────────────────────────────

#[derive(Deserialize)]
pub struct DesignExperiment {
    pub thesis_id: Uuid,
    pub tests_premise: String,
    pub description: String,
    pub expected_if_true: String,
    pub expected_if_false: String,
    pub if_supported: String,           // 지지되면 어떤 효과?
    pub if_weakened: String,            // 약화되면 어떤 방향?
    pub gaps_if_supported: Vec<String>, // 지지돼도 남는 빈공간
    pub config: Option<serde_json::Value>,
}

pub async fn design_experiment(
    State(state): State<AppState>,
    Json(req): Json<DesignExperiment>,
) -> Json<serde_json::Value> {
    // Count existing experiments for this premise (from tags)
    let premise_exp_count = {
        let kb = state.knowledge.read().await;
        kb.insights.values()
            .filter(|i| i.tags.contains(&"experiment".to_string())
                && i.evidence.contains(&req.tests_premise[..req.tests_premise.len().min(20)]))
            .count()
    };

    // EXPERIMENT_GATE V5
    let gate = experiment_gate(
        &req.tests_premise,
        &req.expected_if_true,
        &req.expected_if_false,
        premise_exp_count,
        &req.if_supported,
        &req.if_weakened,
        &req.gaps_if_supported,
    );

    if !gate.passed {
        return Json(serde_json::json!({
            "gate": gate.gate,
            "passed": false,
            "errors": gate.errors,
            "warnings": gate.warnings,
        }));
    }

    let exp_id = Uuid::new_v4();

    // Store experiment design
    let mut kb = state.knowledge.write().await;
    let exp_insight = crate::knowledge::types::Insight::new(
        Uuid::nil(),
        format!("[EXPERIMENT] {}", req.description),
        format!("tests_premise: {}\nexpected_if_true: {}\nexpected_if_false: {}\nthesis: {}",
            req.tests_premise, req.expected_if_true, req.expected_if_false, req.thesis_id),
        vec!["experiment".into(), "planned".into(), req.thesis_id.to_string()],
    );
    kb.add_insight(exp_insight);
    let _ = kb.save();

    {
        let mut log = state.call_log.write().await;
        log.log("POST", "/api/thesis/experiment", &req.description[..req.description.len().min(30)], "designed");
    }

    Json(serde_json::json!({
        "gate": gate.gate,
        "passed": true,
        "warnings": gate.warnings,
        "experiment_id": exp_id,
        "tests_premise": req.tests_premise,
        "if_supported": req.if_supported,
        "if_weakened": req.if_weakened,
        "gaps_if_supported": req.gaps_if_supported,
        "gaps_count": req.gaps_if_supported.len(),
        "premise_experiment_count": premise_exp_count + 1,
        "note": if !req.gaps_if_supported.is_empty() {
            format!("지지돼도 {} 개 gap — 하위 실험 필요", req.gaps_if_supported.len())
        } else {
            "gap 없음 — 지지되면 바로 채택 가능".into()
        },
    }))
}

// ── Add Argument (ARGUMENT_GATE enforced) ───────────────────────────────────

#[derive(Deserialize)]
pub struct AddArgument {
    pub premise: String,
    pub direction: String,     // "for" or "against"
    pub claim: String,         // 경제적 논리
    pub evidence: String,
    pub validity: ValidityInput,
}

#[derive(Deserialize)]
pub struct ValidityInput {
    pub regime_works: Vec<String>,
    pub regime_not_works: Vec<String>,
    pub universe_works: Vec<String>,
    pub universe_not_works: Vec<String>,
    pub summary: String,
}

pub async fn add_argument(
    State(state): State<AppState>,
    Json(req): Json<AddArgument>,
) -> Json<serde_json::Value> {
    let validity = Validity {
        regimes: AxisValidity {
            works: req.validity.regime_works.clone(),
            does_not_work: req.validity.regime_not_works.clone(),
            ..Default::default()
        },
        universes: AxisValidity {
            works: req.validity.universe_works.clone(),
            does_not_work: req.validity.universe_not_works.clone(),
            ..Default::default()
        },
        summary: req.validity.summary.clone(),
        ..Default::default()
    };

    // ARGUMENT_GATE
    let gate = argument_gate(&req.claim, &validity);
    if !gate.passed {
        return Json(serde_json::json!({
            "gate": gate.gate,
            "passed": false,
            "errors": gate.errors,
            "warnings": gate.warnings,
        }));
    }

    let dir_tag = if req.direction == "for" { "argument-for" } else { "argument-against" };

    let mut kb = state.knowledge.write().await;
    let arg_insight = crate::knowledge::types::Insight::new(
        Uuid::nil(),
        format!("[{}] {}", dir_tag.to_uppercase(), req.claim),
        format!("premise: {}\nevidence: {}\nvalidity: {}",
            req.premise, req.evidence, req.validity.summary),
        vec![dir_tag.into(), "argument".into()],
    );
    kb.add_insight(arg_insight);
    let _ = kb.save();

    Json(serde_json::json!({
        "gate": gate.gate,
        "passed": true,
        "warnings": gate.warnings,
        "direction": req.direction,
        "validity_summary": req.validity.summary,
    }))
}
