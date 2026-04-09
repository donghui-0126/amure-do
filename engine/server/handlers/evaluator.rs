/// Experiment Evaluator — deterministic Local Proc pipeline.
/// Steps 1-5 are code-only (no LLM). Step 6 (economic interpretation) is LLM.

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::server::routes::AppState;

// ── Input ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EvaluateRequest {
    pub experiment_name: String,
    pub description: String,
    /// Julia code that produces JSON results when executed
    pub julia_code: String,
    /// If true, also run LLM interpretation (Step 6)
    pub include_llm: Option<bool>,
}

// ── Output Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub experiment_name: String,
    pub time_stability: TimeStability,
    pub universe_stability: UniverseStability,
    pub regime_stability: RegimeStability,
    pub knowledge_check: KnowledgeCheck,
    pub composite: CompositeScore,
    pub economic_interpretation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeStability {
    pub monthly_pnl: Vec<(String, f64)>,
    pub positive_months_pct: f64,
    pub half_year_results: Vec<(String, f64)>,
    pub verdict: String, // "stable", "decaying", "volatile", "unstable"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniverseBreakdown {
    pub name: String,
    pub n_trades: usize,
    pub mean_bp: f64,
    pub net_cum_bp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniverseStability {
    pub breakdowns: Vec<UniverseBreakdown>,
    pub n_positive: usize,
    pub n_total: usize,
    pub verdict: String, // "universal", "conditional", "specific"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeResult {
    pub regime: String,
    pub n_trades: usize,
    pub mean_bp: f64,
    pub net_cum_bp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeStability {
    pub results: Vec<RegimeResult>,
    pub verdict: String, // "robust", "dependent", "fragile"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarInsight {
    pub id: String,
    pub text: String,
    pub similarity: f64,
    pub direction_match: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCheck {
    pub similar: Vec<SimilarInsight>,
    pub verdict: String, // "novel", "reinforcing", "conflicting", "extending"
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeScore {
    pub time_score: f64,
    pub universe_score: f64,
    pub regime_score: f64,
    pub novelty: String,
    pub overall_confidence: f64,
    pub adoptable: bool,
    pub adoption_type: String, // "general", "conditional", "specific"
    pub conditions: Vec<String>,
}

/// Public: evaluate from pre-parsed JSON lines (called by Manager).
pub async fn evaluate_from_lines(json_lines: &[&str], state: &AppState) -> EvaluationResult {
    let time = compute_time_stability(json_lines);
    let universe = compute_universe_stability(json_lines);
    let regime = compute_regime_stability(json_lines);
    let knowledge = compute_knowledge_check(state, "").await;
    let composite = compute_composite(&time, &universe, &regime, &knowledge);

    EvaluationResult {
        experiment_name: String::new(),
        time_stability: time,
        universe_stability: universe,
        regime_stability: regime,
        knowledge_check: knowledge,
        composite,
        economic_interpretation: None,
    }
}

// ── Main Handler ─────────────────────────────────────────────────────────────

pub async fn evaluate(
    State(state): State<AppState>,
    Json(req): Json<EvaluateRequest>,
) -> Json<serde_json::Value> {
    // Log this call
    {
        let mut log = state.call_log.write().await;
        log.log("POST", "/api/evaluate", &req.experiment_name, "running");
    }

    // Step 0: Run experiment via Julia
    let julia_result = run_julia(&req.julia_code).await;
    let julia_output = match julia_result {
        Ok(output) => output,
        Err(e) => return Json(serde_json::json!({"error": format!("Julia exec failed: {}", e)})),
    };

    // Parse Julia output as JSON lines (each step outputs a JSON object)
    let lines: Vec<&str> = julia_output.lines().filter(|l| l.starts_with('{') || l.starts_with('[')).collect();

    // Step 1: Time Stability (from Julia monthly data)
    let time = compute_time_stability(&lines);

    // Step 2: Universe Stability
    let universe = compute_universe_stability(&lines);

    // Step 3: Regime Stability
    let regime = compute_regime_stability(&lines);

    // Step 4: Knowledge Check (local — Rust vector DB)
    let knowledge = compute_knowledge_check(&state, &req.description).await;

    // Step 5: Composite Score
    let composite = compute_composite(&time, &universe, &regime, &knowledge);

    // Step 6: LLM interpretation (optional)
    let interpretation = if req.include_llm.unwrap_or(false) {
        let prompt = format!(
            "실험 '{}' 평가 결과를 경제적으로 해석해줘. 한국어로, 간결하게.\n\n\
             시간안정성: {} (양수월 {:.0}%)\n\
             유니버스: {} ({}/{}개 양수)\n\
             레짐: {}\n\
             기존지식: {}\n\
             종합 confidence: {:.2}\n\n\
             1. 숨은 전제를 찾아줘\n\
             2. 강화 논증 2개\n\
             3. 약화 논증 2개\n\
             4. 레짐/유니버스별 차이의 이유",
            req.experiment_name,
            time.verdict, time.positive_months_pct * 100.0,
            universe.verdict, universe.n_positive, universe.n_total,
            regime.verdict,
            knowledge.verdict,
            composite.overall_confidence,
        );
        run_llm(&prompt).await.ok()
    } else {
        None
    };

    let result = EvaluationResult {
        experiment_name: req.experiment_name.clone(),
        time_stability: time,
        universe_stability: universe,
        regime_stability: regime,
        knowledge_check: knowledge,
        composite,
        economic_interpretation: interpretation,
    };

    // Log completion
    {
        let mut log = state.call_log.write().await;
        log.log("POST", "/api/evaluate", &req.experiment_name, "completed");
    }

    Json(serde_json::json!(result))
}

// ── Step 1: Time Stability (deterministic) ───────────────────────────────────

fn compute_time_stability(json_lines: &[&str]) -> TimeStability {
    // Try to parse monthly data from Julia output
    let monthly: Vec<(String, f64)> = json_lines.iter()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            let m = v.get("monthly")?.as_array()?;
            Some(m.iter().filter_map(|item| {
                let month = item.get("month")?.as_str()?.to_string();
                let pnl = item.get("net_bp")?.as_f64()?;
                Some((month, pnl))
            }).collect::<Vec<_>>())
        })
        .flatten()
        .collect();

    if monthly.is_empty() {
        return TimeStability {
            monthly_pnl: vec![],
            positive_months_pct: 0.0,
            half_year_results: vec![],
            verdict: "no_data".into(),
        };
    }

    let n = monthly.len();
    let n_positive = monthly.iter().filter(|(_, v)| *v > 0.0).count();
    let pos_pct = n_positive as f64 / n as f64;

    // Half-year aggregation
    let mut halves: Vec<(String, f64)> = Vec::new();
    let mut current_half = String::new();
    let mut cum = 0.0;
    for (m, v) in &monthly {
        let half = if m.ends_with("-01") || m.ends_with("-02") || m.ends_with("-03")
            || m.ends_with("-04") || m.ends_with("-05") || m.ends_with("-06") {
            format!("{}-H1", &m[..4])
        } else {
            format!("{}-H2", &m[..4])
        };
        if half != current_half && !current_half.is_empty() {
            halves.push((current_half.clone(), cum));
            cum = 0.0;
        }
        current_half = half;
        cum += v;
    }
    if !current_half.is_empty() {
        halves.push((current_half, cum));
    }

    let n_half_positive = halves.iter().filter(|(_, v)| *v > 0.0).count();

    let verdict = if pos_pct >= 0.7 && n_half_positive == halves.len() {
        "stable"
    } else if n >= 6 {
        // Check decaying: last 3 months vs first 3 months
        let last3: f64 = monthly[n-3..].iter().map(|(_, v)| v).sum();
        let first3: f64 = monthly[..3].iter().map(|(_, v)| v).sum();
        if last3 < first3 * 0.3 { "decaying" }
        else if pos_pct < 0.5 { "unstable" }
        else { "volatile" }
    } else if pos_pct < 0.5 {
        "unstable"
    } else {
        "volatile"
    };

    TimeStability {
        monthly_pnl: monthly,
        positive_months_pct: pos_pct,
        half_year_results: halves,
        verdict: verdict.into(),
    }
}

// ── Step 2: Universe Stability (deterministic) ───────────────────────────────

fn compute_universe_stability(json_lines: &[&str]) -> UniverseStability {
    let breakdowns: Vec<UniverseBreakdown> = json_lines.iter()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            let u = v.get("universes")?.as_array()?;
            Some(u.iter().filter_map(|item| {
                Some(UniverseBreakdown {
                    name: item.get("name")?.as_str()?.to_string(),
                    n_trades: item.get("n_trades")?.as_u64()? as usize,
                    mean_bp: item.get("mean_bp")?.as_f64()?,
                    net_cum_bp: item.get("net_cum_bp")?.as_f64()?,
                })
            }).collect::<Vec<_>>())
        })
        .flatten()
        .collect();

    let n_total = breakdowns.len();
    let n_positive = breakdowns.iter().filter(|b| b.mean_bp > 0.0).count();

    let verdict = if n_total == 0 { "no_data" }
    else if n_positive as f64 / n_total as f64 >= 0.7 { "universal" }
    else if n_positive >= 2 { "conditional" }
    else { "specific" };

    UniverseStability {
        breakdowns,
        n_positive,
        n_total,
        verdict: verdict.into(),
    }
}

