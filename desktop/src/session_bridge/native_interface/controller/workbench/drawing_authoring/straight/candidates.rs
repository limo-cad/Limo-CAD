//! Bounded, indexed candidate construction from immutable rendered geometry.
//! Projection anchors already contain instance and derived-view transforms.
use super::super::super::drawing_paper;
use super::LineTarget;
use limo_cad_occt::{DrawingProjectionAnchorEndpoint as Endpoint, DrawingProjectionDto};
use limo_cad_sketch::{DrawingLineRefDto, DrawingViewDto};
use limo_cad_solid::{EdgeDto, SolidSceneDto};
use std::collections::BTreeMap;

type P = [f64; 2];
const MAX_ITEMS: usize = 200_000;
const MAX_TARGETS: usize = 4096;
pub(in super::super) fn distance(p: P, [a, b]: [P; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let n = d[0] * d[0] + d[1] * d[1];
    let t = if n > 1e-14 {
        ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / n
    } else {
        0.
    }
    .clamp(0., 1.);
    (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
}
pub(in super::super) fn linear(edge: &EdgeDto, budget: &mut usize) -> Result<bool, String> {
    *budget = budget
        .checked_add(edge.points.len())
        .ok_or("Drawing edge budget exceeded")?;
    if *budget > 2_000_000 {
        return Err("Too much edge geometry for dimension picking".into());
    }
    if edge.circle.is_some() || edge.points.len() < 2 {
        return Ok(false);
    }
    let a = edge.points.first().unwrap();
    let b = edge.points.last().unwrap();
    let delta = [b.x - a.x, b.y - a.y, b.z - a.z];
    let length = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !length.is_finite() || length < 1e-7 {
        return Ok(false);
    }
    let tolerance = 1e-5_f64.max(length * 1e-5);
    Ok(edge.points.iter().all(|p| {
        let v = [p.x - a.x, p.y - a.y, p.z - a.z];
        let t = v.iter().zip(delta).map(|(x, y)| x * y).sum::<f64>() / (length * length);
        v.iter()
            .zip(delta)
            .map(|(x, y)| (x - t * y).powi(2))
            .sum::<f64>()
            <= tolerance * tolerance
    }))
}

struct Node {
    bounds: [f64; 4],
    range: std::ops::Range<usize>,
    children: Option<[usize; 2]>,
}
pub(in super::super) struct Visible {
    segments: Vec<[P; 2]>,
    nodes: Vec<Node>,
}
impl Visible {
    pub(in super::super) fn new(
        p: &DrawingProjectionDto,
        include_hidden: bool,
    ) -> Result<Self, String> {
        let lines = || {
            p.visible
                .iter()
                .chain(p.hidden.iter().filter(|_| include_hidden))
        };
        let count = lines()
            .try_fold(0usize, |n, line| {
                n.checked_add(line.points.len().saturating_sub(1))
            })
            .ok_or("Drawing visibility budget exceeded")?;
        if count > MAX_ITEMS {
            return Err("Too many projected segments for dimension picking".into());
        }
        let mut v = Self {
            segments: Vec::with_capacity(count),
            nodes: Vec::new(),
        };
        for line in lines() {
            for points in line.points.windows(2) {
                if points.iter().flatten().any(|n| !n.is_finite()) {
                    return Err("Invalid projected edge coordinates".into());
                }
                v.segments.push([points[0], points[1]]);
            }
        }
        if count > 0 {
            v.build(0..count);
        }
        Ok(v)
    }
    fn build(&mut self, range: std::ops::Range<usize>) -> usize {
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for segment in &self.segments[range.clone()] {
            for point in segment {
                for i in 0..2 {
                    bounds[i] = bounds[i].min(point[i]);
                    bounds[i + 2] = bounds[i + 2].max(point[i]);
                }
            }
        }
        let index = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            range: range.clone(),
            children: None,
        });
        if range.len() > 8 {
            let axis = usize::from(bounds[3] - bounds[1] > bounds[2] - bounds[0]);
            let mid = range.start + range.len() / 2;
            self.segments[range.clone()].select_nth_unstable_by(range.len() / 2, |a, b| {
                (a[0][axis] + a[1][axis]).total_cmp(&(b[0][axis] + b[1][axis]))
            });
            let left = self.build(range.start..mid);
            let right = self.build(mid..range.end);
            self.nodes[index].children = Some([left, right]);
        }
        index
    }
    pub(in super::super) fn coverage(
        &self,
        [a, b]: [P; 2],
        tolerance: f64,
        budget: &mut usize,
    ) -> Result<Vec<[f64; 2]>, String> {
        let d = [b[0] - a[0], b[1] - a[1]];
        let length2 = d[0] * d[0] + d[1] * d[1];
        let length = length2.sqrt();
        if !length.is_finite() || length < 1e-12 {
            return Ok(Vec::new());
        }
        let bounds = [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[0].max(b[0]),
            a[1].max(b[1]),
        ];
        let mut spans = Vec::<[f64; 2]>::new();
        let mut stack = if self.nodes.is_empty() {
            vec![]
        } else {
            vec![0]
        };
        while let Some(i) = stack.pop() {
            *budget += 1;
            if *budget > 2_000_000 {
                return Err("Drawing visibility query budget exceeded".into());
            }
            let node = &self.nodes[i];
            if (0..2).any(|axis| {
                bounds[axis] > node.bounds[axis + 2] + tolerance
                    || bounds[axis + 2] < node.bounds[axis] - tolerance
            }) {
                continue;
            }
            if let Some(children) = node.children {
                stack.extend(children);
            } else {
                for [q, r] in &self.segments[node.range.clone()] {
                    let e = [r[0] - q[0], r[1] - q[1]];
                    let segment_length = e[0].hypot(e[1]);
                    if segment_length < 1e-12
                        || (d[0] * e[1] - d[1] * e[0]).abs() > length * segment_length * 1e-5
                    {
                        continue;
                    }
                    let from_line =
                        |p: P| ((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]).abs() / length;
                    if from_line(*q) > tolerance || from_line(*r) > tolerance {
                        continue;
                    }
                    let project = |p: P| ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length2;
                    let first = project(*q);
                    let second = project(*r);
                    let span = [first.min(second).max(0.), first.max(second).min(1.)];
                    if span[1] > span[0] + 1e-9 {
                        spans.push(span);
                    }
                }
            }
        }
        spans.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut merged = Vec::<[f64; 2]>::new();
        for span in spans {
            if let Some(previous) = merged.last_mut().filter(|p| p[1] + 1e-9 >= span[0]) {
                previous[1] = previous[1].max(span[1]);
            } else {
                merged.push(span);
            }
        }
        Ok(merged)
    }
}
fn segment_key(p: [P; 2]) -> [i64; 4] {
    let mut p = p.map(|v| v.map(|n| (n * 1e5).round() as i64));
    if p[0] > p[1] {
        p.swap(0, 1);
    }
    [p[0][0], p[0][1], p[1][0], p[1][1]]
}
pub(in super::super) fn targets(
    scene: &SolidSceneDto,
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    direction: [f64; 3],
) -> Result<Vec<LineTarget>, String> {
    if !view.scale.is_finite() || view.scale <= 0. || direction.iter().any(|v| !v.is_finite()) {
        return Err("Invalid drawing projection basis".into());
    }
    if projection.anchors.len() > MAX_ITEMS {
        return Err("Too many projected anchors for dimension picking".into());
    }
    let mut scene_edges = BTreeMap::new();
    for body in &scene.bodies {
        for edge in &body.edges {
            if scene_edges.len() >= MAX_ITEMS {
                return Err("Too many model edges for dimension picking".into());
            }
            scene_edges.insert((body.id.0, edge.id.0, edge.key.as_str()), edge);
        }
    }
    let mut pairs = BTreeMap::new();
    for a in &projection.anchors {
        if a.point.iter().chain(&a.model_point).any(|v| !v.is_finite()) {
            return Err("Invalid projected anchor".into());
        }
        let pair = pairs
            .entry((
                a.occurrence_id.map(|id| id.0),
                a.body_id.0,
                a.edge_id.0,
                a.edge_key.as_str(),
            ))
            .or_insert([None, None]);
        pair[usize::from(a.endpoint == Endpoint::End)] = Some(a);
    }
    let visibility = Visible::new(projection, false)?;
    let shown = view
        .show_hidden_lines
        .then(|| Visible::new(projection, true))
        .transpose()?;
    let mut classification = BTreeMap::new();
    let mut points_budget = 0;
    let mut visibility_budget = 0;
    let mut pick_budget = 0usize;
    let mut unique: BTreeMap<[i64; 4], (LineTarget, bool, f64)> = BTreeMap::new();
    let detail_circle = limo_cad_occt::drawing_export::detail_clip_circle(view, projection)?;
    for ((_, body, id, key), pair) in pairs {
        let [Some(a), Some(b)] = pair else {
            continue;
        };
        let Some(edge) = scene_edges.get(&(body, id, key)) else {
            continue;
        };
        let is_linear = if let Some(value) = classification.get(&(body, id, key)) {
            *value
        } else {
            let value = linear(edge, &mut points_budget)?;
            classification.insert((body, id, key), value);
            value
        };
        if !is_linear {
            continue;
        }
        let paper = [a.point, b.point].map(|p| drawing_paper::paper_point(view, p, projection));
        if paper.iter().flatten().any(|v| !v.is_finite()) {
            return Err("Invalid projected straight-edge position".into());
        }
        if (paper[1][0] - paper[0][0]).hypot(paper[1][1] - paper[0][1]) < 0.75 {
            continue;
        }
        let tolerance = 0.03_f64.max(0.12 / view.scale.max(0.01));
        let visible = visibility.coverage([a.point, b.point], tolerance, &mut visibility_budget)?;
        let hidden = visible.is_empty();
        let spans = if let Some(shown) = &shown {
            shown.coverage([a.point, b.point], tolerance, &mut visibility_budget)?
        } else {
            visible
        };
        let mut pick_segments = Vec::new();
        for span in spans {
            let endpoints = span
                .map(|t| std::array::from_fn(|i| paper[0][i] + t * (paper[1][i] - paper[0][i])));
            for clipped in limo_cad_occt::drawing_export::clip_view_polyline_with_detail(
                view,
                &endpoints,
                detail_circle,
            )? {
                pick_segments.extend(clipped.windows(2).map(|p| [p[0], p[1]]));
            }
        }
        if pick_segments.is_empty() {
            continue;
        }
        pick_budget = pick_budget.saturating_add(pick_segments.len());
        if pick_budget > 16_384 {
            return Err("Too many rendered straight-edge pick segments on this view".into());
        }
        let depth = (0..3)
            .map(|i| (a.model_point[i] * 0.5 + b.model_point[i] * 0.5) * direction[i])
            .sum::<f64>();
        if !depth.is_finite() {
            return Err("Invalid projected straight-edge depth".into());
        }
        let target = LineTarget {
            view_id: view.id,
            scale: view.scale,
            paper,
            pick_segments,
            reference: DrawingLineRefDto {
                topology_signature: projection
                    .topology_signatures
                    .get(&body.to_string())
                    .cloned(),
                occurrence_id: a.occurrence_id,
                body_id: a.body_id,
                edge_id: a.edge_id,
                edge_key: a.edge_key.clone(),
                fallback_start: a.model_point,
                fallback_end: b.model_point,
            },
        };
        let key = segment_key(paper);
        let replace = unique.get(&key).is_none_or(|(_, old_hidden, old_depth)| {
            (*old_hidden && !hidden) || (*old_hidden == hidden && depth > *old_depth + 1e-7)
        });
        if replace {
            unique.insert(key, (target, hidden, depth));
        }
        if unique.len() > MAX_TARGETS {
            return Err("Too many straight-edge targets on this sheet".into());
        }
    }
    let mut targets: Vec<_> = unique.into_values().map(|(t, _, _)| t).collect();
    targets.sort_by_key(|t| {
        (
            t.reference.occurrence_id.map(|id| id.0),
            t.reference.body_id.0,
            t.reference.edge_id.0,
        )
    });
    Ok(targets)
}
