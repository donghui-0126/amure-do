/// Hypothesis Judge — final verdict on hypothesis based on all experiments + evaluations.
/// Deterministic scoring + LLM for economic reasoning.

use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::knowledge::types::*;
use crate::server::routes::AppState;

#[derive(Serialize)]
pub struct JudgeResult {
    pub hypothesis_id: String,
    pub hypothesis_title: String,
    pub verdict: String,              // "accept", "conditional_accept", "reject"
    pub knowledge_type: String,       // "general", "conditional", "specific"
    pub overall_confidence: f64,
    pub applies_to: Vec<String>,
    pub does_not_apply: Vec<String>,
    pub untested: Vec<String>,
    pub evidence_summary: String,
    pub economic_reasoning: Option<String>,
    pub experiments_reviewed: usize,
    pub insights_reviewed: usize,
}

pub async fn judge_hypothesis(
    State(state): State<AppState>,
    Path(hyp_id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;

    let hyp = match kb.get_hypothesis(&hyp_id) {
        Some(h) => h,
        None => return Json(serde_json::json!({"error": "Hypothesis not found"})),
    };
    let hyp_title = hyp.title.clone();
    let hyp_rationale = hyp.economic_rationale.clone();

    let experiments = kb.experiments_for_hypothesis(&hyp_id);
    if experiments.is_empty() {
        return Json(serde_json::json!({"error": "No experiments for this hypothesis"}));
    }

    // Collect all insights across experiments
    let mut all_insights: Vec<&Insight> = Vec::new();
    for exp in &experiments {
        all_insights.extend(kb.insights_for_experiment(&exp.id));
    }

    // ── Deterministic Scoring ────────────────────────────────────

    // 1. Experiment consistency
    let n_exp = experiments.len();
    let n_exp_positive = experiments.iter()
        .filter(|e| e.results.as_ref().map(|r| r.mean_ret_bp > 0.0).unwrap_or(false))
        .count();
    let consistency = n_exp_positive as f64 / n_exp as f64;

    // 2. Insight analysis
    let n_insights = all_insights.len();
    let n_accepted = all_insights.iter().filter(|i| i.status == InsightStatus::Accepted).count();
    let n_rejected = all_insights.iter().filter(|i| i.status == InsightStatus::Rejected).count();
    let n_mature = all_insights.iter().filter(|i| i.maturity == Maturity::Mature).count();

    // 3. Check for stability verdicts in insight tags
    let has_stable_time = all_insights.iter().any(|i| i.tags.contains(&"stable".to_string()));
    let has_universal = all_insights.iter().any(|i| i.tags.contains(&"universal".to_string()));
    let has_robust_regime = all_insights.iter().any(|i| i.tags.contains(&"robust".to_string()));
    let has_conflict = all_insights.iter().any(|i| i.tags.contains(&"conflict".to_string()));

    // 4. Compute confidence
    let exp_score = consistency;
    let insight_score = if n_insights > 0 { n_accepted as f64 / n_insights as f64 } else { 0.0 };
    let stability_score = [has_stable_time, has_universal, has_robust_regime]
        .iter().filter(|&&x| x).count() as f64 / 3.0;
    let conflict_penalty = if has_conflict { 0.2 } else { 0.0 };

    let confidence = (exp_score * 0.4 + insight_score * 0.3 + stability_score * 0.3 - conflict_penalty).max(0.0).min(1.0);

    // 5. Determine verdict
    let (verdict, knowledge_type) = if confidence >= 0.7 && has_universal && has_robust_regime {
        ("accept", "general")
    } else if confidence >= 0.4 {
        ("conditional_accept", "conditional")
    } else if confidence >= 0.2 {
        ("conditional_accept", "specific")
    } else {
        ("reject", "rejected")
    };

    // 6. Extract conditions from insights
    let mut applies_to = Vec::new();
    let mut does_not_apply = Vec::new();
    let untested = Vec::new();

    for ins in &all_insights {
        for tag in &ins.tags {
            match tag.as_str() {
                "short" | "momentum" | "oi" => {
                    if !applies_to.contains(tag) { applies_to.push(tag.clone()); }
                }
                "conflict" => {
                    does_not_apply.push(format!("conflict: {}", ins.text.chars().take(50).collect::<String>()));
                }
                _ => {}
            }
        }
    }

    // Check for universe/regime specific tags
    for ins in &all_insights {
        if ins.tags.contains(&"conditional".to_string()) || ins.tags.contains(&"specific".to_string()) {
            let cond = ins.text.chars().take(60).collect::<String>();
            if !applies_to.iter().any(|a| a.contains(&cond[..20.min(cond.len())])) {
                applies_to.push(cond);
            }
        }
    }

    // Evidence summary
    let evidence = format!(
        "{} experiments ({} positive), {} insights ({} accepted, {} rejected, {} mature)",
        n_exp, n_exp_positive, n_insights, n_accepted, n_rejected, n_mature
    );

    // 7. Optional LLM reasoning
    let economic_reasoning = {
        let prompt = format!(
            "가설 '{}'에 대한 최종 판정을 경제적으로 설명해줘. 한국어로, 200자 이내.\n\
             판정: {}\n근거: {}\n유형: {}\nconfidence: {:.2}\n\
             원래 경제적 근거: {}",
            hyp_title, verdict, evidence, knowledge_type, confidence, hyp_rationale
        );
        run_llm_sync(&prompt).ok()
    };

    drop(kb);

    // Update hypothesis status
    {
        let mut kb = state.knowledge.write().await;
        if let Some(h) = kb.hypotheses.get_mut(&hyp_id) {
            h.status = match verdict {
                "accept" | "conditional_accept" => HypothesisStatus::Validated,
                "reject" => HypothesisStatus::Rejected,
                _ => h.status,
            };
            if verdict == "accept" {
                h.maturity = Maturity::Mature;
            }
            h.user_notes.push(UserNote {
                text: format!("Judge verdict: {} ({}, conf={:.2})", verdict, knowledge_type, confidence),
                at: chrono::Utc::now(),
            });
        }
        let _ = kb.save();
    }

    // Log
    {
        let mut log = state.call_log.write().await;
        log.log("POST", "/api/judge", &hyp_title, verdict);
    }

    Json(serde_json::json!(JudgeResult {
        hypothesis_id: hyp_id.to_string(),
        hypothesis_title: hyp_title.clone(),
        verdict: verdict.into(),
        knowledge_type: knowledge_type.into(),
        overall_confidence: confidence,
        applies_to,
        does_not_apply,
        untested,
        evidence_summary: evidence,
        economic_reasoning,
        experiments_reviewed: n_exp,
        insights_reviewed: n_insights,
    }))
}

fn run_llm_sync(prompt: &str) -> Result<String, String> {
    let output = std::process::Command::new("claude")
        .args(["-p", prompt])
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).to_string()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => Err(e.to_string()),
    }
}
