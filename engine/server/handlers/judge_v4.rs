/// JUDGE_GATE V4 — premise-based logical judgment.
/// Not score-based. Checks each premise's argument balance + validity + DSR.

use axum::Json;
use serde::Serialize;


#[derive(Serialize)]
pub struct JudgeV4Result {
    pub verdict: String,
    pub knowledge_type: String,
    pub premise_verdicts: Vec<PremiseVerdict>,
    pub falsification_triggered: bool,
    pub dsr: DsrReport,
    pub validity_intersection: ValiditySummary,
    pub paper_summary: String,
}

#[derive(Serialize)]
pub struct PremiseVerdict {
    pub statement: String,
    pub status: String,
    pub arguments_for: usize,
    pub arguments_against: usize,
    pub balance: String,     // "strong", "moderate", "weak", "refuted"
    pub validity_summary: String,
}

#[derive(Serialize)]
pub struct DsrReport {
    pub total_experiments: usize,
    pub per_premise: Vec<(String, usize)>,
    pub adjusted_p: f64,
    pub warning: String,
}

#[derive(Serialize)]
pub struct ValiditySummary {
    pub works_in: Vec<String>,
    pub does_not_work_in: Vec<String>,
    pub untested: Vec<String>,
}

/// Judge a thesis based on its insights (tagged as premises/arguments).
pub async fn judge_v4(
    axum::extract::State(state): axum::extract::State<crate::server::routes::AppState>,
    Json(req): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let thesis_claim = req["thesis_claim"].as_str().unwrap_or("");
    let falsification = req["falsification"].as_str().unwrap_or("");

    let kb = state.knowledge.read().await;

    // Find all related insights by tags
    let all_insights: Vec<_> = kb.insights.values().collect();

    let _theses: Vec<_> = all_insights.iter().filter(|i| i.tags.contains(&"thesis".to_string())).collect();
    let premises: Vec<_> = all_insights.iter().filter(|i| i.tags.contains(&"premise".to_string())).collect();
    let args_for: Vec<_> = all_insights.iter().filter(|i| i.tags.contains(&"argument-for".to_string())).collect();
    let args_against: Vec<_> = all_insights.iter().filter(|i| i.tags.contains(&"argument-against".to_string())).collect();
    let experiments: Vec<_> = all_insights.iter().filter(|i| i.tags.contains(&"experiment".to_string())).collect();

    // Build premise verdicts
    let mut premise_verdicts = Vec::new();
    let mut all_works = Vec::new();
    let mut all_not_works = Vec::new();

    for p in &premises {
        let stmt = p.text.replace("[PREMISE:", "").split(']').last().unwrap_or("").trim().to_string();

        // Count arguments for/against this premise
        let n_for = args_for.len();  // simplified — ideally match by premise
        let n_against = args_against.len();

        let balance = if n_for > 0 && n_against == 0 { "strong" }
        else if n_for > n_against { "moderate" }
        else if n_for == n_against { "weak" }
        else { "refuted" };

        let status = match balance {
            "strong" | "moderate" => "supported",
            "weak" => "challenged",
            _ => "refuted",
        };

        // Extract validity from arguments' evidence
        let validity_text = args_for.iter()
            .filter_map(|a| {
                a.evidence.split('\n')
                    .find(|l| l.starts_with("validity:"))
                    .map(|l| l.replace("validity: ", ""))
            })
            .next()
            .unwrap_or_default();

        if !validity_text.is_empty() {
            all_works.push(validity_text.clone());
        }

        premise_verdicts.push(PremiseVerdict {
            statement: stmt,
            status: status.into(),
            arguments_for: n_for,
            arguments_against: n_against,
            balance: balance.into(),
            validity_summary: validity_text,
        });
    }

    // Extract does_not_work from AGAINST arguments
    for a in &args_against {
        if let Some(v) = a.evidence.split('\n').find(|l| l.starts_with("validity:")) {
            all_not_works.push(v.replace("validity: ", ""));
        }
    }

    // Falsification check
    let falsification_triggered = if !falsification.is_empty() {
        args_against.iter().any(|a| a.text.to_lowercase().contains(&falsification.to_lowercase()[..falsification.len().min(30)]))
    } else {
        false
    };

    // DSR
    let total_exp = experiments.len();
    let dsr_adjusted_p = if total_exp <= 1 { 0.05 }
    else { 0.05 * (total_exp as f64).ln() / (total_exp as f64).sqrt() };

    let dsr_warning = if total_exp > 10 { "과도한 실험 — 결론 도출 권장".into() }
    else if total_exp > 5 { "실험 횟수 주의 — DSR 조정 적용됨".into() }
    else { "OK".into() };

    // Overall verdict
    let n_supported = premise_verdicts.iter().filter(|p| p.status == "supported").count();
    let n_total = premise_verdicts.len().max(1);
    let support_ratio = n_supported as f64 / n_total as f64;

    let (verdict, knowledge_type) = if falsification_triggered {
        ("reject", "rejected")
    } else if support_ratio >= 0.8 && !all_works.is_empty() {
        ("accept", "conditional")  // always conditional — no knowledge is universal
    } else if support_ratio >= 0.5 {
        ("conditional_accept", "conditional")
    } else {
        ("reject", "insufficient")
    };

    // Paper summary
    let paper = format!(
        "가설: {}\n\n\
         전제 검증 결과 ({}/{}개 지지):\n{}\n\n\
         반증 조건 {}.\n\n\
         DSR: {}회 실험, adjusted p={:.3}\n\n\
         유효 범위:\n  통하는 곳: {}\n  안 통하는 곳: {}\n\n\
         판정: {} ({})",
        thesis_claim,
        n_supported, n_total,
        premise_verdicts.iter().map(|p|
            format!("  - {} [{}] (FOR:{} AGAINST:{}) — {}",
                p.statement, p.status, p.arguments_for, p.arguments_against, p.validity_summary)
        ).collect::<Vec<_>>().join("\n"),
        if falsification_triggered { "발동됨 ⚠️" } else { "미발동" },
        total_exp, dsr_adjusted_p,
        if all_works.is_empty() { "미명시".into() } else { all_works.join(", ") },
        if all_not_works.is_empty() { "미명시".into() } else { all_not_works.join(", ") },
        verdict, knowledge_type,
    );

    Json(serde_json::json!(JudgeV4Result {
        verdict: verdict.into(),
        knowledge_type: knowledge_type.into(),
        premise_verdicts,
        falsification_triggered,
        dsr: DsrReport {
            total_experiments: total_exp,
            per_premise: vec![],
            adjusted_p: dsr_adjusted_p,
            warning: dsr_warning,
        },
        validity_intersection: ValiditySummary {
            works_in: all_works,
            does_not_work_in: all_not_works,
            untested: vec![],
        },
        paper_summary: paper,
    }))
}
