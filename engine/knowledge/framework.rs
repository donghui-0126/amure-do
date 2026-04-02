/// Knowledge Framework — Claim → Reason → Bridge → Evidence
/// Structural argumentation with explicit assumptions and review triggers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Status ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ClaimStatus {
    /// 작업 중 — 아직 근거 수집/논증 진행 중
    Draft,
    /// 수락됨 — Knowledge로 격상. DB에 영구 저장
    Accepted,
    /// 기각됨 — 논증 결과 참이 아니라고 판단
    Rejected,
}

// ── Core Types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: Uuid,
    pub statement: String,
    pub trigger: String,              // 재검토 조건
    pub keywords: Vec<String>,        // 핵심 키워드 (검색/RAG용)
    pub status: ClaimStatus,
    pub reasons: Vec<Uuid>,           // Reason IDs
    pub accepted_at: Option<DateTime<Utc>>,
    pub accept_reason: Option<String>,  // 왜 accept/reject 했는지
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Claim {
    pub fn new(statement: String, trigger: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            statement,
            trigger,
            keywords: Vec::new(),
            status: ClaimStatus::Draft,
            reasons: Vec::new(),
            accepted_at: None,
            accept_reason: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = keywords;
        self
    }

    pub fn is_knowledge(&self) -> bool {
        self.status == ClaimStatus::Accepted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ReasonType {
    Support,
    Rebut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reason {
    pub id: Uuid,
    pub claim_id: Uuid,               // 이 Reason이 지지/반박하는 Claim
    pub reason_type: ReasonType,
    pub statement: String,
    pub bridge: String,                // 왜 이 Reason이 Claim을 지지/반박하는가
    pub keywords: Vec<String>,        // 핵심 키워드
    pub evidences: Vec<Evidence>,
    pub relations: Vec<Relation>,      // 다른 Reason과의 관계
    pub sub_claim_id: Option<Uuid>,    // 재귀: 이 Reason이 하위 Claim이면 그 ID
    pub created_at: DateTime<Utc>,
}

impl Reason {
    pub fn new(claim_id: Uuid, reason_type: ReasonType, statement: String, bridge: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            claim_id,
            reason_type,
            statement,
            bridge,
            keywords: Vec::new(),
            evidences: Vec::new(),
            relations: Vec::new(),
            sub_claim_id: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = keywords;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EvidenceTag {
    Backtest,
    Live,
    Intuition,
    Literature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: Uuid,
    pub tag: EvidenceTag,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

impl Evidence {
    pub fn new(tag: EvidenceTag, description: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            tag,
            description,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RelationType {
    Independent,
    Correlated,
    Conditional,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub other_reason_id: Uuid,
    pub relation_type: RelationType,
    pub note: String,
}

// ── Regimes & Universes ─────────────────────────────────────────────────────
// 사전 정의된 레짐/유니버스. Validation에서 강제.

pub const PREDEFINED_REGIMES: &[&str] = &[
    "bull", "bear", "sideways", "crash",
    "high_vol", "low_vol",
];

pub const PREDEFINED_UNIVERSES: &[&str] = &[
    "all",
    "mcap_large", "mcap_mid", "mcap_small",
    "btc_high_beta", "btc_low_beta",
    "eth_high_beta", "eth_low_beta",
    "recent_return_top", "recent_return_bottom",
    "recent_volume_top", "recent_volume_bottom",
    "oi_value_top", "oi_value_bottom",
];

// ── Experiment ──────────────────────────────────────────────────────────────
// Experiment = Evidence를 생산하는 과정.
// Reason을 검증하기 위해 설계하고, 결과가 나오면 Evidence로 변환.

/// 실험 방법론. 백테스트 전에 데이터 분석이 선행되어야 한다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExperimentMethod {
    /// 분포 분석: 변수의 분포, skewness, tail 특성
    Distributional,
    /// 조건부 분석: A일 때 B가 유의하게 다른가
    Conditional,
    /// 횡단면 분석: IC, rank correlation, quintile spread
    CrossSectional,
    /// Dose-response: 변수 크기 → 결과 크기 monotonicity
    DoseResponse,
    /// 레짐별 안정성: bull/bear/vol별 관계 변화
    Regime,
    /// 시간 안정성: rolling IC, structural break, decay
    Temporal,
    /// Multi-horizon: 여러 fwd return에 대한 IC decay curve
    MultiHorizon,
    /// Entry-exit 기반: 캐싱된 수익률로 진입/청산 전략 시뮬레이션
    EntryExit,
    /// 전통적 백테스트 (선행 분석 없이 하면 warning)
    Backtest,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExperimentStatus {
    Designed,
    Running,
    Completed,
    Interpreted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub id: Uuid,
    pub reason_id: Uuid,
    pub method: ExperimentMethod,          // 실험 방법론 (필수)
    pub description: String,
    pub if_true: String,
    pub if_false: String,
    pub expected_output: String,           // 기대하는 통계량/차트 종류
    pub config: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub verdict: Option<ExperimentVerdict>,
    pub status: ExperimentStatus,
    pub evidence_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Multi-horizon 분석 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiHorizonConfig {
    pub horizons: Vec<String>,             // ["5m", "15m", "1h", "4h", "1d"]
    pub method: String,                    // "ic_decay", "cumulative_ic", "half_life"
}

/// Entry-exit 전략 설정. 미래 수익률을 미리 캐싱하고 단순 수익률만 계산.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryExitConfig {
    pub entry_signal: String,              // 진입 조건 설명
    pub exit_conditions: Vec<String>,      // 청산 조건들
    pub cached_horizons: Vec<String>,      // 캐싱된 fwd return horizons
    pub max_hold_bars: usize,              // 최대 보유 기간 (bars)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentVerdict {
    pub supports_reason: bool,        // 이 실험이 Reason을 지지하는가?
    pub explanation: String,          // 왜 그렇게 판단했는지
    pub gaps: Vec<String>,            // 지지해도 남는 빈공간 → 후속 실험 seed
    pub validity: VerdictValidity,    // 레짐/유니버스/시간별 유효 범위 (필수)
}

/// 실험 결과의 유효 범위. 매 verdict마다 강제.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerdictValidity {
    pub regimes_tested: Vec<String>,     // 검증한 레짐 (bull, bear, sideways, crash 등)
    pub regimes_untested: Vec<String>,   // 미검증 레짐
    pub universes_tested: Vec<String>,   // 검증한 유니버스 (all, top50, mid_cap 등)
    pub universes_untested: Vec<String>, // 미검증 유니버스
    pub periods_tested: Vec<String>,     // 검증한 기간 ("2024-01~2025-12" 등)
    pub periods_untested: Vec<String>,   // 미검증 기간
    pub directions_tested: Vec<String>,  // 검증한 방향 (long, short, both)
}

impl Experiment {
    pub fn new(
        reason_id: Uuid,
        method: ExperimentMethod,
        description: String,
        if_true: String,
        if_false: String,
        expected_output: String,
        config: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            reason_id,
            method,
            description,
            if_true,
            if_false,
            expected_output,
            config,
            result: None,
            verdict: None,
            status: ExperimentStatus::Designed,
            evidence_id: None,
            created_at: Utc::now(),
        }
    }
}

// ── Gates ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct GateResult {
    pub passed: bool,
    pub gate: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// CLAIM_GATE: claim must have trigger.
pub fn claim_gate(statement: &str, trigger: &str) -> GateResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if statement.len() < 10 {
        errors.push("claim이 너무 짧음 — 구체적인 명제 필요".into());
    }
    if trigger.is_empty() {
        errors.push("trigger 필수 — 재검토 조건 없으면 confirmation bias 위험".into());
    }

    // Check for vague claims
    let lower = statement.to_lowercase();
    if lower.contains("좋다") || lower.contains("유효하다") || lower.contains("works") {
        warnings.push("모호한 표현 감지 — 구체적인 조건/범위를 명시하면 좋음".into());
    }

    GateResult {
        passed: errors.is_empty(),
        gate: "CLAIM_GATE".into(),
        errors,
        warnings,
    }
}

/// REASON_GATE: reason must have bridge (explicit assumption).
pub fn reason_gate(statement: &str, bridge: &str, reason_type: &str) -> GateResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if statement.len() < 10 {
        errors.push("reason이 너무 짧음".into());
    }
    if bridge.is_empty() {
        errors.push("bridge 필수 — 이 Reason이 Claim을 왜 지지/반박하는지 연결 가정 명시".into());
    }
    if bridge.len() < 15 {
        warnings.push("bridge가 짧음 — '왜'에 대한 구체적 연결 설명 권장".into());
    }
    if reason_type != "support" && reason_type != "rebut" {
        errors.push("reason_type은 'support' 또는 'rebut'만 허용".into());
    }

    GateResult {
        passed: errors.is_empty(),
        gate: "REASON_GATE".into(),
        errors,
        warnings,
    }
}

/// EVIDENCE_GATE: evidence must have valid tag.
pub fn evidence_gate(description: &str, tag: &str) -> GateResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if description.len() < 5 {
        errors.push("evidence 설명이 너무 짧음".into());
    }

    let valid_tags = ["backtest", "live", "intuition", "literature"];
    if !valid_tags.contains(&tag) {
        errors.push(format!("tag는 {:?} 중 하나여야 함", valid_tags));
    }

    if tag == "intuition" {
        warnings.push("intuition 기반 — 가능하면 backtest/live로 검증 추가 권장".into());
    }

    GateResult {
        passed: errors.is_empty(),
        gate: "EVIDENCE_GATE".into(),
        errors,
        warnings,
    }
}

/// EXPERIMENT_GATE: 실험은 falsifiable하고, 방법론이 명확해야 한다.
pub fn experiment_gate(
    method: &str,
    description: &str,
    if_true: &str,
    if_false: &str,
    expected_output: &str,
    config: &serde_json::Value,
    has_prior_analysis: bool,  // 이 Reason에 backtest 아닌 선행 분석이 있는지
) -> GateResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Method 검증
    let valid_methods = [
        "distributional", "conditional", "cross_sectional", "dose_response",
        "regime", "temporal", "multi_horizon", "entry_exit", "backtest",
    ];
    if !valid_methods.contains(&method) {
        errors.push(format!("method는 {:?} 중 하나", valid_methods));
    }

    if description.len() < 10 {
        errors.push("실험 설명이 너무 짧음".into());
    }
    if if_true.is_empty() {
        errors.push("if_true 필수 — Reason이 참이면 어떤 결과가 나와야 하는지".into());
    }
    if if_false.is_empty() {
        errors.push("if_false 필수 — Reason이 거짓이면 어떤 결과가 나와야 하는지".into());
    }
    if if_true == if_false {
        errors.push("if_true와 if_false가 동일 — 반증 불가능한 실험".into());
    }
    if expected_output.is_empty() {
        errors.push("expected_output 필수 — 어떤 통계량/차트를 기대하는지 명시".into());
    }
    if config.is_null() || (config.is_object() && config.as_object().unwrap().is_empty()) {
        errors.push("config 필수 — 실험 파라미터 없이는 재현 불가".into());
    }

    // Backtest 경고: 선행 데이터 분석 없이 백테스트하면 overfitting 위험
    if (method == "backtest" || method == "entry_exit") && !has_prior_analysis {
        warnings.push("메커니즘 검증(distributional/conditional/cross_sectional) 없이 수익률 기반 실험 — overfitting 위험. 선행 데이터 분석 권장".into());
    }

    // Multi-horizon: horizons 설정 확인
    if method == "multi_horizon" {
        if let Some(obj) = config.as_object() {
            if !obj.contains_key("horizons") {
                errors.push("multi_horizon은 config에 horizons 배열 필수".into());
            }
        }
    }

    // Entry-exit: cached_horizons + exit_conditions 확인
    if method == "entry_exit" {
        if let Some(obj) = config.as_object() {
            if !obj.contains_key("cached_horizons") {
                errors.push("entry_exit은 config에 cached_horizons 필수".into());
            }
            if !obj.contains_key("exit_conditions") {
                warnings.push("exit_conditions 미설정 — 고정 horizon exit만 사용됨".into());
            }
        }
    }

    // 숫자만 기대하는 실험 경고
    let lower = if_true.to_lowercase();
    if (lower.contains("bp") || lower.contains("mean")) && !lower.contains("때문") && !lower.contains("구조") && !lower.contains("왜") {
        warnings.push("기대 결과가 숫자 위주 — 경제적 메커니즘 관점의 기대치도 명시 권장".into());
    }

    GateResult {
        passed: errors.is_empty(),
        gate: "EXPERIMENT_GATE".into(),
        errors,
        warnings,
    }
}

/// VERDICT_GATE: verdict는 explanation + validity + gaps 필수.
/// 레짐/유니버스/기간별 유효 범위를 매번 명시해야 한다.
pub fn verdict_gate(
    explanation: &str,
    supports_reason: bool,
    gaps: &[String],
    validity: &VerdictValidity,
    n_experiments_on_reason: usize,
) -> GateResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if explanation.len() < 10 {
        errors.push("explanation이 너무 짧음 — 왜 이렇게 판단했는지 설명 필수".into());
    }

    // Validity 강제: 레짐, 유니버스, 기간 중 하나라도 비어있으면 에러
    if validity.regimes_tested.is_empty() {
        errors.push("regimes_tested 필수 — 어떤 레짐에서 검증했는지 (bull/bear/sideways/crash)".into());
    }
    if validity.universes_tested.is_empty() {
        errors.push("universes_tested 필수 — 어떤 유니버스에서 검증했는지 (all/top50/mid_cap 등)".into());
    }
    if validity.periods_tested.is_empty() {
        errors.push("periods_tested 필수 — 어떤 기간에서 검증했는지".into());
    }

    // 미검증 영역이 비어있으면 경고 — "모든 레짐에서 검증했다"는 보통 과신
    if validity.regimes_untested.is_empty() && !validity.regimes_tested.is_empty() {
        warnings.push("regimes_untested 비어있음 — 정말 모든 레짐에서 검증했는지 확인. 보통은 미검증 영역이 있다".into());
    }
    if validity.universes_untested.is_empty() && !validity.universes_tested.is_empty() {
        warnings.push("universes_untested 비어있음 — 모든 유니버스에서 검증했다면 대단한 것. 재확인 권장".into());
    }

    // 미검증 영역 → gaps seed 제안
    let untested_count = validity.regimes_untested.len() + validity.universes_untested.len() + validity.periods_untested.len();
    if untested_count > 0 && gaps.is_empty() {
        warnings.push(format!("미검증 영역 {}개인데 gaps 0개 — 미검증 영역을 gap으로 추가 권장", untested_count));
    }

    if supports_reason && gaps.is_empty() {
        warnings.push("지지해도 gaps 0개 — 정말 빈공간이 없는지 재확인. 보통은 있다".into());
    }

    // DSR: 같은 Reason에 실험이 너무 많으면 경고
    if n_experiments_on_reason >= 5 {
        warnings.push(format!(
            "이 Reason에 이미 {}회 실험 — 결론 내리거나 Reason 재설정 권장",
            n_experiments_on_reason
        ));
    }
    if n_experiments_on_reason >= 3 {
        let penalty = (n_experiments_on_reason as f64).ln() / (n_experiments_on_reason as f64).sqrt();
        warnings.push(format!("DSR penalty: {:.2} — 추가 실험의 한계 수익 감소", penalty));
    }

    GateResult {
        passed: errors.is_empty(),
        gate: "VERDICT_GATE".into(),
        errors,
        warnings,
    }
}

/// RELATION_GATE: warn if claiming independent but may be correlated.
pub fn relation_gate(relation_type: &str, note: &str) -> GateResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let valid = ["independent", "correlated", "conditional"];
    if !valid.contains(&relation_type) {
        errors.push(format!("relation_type은 {:?} 중 하나", valid));
    }

    if relation_type == "independent" && note.is_empty() {
        warnings.push("independent 주장 시 왜 독립인지 근거 기재 권장 — 같은 데이터 소스면 correlated".into());
    }

    if relation_type == "conditional" && note.is_empty() {
        errors.push("conditional 관계는 조건 설명 필수".into());
    }

    GateResult {
        passed: errors.is_empty(),
        gate: "RELATION_GATE".into(),
        errors,
        warnings,
    }
}

