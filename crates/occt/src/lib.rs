//! Native OCCT adapter.
//!
//! The C++ bridge is enabled by the `native-occt` feature in the Tauri
//! shell. Keeping the feature off lets the host-neutral workspace and WASM
//! target build on machines that do not have the OCCT SDK installed.

mod interference;
pub use interference::{exact_interference_report, exact_pair_result};

use std::collections::HashSet;

use nbcad_core::{BodyId, EdgeId};
use nbcad_solid::SolidSceneDto;
#[cfg(not(feature = "native-occt"))]
use nbcad_solid::{KernelSceneDto, RecomputePlanDto};
use serde::{Deserialize, Serialize};

/// Orthographic hidden-line projection request. `direction` points from the
/// model toward the viewer; `up` is the desired page-up direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawingProjectionRequest {
    #[serde(default)]
    pub body_ids: Vec<BodyId>,
    pub direction: [f64; 3],
    pub up: [f64; 3],
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default)]
    pub include_tangent_edges: bool,
    #[serde(default = "default_projection_deflection")]
    pub deflection: f64,
    /// Optional exact cutting plane for associative section and removed-section
    /// views. The plane is expressed in model millimetres.
    #[serde(default)]
    pub section_plane: Option<DrawingSectionPlaneDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawingSectionPlaneDto {
    pub point: [f64; 3],
    pub normal: [f64; 3],
    /// Optional viewing depth behind the cutting plane. `None` keeps the
    /// complete rear half-space; a positive value keeps only that finite slab.
    #[serde(default)]
    pub depth: Option<f64>,
}

