/// Knowledge DB — JSON file storage + in-memory index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::types::*;
use super::framework::{Claim, ClaimStatus, Reason, Experiment as FwExperiment, ExperimentStatus as FwExpStatus, ExperimentVerdict, Evidence as FwEvidence, EvidenceTag};
use super::vector_index::VectorIndex;
use super::embedding::{TfIdfVectorizer, KeywordGraph};

pub struct KnowledgeDB {
    dir: PathBuf,
    pub hypotheses: HashMap<Uuid, Hypothesis>,
    pub experiments: HashMap<Uuid, Experiment>,
    pub insights: HashMap<Uuid, Insight>,
    pub claims: HashMap<Uuid, Claim>,
    pub reasons: HashMap<Uuid, Reason>,
    pub fw_experiments: HashMap<Uuid, FwExperiment>,
    pub vector_index: VectorIndex,
    pub vectorizer: Option<TfIdfVectorizer>,
    pub keyword_graph: KeywordGraph,
}

impl KnowledgeDB {
    /// Create or load a KnowledgeDB from a directory.
    pub fn open(dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(dir)?;

        let hypotheses = Self::load_json::<HashMap<Uuid, Hypothesis>>(
            &dir.join("hypotheses.json"),
        )
        .unwrap_or_default();

        let experiments = Self::load_json::<HashMap<Uuid, Experiment>>(
            &dir.join("experiments.json"),
        )
        .unwrap_or_default();

        let insights = Self::load_json::<HashMap<Uuid, Insight>>(
            &dir.join("insights.json"),
        )
        .unwrap_or_default();

        let claims = Self::load_json::<HashMap<Uuid, Claim>>(
            &dir.join("claims.json"),
        )
        .unwrap_or_default();

        let reasons = Self::load_json::<HashMap<Uuid, Reason>>(
            &dir.join("reasons.json"),
        )
        .unwrap_or_default();

        let fw_experiments = Self::load_json::<HashMap<Uuid, FwExperiment>>(
            &dir.join("fw_experiments.json"),
        )
        .unwrap_or_default();

        // Rebuild vector index from insights
        // Using a simple fixed dimension — 0 means no embeddings yet
        let dim = insights
            .values()
            .find(|i| !i.embedding.is_empty())
            .map(|i| i.embedding.len())
            .unwrap_or(0);

        let mut vector_index = VectorIndex::new(if dim > 0 { dim } else { 384 }); // default 384
        for (id, insight) in &insights {
            if !insight.embedding.is_empty() {
                vector_index.upsert(*id, insight.embedding.clone());
            }
        }

        let n_knowledge = claims.values().filter(|c| c.is_knowledge()).count();
        tracing::info!(
            "KnowledgeDB loaded: {} hypotheses, {} experiments, {} insights, {} claims ({} knowledge), {} reasons, {} fw_experiments",
            hypotheses.len(),
            experiments.len(),
            insights.len(),
            claims.len(),
            n_knowledge,
            reasons.len(),
            fw_experiments.len(),
        );

        // Build TF-IDF vectorizer from existing insights
        let docs: Vec<String> = insights.values()
            .map(|i| format!("{} {}", i.text, i.tags.join(" ")))
            .collect();
        let vectorizer = if docs.len() >= 2 {
            let v = TfIdfVectorizer::fit(&docs);
            // Re-embed all insights
            let new_dim = v.dim();
            let mut new_index = VectorIndex::new(new_dim);
            for (id, ins) in &insights {
                let emb = v.transform(&format!("{} {}", ins.text, ins.tags.join(" ")));
                new_index.upsert(*id, emb);
            }
            vector_index = new_index;
            Some(v)
        } else {
            None
        };

        // Build keyword graph
        let mut keyword_graph = KeywordGraph::new();
        for (id, ins) in &insights {
            keyword_graph.add(*id, &ins.tags);
        }

        Ok(Self {
            dir: dir.to_path_buf(),
            hypotheses,
            experiments,
            insights,
            claims,
            reasons,
            fw_experiments,
            vector_index,
            vectorizer,
            keyword_graph,
        })
    }

