/// Thesis → Premise → Argument knowledge structure.
/// Every knowledge has "when does it work" (Validity).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ── Validity: 모든 지식에 강제 ──────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Validity {
    pub regimes: AxisValidity,
    pub universes: AxisValidity,
    pub time_periods: AxisValidity,
    pub directions: AxisValidity,
    pub required_features: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AxisValidity {
    pub works: Vec<String>,
    pub does_not_work: Vec<String>,
    pub untested: Vec<String>,
    pub reason: String,
}

// ── Thesis ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ThesisStatus {
    Active,
    Validated,
    ConditionallyValidated,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thesis {
    pub id: Uuid,
    pub claim: String,
    pub mechanism: String,
    pub falsification: String,
    pub premise_ids: Vec<Uuid>,
    pub counter_theses: Vec<String>,
    pub status: ThesisStatus,
    pub meta: ThesisMeta,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThesisMeta {
    pub total_experiments: usize,
    pub experiments_per_premise: HashMap<String, usize>, // premise_id → count
    pub dsr_adjusted_p: f64,
}

// ── Premise ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PremiseStatus {
    Untested,
    Supported,
    Challenged,
    Refuted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Premise {
    pub id: Uuid,
    pub thesis_id: Uuid,
    pub statement: String,
    pub argument_ids: Vec<Uuid>,
    pub experiment_ids: Vec<Uuid>,
    pub status: PremiseStatus,
    pub validity: Validity,
    pub created_at: DateTime<Utc>,
}

// ── Argument ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ArgumentDirection {
    For,
    Against,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Argument {
    pub id: Uuid,
    pub premise_id: Uuid,
    pub direction: ArgumentDirection,
    pub claim: String,           // 경제적 논리
    pub evidence: String,        // 실험 근거
    pub experiment_id: Option<Uuid>,
    pub validity: Validity,
    pub created_at: DateTime<Utc>,
}

// ── Experiment (V4) ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentV4 {
    pub id: Uuid,
    pub thesis_id: Uuid,
    pub tests_premise: Uuid,
    pub description: String,
    pub expected_if_true: String,
    pub expected_if_false: String,
    pub config: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub validity_breakdown: Option<ValidityBreakdown>,
    pub status: ExperimentV4Status,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExperimentV4Status {
    Planned,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidityBreakdown {
    pub regime_results: Vec<ConditionResult>,
    pub universe_results: Vec<ConditionResult>,
    pub time_results: Vec<ConditionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionResult {
    pub condition: String,
    pub n_trades: usize,
    pub mean_bp: f64,
    pub works: bool,
}

// ── Gates (Validation) ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct GateResult {
    pub passed: bool,
    pub gate: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// THESIS_GATE: validate thesis has mechanism + premises + falsification.
pub fn thesis_gate(claim: &str, mechanism: &str, falsification: &str, premises: &[String]) -> GateResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if claim.len() < 10 {
        errors.push("claim이 너무 짧음 — 구체적인 주장 필요".into());
    }
    if mechanism.is_empty() {
        errors.push("mechanism 필수 — 경제적 메커니즘 없이는 가설이 아님".into());
    } else if mechanism.len() < 20 {
        warnings.push("mechanism이 짧음 — '왜'에 대한 구체적 설명 권장".into());
    }
    if falsification.is_empty() {
        errors.push("falsification 필수 — 반증 조건 없으면 검증 불가".into());
    }
    if premises.len() < 2 {
        errors.push(format!("premises 최소 2개 필요 (현재 {}개)", premises.len()));
    }
    for (i, p) in premises.iter().enumerate() {
        if p.len() < 10 {
            warnings.push(format!("premise {} 이 너무 짧음", i + 1));
        }
    }

    // Check for observation vs hypothesis
    let lower = claim.to_lowercase();
    if lower.contains("mean=") || lower.contains("bp이") || lower.contains("수익률이") {
        warnings.push("관찰 서술 감지 — 숫자가 아닌 메커니즘 기반 가설인지 확인".into());
    }

    GateResult {
        passed: errors.is_empty(),
        gate: "THESIS_GATE".into(),
        errors,
        warnings,
    }
}

/// EXPERIMENT_GATE V5: premise test + support/weaken effects + gaps.
pub fn experiment_gate(
    tests_premise: &str,
    expected_if_true: &str,
    expected_if_false: &str,
    premise_experiment_count: usize,
    if_supported: &str,        // 지지되면 어떤 효과?
    if_weakened: &str,         // 약화되면 어떤 방향?
    gaps_if_supported: &[String], // 지지돼도 남는 빈공간
) -> GateResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if tests_premise.is_empty() {
        errors.push("tests_premise 필수 — 어떤 전제를 검증하는지 명시".into());
    }
    if expected_if_true.is_empty() {
        errors.push("expected_if_true 필수 — 전제가 맞으면 예상 결과".into());
    }
    if expected_if_false.is_empty() {
        errors.push("expected_if_false 필수 — 전제가 틀리면 예상 결과".into());
    }

    // DSR check
    if premise_experiment_count >= 5 {
        warnings.push(format!(
            "이 전제에 대해 이미 {}회 실험 — 결론 내리거나 전제 재설정 권장",
            premise_experiment_count
        ));
    }
    if premise_experiment_count >= 3 {
        let dsr_penalty = (premise_experiment_count as f64).ln() / (premise_experiment_count as f64).sqrt();
        warnings.push(format!("DSR penalty: {:.2} — 추가 실험의 한계 수익 감소", dsr_penalty));
    }

    // V5: support/weaken effects
    if if_supported.is_empty() {
        errors.push("if_supported 필수 — 이 가설이 지지되면 어떤 효과가 있는지".into());
    }
    if if_weakened.is_empty() {
        errors.push("if_weakened 필수 — 이 가설이 약화되면 어떤 방향으로 탐색할지".into());
    }

    // V5: gaps awareness
    if gaps_if_supported.is_empty() {
        warnings.push("gaps_if_supported 비어있음 — 지지돼도 남는 빈공간이 정말 없는지 확인".into());
    } else {
        warnings.push(format!("gaps {} 개 — 지지돼도 하위 실험 {}개 필요",
            gaps_if_supported.len(), gaps_if_supported.len()));
    }

    GateResult {
        passed: errors.is_empty(),
        gate: "EXPERIMENT_GATE_V5".into(),
        errors,
        warnings,
    }
}

/// ARGUMENT_GATE: validate argument has mechanism + validity.
pub fn argument_gate(claim: &str, validity: &Validity) -> GateResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if claim.len() < 20 {
        errors.push("argument claim이 너무 짧음 — 경제적 논리 설명 필요".into());
    }

    // Check validity is populated
    if validity.regimes.works.is_empty() && validity.regimes.does_not_work.is_empty() {
        errors.push("regime validity 필수 — 어떤 레짐에서 통하는지/안 통하는지".into());
    }
    if validity.universes.works.is_empty() && validity.universes.does_not_work.is_empty() {
        errors.push("universe validity 필수 — 어떤 유니버스에서 통하는지/안 통하는지".into());
    }
    if validity.summary.is_empty() {
        warnings.push("validity summary 권장 — 한 줄 요약".into());
    }

    // Check for number-only claims
    let lower = claim.to_lowercase();
    if lower.contains("mean=") && !lower.contains("왜") && !lower.contains("때문") && !lower.contains("구조") {
        warnings.push("숫자만 나열 감지 — '왜' 이 결과가 나왔는지 경제적 이유 필요".into());
    }

    GateResult {
        passed: errors.is_empty(),
        gate: "ARGUMENT_GATE".into(),
        errors,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thesis_gate_pass() {
        let r = thesis_gate(
            "크립토에서 short momentum은 구조적 long bias 때문에 continuation alpha가 있다",
            "구조적 long bias → 청산 cascade → short continuation",
            "long bias 없는 시기에도 short이 유리하면 다른 원인",
            &[
                "크립토 시장은 구조적으로 long 편향이다".into(),
                "long 청산이 cascade를 일으킨다".into(),
            ],
        );
        assert!(r.passed, "errors: {:?}", r.errors);
    }

    #[test]
    fn test_thesis_gate_fail_no_mechanism() {
        let r = thesis_gate(
            "short momentum이 좋다",
            "",
            "반증 조건",
            &["전제1".into(), "전제2".into()],
        );
        assert!(!r.passed);
        assert!(r.errors.iter().any(|e| e.contains("mechanism")));
    }

    #[test]
    fn test_thesis_gate_fail_few_premises() {
        let r = thesis_gate(
            "short momentum continuation",
            "long bias 때문",
            "반증",
            &["전제 하나만".into()],
        );
        assert!(!r.passed);
        assert!(r.errors.iter().any(|e| e.contains("premises")));
    }

    #[test]
    fn test_thesis_gate_warn_observation() {
        let r = thesis_gate(
            "short의 mean=+5bp이므로 유효하다",
            "메커니즘 설명",
            "반증 조건",
            &["전제1 설명".into(), "전제2 설명".into()],
        );
        assert!(r.passed); // pass but with warning
        assert!(!r.warnings.is_empty());
    }

    #[test]
    fn test_experiment_gate_pass() {
        let r = experiment_gate(
            "크립토 시장의 long 편향 검증",
            "funding rate > 0 구간에서 short 성과 더 좋음",
            "funding rate와 무관하면 long bias가 원인 아님",
            1,
            "short momentum 전략 구현 가능, crash filter와 조합",
            "momentum이 아닌 다른 alpha source 탐색: mean reversion, volume event",
            &["bear regime에서 미검증".into(), "small cap 유동성 이슈".into()],
        );
        assert!(r.passed);
        assert!(r.warnings.iter().any(|w| w.contains("gaps 2")));
    }

    #[test]
    fn test_experiment_gate_fail_no_effects() {
        let r = experiment_gate("전제", "true", "false", 1, "", "", &[]);
        assert!(!r.passed);
        assert!(r.errors.iter().any(|e| e.contains("if_supported")));
        assert!(r.errors.iter().any(|e| e.contains("if_weakened")));
    }

    #[test]
    fn test_experiment_gate_dsr_warning() {
        let r = experiment_gate("전제 검증", "결과A", "결과B", 5,
            "효과A", "방향B", &[]);
        assert!(r.passed);
        assert!(r.warnings.iter().any(|w| w.contains("5회")));
    }

    #[test]
    fn test_argument_gate_fail_no_validity() {
        let r = argument_gate(
            "short이 유리하다 왜냐하면 long bias 때문",
            &Validity::default(),
        );
        assert!(!r.passed);
        assert!(r.errors.iter().any(|e| e.contains("regime")));
    }

    #[test]
    fn test_argument_gate_pass() {
        let v = Validity {
            regimes: AxisValidity {
                works: vec!["bull".into(), "sideways".into()],
                does_not_work: vec!["crash".into()],
                ..Default::default()
            },
            universes: AxisValidity {
                works: vec!["mid_cap".into()],
                does_not_work: vec!["small_cap".into()],
                ..Default::default()
            },
            summary: "bull/sideways, mid cap에서 유효".into(),
            ..Default::default()
        };
        let r = argument_gate(
            "구조적 long bias 하에서 청산 cascade가 작동하기 때문에 short continuation이 발생",
            &v,
        );
        assert!(r.passed, "errors: {:?}", r.errors);
    }
}
