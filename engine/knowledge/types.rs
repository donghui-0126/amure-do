use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::strategy::signal_fsm::ExitMode;

// ── Maturity ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Maturity {
    /// 경제적 의미 + 통계적 검증 + 유저 승인
    Mature,
    /// 탐색 중 / 미검증 / 근거 불분명
    Unmature,
}

// ── Hypothesis ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HypothesisStatus {
    Active,
    Validated,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: Uuid,
    pub title: String,
    pub economic_rationale: String,
    pub status: HypothesisStatus,
    pub maturity: Maturity,
    pub created_at: DateTime<Utc>,
    pub user_notes: Vec<UserNote>,
    pub experiment_ids: Vec<Uuid>,
}

impl Hypothesis {
    pub fn new(title: String, economic_rationale: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            title,
            economic_rationale,
            status: HypothesisStatus::Active,
            maturity: Maturity::Unmature,
            created_at: Utc::now(),
            user_notes: Vec::new(),
            experiment_ids: Vec::new(),
        }
    }
}

// ── Experiment ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExperimentStatus {
    Planned,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentConfig {
    pub primary_threshold: f64,
    pub secondary_threshold: f64,
    pub exit_mode: ExitMode,
    pub entry_delay: usize,
    pub min_hold: usize,
    pub fee_bp: f64,
    pub period_start: String,
    pub period_end: String,
    pub direction_filter: DirectionFilter,
    pub crash_filter: Option<CrashFilter>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DirectionFilter {
    Both,
    LongOnly,
    ShortOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashFilter {
    pub long_max_move_bp: f64,
    pub short_max_move_bp: f64,
    pub lookback_bars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentResults {
    pub n_trades: usize,
    pub n_long: usize,
    pub n_short: usize,
    pub mean_ret_bp: f64,
    pub win_rate: f64,
    pub gross_cum_bp: f64,
    pub net_cum_bp: f64,
    pub max_drawdown_bp: f64,
    pub monthly_pnl: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub id: Uuid,
    pub hypothesis_id: Uuid,
    pub description: String,
    pub config: ExperimentConfig,
    pub results: Option<ExperimentResults>,
    pub context_snapshot: Vec<Uuid>,
    pub insight_ids: Vec<Uuid>,
    pub status: ExperimentStatus,
    pub created_at: DateTime<Utc>,
}

impl Experiment {
    pub fn new(hypothesis_id: Uuid, config: ExperimentConfig) -> Self {
        let desc = config.description.clone();
        Self {
            id: Uuid::new_v4(),
            hypothesis_id,
            description: desc,
            config,
            results: None,
            context_snapshot: Vec::new(),
            insight_ids: Vec::new(),
            status: ExperimentStatus::Planned,
            created_at: Utc::now(),
        }
    }
}

// ── Insight ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum InsightStatus {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserNote {
    pub text: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserVerdict {
    pub decision: InsightStatus,
    pub reason: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    pub id: Uuid,
    pub experiment_id: Uuid,
    pub text: String,
    pub evidence: String,
    pub status: InsightStatus,
    pub maturity: Maturity,
    pub confidence: f64,
    pub tags: Vec<String>,
    pub embedding: Vec<f32>,
    pub user_verdict: Option<UserVerdict>,
    pub created_at: DateTime<Utc>,
}

impl Insight {
    pub fn new(
        experiment_id: Uuid,
        text: String,
        evidence: String,
        tags: Vec<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            experiment_id,
            text,
            evidence,
            status: InsightStatus::Pending,
            maturity: Maturity::Unmature,
            confidence: 0.0,
            tags,
            embedding: Vec::new(),
            user_verdict: None,
            created_at: Utc::now(),
        }
    }

    pub fn accept(&mut self, reason: String) {
        self.status = InsightStatus::Accepted;
        self.user_verdict = Some(UserVerdict {
            decision: InsightStatus::Accepted,
            reason,
            at: Utc::now(),
        });
    }

    pub fn reject(&mut self, reason: String) {
        self.status = InsightStatus::Rejected;
        self.user_verdict = Some(UserVerdict {
            decision: InsightStatus::Rejected,
            reason,
            at: Utc::now(),
        });
    }

    pub fn promote(&mut self) {
        self.maturity = Maturity::Mature;
    }

    pub fn demote(&mut self) {
        self.maturity = Maturity::Unmature;
    }
}
