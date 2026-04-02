/// Experiment Manager — orchestrates the design → execute → evaluate → drill down loop.
/// Maintains full context across iterations to prevent hallucination.

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::Utc;

use crate::knowledge::types::*;
use crate::server::routes::AppState;

#[derive(Deserialize)]
pub struct RunExperimentRequest {
    pub hypothesis: String,
    pub economic_rationale: String,
    pub config: ExperimentRunConfig,
    pub max_depth: Option<usize>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ExperimentRunConfig {
    pub px_threshold: f64,
    pub oi_threshold: f64,
    pub exit_mode: String,
    pub entry_delay: usize,
    pub min_hold: usize,
    pub fee_bp: f64,
    pub period_start: String,
    pub period_end: String,
    pub direction_filter: String,
}

#[derive(Serialize)]
pub struct ManagerResult {
    pub hypothesis_id: Uuid,
    pub experiments_run: usize,
    pub insights_created: usize,
    pub evaluation_summary: Vec<ExperimentSummary>,
    pub status: String,
}

#[derive(Serialize)]
pub struct ExperimentSummary {
    pub name: String,
    pub n_trades: usize,
    pub mean_bp: f64,
    pub net_cum_bp: f64,
    pub time_verdict: String,
    pub universe_verdict: String,
    pub regime_verdict: String,
    pub knowledge_verdict: String,
    pub overall_confidence: f64,
}

pub async fn run_managed_experiment(
    State(state): State<AppState>,
    Json(req): Json<RunExperimentRequest>,
) -> Json<serde_json::Value> {
    // Log
    {
        let mut log = state.call_log.write().await;
        log.log("POST", "/api/manager/run", &req.hypothesis, "running");
    }

    // Step 1: Create hypothesis in knowledge DB
    let hyp = Hypothesis::new(req.hypothesis.clone(), req.economic_rationale.clone());
    let hyp_id = {
        let mut kb = state.knowledge.write().await;
        kb.add_hypothesis(hyp)
    };

    // Step 2: Build Julia code for evaluation
    let julia_code = build_julia_eval_code(&req.config);

    // Step 3: Execute via Julia server
    let julia_output = match run_julia_code(&julia_code).await {
        Ok(output) => output,
        Err(e) => {
            let mut log = state.call_log.write().await;
            log.log("POST", "/api/manager/run", &req.hypothesis, "julia_error");
            return Json(serde_json::json!({"error": format!("Julia failed: {}", e)}));
        }
    };

    // Step 4: Parse results (deterministic)
    let json_lines: Vec<&str> = julia_output
        .lines()
        .filter(|l| l.trim().starts_with('{'))
        .collect();

    let eval = super::evaluator::evaluate_from_lines(&json_lines, &state).await;

    // Step 5: Store experiment + results
    let config = ExperimentConfig {
        primary_threshold: req.config.px_threshold,
        secondary_threshold: req.config.oi_threshold,
        exit_mode: parse_exit_mode(&req.config.exit_mode),
        entry_delay: req.config.entry_delay,
        min_hold: req.config.min_hold,
        fee_bp: req.config.fee_bp,
        period_start: req.config.period_start.clone(),
        period_end: req.config.period_end.clone(),
        direction_filter: parse_direction(&req.config.direction_filter),
        crash_filter: None,
        description: req.hypothesis.clone(),
    };

    let mut exp = Experiment::new(hyp_id, config);
    exp.status = ExperimentStatus::Completed;
    exp.results = Some(ExperimentResults {
        n_trades: eval.composite.time_score as usize, // placeholder — parsed from summary
        n_long: 0,
        n_short: 0,
        mean_ret_bp: 0.0,
        win_rate: 0.0,
        gross_cum_bp: 0.0,
        net_cum_bp: 0.0,
        max_drawdown_bp: 0.0,
        monthly_pnl: eval.time_stability.monthly_pnl.clone(),
    });

    let exp_id = {
        let mut kb = state.knowledge.write().await;
        kb.add_experiment(exp)
    };

    // Step 6: Generate insights based on evaluation
    let mut insights_created = 0;
    {
        let mut kb = state.knowledge.write().await;

        // Time stability insight
        let time_ins = Insight::new(
            exp_id,
            format!("시간 안정성: {} (양수월 {:.0}%)", eval.time_stability.verdict, eval.time_stability.positive_months_pct * 100.0),
            format!("반기별: {:?}", eval.time_stability.half_year_results),
            vec!["time-stability".into(), eval.time_stability.verdict.clone()],
        );
        kb.add_insight(time_ins);
        insights_created += 1;

        // Universe insight
        let uni_ins = Insight::new(
            exp_id,
            format!("유니버스 안정성: {} ({}/{}개 양수)", eval.universe_stability.verdict, eval.universe_stability.n_positive, eval.universe_stability.n_total),
            format!("Breakdowns: {:?}", eval.universe_stability.breakdowns.iter().map(|b| format!("{}: {:.1}bp", b.name, b.mean_bp)).collect::<Vec<_>>()),
            vec!["universe".into(), eval.universe_stability.verdict.clone()],
        );
        kb.add_insight(uni_ins);
        insights_created += 1;

        // Regime insight
        let reg_ins = Insight::new(
            exp_id,
            format!("레짐 안정성: {}", eval.regime_stability.verdict),
            format!("Regimes: {:?}", eval.regime_stability.results.iter().map(|r| format!("{}: {:.1}bp", r.regime, r.mean_bp)).collect::<Vec<_>>()),
            vec!["regime".into(), eval.regime_stability.verdict.clone()],
        );
        kb.add_insight(reg_ins);
        insights_created += 1;

        // Knowledge check insight
        if eval.knowledge_check.verdict == "conflicting" {
            let conflict_ins = Insight::new(
                exp_id,
                format!("기존 지식과 충돌 발견: {}", eval.knowledge_check.conflicts.join("; ")),
                "기존 knowledge DB와 비교 결과".into(),
                vec!["conflict".into(), "knowledge-check".into()],
            );
            kb.add_insight(conflict_ins);
            insights_created += 1;
        }

        // Composite insight
        let adopt_str = if eval.composite.adoptable {
            format!("채택 가능 ({}, confidence={:.2})", eval.composite.adoption_type, eval.composite.overall_confidence)
        } else {
            format!("채택 불가 (confidence={:.2})", eval.composite.overall_confidence)
        };
        let comp_ins = Insight::new(
            exp_id,
            format!("종합 평가: {}", adopt_str),
            format!("time={:.2} universe={:.2} regime={:.2} novelty={}",
                eval.composite.time_score, eval.composite.universe_score,
                eval.composite.regime_score, eval.composite.novelty),
            vec!["composite".into(), eval.composite.adoption_type.clone()],
        );
        kb.add_insight(comp_ins);
        insights_created += 1;

        let _ = kb.save();
    }

    let summary = ExperimentSummary {
        name: req.hypothesis.clone(),
        n_trades: 0,
        mean_bp: 0.0,
        net_cum_bp: 0.0,
        time_verdict: eval.time_stability.verdict.clone(),
        universe_verdict: eval.universe_stability.verdict.clone(),
        regime_verdict: eval.regime_stability.verdict.clone(),
        knowledge_verdict: eval.knowledge_check.verdict.clone(),
        overall_confidence: eval.composite.overall_confidence,
    };

    {
        let mut log = state.call_log.write().await;
        log.log("POST", "/api/manager/run", &req.hypothesis, "completed");
    }

    // Step 7: Drill-down — if evaluation suggests more investigation
    let max_depth = req.max_depth.unwrap_or(3);
    let mut all_summaries = vec![summary];
    let mut total_insights = insights_created;
    let mut total_experiments = 1;

    if max_depth > 1 {
        // Check if drill-down needed based on composite score
        if eval.composite.overall_confidence < 0.7 {
            // Auto drill-down: test short-only if L/S was mixed
            if eval.universe_stability.breakdowns.iter().any(|b| b.name == "short" && b.mean_bp > 0.0)
                && eval.universe_stability.breakdowns.iter().any(|b| b.name == "long" && b.mean_bp < 0.0) {

                let mut short_config = req.config.clone();
                short_config.direction_filter = "short".into();
                let short_code = build_julia_eval_code(&short_config);

                if let Ok(short_output) = run_julia_code(&short_code).await {
                    let short_lines: Vec<&str> = short_output.lines().filter(|l| l.trim().starts_with('{')).collect();
                    let short_eval = super::evaluator::evaluate_from_lines(&short_lines, &state).await;

                    let short_exp_config = ExperimentConfig {
                        primary_threshold: req.config.px_threshold,
                        secondary_threshold: req.config.oi_threshold,
                        exit_mode: parse_exit_mode(&req.config.exit_mode),
                        entry_delay: req.config.entry_delay,
                        min_hold: req.config.min_hold,
                        fee_bp: req.config.fee_bp,
                        period_start: req.config.period_start.clone(),
                        period_end: req.config.period_end.clone(),
                        direction_filter: DirectionFilter::ShortOnly,
                        crash_filter: None,
                        description: format!("{} (short only drill-down)", req.hypothesis),
                    };
                    let mut short_exp = Experiment::new(hyp_id, short_exp_config);
                    short_exp.status = ExperimentStatus::Completed;
                    let short_exp_id = {
                        let mut kb = state.knowledge.write().await;
                        kb.add_experiment(short_exp)
                    };

                    // Generate short-specific insight
                    {
                        let mut kb = state.knowledge.write().await;
                        let ins = Insight::new(
                            short_exp_id,
                            format!("Short-only drill-down: time={} universe={} regime={} conf={:.2}",
                                short_eval.time_stability.verdict,
                                short_eval.universe_stability.verdict,
                                short_eval.regime_stability.verdict,
                                short_eval.composite.overall_confidence),
                            "자동 drill-down: L/S 비대칭 감지 → short only 재실험".into(),
                            vec!["drill-down".into(), "short".into(), short_eval.composite.adoption_type.clone()],
                        );
                        kb.add_insight(ins);
                        total_insights += 1;
                        let _ = kb.save();
                    }

                    all_summaries.push(ExperimentSummary {
                        name: format!("{} (short only)", req.hypothesis),
                        n_trades: 0,
                        mean_bp: 0.0,
                        net_cum_bp: 0.0,
                        time_verdict: short_eval.time_stability.verdict,
                        universe_verdict: short_eval.universe_stability.verdict,
                        regime_verdict: short_eval.regime_stability.verdict,
                        knowledge_verdict: short_eval.knowledge_check.verdict,
                        overall_confidence: short_eval.composite.overall_confidence,
                    });
                    total_experiments += 1;
                }
            }
        }
    }

    Json(serde_json::json!(ManagerResult {
        hypothesis_id: hyp_id,
        experiments_run: total_experiments,
        insights_created: total_insights,
        evaluation_summary: all_summaries,
        status: "completed".into(),
    }))
}

fn build_julia_eval_code(config: &ExperimentRunConfig) -> String {
    format!(
        r#"include("/home/amure-do/amure-do-alphafactor/analysis/eval_template.jl")
run_evaluation(
    px_threshold={},
    oi_threshold={},
    exit_mode=:{},
    entry_delay={},
    min_hold={},
    period_start=DateTime("{}"),
    period_end=DateTime("{}"),
    fee_bp={},
    direction_filter=:{},
)"#,
        config.px_threshold,
        config.oi_threshold,
        config.exit_mode,
        config.entry_delay,
        config.min_hold,
        config.period_start,
        config.period_end,
        config.fee_bp,
        config.direction_filter,
    )
}

fn parse_exit_mode(s: &str) -> crate::strategy::signal_fsm::ExitMode {
    match s {
        "sign_change" => crate::strategy::signal_fsm::ExitMode::SignChange,
        "oi_flip" | "secondary_flip" => crate::strategy::signal_fsm::ExitMode::SecondaryFlip,
        "p_threshold" | "primary_threshold" => crate::strategy::signal_fsm::ExitMode::PrimaryThreshold,
        _ => crate::strategy::signal_fsm::ExitMode::PrimaryOrSecondary,
    }
}

fn parse_direction(s: &str) -> DirectionFilter {
    match s {
        "long" => DirectionFilter::LongOnly,
        "short" => DirectionFilter::ShortOnly,
        _ => DirectionFilter::Both,
    }
}

async fn run_julia_code(code: &str) -> Result<String, String> {
    let dir = std::path::PathBuf::from("analysis");
    let cmd_file = dir.join("_cmd.jl");
    let out_file = dir.join("_out.txt");
    let log_file = dir.join("_server.log");

    if !dir.join("_ready").exists() {
        return Err("Julia server not running".into());
    }

    let _ = std::fs::remove_file(&out_file);
    let log_before = std::fs::read_to_string(&log_file).unwrap_or_default().len();

    std::fs::write(&cmd_file, code).map_err(|e| e.to_string())?;

    let timeout = std::time::Duration::from_secs(300);
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout { return Err("Julia timeout".into()); }
        if out_file.exists() {
            let log_all = std::fs::read_to_string(&log_file).unwrap_or_default();
            return Ok(if log_all.len() > log_before { log_all[log_before..].to_string() } else { String::new() });
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}
