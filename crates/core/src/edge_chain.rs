//! Host-neutral edge connectivity. No CAM, renderer or kernel dependencies.
//! Endpoints join only inside a named scope; branches never choose a path
//! by iteration order. Callers may restrict candidates to a B-rep face wire.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const JOIN_TOLERANCE: f64 = 1.0e-4;
const MAX_EDGES: usize = 20_000;

#[derive(Debug, Clone)]
pub struct Edge {
    pub key: String,
    pub scope: String,
    pub points: Vec<[f64; 3]>,
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    pub keys: Vec<String>,
    /// Model coordinates, without a duplicate closing point.
    pub points: Vec<[f64; 3]>,
    pub closed: bool,
}

pub fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

struct Graph<'a> {
    edges: &'a [Edge],
    ends: Vec<[usize; 2]>,
    incident: Vec<Vec<usize>>,
}

impl<'a> Graph<'a> {
    fn new(edges: &'a [Edge]) -> Result<Self, String> {
        if edges.len() > MAX_EDGES {
            return Err("Too many edges in one chain query.".into());
        }
        let mut nodes: Vec<([f64; 3], String)> = Vec::new();
        let mut buckets: BTreeMap<(String, [i64; 3]), Vec<usize>> = BTreeMap::new();
        let mut incident: Vec<Vec<usize>> = Vec::new();
        let mut ends = Vec::new();
        let mut identities = BTreeSet::new();
        for (index, edge) in edges.iter().enumerate() {
            if !identities.insert(&edge.key)
                || edge.points.len() < 2
                || edge
                    .points
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 1e10)
            {
                return Err("An edge has duplicate identity or invalid geometry.".into());
            }
            let mut pair = [0; 2];
            for (end, point) in [edge.points[0], *edge.points.last().unwrap()]
                .into_iter()
                .enumerate()
            {
                let cell = point.map(|v| (v / JOIN_TOLERANCE).floor() as i64);
                let mut matches = BTreeSet::new();
                for x in -1..=1 {
                    for y in -1..=1 {
                        for z in -1..=1 {
                            let neighbor = [cell[0] + x, cell[1] + y, cell[2] + z];
                            for &node in buckets
                                .get(&(edge.scope.clone(), neighbor))
                                .into_iter()
                                .flatten()
                            {
                                if distance(nodes[node].0, point) <= JOIN_TOLERANCE {
                                    matches.insert(node);
                                }
                            }
                        }
                    }
                }
                if matches.len() > 1 {
                    return Err(
                        "Ambiguous endpoint tolerance: nearby vertices cannot be joined safely."
                            .into(),
                    );
                }
                let node = if let Some(&node) = matches.first() {
                    node
                } else {
                    let node = nodes.len();
                    nodes.push((point, edge.scope.clone()));
                    incident.push(Vec::new());
                    buckets
                        .entry((edge.scope.clone(), cell))
                        .or_default()
                        .push(node);
                    node
                };
                pair[end] = node;
                incident[node].push(index);
            }
            ends.push(pair);
        }
        Ok(Self {
            edges,
            ends,
            incident,
        })
    }

    fn walk(&self, selected: &BTreeSet<usize>, seed: usize) -> Result<Chain, String> {
        if selected.is_empty() {
            return Err("Select at least one edge.".into());
        }
        let mut endpoints = Vec::new();
        for (node, edges) in self.incident.iter().enumerate() {
            match edges.iter().filter(|e| selected.contains(e)).count() {
                0 | 2 => {}
                1 => endpoints.push(node),
                _ => {
                    return Err(
                        "The selected edges branch. Select one unambiguous open or closed chain."
                            .into(),
                    )
                }
            }
        }
        if !endpoints.is_empty() && endpoints.len() != 2 {
            return Err(
                "The selected edges are disconnected. Each edge must touch the next.".into(),
            );
        }
        let closed = endpoints.is_empty();
        let mut current = if closed {
            self.ends[seed][0]
        } else {
            endpoints[0]
        };
        let mut used = BTreeSet::new();
        let mut keys = Vec::new();
        let mut points = Vec::new();
        let mut seed_forward = true;
        loop {
            let next = if used.is_empty() && closed {
                Some(seed)
            } else {
                self.incident[current]
                    .iter()
                    .find(|e| selected.contains(*e) && !used.contains(*e))
                    .copied()
            };
            let Some(index) = next else {
                break;
            };
            if !used.insert(index) {
                break;
            }
            let forward = self.ends[index][0] == current;
            if index == seed {
                seed_forward = forward;
            }
            let mut segment = self.edges[index].points.clone();
            if !forward {
                segment.reverse();
            }
            if points.is_empty() {
                points.extend(segment);
            } else {
                points.extend(segment.into_iter().skip(1));
            }
            keys.push(self.edges[index].key.clone());
            current = self.ends[index][if forward { 1 } else { 0 }];
        }
        if used.len() != selected.len() {
            return Err("The selected edges contain more than one chain.".into());
        }
        if !closed && !seed_forward {
            points.reverse();
            keys.reverse();
        }
        points.dedup_by(|a, b| distance(*a, *b) <= JOIN_TOLERANCE);
        if closed {
            points.pop();
        }
        if points.len() < if closed { 3 } else { 2 } {
            return Err("The selected chain collapses to a point or line.".into());
        }
        Ok(Chain {
            keys,
            points,
            closed,
        })
    }
}

