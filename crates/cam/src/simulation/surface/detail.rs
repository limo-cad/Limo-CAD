//! Conforming, error-driven display subdivision. Shared edges get one shared
//! point, including when only one adjacent triangle needs more detail. Large
//! flat faces cost no additional triangles; the existing hard budget applies.
use super::*;

#[derive(Clone)]
struct Triangle {
    indices: [usize; 3],
    feature: Option<Feature>,
    normal: [f64; 3],
}

struct Edge {
    indices: [usize; 2],
    features: Vec<Feature>,
    uses: usize,
    midpoint: Option<usize>,
}

impl Refiner<'_> {
    pub(in super::super) fn refine_mesh(
        &self,
        mesh: CamSimulationMeshDto,
        max_triangles: usize,
        cell: [f64; 3],
    ) -> CamSimulationMeshDto {
        if !self.analytic_stock() || mesh.triangle_count == 0 {
            return mesh;
        }
        let mut vertices = Vec::<[f64; 3]>::new();
        let mut lookup = HashMap::new();
        let mut triangles = Vec::with_capacity(mesh.triangle_count);
        for (p, n) in mesh
            .positions
            .as_chunks::<9>()
            .0
            .iter()
            .zip(mesh.normals.as_chunks::<9>().0)
        {
            let indices = std::array::from_fn(|i| {
                let point = [p[i * 3], p[i * 3 + 1], p[i * 3 + 2]];

                let key = point.map(|v| if v == 0. { 0 } else { v.to_bits() });
                *lookup.entry(key).or_insert_with(|| {
                    let id = vertices.len();
                    vertices.push(point.map(f64::from));
                    id
                })
            });
            let points = indices.map(|i| vertices[i]);
            let normal = unit(std::array::from_fn(|i| (n[i] + n[i + 3] + n[i + 6]) as f64));
            let key = std::array::from_fn(|i| if i < 9 { p[i] } else { n[i - 9] }.to_bits());
            triangles.push(Triangle {
                indices,
                feature: self.cached_feature(key, || self.triangle_feature(points, normal)),
                normal,
            });
        }

        let error = cell.into_iter().fold(f64::INFINITY, f64::min) * 0.025;
        triangles = self.split_creases(triangles, &mut vertices, max_triangles, error);
        for _ in 0..2 {
            let mut edge_map = HashMap::<[usize; 2], usize>::new();
            let mut edges = Vec::<Edge>::new();
            for triangle in &triangles {
                for i in 0..3 {
                    let key = edge_key(triangle.indices[i], triangle.indices[(i + 1) % 3]);
                    let id = *edge_map.entry(key).or_insert_with(|| {
                        let id = edges.len();
                        edges.push(Edge {
                            indices: key,
                            features: Vec::with_capacity(2),
                            uses: 0,
                            midpoint: None,
                        });
                        id
                    });
                    edges[id].uses += 1;
                    if let Some(f) = triangle.feature {
                        if !edges[id].features.contains(&f) {
                            edges[id].features.push(f);
                        }
                    }
                }
            }
            let mut candidates = Vec::new();
            for (id, edge) in edges.iter().enumerate() {
                if edge.features.is_empty() || edge.uses > 2 {
                    continue;
                }
                let [a, b] = edge.indices.map(|i| vertices[i]);
                let key = (
                    std::array::from_fn(|i| if i < 3 { a[i] } else { b[i - 3] }.to_bits()),
                    [
                        edge.features.first().copied(),
                        edge.features.get(1).copied(),
                    ],
                );
                let candidate = (edge.features.len() <= 2)
                    .then(|| {
                        self.cached_edge(key, || {
                            let middle = add(a, b).map(|v| v * 0.5);
                            let projected = self.project_features(middle, &edge.features);
                            let change = dot(sub(projected, middle), sub(projected, middle)).sqrt();
                            let length = dot(sub(a, b), sub(a, b)).sqrt();
                            (change > error
                                && change < (length * 0.5).min(self.band * 0.5)
                                && self
                                    .sample(projected)
                                    .is_some_and(|s| s.distance.abs() < error * 0.1)
                                && edge.features.iter().all(|&f| {
                                    self.component(f, projected).distance.abs() < error * 0.1
                                }))
                            .then_some((projected, change))
                        })
                    })
                    .flatten();
                if let Some((projected, change)) = candidate {
                    candidates.push((id, projected, change));
                }
            }

            candidates.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
            let mut count = triangles.len();
            let mut added = 0;
            for (id, point, _) in candidates {
                let edge = &mut edges[id];
                if count + edge.uses > max_triangles {
                    continue;
                }
                edge.midpoint = Some(vertices.len());
                vertices.push(point);
                count += edge.uses;
                added += 1;
            }
            if added == 0 {
                break;
            }
            let mut refined = Vec::with_capacity(count);
            for triangle in triangles {
                let [a, b, c] = triangle.indices;
                let mids = [(a, b), (b, c), (c, a)]
                    .map(|(a, b)| edges[edge_map[&edge_key(a, b)]].midpoint);
                let pieces = split([a, b, c], mids);
                for indices in pieces {
                    refined.push(Triangle {
                        indices,
                        feature: triangle.feature,
                        normal: triangle.normal,
                    });
                }
            }
            triangles = refined;
        }
        let mut result = CamSimulationMeshDto {
            positions: Vec::with_capacity(triangles.len() * 9),
            normals: Vec::with_capacity(triangles.len() * 9),
            triangle_count: 0,
        };
        for triangle in triangles {
            let points = triangle.indices.map(|id| vertices[id]);
            let normals = points.map(|point| {
                triangle
                    .feature
                    .map_or(triangle.normal, |f| {
                        let normal = self.component(f, point).normal;
                        if dot(normal, triangle.normal) > 0.05 {
                            unit(normal)
                        } else {
                            triangle.normal
                        }
                    })
                    .map(|v| v as f32)
            });
            push_triangle_with_normals(&mut result.positions, &mut result.normals, points, normals);
        }
        result.triangle_count = result.positions.len() / 9;
        result
    }

    fn triangle_feature(&self, points: [[f64; 3]; 3], normal: [f64; 3]) -> Option<Feature> {
        let center = std::array::from_fn(|i| points.iter().map(|p| p[i]).sum::<f64>() / 3.);
        let mut candidates = Vec::with_capacity(4);
        for p in points.into_iter().chain([center]) {
            if let Some(s) = self.sample(p) {
                if !candidates.contains(&s.feature) {
                    candidates.push(s.feature);
                }
            }
        }
        candidates
            .into_iter()
            .filter(|&f| dot(self.component(f, center).normal, normal) > 0.05)
            .min_by(|&a, &b| {
                let error = |f| {
                    points
                        .iter()
                        .map(|&p| self.component(f, p).distance.powi(2))
                        .sum::<f64>()
                };
                error(a).total_cmp(&error(b)).then(a.cmp(&b))
            })
    }

    /// A grid triangle can straddle a narrow bevel/wall intersection even
    /// after its shared vertices have been reconstructed. Insert the actual
    /// intersection curve before shading: one normal per side, not a whole
    /// cylinder-colored triangle bleeding into the chamfer.
    fn split_creases(
        &self,
        input: Vec<Triangle>,
        vertices: &mut Vec<[f64; 3]>,
        budget: usize,
        error: f64,
    ) -> Vec<Triangle> {
        let original_vertices = vertices.len();
        let mut output = Vec::with_capacity(input.len());
        let mut intersections = HashMap::<([usize; 2], [Feature; 2]), usize>::new();
        for (index, triangle) in input.iter().enumerate() {
            let points = triangle.indices.map(|i| vertices[i]);
            let mut features = Vec::new();
            for point in points {
                if let Some(s) = self.sample(point) {
                    if !features.contains(&s.feature) {
                        features.push(s.feature);
                    }
                }
            }
            features.sort();
            if features.len() != 2 {
                output.push(triangle.clone());
                continue;
            }
            let features = [features[0], features[1]];
            let center = std::array::from_fn(|i| points.iter().map(|p| p[i]).sum::<f64>() / 3.);
            if dot(
                self.component(features[0], center).normal,
                self.component(features[1], center).normal,
            ) > 0.995
            {
                output.push(triangle.clone());
                continue;
            }

            let epsilon = error * 0.002;
            let values = points.map(|p| {
                let value = self.dominance(features, p);
                if value.abs() < epsilon {
                    0.
                } else {
                    value
                }
            });
            if !values.iter().any(|v| *v > 0.) || !values.iter().any(|v| *v < 0.) {
                output.push(triangle.clone());
                continue;
            }
            let mut crossing = [None; 3];
            for i in 0..3 {
                let j = (i + 1) % 3;
                if values[i] * values[j] >= 0. {
                    continue;
                }
                let edge = edge_key(triangle.indices[i], triangle.indices[j]);
                let key = (edge, features);
                let id = *intersections.entry(key).or_insert_with(|| {
                    let a = vertices[edge[0]];
                    let b = vertices[edge[1]];
                    let mut lo = 0.;
                    let mut hi = 1.;
                    let first = self.dominance(features, a);
                    for _ in 0..24 {
                        let t = (lo + hi) * 0.5;
                        let p = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
                        if (self.dominance(features, p) < 0.) == (first < 0.) {
                            lo = t;
                        } else {
                            hi = t;
                        }
                    }
                    let t = (lo + hi) * 0.5;
                    let p = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
                    let projected = self.project_features(p, &features);
                    let point = if dot(sub(projected, p), sub(projected, p)) < error.powi(2) * 16.
                        && self
                            .sample(projected)
                            .is_some_and(|s| s.distance.abs() < error * 0.1)
                    {
                        projected
                    } else {
                        p
                    };
                    let id = vertices.len();
                    vertices.push(point);
                    id
                });
                crossing[i] = Some(id);
            }
            for (side, feature) in features.iter().enumerate() {
                let mut polygon = Vec::with_capacity(4);
                for i in 0..3 {
                    if if side == 0 {
                        values[i] >= 0.
                    } else {
                        values[i] <= 0.
                    } {
                        polygon.push(triangle.indices[i]);
                    }
                    if let Some(id) = crossing[i] {
                        polygon.push(id);
                    }
                }
                for i in 1..polygon.len().saturating_sub(1) {
                    output.push(Triangle {
                        indices: [polygon[0], polygon[i], polygon[i + 1]],
                        feature: Some(*feature),
                        normal: triangle.normal,
                    });
                }
            }
            if output.len() + input.len() - index - 1 > budget {
                vertices.truncate(original_vertices);
                return input;
            }
        }
        output
    }

    fn dominance(&self, features: [Feature; 2], point: [f64; 3]) -> f64 {
        let a = self.component(features[0], point).distance;
        let b = self.component(features[1], point).distance;

        let same_cut = matches!(features, [Feature::Cut(a, _), Feature::Cut(b, _)] if a == b);
        if same_cut {
            b - a
        } else {
            a - b
        }
    }
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let length = dot(v, v).sqrt();
    if length > EPSILON {
        v.map(|c| c / length)
    } else {
        [0., 0., 1.]
    }
}

