//! Scene adapters for the shared geometry chain solver. Queries are read-only.
use crate::SketchDto;
use limo_cad_core::{
    edge_chain::{self, Chain, Edge, JOIN_TOLERANCE},
    BodyId,
};
use limo_cad_solid::SolidSceneDto;
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainSource {
    Model,
    Sketch,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainMode {
    Manual,
    Closed,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EdgeChainRequest {
    pub source: ChainSource,
    #[serde(default)]
    pub body_ids: Vec<BodyId>,
    /// Optional working-plane normal; absent for general 3D chain consumers.
    pub normal: Option<[f64; 3]>,
    pub keys: Vec<String>,
    pub mode: ChainMode,
    #[serde(default)]
    pub reversed: bool,
}

pub fn candidates(
    scene: &SolidSceneDto,
    sketches: &[SketchDto],
    source: ChainSource,
    bodies: &[BodyId],
) -> Vec<Edge> {
    match source {
        ChainSource::Model => scene
            .bodies
            .iter()
            .filter(|b| bodies.is_empty() || bodies.contains(&b.id))
            .flat_map(|body| {
                body.edges
                    .iter()
                    .filter_map(|edge| {
                        let mut points: Vec<[f64; 3]> =
                            edge.points.iter().map(|p| [p.x, p.y, p.z]).collect();
                        if points.len() < 2 {
                            return None;
                        }
                        let closed = edge.circle.is_some_and(|c| c.closed)
                            || edge_chain::distance(points[0], *points.last().unwrap())
                                <= JOIN_TOLERANCE;
                        if closed
                            && edge_chain::distance(points[0], *points.last().unwrap())
                                <= JOIN_TOLERANCE
                        {
                            points.pop();
                        }
                        Some(Edge {
                            key: format!("edge:{}:{}", body.id.0, edge.key),
                            scope: format!("body:{}", body.id.0),
                            points,
                            closed,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect(),
        ChainSource::Sketch => sketches
            .iter()
            .flat_map(|sketch| {
                sketch.entities.iter().filter_map(|entity| {
                    let (uv, closed) = sketch_entity_polyline(entity)?;
                    Some(Edge {
                        key: format!("sketch:{}:{}", sketch.name, entity.id().0),
                        scope: format!("sketch:{}", sketch.name),
                        points: uv.into_iter().map(|p| sketch.basis.to_3d(p)).collect(),
                        closed,
                    })
                })
            })
            .collect(),
    }
}

fn sketch_entity_polyline(entity: &crate::dto::EntityDto) -> Option<(Vec<[f64; 2]>, bool)> {
    match entity {
        crate::dto::EntityDto::Line { start, end, .. } => {
            Some((vec![[start.x, start.y], [end.x, end.y]], false))
        }
        crate::dto::EntityDto::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        } => {
            let mut sweep = end_angle - start_angle;
            while sweep <= 0.0 {
                sweep += TAU;
            }
            let steps = ((sweep.to_degrees() / 5.0).ceil() as usize).max(4);
            Some((
                (0..=steps)
                    .map(|index| {
                        let angle = start_angle + sweep * index as f64 / steps as f64;
                        [
                            center.x + radius * angle.cos(),
                            center.y + radius * angle.sin(),
                        ]
                    })
                    .collect(),
                false,
            ))
        }
        crate::dto::EntityDto::Circle { center, radius, .. } => {
            let steps = 72usize;
            Some((
                (0..steps)
                    .map(|index| {
                        let angle = TAU * index as f64 / steps as f64;
                        [
                            center.x + radius * angle.cos(),
                            center.y + radius * angle.sin(),
                        ]
                    })
                    .collect(),
                true,
            ))
        }
        crate::dto::EntityDto::Spline { tessellation, .. } if tessellation.len() >= 2 => Some((
            tessellation
                .iter()
                .map(|point| [point.x, point.y])
                .collect(),
            false,
        )),
        crate::dto::EntityDto::Point { .. } | crate::dto::EntityDto::Spline { .. } => None,
    }
}

pub fn resolve(
    scene: &SolidSceneDto,
    sketches: &[SketchDto],
    request: &EdgeChainRequest,
) -> Result<Chain, String> {
    let all = candidates(scene, sketches, request.source, &request.body_ids);
    if request.mode == ChainMode::Manual {
        return edge_chain::resolve(&all, &request.keys, request.reversed);
    }
    if request.keys.len() != 1 {
        return Err("Automatic closed-chain selection needs one seed edge.".into());
    }
    let key = &request.keys[0];
    let seed = all
        .iter()
        .find(|e| &e.key == key)
        .ok_or("The picked edge no longer exists.")?;
    let normal = if let Some(n) = request.normal {
        let length = n.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !length.is_finite() || length < 1e-9 {
            return Err("Invalid chain working plane.".into());
        }
        Some(n.map(|v| v / length))
    } else {
        None
    };
    let coplanar = |edge: &Edge| {
        normal.is_none_or(|n| {
            edge.points.iter().all(|p| {
                (0..3)
                    .map(|i| (p[i] - seed.points[0][i]) * n[i])
                    .sum::<f64>()
                    .abs()
                    <= JOIN_TOLERANCE
            })
        })
    };
    if !coplanar(seed) {
        return Err(
            "This edge is not in the working plane. Use Manual edges for a 3D chain.".into(),
        );
    }
    let allowed = all
        .iter()
        .filter(|e| e.scope == seed.scope && coplanar(e))
        .cloned()
        .collect::<Vec<_>>();

    let mut alternatives = Vec::new();
    if request.source == ChainSource::Model {
        for body in &scene.bodies {
            if seed.scope != format!("body:{}", body.id.0) {
                continue;
            }
            let local = key
                .strip_prefix(&format!("edge:{}:", body.id.0))
                .unwrap_or("");
            for face in &body.faces {
                if !face.edge_keys.iter().any(|k| k == local) {
                    continue;
                }
                if let Some(n) = normal {
                    if !face.plane.is_some_and(|p| {
                        (0..3).map(|i| p.normal[i] * n[i]).sum::<f64>().abs() > 1. - 1e-8
                    }) {
                        continue;
                    }
                }
                let edges = allowed
                    .iter()
                    .filter(|e| {
                        face.edge_keys
                            .iter()
                            .any(|k| e.key == format!("edge:{}:{k}", body.id.0))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if let Ok(chain) = edge_chain::closed_component(&edges, key) {
                    let mut identity = chain.keys.clone();
                    identity.sort();
                    if !alternatives.iter().any(|(keys, _)| keys == &identity) {
                        alternatives.push((identity, chain));
                    }
                }
            }
        }
    }
    let mut chain=match alternatives.len() {
        0=>edge_chain::closed_component(&allowed,key)?,
        1=>alternatives.pop().unwrap().1,
        _=>return Err("More than one face loop uses this edge. Use Manual edges to choose the intended boundary.".into()),
    };
    if request.reversed {
        chain.points.reverse();
    }
    Ok(chain)
}