fn default_projection_deflection() -> f64 {
    0.05
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawingPolylineDto {
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawingProjectionDto {
    pub visible: Vec<DrawingPolylineDto>,
    pub hidden: Vec<DrawingPolylineDto>,
    #[serde(default)]
    pub anchors: Vec<DrawingProjectionAnchorDto>,
    #[serde(default)]
    pub circles: Vec<DrawingProjectedCircleDto>,
    /// Exact OCCT intersection curves when a cutting plane was requested.
    #[serde(default)]
    pub section: Vec<DrawingPolylineDto>,
    /// min x, min y, max x, max y in model millimetres.
    pub bounds: [f64; 4],
}

/// Analytic circular intent recovered from stable B-rep edge tessellation.
/// Only edges viewed close to normal are exposed because an oblique circle is
/// an ellipse on paper and cannot carry an unambiguous diameter/radius mark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawingProjectedCircleDto {
    pub body_id: BodyId,
    pub edge_id: EdgeId,
    pub edge_key: String,
    pub center_model: [f64; 3],
    pub normal_model: [f64; 3],
    pub center: [f64; 2],
    pub radius: f64,
    pub closed: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingProjectionAnchorEndpoint {
    Start,
    End,
}

/// Exact topological endpoint projected into the same model-millimetre page
/// coordinates as hidden-line output. Every edge endpoint is retained: two
/// edges sharing one geometric vertex are distinct associative references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawingProjectionAnchorDto {
    pub body_id: BodyId,
    pub edge_id: EdgeId,
    pub edge_key: String,
    pub endpoint: DrawingProjectionAnchorEndpoint,
    pub model_point: [f64; 3],
    pub point: [f64; 2],
    pub hidden: bool,
}

/// Project stable topology endpoints with the same orthographic basis used by
/// OCCT HLR. The UI may visually collapse coincident pick markers, but the data
/// contract preserves the exact edge identity required by associative notes.
pub fn drawing_projection_anchors(
    scene: &SolidSceneDto,
    request: &DrawingProjectionRequest,
    projection: &DrawingProjectionDto,
) -> Result<Vec<DrawingProjectionAnchorDto>, OcctError> {
    let direction = normalize(request.direction)?;
    let right = normalize(cross(request.up, direction))?;
    let page_up = normalize(cross(direction, right))?;
    let selected = request.body_ids.iter().copied().collect::<HashSet<_>>();
    let mut anchors = Vec::new();
    for body in &scene.bodies {
        if !selected.is_empty() && !selected.contains(&body.id) {
            continue;
        }
        for edge in &body.edges {
            let Some(first) = edge.points.first() else {
                continue;
            };
            let Some(last) = edge.points.last() else {
                continue;
            };
            for (endpoint, model_point) in [
                (DrawingProjectionAnchorEndpoint::Start, first),
                (DrawingProjectionAnchorEndpoint::End, last),
            ] {
                let model_point = [model_point.x, model_point.y, model_point.z];
                let point = [dot(model_point, right), dot(model_point, page_up)];
                let hidden = !point_touches_polylines(
                    point,
                    &projection.visible,
                    request.deflection.max(1.0e-4) * 2.5,
                );
                anchors.push(DrawingProjectionAnchorDto {
                    body_id: body.id,
                    edge_id: edge.id,
                    edge_key: edge.key.clone(),
                    endpoint,
                    model_point,
                    point,
                    hidden,
                });
            }
        }
    }
    anchors.sort_by_key(|anchor| {
        (
            anchor.body_id,
            anchor.edge_id,
            endpoint_order(anchor.endpoint),
        )
    });
    Ok(anchors)
}

/// Recover circular B-rep edges from their deterministic model-space samples
/// and project the fitted centers through the exact same drawing basis.
pub fn drawing_projection_circles(
    scene: &SolidSceneDto,
    request: &DrawingProjectionRequest,
    projection: &DrawingProjectionDto,
) -> Result<Vec<DrawingProjectedCircleDto>, OcctError> {
    let direction = normalize(request.direction)?;
    let right = normalize(cross(request.up, direction))?;
    let page_up = normalize(cross(direction, right))?;
    let selected = request.body_ids.iter().copied().collect::<HashSet<_>>();
    let mut candidates = Vec::new();
    for body in &scene.bodies {
        if !selected.is_empty() && !selected.contains(&body.id) {
            continue;
        }
        for edge in &body.edges {
            let points = edge
                .points
                .iter()
                .map(|point| [point.x, point.y, point.z])
                .collect::<Vec<_>>();
            let Some((center_model, normal_model, radius, closed)) = fit_circle(&points) else {
                continue;
            };
            // A circle viewed obliquely is an ellipse. Keep radial tools on
            // true circular projections, matching conventional drafting.
            if dot(normal_model, direction).abs() < 0.995 {
                continue;
            }
            let center = [dot(center_model, right), dot(center_model, page_up)];
            let hidden = !points.iter().any(|point| {
                let projected = [dot(*point, right), dot(*point, page_up)];
                point_touches_polylines(
                    projected,
                    &projection.visible,
                    request.deflection.max(1.0e-4) * 2.5,
                )
            });
            candidates.push((
                DrawingProjectedCircleDto {
                    body_id: body.id,
                    edge_id: edge.id,
                    edge_key: edge.key.clone(),
                    center_model,
                    normal_model,
                    center,
                    radius,
                    closed,
                    hidden,
                },
                dot(center_model, direction),
            ));
        }
    }
    let snapshot = candidates.clone();
    for (circle, depth) in &mut candidates {
        if !circle.hidden {
            continue;
        }
        let stack = snapshot
            .iter()
            .filter(|(other, _)| same_projected_circle(circle, other))
            .collect::<Vec<_>>();
        if stack.len() < 2 || stack.iter().any(|(other, _)| !other.hidden) {
            continue;
        }
        let front_depth = stack
            .iter()
            .map(|(_, candidate_depth)| *candidate_depth)
            .fold(f64::NEG_INFINITY, f64::max);
        if *depth >= front_depth - 1.0e-7 {
            circle.hidden = false;
        }
    }
    let mut circles = candidates
        .into_iter()
        .map(|(circle, _)| circle)
        .collect::<Vec<_>>();
    circles.sort_by_key(|circle| (circle.body_id, circle.edge_id));
    Ok(circles)
}

fn same_projected_circle(
    left: &DrawingProjectedCircleDto,
    right: &DrawingProjectedCircleDto,
) -> bool {
    let scale = left.radius.max(right.radius).max(1.0);
    let center_distance = ((left.center[0] - right.center[0]).powi(2)
        + (left.center[1] - right.center[1]).powi(2))
    .sqrt();
    center_distance <= scale * 1.0e-5 && (left.radius - right.radius).abs() <= scale * 1.0e-5
}

fn fit_circle(points: &[[f64; 3]]) -> Option<([f64; 3], [f64; 3], f64, bool)> {
    if points.len() < 5 {
        return None;
    }
    let first = points[0];
    let last = *points.last()?;
    let closed_guess = distance(first, last)
        <= points
            .windows(2)
            .map(|pair| distance(pair[0], pair[1]))
            .fold(0.0_f64, f64::max)
            * 1.5;
    let (a, b, c) = if closed_guess {
        (
            points[0],
            points[points.len() / 3],
            points[points.len() * 2 / 3],
        )
    } else {
        (points[0], points[points.len() / 2], last)
    };
    let ab = subtract3(b, a);
    let ac = subtract3(c, a);
    let normal_raw = cross(ab, ac);
    let normal_sq = dot(normal_raw, normal_raw);
    if normal_sq < 1.0e-16 {
        return None;
    }
    let ab_sq = dot(ab, ab);
    let ac_sq = dot(ac, ac);
    let term_a = scale3(cross(ac, normal_raw), ab_sq);
    let term_b = scale3(cross(normal_raw, ab), ac_sq);
    let center = add3(a, scale3(add3(term_a, term_b), 1.0 / (2.0 * normal_sq)));
    let radius = distance(center, a);
    if !radius.is_finite() || radius <= 1.0e-7 {
        return None;
    }
    let normal = scale3(normal_raw, 1.0 / normal_sq.sqrt());
    let radial_tolerance = radius.mul_add(2.0e-3, 2.0e-5);
    let planar_tolerance = radius.mul_add(1.0e-3, 2.0e-5);
    if points.iter().any(|point| {
        (distance(*point, center) - radius).abs() > radial_tolerance
            || dot(subtract3(*point, center), normal).abs() > planar_tolerance
    }) {
        return None;
    }
    let closed = distance(first, last) <= radial_tolerance * 2.0;
    Some((center, normal, radius, closed))
}

fn add3(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn subtract3(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale3(vector: [f64; 3], factor: f64) -> [f64; 3] {
    [vector[0] * factor, vector[1] * factor, vector[2] * factor]
}

fn distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    let delta = subtract3(left, right);
    dot(delta, delta).sqrt()
}

fn normalize(vector: [f64; 3]) -> Result<[f64; 3], OcctError> {
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(OcctError(
            "drawing projection basis contains non-finite values".to_string(),
        ));
    }
    let length = dot(vector, vector).sqrt();
    if length < 1.0e-9 {
        return Err(OcctError(
            "drawing projection basis is degenerate".to_string(),
        ));
    }
    Ok([vector[0] / length, vector[1] / length, vector[2] / length])
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn point_touches_polylines(
    point: [f64; 2],
    polylines: &[DrawingPolylineDto],
    tolerance: f64,
) -> bool {
    polylines.iter().any(|polyline| {
        polyline
            .points
            .windows(2)
            .any(|segment| point_segment_distance(point, segment[0], segment[1]) <= tolerance)
    })
}

fn point_segment_distance(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> f64 {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length_sq = delta[0] * delta[0] + delta[1] * delta[1];
    if length_sq <= 1.0e-18 {
        return ((point[0] - start[0]).powi(2) + (point[1] - start[1]).powi(2)).sqrt();
    }
    let t = (((point[0] - start[0]) * delta[0] + (point[1] - start[1]) * delta[1]) / length_sq)
        .clamp(0.0, 1.0);
    let closest = [start[0] + delta[0] * t, start[1] + delta[1] * t];
    ((point[0] - closest[0]).powi(2) + (point[1] - closest[1]).powi(2)).sqrt()
}

fn endpoint_order(endpoint: DrawingProjectionAnchorEndpoint) -> u8 {
    match endpoint {
        DrawingProjectionAnchorEndpoint::Start => 0,
        DrawingProjectionAnchorEndpoint::End => 1,
    }
}

#[derive(Debug, Clone)]
pub struct OcctError(pub String);

/// One retained source B-rep placed as an assembly occurrence. The transform
/// uses translation plus an x/y/z/w unit quaternion.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlacedBodyQueryDto {
    pub body_id: nbcad_core::BodyId,
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExactInterferenceResultDto {
    pub minimum_clearance_mm: f64,
    pub overlap_volume_mm3: f64,
    pub closest_point_a: [f64; 3],
    pub closest_point_b: [f64; 3],
}

impl std::fmt::Display for OcctError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for OcctError {}

/// Stateful native kernel. The feature-enabled implementation is supplied
/// by `native.rs`; this placeholder keeps non-native workspace builds clean.
#[cfg(not(feature = "native-occt"))]
#[derive(Debug, Default)]
pub struct OcctKernel;

#[cfg(not(feature = "native-occt"))]
impl OcctKernel {
    pub fn new() -> Result<Self, OcctError> {
        Err(OcctError(
            "native OCCT support was not enabled at compile time".to_string(),
        ))
    }

    pub fn recompute(&mut self, _plan: &RecomputePlanDto) -> Result<KernelSceneDto, OcctError> {
        Err(OcctError(
            "native OCCT support was not enabled at compile time".to_string(),
        ))
    }

    pub fn drawing_projection(
        &self,
        _request: &DrawingProjectionRequest,
    ) -> Result<DrawingProjectionDto, OcctError> {
        Err(OcctError(
            "native OCCT support was not enabled at compile time".to_string(),
        ))
    }

    pub fn exact_interference(
        &self,
        _a: PlacedBodyQueryDto,
        _b: PlacedBodyQueryDto,
    ) -> Result<ExactInterferenceResultDto, OcctError> {
        Err(OcctError(
            "native OCCT support was not enabled at compile time".to_string(),
        ))
    }
}

#[cfg(feature = "native-occt")]
mod native;
#[cfg(feature = "native-occt")]
pub use native::OcctKernel;

#[cfg(test)]
mod drawing_anchor_tests {
    use super::*;
    use nbcad_core::FeatureId;
    use nbcad_solid::{BodyDto, EdgeDto, MeshDto, Point3Dto};

    #[test]
    fn projects_stable_topology_endpoints_into_hlr_coordinates() {
        let scene = SolidSceneDto {
            bodies: vec![BodyDto {
                id: BodyId(3),
                name: "Body1".to_string(),
                feature_id: FeatureId(1),
                mesh: MeshDto {
                    positions: vec![],
                    normals: vec![],
                    indices: vec![],
                },
                faces: vec![],
                edges: vec![EdgeDto {
                    id: EdgeId(7),
                    key: "edge-7".to_string(),
                    points: vec![
                        Point3Dto {
                            x: -10.0,
                            y: -5.0,
                            z: 2.0,
                        },
                        Point3Dto {
                            x: 10.0,
                            y: -5.0,
                            z: 2.0,
                        },
                    ],
                    circle: None,
                    refinable: true,
                }],
            }],
            errors: vec![],
        };
        let request = DrawingProjectionRequest {
            body_ids: vec![BodyId(3)],
            direction: [0.0, 0.0, 1.0],
            up: [0.0, 1.0, 0.0],
            include_hidden: false,
            include_tangent_edges: false,
            deflection: 0.05,
            section_plane: None,
        };
        let projection = DrawingProjectionDto {
            visible: vec![DrawingPolylineDto {
                points: vec![[-10.0, -5.0], [10.0, -5.0]],
            }],
            hidden: vec![],
            anchors: vec![],
            circles: vec![],
            section: vec![],
            bounds: [-10.0, -5.0, 10.0, -5.0],
        };

        let anchors = drawing_projection_anchors(&scene, &request, &projection).unwrap();
        assert_eq!(anchors.len(), 2);
        assert_eq!(anchors[0].edge_id, EdgeId(7));
        assert_eq!(anchors[0].point, [-10.0, -5.0]);
        assert_eq!(anchors[1].point, [10.0, -5.0]);
        assert!(anchors.iter().all(|anchor| !anchor.hidden));
    }

    #[test]
    fn retains_distinct_edge_references_at_a_shared_vertex() {
        let edge = |id, key: &str, first: [f64; 3], last: [f64; 3]| EdgeDto {
            id: EdgeId(id),
            key: key.to_string(),
            points: vec![
                Point3Dto {
                    x: first[0],
                    y: first[1],
                    z: first[2],
                },
                Point3Dto {
                    x: last[0],
                    y: last[1],
                    z: last[2],
                },
            ],
            circle: None,
            refinable: true,
        };
        let scene = SolidSceneDto {
            bodies: vec![BodyDto {
                id: BodyId(3),
                name: "Body1".to_string(),
                feature_id: FeatureId(1),
                mesh: MeshDto {
                    positions: vec![],
                    normals: vec![],
                    indices: vec![],
                },
                faces: vec![],
                edges: vec![
                    edge(7, "edge-7", [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
                    edge(8, "edge-8", [0.0, 0.0, 0.0], [0.0, 10.0, 0.0]),
                ],
            }],
            errors: vec![],
        };
        let request = DrawingProjectionRequest {
            body_ids: vec![BodyId(3)],
            direction: [0.0, 0.0, 1.0],
            up: [0.0, 1.0, 0.0],
            include_hidden: false,
            include_tangent_edges: false,
            deflection: 0.05,
            section_plane: None,
        };
        let projection = DrawingProjectionDto {
            visible: vec![DrawingPolylineDto {
                points: vec![[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]],
            }],
            hidden: vec![],
            anchors: vec![],
            circles: vec![],
            section: vec![],
            bounds: [0.0, 0.0, 10.0, 10.0],
        };

        let anchors = drawing_projection_anchors(&scene, &request, &projection).unwrap();
        assert_eq!(anchors.len(), 4);
        assert!(anchors.iter().any(|anchor| anchor.edge_id == EdgeId(7)
            && anchor.endpoint == DrawingProjectionAnchorEndpoint::Start));
        assert!(anchors.iter().any(|anchor| anchor.edge_id == EdgeId(8)
            && anchor.endpoint == DrawingProjectionAnchorEndpoint::Start));
    }

    #[test]
    fn recognizes_circular_edges_and_exposes_only_the_front_rim() {
        let circle_points = |z: f64| {
            (0..=32)
                .map(|index| {
                    let angle = std::f64::consts::TAU * f64::from(index) / 32.0;
                    Point3Dto {
                        x: 4.0 + angle.cos() * 8.0,
                        y: -3.0 + angle.sin() * 8.0,
                        z,
                    }
                })
                .collect::<Vec<_>>()
        };
        let scene = SolidSceneDto {
            bodies: vec![BodyDto {
                id: BodyId(9),
                name: "Cylinder".to_string(),
                feature_id: FeatureId(2),
                mesh: MeshDto {
                    positions: vec![],
                    normals: vec![],
                    indices: vec![],
                },
                faces: vec![],
                edges: vec![
                    EdgeDto {
                        id: EdgeId(1),
                        key: "bottom-rim".to_string(),
                        points: circle_points(0.0),
                        circle: None,
                        refinable: true,
                    },
                    EdgeDto {
                        id: EdgeId(2),
                        key: "top-rim".to_string(),
                        points: circle_points(10.0),
                        circle: None,
                        refinable: true,
                    },
                ],
            }],
            errors: vec![],
        };
        let request = DrawingProjectionRequest {
            body_ids: vec![],
            direction: [0.0, 0.0, 1.0],
            up: [0.0, 1.0, 0.0],
            include_hidden: false,
            include_tangent_edges: false,
            deflection: 0.05,
            section_plane: None,
        };
        // Empty visible HLR simulates the coplanar-boundary ambiguity that the
        // front-rim fallback is designed to resolve.
        let projection = DrawingProjectionDto {
            visible: vec![],
            hidden: vec![],
            anchors: vec![],
            circles: vec![],
            section: vec![],
            bounds: [-4.0, -11.0, 12.0, 5.0],
        };

        let circles = drawing_projection_circles(&scene, &request, &projection).unwrap();
        assert_eq!(circles.len(), 2);
        assert!(circles.iter().all(|circle| circle.closed));
        assert!(circles
            .iter()
            .all(|circle| (circle.radius - 8.0).abs() < 1.0e-8));
        assert!(
            circles
                .iter()
                .find(|circle| circle.edge_id == EdgeId(1))
                .unwrap()
                .hidden
        );
        assert!(
            !circles
                .iter()
                .find(|circle| circle.edge_id == EdgeId(2))
                .unwrap()
                .hidden
        );
    }
}