fn edge_key(a: usize, b: usize) -> [usize; 2] {
    if a < b {
        [a, b]
    } else {
        [b, a]
    }
}

fn split([a, b, c]: [usize; 3], mids: [Option<usize>; 3]) -> Vec<[usize; 3]> {
    match mids {
        [None, None, None] => vec![[a, b, c]],
        [Some(ab), None, None] => vec![[a, ab, c], [ab, b, c]],
        [None, Some(bc), None] => vec![[b, bc, a], [bc, c, a]],
        [None, None, Some(ca)] => vec![[c, ca, b], [ca, a, b]],
        [Some(ab), Some(bc), None] => vec![[ab, b, bc], [a, ab, c], [ab, bc, c]],
        [None, Some(bc), Some(ca)] => vec![[bc, c, ca], [b, bc, a], [bc, ca, a]],
        [Some(ab), None, Some(ca)] => vec![[ca, a, ab], [c, ca, b], [ca, ab, b]],
        [Some(ab), Some(bc), Some(ca)] => vec![[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crease_splitting_reserves_budget_for_the_unprocessed_faces() {
        let spec = GridSpec {
            min: Point3Dto::new(-4., -4., -3.),
            dimensions: [80, 80, 30],
            cell_size: [0.1; 3],
            edge: 0.1,
        };
        let tool: CamToolDto = serde_json::from_value(serde_json::json!({
            "id":1, "number":1, "name":"Chamfer", "kind":"chamfer_mill",
            "diameter":6., "flute_length":12., "overall_length":30., "point_angle_degrees":90.
        }))
        .unwrap();
        let mut cuts = DisplayCuts {
            initial: Some(StockBoundary::new(&spec, CamResolvedStockDto::Box)),
            ..Default::default()
        };
        let tip = Point3Dto::new(0., 0., -1.);
        cuts.record(&tool, tip, tip);
        let refiner = Refiner::new(&cuts, spec.cell_size);
        let mut vertices = vec![
            [1.1, -0.1, 0.],
            [0.9, -0.1, -0.1],
            [1.1, 0.1, 0.],
            [2., 2., 0.],
            [3., 2., 0.],
            [2., 3., 0.],
        ];
        let faces = vec![
            Triangle {
                indices: [0, 1, 2],
                feature: Some(Feature::Stock(5)),
                normal: [0., 0., 1.],
            },
            Triangle {
                indices: [3, 4, 5],
                feature: Some(Feature::Stock(5)),
                normal: [0., 0., 1.],
            },
        ];
        let split = refiner.split_creases(faces.clone(), &mut vertices, 3, 0.0025);
        assert_eq!(
            split.len(),
            2,
            "whole crease pass must fall back before later flat faces exceed the budget"
        );
        assert_eq!(vertices.len(), 6);
        let split = refiner.split_creases(faces, &mut vertices, 4, 0.0025);
        assert_eq!(
            split.len(),
            4,
            "the test must actually exercise a crease split"
        );
    }

    #[test]
    fn every_shared_edge_split_preserves_area_winding_and_boundary() {
        let p = [[0., 0.], [2., 0.], [0., 2.], [1., 0.], [1., 1.], [0., 1.]];
        for mask in 0usize..8 {
            let mids = std::array::from_fn(|i| (mask & (1 << i) != 0).then_some(i + 3));
            let triangles = split([0, 1, 2], mids);
            assert_eq!(triangles.len(), 1 + mask.count_ones() as usize);
            let mut edges = HashMap::<[usize; 2], usize>::new();
            let mut area = 0.;
            for indices in triangles {
                let [a, b, c] = indices.map(|i| p[i]);
                let doubled = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                assert!(doubled > 0.);
                area += doubled;
                for i in 0..3 {
                    *edges
                        .entry(edge_key(indices[i], indices[(i + 1) % 3]))
                        .or_default() += 1;
                }
            }
            assert_eq!(area, 4.);
            for (i, mid) in mids.into_iter().enumerate() {
                let end = (i + 1) % 3;
                if let Some(mid) = mid {
                    assert_eq!(edges.remove(&edge_key(i, mid)), Some(1));
                    assert_eq!(edges.remove(&edge_key(mid, end)), Some(1));
                } else {
                    assert_eq!(edges.remove(&edge_key(i, end)), Some(1));
                }
            }
            assert!(edges.values().all(|n| *n == 2));
        }
    }
}
