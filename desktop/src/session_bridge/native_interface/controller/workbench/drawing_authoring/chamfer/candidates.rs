//! Indexed, finite work over immutable published geometry; no kernel calls.
use super::super::{
    super::drawing_paper,
    straight::candidates::{linear, Visible},
};
use super::*;
use limo_cad_occt::{DrawingProjectionAnchorEndpoint as Endpoint, DrawingProjectionDto};
use limo_cad_solid::SolidSceneDto;
use std::collections::BTreeMap;
const MAX_ITEMS: usize = 200_000;
const MAX_WORK: usize = 2_000_000;
struct Edge {
    target: Target,
    points: [[f64; 3]; 2],
    length: f64,
    hidden: bool,
    depth: f64,
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = norm(a);
    a.map(|v| v / n)
}
pub(in super::super) fn targets(
    scene: &SolidSceneDto,
    view: &DrawingViewDto,
    p: &DrawingProjectionDto,
    direction: [f64; 3],
) -> Result<Vec<Target>, String> {
    if !view.scale.is_finite()
        || view.scale <= 0.
        || direction.iter().any(|v| !v.is_finite())
        || norm(direction) < 1e-9
    {
        return Err("Invalid chamfer projection basis".into());
    }
    if p.anchors.len() > MAX_ITEMS {
        return Err("Too many chamfer projection anchors".into());
    }
    let direction = unit(direction);
    let mut model = BTreeMap::new();
    for body in &scene.bodies {
        for edge in &body.edges {
            if model.len() >= MAX_ITEMS {
                return Err("Too many model edges for chamfer picking".into());
            }
            model.insert((body.id.0, edge.id.0, edge.key.as_str()), edge);
        }
    }
    let mut pairs = BTreeMap::new();
    for a in &p.anchors {
        if a.point.iter().chain(&a.model_point).any(|v| !v.is_finite()) {
            return Err("Invalid chamfer anchor".into());
        }
        let pair = pairs
            .entry((
                a.occurrence_id.map(|i| i.0),
                a.body_id.0,
                a.edge_id.0,
                a.edge_key.as_str(),
            ))
            .or_insert([None, None]);
        pair[usize::from(a.endpoint == Endpoint::End)] = Some(a);
    }
    let visible = Visible::new(p, false)?;
    let shown = view
        .show_hidden_lines
        .then(|| Visible::new(p, true))
        .transpose()?;
    let clip = limo_cad_occt::drawing_export::detail_clip_circle(view, p)?;
    let mut classified = BTreeMap::new();
    let (mut geometry_work, mut visibility_work, mut segment_count) = (0, 0, 0usize);
    let mut text_bytes = 0usize;
    let mut edges = Vec::new();
    for ((_, body, id, key), pair) in pairs {
        let [Some(a), Some(b)] = pair else { continue };
        let Some(edge) = model.get(&(body, id, key)) else {
            continue;
        };
        let straight = if let Some(v) = classified.get(&(body, id, key)) {
            *v
        } else {
            let v = linear(edge, &mut geometry_work)?;
            classified.insert((body, id, key), v);
            v
        };
        if !straight {
            continue;
        }
        let delta = sub(b.model_point, a.model_point);
        let length = norm(delta);
        if !length.is_finite() || length <= 1e-7 || dot(unit(delta), direction).abs() > 1e-4 {
            continue;
        }
        let paper = [a.point, b.point].map(|v| drawing_paper::paper_point(view, v, p));
        if paper.iter().flatten().any(|v| !v.is_finite()) {
            return Err("Invalid chamfer paper point".into());
        }
        if (paper[1][0] - paper[0][0]).hypot(paper[1][1] - paper[0][1]) < 0.75 {
            continue;
        }
        let tolerance = 0.03_f64.max(0.12 / view.scale.max(0.01));
        let visible_spans =
            visible.coverage([a.point, b.point], tolerance, &mut visibility_work)?;
        let hidden = visible_spans.is_empty();
        let spans = if let Some(shown) = &shown {
            shown.coverage([a.point, b.point], tolerance, &mut visibility_work)?
        } else {
            visible_spans
        };
        let mut pick_segments = Vec::new();
        for span in spans {
            let ends = span
                .map(|t| std::array::from_fn(|i| paper[0][i] + t * (paper[1][i] - paper[0][i])));
            for polyline in
                limo_cad_occt::drawing_export::clip_view_polyline_with_detail(view, &ends, clip)?
            {
                pick_segments.extend(polyline.windows(2).map(|s| [s[0], s[1]]));
            }
        }
        segment_count = segment_count.saturating_add(pick_segments.len());
        if segment_count > 16_384 {
            return Err("Too many chamfer pick segments".into());
        }
        if edges.len() >= 16_384 {
            return Err("Too many chamfer carrier edges".into());
        }
        text_bytes = text_bytes.saturating_add(a.edge_key.len()).saturating_add(
            p.topology_signatures
                .get(&body.to_string())
                .map_or(0, String::len),
        );
        if text_bytes > 1024 * 1024 {
            return Err("Chamfer references exceed the text budget".into());
        }
        let first = anchors::endpoint_ref(a, p);
        let second = anchors::endpoint_ref(b, p);
        let reference = DrawingLineRefDto {
            topology_signature: first.topology_signature.clone(),
            occurrence_id: first.occurrence_id,
            body_id: first.body_id,
            edge_id: first.edge_id,
            edge_key: first.edge_key.clone(),
            fallback_start: a.model_point,
            fallback_end: b.model_point,
        };
        let depth = dot(
            std::array::from_fn(|i| a.model_point[i] * 0.5 + b.model_point[i] * 0.5),
            direction,
        );
        if !depth.is_finite() {
            return Err("Invalid chamfer depth".into());
        }
        edges.push(Edge {
            target: Target {
                line: straight::LineTarget {
                    view_id: view.id,
                    reference,
                    paper,
                    pick_segments,
                    scale: view.scale,
                },
                first,
                second,
                length: 0.,
                angle: 0.,
            },
            points: [a.model_point, b.model_point],
            length,
            hidden,
            depth,
        });
    }
    let mut adjacency: BTreeMap<_, Vec<(f64, usize, usize)>> = BTreeMap::new();
    for (i, e) in edges.iter().enumerate() {
        let key = (
            e.target.first.occurrence_id.map(|i| i.0),
            e.target.first.body_id.0,
        );
        let entries = adjacency.entry(key).or_default();
        for endpoint in 0..2 {
            entries.push((e.points[endpoint][0], i, endpoint));
        }
    }
    for entries in adjacency.values_mut() {
        entries.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    }
    let mut work = 0usize;
    let mut unique: BTreeMap<[i64; 4], (Target, bool, f64)> = BTreeMap::new();
    for e in &edges {
        if (e.hidden && !view.show_hidden_lines) || e.target.line.pick_segments.is_empty() {
            continue;
        }
        let key = (
            e.target.first.occurrence_id.map(|i| i.0),
            e.target.first.body_id.0,
        );
        let entries = &adjacency[&key];
        let radius = 1e-6_f64.max(e.length * 1e-5);
        let mut ends = [false; 2];
        let mut best: Option<(usize, f64)> = None;
        for (endpoint, attached) in ends.iter_mut().enumerate() {
            let point = e.points[endpoint];
            let begin = entries.partition_point(|v| v.0 < point[0] - radius);
            for &(_, index, other_end) in entries[begin..]
                .iter()
                .take_while(|v| v.0 <= point[0] + radius)
            {
                work += 1;
                if work > MAX_WORK {
                    return Err("Chamfer adjacency query budget exceeded".into());
                }
                let carrier = &edges[index];
                if carrier.target.first.edge_id == e.target.first.edge_id {
                    continue;
                }
                let tolerance = 1e-6_f64.max(e.length.min(carrier.length) * 1e-5);
                if norm(sub(point, carrier.points[other_end])) > tolerance {
                    continue;
                }
                let angle = dot(
                    unit(sub(e.points[1 - endpoint], point)),
                    unit(sub(carrier.points[1 - other_end], point)),
                )
                .clamp(-1., 1.)
                .acos()
                .to_degrees();
                let angle = angle.min(180. - angle);
                if !(2. ..=88.).contains(&angle) {
                    continue;
                }
                *attached = true;
                let better = best.is_none_or(|(old, _)| {
                    let old = &edges[old];
                    (
                        carrier.hidden,
                        std::cmp::Reverse(ordered_length(carrier.length)),
                        carrier.target.first.edge_id.0,
                    ) < (
                        old.hidden,
                        std::cmp::Reverse(ordered_length(old.length)),
                        old.target.first.edge_id.0,
                    )
                });
                if better {
                    best = Some((index, angle));
                }
            }
        }
        if !ends.into_iter().all(|v| v) {
            continue;
        }
        let Some((_, angle)) = best else { continue };
        let mut target = e.target.clone();
        target.angle = angle;
        target.length = e.length * angle.to_radians().cos();
        if !target.length.is_finite() || target.length <= 1e-7 {
            continue;
        }
        let mut endpoints = target
            .line
            .paper
            .map(|p| p.map(|n| (n * 100.).round() as i64));
        if endpoints[0] > endpoints[1] {
            endpoints.swap(0, 1);
        }
        let key = [
            endpoints[0][0],
            endpoints[0][1],
            endpoints[1][0],
            endpoints[1][1],
        ];
        if unique.get(&key).is_none_or(|(old, hidden, depth)| {
            (!e.hidden & *hidden)
                || (e.hidden == *hidden
                    && (e.depth > *depth
                        || (e.depth == *depth && target.first.edge_id.0 < old.first.edge_id.0)))
        }) {
            unique.insert(key, (target, e.hidden, e.depth));
        }
        if unique.len() > 4096 {
            return Err("Too many chamfer targets".into());
        }
    }
    let mut result: Vec<_> = unique.into_values().map(|(t, _, _)| t).collect();
    result.sort_by_key(|t| {
        (
            t.first.occurrence_id.map(|i| i.0),
            t.first.body_id.0,
            t.first.edge_id.0,
        )
    });
    Ok(result)
}
fn ordered_length(v: f64) -> u64 {
    v.to_bits()
}