// ── Step 3: Regime Stability (deterministic) ─────────────────────────────────

fn compute_regime_stability(json_lines: &[&str]) -> RegimeStability {
    let results: Vec<RegimeResult> = json_lines.iter()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            let r = v.get("regimes")?.as_array()?;
            Some(r.iter().filter_map(|item| {
                Some(RegimeResult {
                    regime: item.get("regime")?.as_str()?.to_string(),
                    n_trades: item.get("n_trades")?.as_u64()? as usize,
                    mean_bp: item.get("mean_bp")?.as_f64()?,
                    net_cum_bp: item.get("net_cum_bp")?.as_f64()?,
                })
            }).collect::<Vec<_>>())
        })
        .flatten()
        .collect();

    let n_total = results.len();
    let n_positive = results.iter().filter(|r| r.mean_bp > 0.0).count();

    let verdict = if n_total == 0 { "no_data" }
    else if n_positive as f64 / n_total as f64 >= 0.75 { "robust" }
    else if n_positive >= 2 { "dependent" }
    else { "fragile" };

    RegimeStability {
        results,
        verdict: verdict.into(),
    }
}

// ── Step 4: Knowledge Check (Rust local — Vector DB) ─────────────────────────

async fn compute_knowledge_check(state: &AppState, description: &str) -> KnowledgeCheck {
    let kb = state.knowledge.read().await;

    // Text-based search for similar insights
    let similar_results = kb.search_by_text(description, 5);

    let similar: Vec<SimilarInsight> = similar_results.iter().map(|r| {
        SimilarInsight {
            id: r.insight_id.to_string(),
            text: r.text.clone(),
            similarity: r.score,
            direction_match: true, // simplified — full version would compare conclusions
        }
    }).collect();

    let mut conflicts = Vec::new();

    // Check for conflicting insights
    for s in &similar {
        if !s.direction_match {
            conflicts.push(format!("Conflicts with: {}", s.text));
        }
    }

    let verdict = if similar.is_empty() {
        "novel"
    } else if !conflicts.is_empty() {
        "conflicting"
    } else if similar.iter().any(|s| s.similarity > 0.8) {
        "reinforcing"
    } else {
        "extending"
    };

    KnowledgeCheck {
        similar,
        verdict: verdict.into(),
        conflicts,
    }
}

