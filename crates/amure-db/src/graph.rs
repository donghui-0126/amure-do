/// AmureGraph — 인메모리 그래프 엔진.
/// Adjacency list 기반, BFS walk, 노드/엣지 CRUD.

use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

use crate::edge::{Edge, EdgeKind};
use crate::node::{Node, NodeKind, NodeStatus};

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Out,
    In,
    Both,
}

pub struct AmureGraph {
    pub nodes: HashMap<Uuid, Node>,
    pub edges: HashMap<Uuid, Edge>,
    adjacency: HashMap<Uuid, Vec<Uuid>>,    // node_id → outgoing edge_ids
    reverse_adj: HashMap<Uuid, Vec<Uuid>>,  // node_id → incoming edge_ids
}

impl AmureGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            adjacency: HashMap::new(),
            reverse_adj: HashMap::new(),
        }
    }

    // ── Node CRUD ──────────────────────────────────────────────────────

    pub fn add_node(&mut self, node: Node) -> Uuid {
        let id = node.id;
        self.nodes.insert(id, node);
        self.adjacency.entry(id).or_default();
        self.reverse_adj.entry(id).or_default();
        id
    }

    pub fn get_node(&self, id: &Uuid) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn get_node_mut(&mut self, id: &Uuid) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    pub fn remove_node(&mut self, id: &Uuid) -> Option<Node> {
        // Collect edge IDs to remove
        let mut edge_ids = Vec::new();
        if let Some(out_edges) = self.adjacency.get(id) {
            edge_ids.extend(out_edges.iter());
        }
        if let Some(in_edges) = self.reverse_adj.get(id) {
            edge_ids.extend(in_edges.iter());
        }
        let edge_ids: Vec<Uuid> = edge_ids.into_iter().copied().collect();

        // Remove edges
        for eid in edge_ids {
            self.remove_edge(&eid);
        }

        self.adjacency.remove(id);
        self.reverse_adj.remove(id);
        self.nodes.remove(id)
    }

    pub fn nodes_by_kind(&self, kind: NodeKind) -> Vec<&Node> {
        self.nodes.values().filter(|n| n.kind == kind).collect()
    }

    pub fn nodes_by_status(&self, status: NodeStatus) -> Vec<&Node> {
        self.nodes.values().filter(|n| n.status == status).collect()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    // ── Edge CRUD ──────────────────────────────────────────────────────

    pub fn add_edge(&mut self, edge: Edge) -> Uuid {
        let id = edge.id;
        self.adjacency.entry(edge.source).or_default().push(id);
        self.reverse_adj.entry(edge.target).or_default().push(id);
        self.edges.insert(id, edge);
        id
    }

    pub fn get_edge(&self, id: &Uuid) -> Option<&Edge> {
        self.edges.get(id)
    }

    pub fn remove_edge(&mut self, id: &Uuid) -> Option<Edge> {
        if let Some(edge) = self.edges.remove(id) {
            if let Some(adj) = self.adjacency.get_mut(&edge.source) {
                adj.retain(|eid| eid != id);
            }
            if let Some(radj) = self.reverse_adj.get_mut(&edge.target) {
                radj.retain(|eid| eid != id);
            }
            Some(edge)
        } else {
            None
        }
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    // ── Traversal ──────────────────────────────────────────────────────

    /// 노드의 이웃 조회. direction + edge_kind 필터.
    pub fn neighbors(
        &self,
        node_id: &Uuid,
        direction: Direction,
        edge_filter: Option<&[EdgeKind]>,
    ) -> Vec<(Uuid, &Edge)> {
        let mut result = Vec::new();

        let check_filter = |edge: &Edge| -> bool {
            edge_filter.map_or(true, |kinds| kinds.contains(&edge.kind))
        };

        // Outgoing
        if matches!(direction, Direction::Out | Direction::Both) {
            if let Some(edge_ids) = self.adjacency.get(node_id) {
                for eid in edge_ids {
                    if let Some(edge) = self.edges.get(eid) {
                        if check_filter(edge) {
                            result.push((edge.target, edge));
                        }
                    }
                }
            }
        }

        // Incoming
        if matches!(direction, Direction::In | Direction::Both) {
            if let Some(edge_ids) = self.reverse_adj.get(node_id) {
                for eid in edge_ids {
                    if let Some(edge) = self.edges.get(eid) {
                        if check_filter(edge) {
                            result.push((edge.source, edge));
                        }
                    }
                }
            }
        }

        result
    }

    /// BFS walk. max_hops 이내 도달 가능한 노드 + 거리 반환.
    pub fn walk(
        &self,
        start: &Uuid,
        max_hops: usize,
        edge_filter: Option<&[EdgeKind]>,
    ) -> Vec<(Uuid, usize)> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        visited.insert(*start);
        queue.push_back((*start, 0usize));

        while let Some((node_id, depth)) = queue.pop_front() {
            result.push((node_id, depth));

            if depth >= max_hops {
                continue;
            }

            for (neighbor_id, _edge) in self.neighbors(&node_id, Direction::Both, edge_filter) {
                if visited.insert(neighbor_id) {
                    queue.push_back((neighbor_id, depth + 1));
                }
            }
        }

        result
    }

    /// 노드 ID 목록에서 서브그래프 추출 (시각화용)
    pub fn subgraph(&self, node_ids: &[Uuid]) -> (Vec<&Node>, Vec<&Edge>) {
        let id_set: HashSet<&Uuid> = node_ids.iter().collect();
        let nodes: Vec<&Node> = node_ids.iter().filter_map(|id| self.nodes.get(id)).collect();
        let edges: Vec<&Edge> = self.edges.values()
            .filter(|e| id_set.contains(&e.source) && id_set.contains(&e.target))
            .collect();
        (nodes, edges)
    }

    /// 통계 요약
    pub fn summary(&self) -> GraphSummary {
        let mut kind_counts = HashMap::new();
        for node in self.nodes.values() {
            *kind_counts.entry(format!("{:?}", node.kind)).or_insert(0usize) += 1;
        }
        let mut edge_counts = HashMap::new();
        for edge in self.edges.values() {
            *edge_counts.entry(format!("{:?}", edge.kind)).or_insert(0usize) += 1;
        }
        let failed = self.nodes.values().filter(|n| n.is_failed()).count();

        GraphSummary {
            n_nodes: self.nodes.len(),
            n_edges: self.edges.len(),
            n_failed: failed,
            node_kinds: kind_counts,
            edge_kinds: edge_counts,
        }
    }
}

