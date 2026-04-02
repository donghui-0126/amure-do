/// Activity feed — 실시간 서버 활동 로그.
/// Ring buffer로 최근 N개 이벤트 유지.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;

const MAX_EVENTS: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub struct ActivityEvent {
    pub id: u64,
    pub timestamp: DateTime<Utc>,
    pub category: String,   // "claim", "reason", "experiment", "verdict", "gate", "llm", "search", "system"
    pub action: String,     // "created", "accepted", "rejected", "failed", "started", "completed"
    pub summary: String,    // 한줄 요약
    pub detail: Option<String>,
}

pub type ActivityLog = Arc<RwLock<ActivityState>>;

pub struct ActivityState {
    events: Vec<ActivityEvent>,
    next_id: u64,
}

impl ActivityState {
    pub fn new() -> Self {
        Self {
            events: Vec::with_capacity(MAX_EVENTS),
            next_id: 1,
        }
    }

    pub fn push(&mut self, category: &str, action: &str, summary: &str, detail: Option<String>) {
        let event = ActivityEvent {
            id: self.next_id,
            timestamp: Utc::now(),
            category: category.into(),
            action: action.into(),
            summary: summary.into(),
            detail,
        };
        self.next_id += 1;
        self.events.push(event);
        if self.events.len() > MAX_EVENTS {
            self.events.drain(0..self.events.len() - MAX_EVENTS);
        }
    }

    /// since_id 이후 이벤트 반환 (polling용)
    pub fn since(&self, since_id: u64) -> Vec<&ActivityEvent> {
        self.events.iter().filter(|e| e.id > since_id).collect()
    }

    /// 최근 N개
    pub fn recent(&self, n: usize) -> Vec<&ActivityEvent> {
        let start = self.events.len().saturating_sub(n);
        self.events[start..].iter().collect()
    }
}
