/// Monitor — server logs, task status, API call flow.

use axum::Json;
use axum::extract::State;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

const JULIA_LOG: &str = "analysis/_server.log";
const RUST_LOG: &str = "server.log";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCall {
    pub timestamp: String,
    pub method: String,
    pub path: String,
    pub context: String,     // what triggered this
    pub status: String,      // ok, error, pending
}

#[derive(Default, Serialize, Deserialize)]
pub struct CallLog {
    pub calls: Vec<ApiCall>,
}

pub type CallLogStore = Arc<RwLock<CallLog>>;

impl CallLog {
    pub fn log(&mut self, method: &str, path: &str, context: &str, status: &str) {
        self.calls.push(ApiCall {
            timestamp: Utc::now().format("%H:%M:%S").to_string(),
            method: method.into(),
            path: path.into(),
            context: context.into(),
            status: status.into(),
        });
        // Keep last 100
        if self.calls.len() > 100 {
            self.calls.drain(0..self.calls.len() - 100);
        }
    }
}

pub async fn julia_log() -> Json<serde_json::Value> {
    Json(serde_json::json!({"log": read_tail(JULIA_LOG, 80)}))
}

pub async fn rust_log() -> Json<serde_json::Value> {
    Json(serde_json::json!({"log": read_tail(RUST_LOG, 80)}))
}

pub async fn call_log(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let log = state.call_log.read().await;
    Json(serde_json::json!({"calls": log.calls, "count": log.calls.len()}))
}

fn read_tail(path: &str, n_lines: usize) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(n_lines);
            lines[start..].join("\n")
        }
        Err(_) => format!("Cannot read {}", path),
    }
}
