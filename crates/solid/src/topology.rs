//! Host-neutral edge-chain selection. Index endpoints once rather than scanning
//! every edge again at each step of a large chain.
use crate::{BodyDto, EdgeDto, Point3Dto};
use limo_cad_core::EdgeId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const JOIN_TOLERANCE: f64 = 1e-4;
type Cell = [i64; 3];
fn point(p: &Point3Dto) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn cell(p: [f64; 3]) -> Option<Cell> {
    let q = p.map(|v| (v / JOIN_TOLERANCE).floor());
    q.iter()
        .all(|v| v.is_finite() && v.abs() < 9e15)
        .then(|| q.map(|v| v as i64))
}
fn endpoint(edge: &EdgeDto, start: bool) -> Option<([f64; 3], [f64; 3])> {
    if edge.points.len() < 2 {
        return None;
    }
    let (a, b) = if start {
        (&edge.points[0], &edge.points[1])
    } else {
        (
            &edge.points[edge.points.len() - 1],
            &edge.points[edge.points.len() - 2],
        )
    };
    let a = point(a);
    let b = point(b);
    let d = std::array::from_fn::<_, 3, _>(|i| b[i] - a[i]);
    let length = d.iter().map(|v| v * v).sum::<f64>().sqrt();
    (length.is_finite() && length > f64::EPSILON).then(|| (a, d.map(|v| v / length)))
}

