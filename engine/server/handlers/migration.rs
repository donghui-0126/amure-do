/// POI Migration — now delegates to Julia server for heavy computation.

use axum::Json;

pub async fn run_poi_migration() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "Use /api/julia/exec to run migration via Julia server",
        "example": {
            "endpoint": "POST /api/julia/file",
            "body": {"file": "run_poi_research6.jl", "timeout_secs": 300}
        }
    }))
}