/// Stitch exactly the selected entities, never bridging a gap or branch.
pub fn resolve(edges: &[Edge], keys: &[String], reversed: bool) -> Result<Chain, String> {
    if keys.is_empty() {
        return Err("Select at least one edge.".into());
    }
    if keys.len() > MAX_EDGES {
        return Err("Too many edges in one chain query.".into());
    }
    let mut seen = BTreeSet::new();
    let picked = keys
        .iter()
        .map(|key| {
            if !seen.insert(key) {
                return Err("An edge was selected more than once.".into());
            }
            edges
                .iter()
                .find(|e| &e.key == key)
                .cloned()
                .ok_or_else(|| format!("Referenced edge '{key}' no longer exists."))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut chain = if picked.iter().any(|e| e.closed) {
        if picked.len() != 1 {
            return Err("A closed edge is already a complete loop. Select it on its own.".into());
        }
        let edge = &picked[0];
        if edge.points.len() < 3
            || edge
                .points
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e10)
        {
            return Err("The closed edge has invalid geometry.".into());
        }
        let mut points = edge.points.clone();
        points.dedup_by(|a, b| distance(*a, *b) <= JOIN_TOLERANCE);
        if distance(points[0], *points.last().unwrap()) <= JOIN_TOLERANCE {
            points.pop();
        }
        if points.len() < 3 {
            return Err("The closed edge has fewer than three distinct samples.".into());
        }
        Chain {
            keys: keys.to_vec(),
            points,
            closed: true,
        }
    } else {
        Graph::new(&picked)?.walk(&(0..picked.len()).collect(), 0)?
    };
    chain.keys = keys.to_vec();
    if reversed {
        chain.points.reverse();
    }
    Ok(chain)
}

/// Find the component around one seed. Candidate filtering (face boundary,
/// plane, body, sketch) belongs to the caller; ambiguity fails closed.
pub fn closed_component(edges: &[Edge], key: &str) -> Result<Chain, String> {
    let seed = edges
        .iter()
        .position(|e| e.key == key)
        .ok_or("The selected edge no longer exists.")?;
    if edges[seed].closed {
        return resolve(edges, &[key.to_owned()], false);
    }
    let graph = Graph::new(edges)?;
    let mut selected = BTreeSet::new();
    let mut pending = vec![seed];
    while let Some(index) = pending.pop() {
        if !selected.insert(index) {
            continue;
        }
        for node in graph.ends[index] {
            pending.extend(graph.incident[node].iter().copied());
        }
    }
    let chain = graph.walk(&selected, seed)?;
    if !chain.closed {
        return Err("No closed loop was found. Use Manual edges for an open chain.".into());
    }
    Ok(chain)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edge(key: &str, a: [f64; 3], b: [f64; 3]) -> Edge {
        Edge {
            key: key.into(),
            scope: "body:1".into(),
            points: vec![a, b],
            closed: false,
        }
    }
    fn square() -> Vec<Edge> {
        vec![
            edge("a", [0., 0., 0.], [3., 0., 0.]),
            edge("b", [3., 3., 0.], [3., 0., 0.]),
            edge("c", [3., 3., 0.], [0., 3., 0.]),
            edge("d", [0., 0., 0.], [0., 3., 0.]),
        ]
    }
    #[test]
    fn closed_from_one_seed_and_manual_reversal() {
        let edges = square();
        let chain = closed_component(&edges, "b").unwrap();
        assert!(chain.closed);
        assert_eq!(chain.keys.len(), 4);
        assert_eq!(chain.points.len(), 4);
        let open = resolve(&edges, &["a".into(), "b".into()], false).unwrap();
        assert!(!open.closed);
        assert_eq!(open.points, vec![[0., 0., 0.], [3., 0., 0.], [3., 3., 0.]]);
        let reversed = resolve(&edges, &["a".into(), "b".into()], true).unwrap();
        assert_eq!(
            reversed.points,
            open.points.into_iter().rev().collect::<Vec<_>>()
        );
    }
    #[test]
    fn branches_gaps_duplicates_and_cross_body_joins_fail() {
        let mut edges = square();
        edges.push(edge("branch", [3., 0., 0.], [4., 0., 0.]));
        assert!(closed_component(&edges, "a").is_err());
        assert!(resolve(&edges, &["a".into(), "c".into()], false).is_err());
        assert!(resolve(&edges, &["a".into(), "a".into()], false).is_err());
        edges[1].scope = "body:2".into();
        assert!(resolve(&edges, &["a".into(), "b".into()], false).is_err());
    }
    #[test]
    fn separate_loops_and_curved_closed_edges_are_not_merged() {
        let mut edges = square();
        let mut hole = square();
        for e in &mut hole {
            e.key.push('h');
            for p in &mut e.points {
                p[0] = p[0] / 3. + 1.;
                p[1] = p[1] / 3. + 1.;
            }
        }
        edges.extend(hole);
        assert_eq!(closed_component(&edges, "a").unwrap().keys.len(), 4);
        let mut circle = edges[0].clone();
        circle.key = "spline loop".into();
        circle.points = vec![[8., 0., 0.], [9., 1., 0.], [8., 2., 0.], [7., 1., 0.]];
        circle.closed = true;
        edges.push(circle);
        assert!(closed_component(&edges, "spline loop").unwrap().closed);
    }
}
