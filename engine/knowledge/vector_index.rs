/// Custom vector index — brute-force cosine similarity.
/// Sufficient for < 10K vectors. No external dependencies.

use uuid::Uuid;

pub struct VectorIndex {
    entries: Vec<(Uuid, Vec<f32>)>,
    dim: usize,
}

impl VectorIndex {
    pub fn new(dim: usize) -> Self {
        Self {
            entries: Vec::new(),
            dim,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert or update a vector.
    pub fn upsert(&mut self, id: Uuid, embedding: Vec<f32>) {
        assert_eq!(embedding.len(), self.dim, "Embedding dimension mismatch");
        if let Some(entry) = self.entries.iter_mut().find(|(eid, _)| *eid == id) {
            entry.1 = embedding;
        } else {
            self.entries.push((id, embedding));
        }
    }

    /// Remove a vector by ID.
    pub fn remove(&mut self, id: &Uuid) {
        self.entries.retain(|(eid, _)| eid != id);
    }

    /// Search top-k by cosine similarity.
    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<(Uuid, f32)> {
        assert_eq!(query.len(), self.dim);

        let q_norm = l2_norm(query);
        if q_norm < 1e-10 {
            return Vec::new();
        }

        let mut scores: Vec<(Uuid, f32)> = self
            .entries
            .iter()
            .map(|(id, vec)| {
                let v_norm = l2_norm(vec);
                if v_norm < 1e-10 {
                    (*id, 0.0f32)
                } else {
                    let dot: f32 = query.iter().zip(vec.iter()).map(|(a, b)| a * b).sum();
                    (*id, dot / (q_norm * v_norm))
                }
            })
            .collect();

        // Sort descending by similarity
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_k);
        scores
    }
}

fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_identical() {
        let mut idx = VectorIndex::new(3);
        let id = Uuid::new_v4();
        idx.upsert(id, vec![1.0, 0.0, 0.0]);

        let results = idx.search(&[1.0, 0.0, 0.0], 1);
        assert_eq!(results.len(), 1);
        assert!((results[0].1 - 1.0).abs() < 1e-5); // identical = cosine 1.0
    }

    #[test]
    fn test_cosine_orthogonal() {
        let mut idx = VectorIndex::new(3);
        let id = Uuid::new_v4();
        idx.upsert(id, vec![1.0, 0.0, 0.0]);

        let results = idx.search(&[0.0, 1.0, 0.0], 1);
        assert!((results[0].1).abs() < 1e-5); // orthogonal = cosine 0.0
    }

    #[test]
    fn test_top_k_ordering() {
        let mut idx = VectorIndex::new(3);
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        idx.upsert(id1, vec![1.0, 0.0, 0.0]);   // most similar to query
        idx.upsert(id2, vec![0.5, 0.5, 0.0]);   // medium
        idx.upsert(id3, vec![0.0, 0.0, 1.0]);   // least similar

        let results = idx.search(&[1.0, 0.0, 0.0], 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, id1);
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn test_upsert_update() {
        let mut idx = VectorIndex::new(3);
        let id = Uuid::new_v4();
        idx.upsert(id, vec![1.0, 0.0, 0.0]);
        idx.upsert(id, vec![0.0, 1.0, 0.0]); // update

        assert_eq!(idx.len(), 1); // should not duplicate
        let results = idx.search(&[0.0, 1.0, 0.0], 1);
        assert!((results[0].1 - 1.0).abs() < 1e-5); // should match updated vector
    }

    #[test]
    fn test_remove() {
        let mut idx = VectorIndex::new(3);
        let id = Uuid::new_v4();
        idx.upsert(id, vec![1.0, 0.0, 0.0]);
        assert_eq!(idx.len(), 1);
        idx.remove(&id);
        assert_eq!(idx.len(), 0);
    }
}
