/// Hybrid search: vector similarity + keyword matching + maturity filter.

use uuid::Uuid;

use super::db::KnowledgeDB;
use super::types::{InsightStatus, Maturity};

#[derive(Debug, Clone)]
pub struct SearchFilters {
    pub maturity: Option<Maturity>,
    pub status: Option<InsightStatus>,
    pub tags: Vec<String>,
    pub top_k: usize,
}

impl Default for SearchFilters {
    fn default() -> Self {
        Self {
            maturity: None,
            status: None,
            tags: Vec::new(),
            top_k: 10,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub insight_id: Uuid,
    pub score: f64,
    pub text: String,
    pub tags: Vec<String>,
    pub maturity: Maturity,
    pub status: InsightStatus,
}

impl KnowledgeDB {
    /// Hybrid search across insights.
    /// Combines vector similarity with keyword matching and filters.
    pub fn search_insights(
        &self,
        query_embedding: Option<&[f32]>,
        query_keywords: &[String],
        filters: &SearchFilters,
    ) -> Vec<SearchResult> {
        // 1. Vector similarity scores
        let vector_scores: Vec<(Uuid, f32)> = if let Some(emb) = query_embedding {
            if !emb.is_empty() && self.vector_index.len() > 0 {
                self.vector_index.search(emb, filters.top_k * 3)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // 2. Keyword match scores
        let keyword_scores: Vec<(Uuid, f32)> = if !query_keywords.is_empty() {
            self.insights
                .iter()
                .map(|(id, ins)| {
                    let matches = query_keywords
                        .iter()
                        .filter(|kw| {
                            ins.tags.iter().any(|t| t.contains(kw.as_str()))
                                || ins.text.to_lowercase().contains(&kw.to_lowercase())
                        })
                        .count();
                    (*id, matches as f32 / query_keywords.len().max(1) as f32)
                })
                .filter(|(_, score)| *score > 0.0)
                .collect()
        } else {
            Vec::new()
        };

        // 3. Merge scores
        let mut combined: std::collections::HashMap<Uuid, f64> = std::collections::HashMap::new();

        for (id, score) in &vector_scores {
            *combined.entry(*id).or_default() += *score as f64 * 0.6; // vector weight
        }
        for (id, score) in &keyword_scores {
            *combined.entry(*id).or_default() += *score as f64 * 0.3; // keyword weight
        }

        // Maturity bonus
        for (id, score) in combined.iter_mut() {
            if let Some(ins) = self.insights.get(id) {
                if ins.maturity == Maturity::Mature {
                    *score += 0.1;
                }
            }
        }

        // 4. Filter and sort
        let mut results: Vec<SearchResult> = combined
            .into_iter()
            .filter_map(|(id, score)| {
                let ins = self.insights.get(&id)?;

                // Apply filters
                if let Some(mat) = &filters.maturity {
                    if ins.maturity != *mat {
                        return None;
                    }
                }
                if let Some(status) = &filters.status {
                    if ins.status != *status {
                        return None;
                    }
                }
                if !filters.tags.is_empty()
                    && !filters.tags.iter().any(|t| ins.tags.contains(t))
                {
                    return None;
                }

                Some(SearchResult {
                    insight_id: id,
                    score,
                    text: ins.text.clone(),
                    tags: ins.tags.clone(),
                    maturity: ins.maturity,
                    status: ins.status,
                })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(filters.top_k);
        results
    }

    /// Text search with multi-token OR matching + score by match count.
    pub fn search_by_text(&self, query: &str, top_k: usize) -> Vec<SearchResult> {
        let tokens: Vec<String> = query.to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .filter(|s| s.len() >= 2)
            .collect();

        if tokens.is_empty() {
            return Vec::new();
        }

        let mut results: Vec<SearchResult> = self
            .insights
            .values()
            .filter_map(|ins| {
                let text_lower = ins.text.to_lowercase();
                let tags_lower: Vec<String> = ins.tags.iter().map(|t| t.to_lowercase()).collect();

                let matches: usize = tokens.iter().filter(|tok| {
                    text_lower.contains(tok.as_str())
                        || tags_lower.iter().any(|t| t.contains(tok.as_str()))
                }).count();

                if matches == 0 { return None; }

                let base_score = matches as f64 / tokens.len() as f64;
                let maturity_bonus = if ins.maturity == Maturity::Mature { 0.2 } else { 0.0 };

                Some(SearchResult {
                    insight_id: ins.id,
                    score: base_score + maturity_bonus,
                    text: ins.text.clone(),
                    tags: ins.tags.clone(),
                    maturity: ins.maturity,
                    status: ins.status,
                })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        results
    }
}