    /// Save all data to disk.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        Self::save_json(&self.dir.join("hypotheses.json"), &self.hypotheses)?;
        Self::save_json(&self.dir.join("experiments.json"), &self.experiments)?;
        Self::save_json(&self.dir.join("insights.json"), &self.insights)?;
        Self::save_json(&self.dir.join("claims.json"), &self.claims)?;
        Self::save_json(&self.dir.join("reasons.json"), &self.reasons)?;
        Self::save_json(&self.dir.join("fw_experiments.json"), &self.fw_experiments)?;
        Ok(())
    }

    // ── Hypothesis CRUD ─────────────────────────────────────────────────

    pub fn add_hypothesis(&mut self, h: Hypothesis) -> Uuid {
        let id = h.id;
        self.hypotheses.insert(id, h);
        id
    }

    pub fn get_hypothesis(&self, id: &Uuid) -> Option<&Hypothesis> {
        self.hypotheses.get(id)
    }

    pub fn list_hypotheses(&self) -> Vec<&Hypothesis> {
        let mut list: Vec<&Hypothesis> = self.hypotheses.values().collect();
        list.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        list
    }

    // ── Experiment CRUD ─────────────────────────────────────────────────

    pub fn add_experiment(&mut self, exp: Experiment) -> Uuid {
        let id = exp.id;
        let hyp_id = exp.hypothesis_id;
        self.experiments.insert(id, exp);
        // Link to hypothesis
        if let Some(h) = self.hypotheses.get_mut(&hyp_id) {
            if !h.experiment_ids.contains(&id) {
                h.experiment_ids.push(id);
            }
        }
        id
    }

    pub fn get_experiment(&self, id: &Uuid) -> Option<&Experiment> {
        self.experiments.get(id)
    }

    pub fn get_experiment_mut(&mut self, id: &Uuid) -> Option<&mut Experiment> {
        self.experiments.get_mut(id)
    }

    pub fn experiments_for_hypothesis(&self, hyp_id: &Uuid) -> Vec<&Experiment> {
        self.experiments
            .values()
            .filter(|e| e.hypothesis_id == *hyp_id)
            .collect()
    }

    // ── Insight CRUD ────────────────────────────────────────────────────

    pub fn add_insight(&mut self, mut insight: Insight) -> Uuid {
        let id = insight.id;
        let exp_id = insight.experiment_id;

        // Auto-generate embedding if vectorizer exists
        if let Some(ref vectorizer) = self.vectorizer {
            let emb = vectorizer.transform(&format!("{} {}", insight.text, insight.tags.join(" ")));
            insight.embedding = emb.clone();
            self.vector_index.upsert(id, emb);
        } else if !insight.embedding.is_empty() {
            self.vector_index.upsert(id, insight.embedding.clone());
        }

        // Update keyword graph
        self.keyword_graph.add(id, &insight.tags);

        self.insights.insert(id, insight);
        // Link to experiment
        if let Some(exp) = self.experiments.get_mut(&exp_id) {
            if !exp.insight_ids.contains(&id) {
                exp.insight_ids.push(id);
            }
        }
        id
    }

    pub fn get_insight(&self, id: &Uuid) -> Option<&Insight> {
        self.insights.get(id)
    }

    pub fn get_insight_mut(&mut self, id: &Uuid) -> Option<&mut Insight> {
        self.insights.get_mut(id)
    }

    pub fn accept_insight(&mut self, id: &Uuid, reason: String) -> bool {
        if let Some(ins) = self.insights.get_mut(id) {
            ins.accept(reason);
            true
        } else {
            false
        }
    }

    pub fn reject_insight(&mut self, id: &Uuid, reason: String) -> bool {
        if let Some(ins) = self.insights.get_mut(id) {
            ins.reject(reason);
            true
        } else {
            false
        }
    }

    pub fn promote_insight(&mut self, id: &Uuid) -> bool {
        if let Some(ins) = self.insights.get_mut(id) {
            ins.promote();
            true
        } else {
            false
        }
    }

    pub fn pending_insights(&self) -> Vec<&Insight> {
        self.insights
            .values()
            .filter(|i| i.status == InsightStatus::Pending)
            .collect()
    }

    pub fn insights_for_experiment(&self, exp_id: &Uuid) -> Vec<&Insight> {
        self.insights
            .values()
            .filter(|i| i.experiment_id == *exp_id)
            .collect()
    }

    // ── Claim CRUD (Framework) ────────────────────────────────────────

    pub fn add_claim(&mut self, claim: Claim) -> Uuid {
        let id = claim.id;
        self.claims.insert(id, claim);
        id
    }

    pub fn get_claim(&self, id: &Uuid) -> Option<&Claim> {
        self.claims.get(id)
    }

    pub fn get_claim_mut(&mut self, id: &Uuid) -> Option<&mut Claim> {
        self.claims.get_mut(id)
    }

    /// Draft 상태 Claim 목록
    pub fn draft_claims(&self) -> Vec<&Claim> {
        let mut list: Vec<&Claim> = self.claims.values()
            .filter(|c| c.status == ClaimStatus::Draft)
            .collect();
        list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        list
    }

    /// Accepted = Knowledge. 확정된 지식 목록
    pub fn knowledge(&self) -> Vec<&Claim> {
        let mut list: Vec<&Claim> = self.claims.values()
            .filter(|c| c.is_knowledge())
            .collect();
        list.sort_by(|a, b| b.accepted_at.cmp(&a.accepted_at));
        list
    }

    /// Claim을 Knowledge로 격상
    pub fn accept_claim(&mut self, id: &Uuid, reason: String) -> bool {
        if let Some(claim) = self.claims.get_mut(id) {
            claim.status = ClaimStatus::Accepted;
            claim.accepted_at = Some(chrono::Utc::now());
            claim.accept_reason = Some(reason);
            claim.updated_at = chrono::Utc::now();
            true
        } else {
            false
        }
    }

    /// Claim을 기각
    pub fn reject_claim(&mut self, id: &Uuid, reason: String) -> bool {
        if let Some(claim) = self.claims.get_mut(id) {
            claim.status = ClaimStatus::Rejected;
            claim.accept_reason = Some(reason);
            claim.updated_at = chrono::Utc::now();
            true
        } else {
            false
        }
    }

    // ── Reason CRUD ────────────────────────────────────────────────────

    pub fn add_reason(&mut self, reason: Reason) -> Uuid {
        let id = reason.id;
        let claim_id = reason.claim_id;
        self.reasons.insert(id, reason);
        // Link to claim
        if let Some(claim) = self.claims.get_mut(&claim_id) {
            if !claim.reasons.contains(&id) {
                claim.reasons.push(id);
                claim.updated_at = chrono::Utc::now();
            }
        }
        id
    }

    pub fn get_reason(&self, id: &Uuid) -> Option<&Reason> {
        self.reasons.get(id)
    }

    pub fn get_reason_mut(&mut self, id: &Uuid) -> Option<&mut Reason> {
        self.reasons.get_mut(id)
    }

    pub fn reasons_for_claim(&self, claim_id: &Uuid) -> Vec<&Reason> {
        self.reasons.values()
            .filter(|r| r.claim_id == *claim_id)
            .collect()
    }

    // ── Framework Experiment CRUD ─────────────────────────────────────

    pub fn add_fw_experiment(&mut self, exp: FwExperiment) -> Uuid {
        let id = exp.id;
        self.fw_experiments.insert(id, exp);
        id
    }

    pub fn get_fw_experiment(&self, id: &Uuid) -> Option<&FwExperiment> {
        self.fw_experiments.get(id)
    }

    pub fn get_fw_experiment_mut(&mut self, id: &Uuid) -> Option<&mut FwExperiment> {
        self.fw_experiments.get_mut(id)
    }

    pub fn experiments_for_reason(&self, reason_id: &Uuid) -> Vec<&FwExperiment> {
        self.fw_experiments.values()
            .filter(|e| e.reason_id == *reason_id)
            .collect()
    }

    /// Verdict 판정 → Evidence 자동 생성 → Reason에 첨부
    pub fn verdict_experiment(
        &mut self,
        exp_id: &Uuid,
        verdict: ExperimentVerdict,
    ) -> Option<Uuid> {
        // 1. verdict 저장
        let exp = self.fw_experiments.get_mut(exp_id)?;
        let reason_id = exp.reason_id;
        let supports = verdict.supports_reason;
        let desc = format!(
            "[{}] {} — {}",
            if supports { "지지" } else { "약화" },
            exp.description,
            verdict.explanation,
        );
        exp.verdict = Some(verdict);
        exp.status = FwExpStatus::Interpreted;

        // 2. Evidence 생성
        let evidence = FwEvidence::new(EvidenceTag::Backtest, desc);
        let ev_id = evidence.id;
        exp.evidence_id = Some(ev_id);

        // 3. Reason에 Evidence 첨부
        if let Some(reason) = self.reasons.get_mut(&reason_id) {
            reason.evidences.push(evidence);
        }

        Some(ev_id)
    }

    /// Claim 삭제 (관련 Reasons, Experiments도 함께)
    pub fn delete_claim(&mut self, claim_id: &Uuid) -> bool {
        if self.claims.remove(claim_id).is_none() {
            return false;
        }
        let reason_ids: Vec<Uuid> = self.reasons.values()
            .filter(|r| r.claim_id == *claim_id)
            .map(|r| r.id)
            .collect();
        for rid in &reason_ids {
            let exp_ids: Vec<Uuid> = self.fw_experiments.values()
                .filter(|e| e.reason_id == *rid)
                .map(|e| e.id)
                .collect();
            for eid in &exp_ids {
                self.fw_experiments.remove(eid);
            }
            self.reasons.remove(rid);
        }
        true
    }

    /// Claim 검색: 키워드 직접 매칭(primary) + 텍스트 토큰 매칭(secondary)
    pub fn search_claims(&self, query: &str, top_k: usize) -> Vec<ClaimSearchResult> {
        use super::embedding::tokenize;

        let query_tokens = tokenize(query);
        if query_tokens.is_empty() { return Vec::new(); }

        let mut results: Vec<ClaimSearchResult> = self.claims.values().filter_map(|claim| {
            let reasons = self.reasons_for_claim(&claim.id);
            let all_kws: Vec<&str> = claim.keywords.iter().map(|s| s.as_str())
                .chain(reasons.iter().flat_map(|r| r.keywords.iter().map(|s| s.as_str())))
                .collect();

            // 1. 키워드 직접 매칭 (가중치 높음: 0.4 per match)
            let kw_matches: usize = query_tokens.iter().filter(|qt| {
                all_kws.iter().any(|k| k.to_lowercase().contains(qt.as_str()))
            }).count();

            // 2. 텍스트 토큰 매칭 (가중치 낮음: 0.15 per match)
            let full_text = format!("{} {} {}",
                claim.statement,
                claim.trigger,
                reasons.iter().map(|r| format!("{} {}", r.statement, r.bridge)).collect::<Vec<_>>().join(" ")
            ).to_lowercase();

            let text_matches: usize = query_tokens.iter().filter(|qt| {
                full_text.contains(qt.as_str())
            }).count();

            // 키워드도 텍스트도 안 맞으면 skip
            if kw_matches == 0 && text_matches == 0 { return None; }

            // 점수: 키워드 매칭 비율 * 0.4 + 텍스트 매칭 비율 * 0.15
            let n_tokens = query_tokens.len().max(1) as f64;
            let score = (kw_matches as f64 / n_tokens) * 1.0 + (text_matches as f64 / n_tokens) * 0.2;

            let matched_reasons: Vec<String> = reasons.iter().filter(|r| {
                let r_lower = format!("{} {} {}", r.statement, r.bridge, r.keywords.join(" ")).to_lowercase();
                query_tokens.iter().any(|qt| r_lower.contains(qt.as_str()))
            }).map(|r| r.statement.clone()).collect();

            Some(ClaimSearchResult {
                claim_id: claim.id,
                statement: claim.statement.clone(),
                keywords: claim.keywords.clone(),
                status: claim.status,
                score,
                n_reasons: reasons.len(),
                matched_reasons,
            })
        }).collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        results
    }

    /// Claim + 모든 Reason/Evidence를 트리로 조회
    pub fn claim_tree(&self, claim_id: &Uuid) -> Option<ClaimTree<'_>> {
        let claim = self.claims.get(claim_id)?;
        let reasons: Vec<&Reason> = self.reasons_for_claim(claim_id);
        Some(ClaimTree {
            claim,
            reasons,
        })
    }

    // ── Summary ─────────────────────────────────────────────────────────

    pub fn summary(&self) -> KnowledgeSummary {
        let total_insights = self.insights.len();
        let pending = self.insights.values().filter(|i| i.status == InsightStatus::Pending).count();
        let accepted = self.insights.values().filter(|i| i.status == InsightStatus::Accepted).count();
        let rejected = self.insights.values().filter(|i| i.status == InsightStatus::Rejected).count();
        let mature = self.insights.values().filter(|i| i.maturity == Maturity::Mature).count();

        let n_claims = self.claims.len();
        let n_knowledge = self.claims.values().filter(|c| c.is_knowledge()).count();
        let n_draft_claims = self.claims.values().filter(|c| c.status == ClaimStatus::Draft).count();

        KnowledgeSummary {
            n_hypotheses: self.hypotheses.len(),
            n_experiments: self.experiments.len(),
            n_insights: total_insights,
            n_pending: pending,
            n_accepted: accepted,
            n_rejected: rejected,
            n_mature: mature,
            n_claims,
            n_knowledge,
            n_draft_claims,
            n_reasons: self.reasons.len(),
            n_fw_experiments: self.fw_experiments.len(),
        }
    }

    // ── JSON helpers ────────────────────────────────────────────────────

    fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn save_json<T: serde::Serialize>(
        path: &Path,
        data: &T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(data)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct KnowledgeSummary {
    pub n_hypotheses: usize,
    pub n_experiments: usize,
    pub n_insights: usize,
    pub n_pending: usize,
    pub n_accepted: usize,
    pub n_rejected: usize,
    pub n_mature: usize,
    // Framework
    pub n_claims: usize,
    pub n_knowledge: usize,
    pub n_draft_claims: usize,
    pub n_reasons: usize,
    pub n_fw_experiments: usize,
}

#[derive(Debug)]
pub struct ClaimTree<'a> {
    pub claim: &'a Claim,
    pub reasons: Vec<&'a Reason>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClaimSearchResult {
    pub claim_id: Uuid,
    pub statement: String,
    pub keywords: Vec<String>,
    pub status: ClaimStatus,
    pub score: f64,
    pub n_reasons: usize,
    pub matched_reasons: Vec<String>,
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na > 1e-10 && nb > 1e-10 { dot / (na * nb) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn temp_db() -> (tempfile::TempDir, KnowledgeDB) {
        let dir = tempdir().unwrap();
        let db = KnowledgeDB::open(dir.path()).unwrap();
        (dir, db)
    }

    #[test]
    fn test_hypothesis_crud() {
        let (_dir, mut db) = temp_db();
        let h = Hypothesis::new(
            "POI Momentum".into(),
            "Price momentum + OI = continuation".into(),
        );
        let id = db.add_hypothesis(h);
        assert!(db.get_hypothesis(&id).is_some());
        assert_eq!(db.list_hypotheses().len(), 1);
    }

    #[test]
    fn test_experiment_links_to_hypothesis() {
        let (_dir, mut db) = temp_db();
        let h = Hypothesis::new("test".into(), "test".into());
        let hid = db.add_hypothesis(h);

        let config = ExperimentConfig {
            primary_threshold: 0.1,
            secondary_threshold: 0.25,
            exit_mode: crate::strategy::signal_fsm::ExitMode::PrimaryOrSecondary,
            entry_delay: 1,
            min_hold: 0,
            fee_bp: 9.0,
            period_start: "2025-01-01".into(),
            period_end: "2026-02-28".into(),
            direction_filter: DirectionFilter::Both,
            crash_filter: None,
            description: "baseline".into(),
        };
        let exp = Experiment::new(hid, config);
        let eid = db.add_experiment(exp);

        assert!(db.get_experiment(&eid).is_some());
        assert!(db.get_hypothesis(&hid).unwrap().experiment_ids.contains(&eid));
    }

    #[test]
    fn test_insight_accept_reject() {
        let (_dir, mut db) = temp_db();
        let h = Hypothesis::new("test".into(), "test".into());
        let hid = db.add_hypothesis(h);
        let config = ExperimentConfig {
            primary_threshold: 0.1, secondary_threshold: 0.25,
            exit_mode: crate::strategy::signal_fsm::ExitMode::PrimaryOrSecondary,
            entry_delay: 1, min_hold: 0, fee_bp: 9.0,
            period_start: "2025-01-01".into(), period_end: "2026-02-28".into(),
            direction_filter: DirectionFilter::Both, crash_filter: None,
            description: "test".into(),
        };
        let exp = Experiment::new(hid, config);
        let eid = db.add_experiment(exp);

        let ins = Insight::new(eid, "Short momentum works".into(), "data says so".into(), vec!["momentum".into()]);
        let iid = db.add_insight(ins);

        assert_eq!(db.pending_insights().len(), 1);

        db.accept_insight(&iid, "Makes economic sense".into());
        assert_eq!(db.pending_insights().len(), 0);
        assert_eq!(db.get_insight(&iid).unwrap().status, InsightStatus::Accepted);
    }

    #[test]
    fn test_save_load_roundtrip() {
        let (dir, mut db) = temp_db();
        let h = Hypothesis::new("test".into(), "test".into());
        db.add_hypothesis(h);
        db.save().unwrap();

        let db2 = KnowledgeDB::open(dir.path()).unwrap();
        assert_eq!(db2.list_hypotheses().len(), 1);
    }
}
