/// Adaptive Mode — track user disagreements, learn preferences, recursive experiments.

use axum::extract::State;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const PROFILE_FILE: &str = "data/knowledge_db/user_profile.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserProfile {
    pub adaptive_mode: bool,
    pub disagreements: Vec<Disagreement>,
    pub tendencies: HashMap<String, String>,  // key → description
    pub hooks: Vec<ProfileHook>,              // hooks derived from behavior
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disagreement {
    pub timestamp: String,
    pub context: String,       // what Claude suggested
    pub user_action: String,   // what user did instead
    pub user_reason: String,   // why
    pub category: String,      // "accept_vs_reject", "experiment_design", "canvas_edit"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileHook {
    pub trigger: String,       // when this hook fires
    pub instruction: String,   // what to do
    pub source: String,        // which disagreement(s) led to this
    pub active: bool,
}

impl UserProfile {
    pub fn load() -> Self {
        std::fs::read_to_string(PROFILE_FILE)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(PROFILE_FILE, json);
        }
    }

    /// Analyze disagreements and extract tendencies.
    pub fn analyze_tendencies(&mut self) {
        let n = self.disagreements.len();
        if n < 2 { return; }

        // Count categories
        let mut cats: HashMap<String, usize> = HashMap::new();
        for d in &self.disagreements {
            *cats.entry(d.category.clone()).or_default() += 1;
        }

        // Extract patterns
        self.tendencies.clear();

        // Check if user tends to accept more than reject
        let accepts = self.disagreements.iter().filter(|d| d.user_action.contains("accept")).count();
        let rejects = self.disagreements.iter().filter(|d| d.user_action.contains("reject")).count();
        if accepts > rejects * 2 {
            self.tendencies.insert("verdict_bias".into(), "유저는 채택 경향이 강함 — 보수적 기각 기준 적용 필요".into());
        } else if rejects > accepts * 2 {
            self.tendencies.insert("verdict_bias".into(), "유저는 기각 경향이 강함 — 더 강한 근거가 필요".into());
        }

        // Check common reasons
        let reasons: Vec<&str> = self.disagreements.iter().map(|d| d.user_reason.as_str()).collect();
        if reasons.iter().any(|r| r.contains("경제적") || r.contains("메커니즘")) {
            self.tendencies.insert("focus".into(), "유저는 경제적 메커니즘을 중시함 — 숫자보다 논리".into());
        }
        if reasons.iter().any(|r| r.contains("일반화") || r.contains("유니버스")) {
            self.tendencies.insert("generalization".into(), "유저는 일반화 가능성을 중시함 — 특수 케이스에 회의적".into());
        }

        // Generate hooks from tendencies
        self.hooks.clear();
        for (key, desc) in &self.tendencies {
            self.hooks.push(ProfileHook {
                trigger: format!("experiment_design (tendency: {})", key),
                instruction: desc.clone(),
                source: format!("{} disagreements analyzed", n),
                active: true,
            });
        }
    }
}

// ── API Handlers ────────────────────────────────────────────────────────────

pub async fn get_profile(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let profile = state.profile.read().await;
    Json(serde_json::json!(&*profile))
}

pub async fn toggle_adaptive(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let mut profile = state.profile.write().await;
    profile.adaptive_mode = !profile.adaptive_mode;
    profile.save();
    Json(serde_json::json!({"adaptive_mode": profile.adaptive_mode}))
}

#[derive(Deserialize)]
pub struct RecordDisagreement {
    pub context: String,
    pub user_action: String,
    pub user_reason: String,
    pub category: String,
}

pub async fn record_disagreement(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<RecordDisagreement>,
) -> Json<serde_json::Value> {
    let mut profile = state.profile.write().await;
    if !profile.adaptive_mode {
        return Json(serde_json::json!({"status": "adaptive mode off — not recording"}));
    }

    profile.disagreements.push(Disagreement {
        timestamp: Utc::now().to_rfc3339(),
        context: req.context,
        user_action: req.user_action.clone(),
        user_reason: req.user_reason,
        category: req.category,
    });

    profile.analyze_tendencies();
    profile.save();

    // Log
    let mut log = state.call_log.write().await;
    log.log("ADAPTIVE", "disagreement", &req.user_action, "recorded");

    Json(serde_json::json!({
        "status": "recorded",
        "total_disagreements": profile.disagreements.len(),
        "tendencies": profile.tendencies,
        "hooks": profile.hooks,
    }))
}

#[derive(Deserialize)]
pub struct UpdateTendency {
    pub key: String,
    pub value: String,
}

pub async fn update_tendency(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<UpdateTendency>,
) -> Json<serde_json::Value> {
    let mut profile = state.profile.write().await;
    profile.tendencies.insert(req.key.clone(), req.value.clone());
    profile.save();
    Json(serde_json::json!({"status": "updated", "key": req.key}))
}

#[derive(Deserialize)]
pub struct UpdateHook {
    pub index: usize,
    pub active: Option<bool>,
    pub instruction: Option<String>,
}

pub async fn update_hook(
    State(state): State<crate::server::routes::AppState>,
    Json(req): Json<UpdateHook>,
) -> Json<serde_json::Value> {
    let mut profile = state.profile.write().await;
    if req.index >= profile.hooks.len() {
        return Json(serde_json::json!({"error": "Hook index out of range"}));
    }
    if let Some(active) = req.active {
        profile.hooks[req.index].active = active;
    }
    if let Some(inst) = &req.instruction {
        profile.hooks[req.index].instruction = inst.clone();
    }
    profile.save();
    Json(serde_json::json!({"status": "updated"}))
}

/// Get active hooks as context string (for injection into experiment design).
pub async fn get_active_hooks_context(
    State(state): State<crate::server::routes::AppState>,
) -> Json<serde_json::Value> {
    let profile = state.profile.read().await;
    let ctx: Vec<String> = profile.hooks.iter()
        .filter(|h| h.active)
        .map(|h| format!("[HOOK: {}] {}", h.trigger, h.instruction))
        .collect();

    Json(serde_json::json!({
        "adaptive_mode": profile.adaptive_mode,
        "active_hooks": ctx,
        "tendencies": profile.tendencies,
    }))
}
