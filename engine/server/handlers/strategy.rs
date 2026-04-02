/// Strategy Generator — combine accumulated knowledge into executable configs.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::server::routes::AppState;

#[derive(Serialize)]
pub struct StrategyConfig {
    pub name: String,
    pub description: String,
    pub knowledge_sources: Vec<String>,
    pub conditions: StrategyConditions,
    pub parameters: serde_json::Value,
    pub confidence: f64,
}

#[derive(Serialize)]
pub struct StrategyConditions {
    pub regimes: Vec<String>,
    pub universes: Vec<String>,
    pub directions: Vec<String>,
    pub required_features: Vec<String>,
}

/// Auto-generate strategy configs from accepted knowledge.
pub async fn generate_strategies(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;

    // Collect accepted insights with their validity info
    let accepted: Vec<_> = kb.insights.values()
        .filter(|i| i.status == crate::knowledge::types::InsightStatus::Accepted)
        .collect();

    let arguments: Vec<_> = kb.insights.values()
        .filter(|i| i.tags.contains(&"argument-for".to_string()))
        .collect();

    if accepted.is_empty() && arguments.is_empty() {
        return Json(serde_json::json!({
            "strategies": [],
            "note": "No accepted knowledge yet. Accept insights first."
        }));
    }

    let mut strategies = Vec::new();

    // Extract regime/universe conditions from arguments
    let mut all_regimes_works: Vec<String> = Vec::new();
    let mut all_regimes_not: Vec<String> = Vec::new();
    let mut all_universe_works: Vec<String> = Vec::new();
    let mut all_directions: Vec<String> = Vec::new();
    let mut sources: Vec<String> = Vec::new();

    for arg in &arguments {
        let evidence = &arg.evidence;
        // Parse validity from evidence text
        if let Some(v_line) = evidence.split('\n').find(|l| l.starts_with("validity:")) {
            let v = v_line.replace("validity: ", "");
            if v.contains("bull") { all_regimes_works.push("bull".into()); }
            if v.contains("sideways") { all_regimes_works.push("sideways".into()); }
            if v.contains("crash") { all_regimes_not.push("crash".into()); }
            if v.contains("large") { all_universe_works.push("large_cap".into()); }
        }
        if arg.tags.contains(&"short".to_string()) { all_directions.push("short".into()); }
        if arg.tags.contains(&"momentum".to_string()) { sources.push(arg.text.chars().take(50).collect::<String>()); }
    }

    // Deduplicate
    all_regimes_works.sort(); all_regimes_works.dedup();
    all_regimes_not.sort(); all_regimes_not.dedup();
    all_universe_works.sort(); all_universe_works.dedup();
    all_directions.sort(); all_directions.dedup();

    if all_directions.is_empty() { all_directions.push("both".into()); }

    // Generate strategy from combined knowledge
    let n_sources = accepted.len() + arguments.len();
    let confidence = (n_sources as f64 / 10.0).min(1.0);

    strategies.push(StrategyConfig {
        name: "Knowledge-based POI Strategy".into(),
        description: format!("Generated from {} accepted insights + {} arguments", accepted.len(), arguments.len()),
        knowledge_sources: sources,
        conditions: StrategyConditions {
            regimes: if all_regimes_works.is_empty() { vec!["all".into()] } else { all_regimes_works },
            universes: if all_universe_works.is_empty() { vec!["all".into()] } else { all_universe_works },
            directions: all_directions,
            required_features: vec!["momentum".into(), "oi".into()],
        },
        parameters: serde_json::json!({
            "px_threshold": 0.1,
            "oi_threshold": 0.25,
            "exit_mode": "oi_flip",
            "min_hold": 72,
            "fee_bp": 9.0,
            "crash_filter_3h": 300,
        }),
        confidence,
    });

    Json(serde_json::json!({
        "strategies": strategies,
        "total_knowledge_used": n_sources,
    }))
}

/// Get Julia code to execute a strategy config.
pub async fn strategy_to_julia(
    Json(config): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let name = config["name"].as_str().unwrap_or("strategy");
    let params = &config["parameters"];
    let conditions = &config["conditions"];

    let directions = conditions["directions"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(","))
        .unwrap_or("both".into());

    let direction_filter = if directions.contains("short") && !directions.contains("long") {
        ":short"
    } else if directions.contains("long") && !directions.contains("short") {
        ":long"
    } else {
        ":both"
    };

    let code = format!(r#"
# Auto-generated strategy: {name}
include("analysis/run_structured.jl")
run_structured_experiment(
    symbol="BTCUSDT",
    data_source="crypto",
    fast_span={fast},
    slow_span={slow},
    fee_bp={fee},
)
"#,
        name = name,
        fast = params["fast_span"].as_u64().unwrap_or(24),
        slow = params["slow_span"].as_u64().unwrap_or(120),
        fee = params["fee_bp"].as_f64().unwrap_or(9.0),
    );

    Json(serde_json::json!({
        "name": name,
        "julia_code": code,
        "direction_filter": direction_filter,
        "note": "POST this code to /api/julia/exec to run",
    }))
}