impl Default for AmureGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphSummary {
    pub n_nodes: usize,
    pub n_edges: usize,
    pub n_failed: usize,
    pub node_kinds: HashMap<String, usize>,
    pub edge_kinds: HashMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_claim(statement: &str, kw: &[&str]) -> Node {
        Node::new(NodeKind::Claim, statement.into(), kw.iter().map(|s| s.to_string()).collect())
    }

    fn make_reason(statement: &str, kw: &[&str]) -> Node {
        Node::new(NodeKind::Reason, statement.into(), kw.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn test_add_and_get() {
        let mut g = AmureGraph::new();
        let node = make_claim("OI는 momentum 선행지표", &["OI", "momentum"]);
        let id = g.add_node(node);
        assert!(g.get_node(&id).is_some());
        assert_eq!(g.node_count(), 1);
    }

    #[test]
    fn test_remove_node_cascades_edges() {
        let mut g = AmureGraph::new();
        let c = g.add_node(make_claim("claim", &[]));
        let r = g.add_node(make_reason("reason", &[]));
        g.add_edge(Edge::new(r, c, EdgeKind::Support));
        assert_eq!(g.edge_count(), 1);

        g.remove_node(&c);
        assert_eq!(g.node_count(), 1);
        assert_eq!(g.edge_count(), 0);
    }

    #[test]
    fn test_neighbors() {
        let mut g = AmureGraph::new();
        let c = g.add_node(make_claim("claim", &[]));
        let r1 = g.add_node(make_reason("support", &[]));
        let r2 = g.add_node(make_reason("rebut", &[]));
        g.add_edge(Edge::new(r1, c, EdgeKind::Support));
        g.add_edge(Edge::new(r2, c, EdgeKind::Rebut));

        // Incoming to claim
        let neighbors = g.neighbors(&c, Direction::In, None);
        assert_eq!(neighbors.len(), 2);

        // Filter support only
        let support_only = g.neighbors(&c, Direction::In, Some(&[EdgeKind::Support]));
        assert_eq!(support_only.len(), 1);
    }

    #[test]
    fn test_walk_bfs() {
        let mut g = AmureGraph::new();
        let c = g.add_node(make_claim("claim", &[]));
        let r = g.add_node(make_reason("reason", &[]));
        let e = g.add_node(Node::new(NodeKind::Evidence, "evidence".into(), vec![]));
        g.add_edge(Edge::new(r, c, EdgeKind::Support));
        g.add_edge(Edge::new(e, r, EdgeKind::DerivedFrom));

        // Walk from claim, 2 hops
        let walked = g.walk(&c, 2, None);
        assert_eq!(walked.len(), 3); // claim(0), reason(1), evidence(2)

        // Walk from claim, 1 hop
        let walked_1 = g.walk(&c, 1, None);
        assert_eq!(walked_1.len(), 2); // claim(0), reason(1)
    }

    #[test]
    fn test_subgraph() {
        let mut g = AmureGraph::new();
        let c = g.add_node(make_claim("claim", &[]));
        let r = g.add_node(make_reason("reason", &[]));
        let other = g.add_node(make_claim("other", &[]));
        g.add_edge(Edge::new(r, c, EdgeKind::Support));

        let (nodes, edges) = g.subgraph(&[c, r]);
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);

        // other is not in subgraph
        let (nodes2, _) = g.subgraph(&[c, other]);
        assert_eq!(nodes2.len(), 2);
    }

    #[test]
    fn test_summary() {
        let mut g = AmureGraph::new();
        g.add_node(make_claim("c1", &[]));
        g.add_node(make_reason("r1", &[]).with_status(NodeStatus::Weakened));
        let s = g.summary();
        assert_eq!(s.n_nodes, 2);
        assert_eq!(s.n_failed, 1);
    }
}