/// Expand valid seeds through endpoint joins whose tessellated tangents are
/// within five degrees. The order is deterministic and retained seeds come
/// first. Non-refinable, missing and malformed edges cannot enter the chain.
pub fn tangent_chain_edges(body: &BodyDto, seeds: &[EdgeId]) -> Vec<EdgeId> {
    let endpoints: Vec<_> = body
        .edges
        .iter()
        .map(|edge| {
            if edge.refinable {
                [endpoint(edge, true), endpoint(edge, false)]
            } else {
                [None, None]
            }
        })
        .collect();
    let mut index: BTreeMap<Cell, Vec<(usize, usize)>> = BTreeMap::new();
    for (edge, ends) in endpoints.iter().enumerate() {
        for (end, value) in ends.iter().enumerate() {
            if let Some((p, _)) = value {
                if let Some(key) = cell(*p) {
                    index.entry(key).or_default().push((edge, end));
                }
            }
        }
    }
    let by_id: BTreeMap<_, _> = body
        .edges
        .iter()
        .enumerate()
        .filter(|(i, e)| {
            e.refinable
                && endpoints[*i]
                    .iter()
                    .flatten()
                    .any(|(p, _)| cell(*p).is_some())
        })
        .map(|(i, e)| (e.id, i))
        .collect();
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::new();
    let mut selected = Vec::new();
    for id in seeds {
        if let Some(&edge) = by_id.get(id) {
            if seen.insert(edge) {
                queue.push_back(edge);
                selected.push(*id);
            }
        }
    }
    let cosine = (std::f64::consts::PI / 36.).cos();
    while let Some(edge) = queue.pop_front() {
        for (p, t) in endpoints[edge].iter().flatten() {
            let Some(key) = cell(*p) else {
                continue;
            };
            for x in -1..=1 {
                for y in -1..=1 {
                    for z in -1..=1 {
                        let Some(candidates) = index.get(&[key[0] + x, key[1] + y, key[2] + z])
                        else {
                            continue;
                        };
                        for &(candidate, end) in candidates {
                            if seen.contains(&candidate) {
                                continue;
                            }
                            let (q, u) = endpoints[candidate][end].unwrap();
                            let distance = (0..3).map(|i| (p[i] - q[i]).powi(2)).sum::<f64>();
                            let tangent = (0..3).map(|i| t[i] * u[i]).sum::<f64>().abs();
                            if distance <= JOIN_TOLERANCE.powi(2) && tangent >= cosine {
                                seen.insert(candidate);
                                queue.push_back(candidate);
                                selected.push(body.edges[candidate].id);
                            }
                        }
                    }
                }
            }
        }
    }
    selected
}
/// Shared straight-edge test for reference picking and typed native forms.
/// Work in f64 so small edges at large document coordinates remain selectable.
pub fn edge_is_straight(edge: &crate::EdgeDto) -> bool {
    if edge.circle.is_some() || edge.points.len() < 2 {
        return false;
    }
    let a = edge.points.first().unwrap();
    let b = edge.points.last().unwrap();
    let d = [b.x - a.x, b.y - a.y, b.z - a.z];
    let length = d[0].hypot(d[1]).hypot(d[2]);
    if !length.is_finite() || length <= 1e-6 {
        return false;
    }
    let u = d.map(|v| v / length);
    edge.points.iter().all(|p| {
        let v = [p.x - a.x, p.y - a.y, p.z - a.z];
        let cross = [
            v[1] * u[2] - v[2] * u[1],
            v[2] * u[0] - v[0] * u[2],
            v[0] * u[1] - v[1] * u[0],
        ];
        let distance = cross[0].hypot(cross[1]).hypot(cross[2]);
        distance.is_finite() && distance <= (length * 1e-5).max(1e-5)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn body(edges: serde_json::Value) -> BodyDto {
        serde_json::from_value(json!({"id":1,"name":"Body","feature_id":1,
            "mesh":{"positions":[],"normals":[],"indices":[]},"faces":[],"edges":edges}))
        .unwrap()
    }
    fn edge(id: u64, a: [f64; 3], b: [f64; 3], refinable: bool) -> serde_json::Value {
        json!({"id":id,"key":id.to_string(),"points":[{"x":a[0],"y":a[1],"z":a[2]},
            {"x":b[0],"y":b[1],"z":b[2]}],"refinable":refinable})
    }
    #[test]
    fn expands_both_directions_across_cells_but_not_sharp_or_ineligible_edges() {
        let model = body(json!([
            edge(1, [-1., 0., 0.], [0., 0., 0.], true),
            edge(2, [0.00005, 0., 0.], [1., 0., 0.], true),
            edge(3, [1., 0., 0.], [2., 0., 0.], true),
            edge(4, [1., 0., 0.], [1., 1., 0.], true),
            edge(5, [2., 0., 0.], [3., 0., 0.], false),
            edge(6, [2., 0., 0.], [2., 0., 0.], true)
        ]));
        assert_eq!(
            tangent_chain_edges(&model, &[EdgeId(2), EdgeId(2), EdgeId(99)]),
            vec![EdgeId(2), EdgeId(1), EdgeId(3)]
        );
    }
    #[test]
    fn long_chain_is_complete_and_deterministic() {
        let model = body(serde_json::Value::Array(
            (0..5000)
                .map(|i| edge(i + 1, [i as f64, 0., 0.], [(i + 1) as f64, 0., 0.], true))
                .collect(),
        ));
        let expected: Vec<_> = (1..=5000).map(EdgeId).collect();
        assert_eq!(tangent_chain_edges(&model, &[EdgeId(1)]), expected);
    }
    #[test]
    fn straight_reference_edges_retain_precision_and_reject_invalid_geometry() {
        let mut model = body(json!([edge(1, [1e9, 0., 0.], [1e9 + 0.01, 0., 0.], false)]));
        assert!(edge_is_straight(&model.edges[0]));
        model.edges[0].points.insert(
            1,
            Point3Dto {
                x: 1e9 + 0.005,
                y: 0.001,
                z: 0.,
            },
        );
        assert!(!edge_is_straight(&model.edges[0]));
        model.edges[0].points[1].y = f64::NAN;
        assert!(!edge_is_straight(&model.edges[0]));
        model.edges[0].points.clear();
        assert!(!edge_is_straight(&model.edges[0]));
    }
}
