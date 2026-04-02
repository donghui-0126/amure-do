/// Self-contained embedding: TF-IDF vectors + cosine similarity.
/// No external model needed. Keyword co-occurrence for relational scoring.

use std::collections::HashMap;

/// Simple tokenizer: lowercase + split on non-alphanumeric + Korean char support.
pub fn tokenize(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut tokens = Vec::new();
    let mut current = String::new();

    let is_korean = |c: char| ('\u{AC00}'..='\u{D7A3}').contains(&c);
    let is_ascii_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';

    for ch in lower.chars() {
        if is_ascii_word(ch) || is_korean(ch) {
            // Split on Korean↔ASCII boundary
            if !current.is_empty() {
                let last = current.chars().last().unwrap();
                if (is_korean(last) && is_ascii_word(ch)) || (is_ascii_word(last) && is_korean(ch)) {
                    if current.len() >= 2 { tokens.push(current.clone()); }
                    current.clear();
                }
            }
            current.push(ch);
        } else {
            if current.len() >= 2 {
                tokens.push(current.clone());
            }
            current.clear();
        }
    }
    if current.len() >= 2 {
        tokens.push(current);
    }
    tokens
}

/// TF-IDF Vectorizer — builds vocabulary from corpus, produces sparse vectors.
pub struct TfIdfVectorizer {
    vocab: HashMap<String, usize>,       // word → index
    idf: Vec<f32>,                       // inverse document frequency per word
    dim: usize,
}

impl TfIdfVectorizer {
    /// Build from a corpus of documents.
    pub fn fit(documents: &[String]) -> Self {
        let n_docs = documents.len().max(1);

        // Build vocabulary + document frequency
        let mut df: HashMap<String, usize> = HashMap::new();
        let mut all_tokens: Vec<Vec<String>> = Vec::new();

        for doc in documents {
            let tokens = tokenize(doc);
            let unique: std::collections::HashSet<&String> = tokens.iter().collect();
            for t in &unique {
                *df.entry((*t).clone()).or_default() += 1;
            }
            all_tokens.push(tokens);
        }

        // Filter: keep words that appear in at least 1 doc and at most 90% of docs
        let max_df = (n_docs as f64 * 0.9).ceil() as usize;
        let mut vocab: Vec<(String, usize)> = df.iter()
            .filter(|(_, count)| **count >= 1 && **count <= max_df)
            .map(|(word, count)| (word.clone(), *count))
            .collect();
        vocab.sort_by(|a, b| b.1.cmp(&a.1)); // sort by frequency desc
        vocab.truncate(500); // max 500 features

        let vocab_map: HashMap<String, usize> = vocab.iter()
            .enumerate()
            .map(|(i, (word, _))| (word.clone(), i))
            .collect();

        let dim = vocab_map.len();

        // Compute IDF
        let mut idf = vec![0.0f32; dim];
        for (word, idx) in &vocab_map {
            let doc_freq = df.get(word).copied().unwrap_or(1) as f32;
            idf[*idx] = ((n_docs as f32 + 1.0) / (doc_freq + 1.0)).ln() + 1.0;
        }

        Self { vocab: vocab_map, idf, dim }
    }

    /// Transform a single document into a TF-IDF vector.
    pub fn transform(&self, text: &str) -> Vec<f32> {
        if self.dim == 0 {
            return vec![0.0; 1];
        }

        let tokens = tokenize(text);
        let n_tokens = tokens.len().max(1) as f32;

        // Term frequency
        let mut tf = HashMap::new();
        for t in &tokens {
            *tf.entry(t.clone()).or_insert(0u32) += 1;
        }

        // TF-IDF vector
        let mut vec = vec![0.0f32; self.dim];
        for (word, count) in &tf {
            if let Some(idx) = self.vocab.get(word) {
                vec[*idx] = (*count as f32 / n_tokens) * self.idf[*idx];
            }
        }

        // L2 normalize
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-10 {
            for v in &mut vec {
                *v /= norm;
            }
        }

        vec
    }

    pub fn dim(&self) -> usize {
        self.dim.max(1)
    }
}