// ── Step 5: Composite Score (deterministic) ──────────────────────────────────

fn compute_composite(
    time: &TimeStability,
    universe: &UniverseStability,
    regime: &RegimeStability,
    knowledge: &KnowledgeCheck,
) -> CompositeScore {
    let time_score = match time.verdict.as_str() {
        "stable" => 1.0,
        "volatile" => 0.5,
        "decaying" => 0.3,
        "unstable" => 0.1,
        _ => 0.0,
    };

    let universe_score = if universe.n_total == 0 { 0.0 }
    else { universe.n_positive as f64 / universe.n_total as f64 };

    let regime_score = match regime.verdict.as_str() {
        "robust" => 1.0,
        "dependent" => 0.5,
        "fragile" => 0.2,
        _ => 0.0,
    };

    let overall = (time_score * 0.35 + universe_score * 0.35 + regime_score * 0.3).min(1.0);

    let (adoptable, adoption_type) = if overall >= 0.7 {
        (true, "general")
    } else if overall >= 0.4 {
        (true, "conditional")
    } else if overall >= 0.2 {
        (false, "specific")
    } else {
        (false, "reject")
    };

    let mut conditions = Vec::new();
    if universe.verdict == "conditional" {
        for b in &universe.breakdowns {
            if b.mean_bp > 0.0 {
                conditions.push(format!("{}: +{:.1}bp", b.name, b.mean_bp));
            }
        }
    }
    if regime.verdict == "dependent" {
        for r in &regime.results {
            if r.mean_bp > 0.0 {
                conditions.push(format!("regime_{}: +{:.1}bp", r.regime, r.mean_bp));
            }
        }
    }

    CompositeScore {
        time_score,
        universe_score,
        regime_score,
        novelty: knowledge.verdict.clone(),
        overall_confidence: overall,
        adoptable,
        adoption_type: adoption_type.into(),
        conditions,
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn run_julia(code: &str) -> Result<String, String> {
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
        if start.elapsed() > timeout {
            return Err("Julia timeout".into());
        }
        if out_file.exists() {
            let log_all = std::fs::read_to_string(&log_file).unwrap_or_default();
            let log_new = if log_all.len() > log_before {
                log_all[log_before..].to_string()
            } else {
                String::new()
            };
            return Ok(log_new);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

async fn run_llm(prompt: &str) -> Result<String, String> {
    let output = std::process::Command::new("claude")
        .args(["-p", prompt])
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).to_string()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => Err(e.to_string()),
    }
}
