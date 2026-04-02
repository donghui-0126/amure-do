use axum::extract::State;
use axum::response::Html;
use axum::Json;

use crate::server::routes::AppState;

const DASHBOARD_HTML: &str = include_str!("../../dashboard/index.html");

pub async fn serve_dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

pub async fn overview(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;
    let summary = kb.summary();

    // Check Julia server status
    let julia_alive = std::path::Path::new("analysis/_ready").exists();

    Json(serde_json::json!({
        "knowledge": summary,
        "julia_server": {
            "alive": julia_alive,
        },
    }))
}

pub async fn knowledge_tree(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let kb = state.knowledge.read().await;

    let tree: Vec<serde_json::Value> = kb
        .list_hypotheses()
        .iter()
        .map(|h| {
            let experiments: Vec<serde_json::Value> = kb
                .experiments_for_hypothesis(&h.id)
                .iter()
                .map(|e| {
                    let insights: Vec<serde_json::Value> = kb
                        .insights_for_experiment(&e.id)
                        .iter()
                        .map(|i| {
                            serde_json::json!({
                                "id": i.id,
                                "text": i.text,
                                "status": i.status,
                                "maturity": i.maturity,
                                "confidence": i.confidence,
                            })
                        })
                        .collect();

                    serde_json::json!({
                        "id": e.id,
                        "description": e.description,
                        "status": e.status,
                        "results_summary": e.results.as_ref().map(|r| serde_json::json!({
                            "n_trades": r.n_trades,
                            "mean_ret_bp": r.mean_ret_bp,
                            "net_cum_bp": r.net_cum_bp,
                        })),
                        "insights": insights,
                    })
                })
                .collect();

            serde_json::json!({
                "id": h.id,
                "title": h.title,
                "status": h.status,
                "maturity": h.maturity,
                "economic_rationale": h.economic_rationale,
                "experiments": experiments,
            })
        })
        .collect();

    Json(serde_json::json!({"tree": tree}))
}