/// Keyword graph: tracks co-occurrence of keywords across insights.
pub struct KeywordGraph {
    /// keyword → set of insight IDs that contain it
    pub keyword_to_ids: HashMap<String, Vec<uuid::Uuid>>,
    /// Pair co-occurrence count
    pub cooccurrence: HashMap<(String, String), usize>,
}

impl KeywordGraph {
    pub fn new() -> Self {
        Self {
            keyword_to_ids: HashMap::new(),
            cooccurrence: HashMap::new(),
        }
    }

    /// Add an insight's keywords to the graph.
    pub fn add(&mut self, id: uuid::Uuid, keywords: &[String]) {
        for kw in keywords {
            self.keyword_to_ids.entry(kw.clone()).or_default().push(id);
        }
        // Update co-occurrence
        for i in 0..keywords.len() {
            for j in (i+1)..keywords.len() {
                let pair = if keywords[i] < keywords[j] {
                    (keywords[i].clone(), keywords[j].clone())
                } else {
                    (keywords[j].clone(), keywords[i].clone())
                };
                *self.cooccurrence.entry(pair).or_default() += 1;
            }
        }
    }

    /// Find insights related by shared keywords.
    pub fn find_related(&self, keywords: &[String], top_k: usize) -> Vec<(uuid::Uuid, f32)> {
        let mut scores: HashMap<uuid::Uuid, f32> = HashMap::new();

        for kw in keywords {
            if let Some(ids) = self.keyword_to_ids.get(kw) {
                let weight = 1.0 / (ids.len() as f32).max(1.0); // rarer keyword = higher weight
                for &id in ids {
                    *scores.entry(id).or_default() += weight;
                }
            }
        }

        let mut results: Vec<(uuid::Uuid, f32)> = scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        results
    }

    /// Keyword relatedness: how related are two sets of keywords?
    pub fn relatedness(&self, a: &[String], b: &[String]) -> f32 {
        let mut score = 0.0f32;
        let mut pairs = 0;
        for ka in a {
            for kb in b {
                let pair = if ka < kb { (ka.clone(), kb.clone()) } else { (kb.clone(), ka.clone()) };
                if let Some(count) = self.cooccurrence.get(&pair) {
                    score += *count as f32;
                }
                pairs += 1;
            }
        }
        if pairs > 0 { score / pairs as f32 } else { 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("Short momentum은 크립토에서 continuation alpha가 있다");
        assert!(tokens.contains(&"short".to_string()));
        assert!(tokens.contains(&"momentum".to_string()));
        assert!(tokens.contains(&"continuation".to_string()));
        assert!(tokens.contains(&"alpha".to_string()));
    }

    #[test]
    fn test_tfidf_basic() {
        let docs = vec![
            "short momentum crypto".into(),
            "long momentum equity".into(),
            "short mean reversion".into(),
        ];
        let vectorizer = TfIdfVectorizer::fit(&docs);
        let v1 = vectorizer.transform("short momentum");
        let v2 = vectorizer.transform("long equity");

        // v1 and "short momentum crypto" should be more similar than v2
        let sim1 = cosine_sim(&v1, &vectorizer.transform("short momentum crypto"));
        let sim2 = cosine_sim(&v2, &vectorizer.transform("short momentum crypto"));
        assert!(sim1 > sim2, "sim1={} should > sim2={}", sim1, sim2);
    }

    #[test]
    fn test_keyword_graph() {
        let mut graph = KeywordGraph::new();
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        graph.add(id1, &["momentum".into(), "short".into(), "crypto".into()]);
        graph.add(id2, &["momentum".into(), "long".into(), "equity".into()]);

        let related = graph.find_related(&["momentum".into()], 5);
        assert_eq!(related.len(), 2); // both have "momentum"

        let rel = graph.relatedness(&["short".into(), "crypto".into()], &["momentum".into(), "short".into()]);
        assert!(rel > 0.0);
    }

    fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if na > 1e-10 && nb > 1e-10 { dot / (na * nb) } else { 0.0 }
    }
}