/// Check entire claim structure for completeness.
pub fn structural_check(
    n_support: usize,
    n_rebut: usize,
    n_evidence: usize,
    has_trigger: bool,
    n_correlated: usize,
    n_independent: usize,
) -> GateResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if !has_trigger {
        errors.push("Claim에 trigger 없음".into());
    }
    if n_support == 0 {
        errors.push("support Reason이 0개 — 근거 없는 주장".into());
    }
    if n_rebut == 0 {
        warnings.push("rebut Reason이 0개 — 반론 없이는 confirmation bias 위험. 최소 1개 반론 추가 권장".into());
    }
    if n_evidence == 0 {
        errors.push("Evidence가 0개 — 실험/관찰 근거 필요".into());
    }
    if n_correlated > 0 && n_independent == 0 {
        warnings.push("모든 근거가 correlated — 독립적 근거 추가 필요. 실제 증거 강도가 보이는 것보다 약할 수 있음".into());
    }
    if n_support > 0 && n_rebut == 0 && n_evidence > 3 {
        warnings.push("support 많고 rebut 없음 — 의도적 confirmation bias 가능성. 반론을 적극 탐색".into());
    }

    GateResult {
        passed: errors.is_empty(),
        gate: "STRUCTURAL_CHECK".into(),
        errors,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claim_gate_pass() {
        let r = claim_gate(
            "OI 변화량은 cross-sectional momentum의 선행지표다",
            "거래소가 OI 계산 방식을 변경하면 재검토",
        );
        assert!(r.passed);
    }

    #[test]
    fn test_claim_gate_no_trigger() {
        let r = claim_gate("some claim about momentum", "");
        assert!(!r.passed);
        assert!(r.errors.iter().any(|e| e.contains("trigger")));
    }

    #[test]
    fn test_reason_gate_no_bridge() {
        let r = reason_gate("OI 증가 + 가격 상승 = continuation", "", "support");
        assert!(!r.passed);
        assert!(r.errors.iter().any(|e| e.contains("bridge")));
    }

    #[test]
    fn test_reason_gate_pass() {
        let r = reason_gate(
            "OI 증가 + 가격 상승 조합이 continuation 확률을 높인다",
            "OI 증가 = 신규 포지션 유입 = conviction 있는 참여자 → 추세 지속력 증가",
            "support",
        );
        assert!(r.passed);
    }

    #[test]
    fn test_evidence_gate_bad_tag() {
        let r = evidence_gate("some evidence", "unknown");
        assert!(!r.passed);
    }

    #[test]
    fn test_evidence_gate_intuition_warning() {
        let r = evidence_gate("경험적으로 그렇다", "intuition");
        assert!(r.passed);
        assert!(!r.warnings.is_empty());
    }

    #[test]
    fn test_relation_gate_independent_no_note() {
        let r = relation_gate("independent", "");
        assert!(r.passed); // pass but warning
        assert!(!r.warnings.is_empty());
    }

    #[test]
    fn test_relation_gate_conditional_no_note() {
        let r = relation_gate("conditional", "");
        assert!(!r.passed); // fail — condition required
    }

    #[test]
    fn test_structural_no_support() {
        let r = structural_check(0, 1, 2, true, 0, 1);
        assert!(!r.passed);
    }

    #[test]
    fn test_structural_no_rebut_warning() {
        let r = structural_check(2, 0, 3, true, 0, 2);
        assert!(r.passed);
        assert!(r.warnings.iter().any(|w| w.contains("rebut")));
    }

    #[test]
    fn test_structural_all_correlated_warning() {
        let r = structural_check(2, 1, 3, true, 2, 0);
        assert!(r.passed);
        assert!(r.warnings.iter().any(|w| w.contains("correlated")));
    }
}
