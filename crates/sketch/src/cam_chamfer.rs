//! Resolve modeled 45-degree chamfers from exact face/edge membership.
//! Never infer a bevel from a display-mesh silhouette or an unrelated face.
use limo_cad_cam::{CamChainRefDto, CamChainSource, CamSetupDto, ContourCompensation, Point2Dto};
use limo_cad_core::edge_chain::{self, JOIN_TOLERANCE as TOL};
use limo_cad_solid::{Point3Dto, SolidSceneDto};
use serde::{Deserialize, Serialize};
#[cfg(test)]
#[path = "cam_chamfer_tests.rs"]
mod tests;
use crate::edge_selection::{self, ChainSource};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
pub struct CamChamferGeometryRequest {
    pub setup_id: u64,
    pub chain_ref: CamChainRefDto,
}

#[derive(Debug, Clone, Serialize)]
pub struct CamChamferGeometry {
    pub path: Vec<Point2Dto>,
    pub closed: bool,
    pub top_z: f64,
    pub width: f64,
    pub wall_side: ContourCompensation,
    pub selected_z: f64,
    /// Additional corner faces interrupt the lower rim; the constant-angle
    /// pass may leave material on those transitions rather than gouging them.
    pub corner_transitions: bool,
}

pub fn resolve(
    scene: &SolidSceneDto,
    setup: &CamSetupDto,
    reference: &CamChainRefDto,
) -> Result<CamChamferGeometry, String> {
    if reference.source != CamChainSource::Model {
        return Err("Modeled chamfers require model edges.".into());
    }
    let edges = edge_selection::candidates(scene, &[], ChainSource::Model, &setup.body_ids);
    let selected = edge_chain::resolve(&edges, &reference.keys, reference.reversed)?;
    let z = |p: [f64; 3]| {
        (p[0] - setup.wcs.origin.x) * setup.wcs.z_axis[0]
            + (p[1] - setup.wcs.origin.y) * setup.wcs.z_axis[1]
            + (p[2] - setup.wcs.origin.z) * setup.wcs.z_axis[2]
    };
    let array = |p: Point3Dto| [p.x, p.y, p.z];
    let dot = |a: [f64; 3], b: [f64; 3]| (0..3).map(|i| a[i] * b[i]).sum::<f64>();
    let level = selected
        .points
        .first()
        .map(|p| z(*p))
        .ok_or("Empty chamfer selection.")?;
    if selected.points.iter().any(|p| (z(*p) - level).abs() > TOL) {
        return Err("2D chamfer edges must share one setup-Z plane.".into());
    }
    let mut upper_keys = Vec::new();
    let mut lower_keys = Vec::new();
    let mut cone_material_inside = None;
    let mut outward = BTreeMap::new();
    let mut top: Option<f64> = None;
    let mut bottom: Option<f64> = None;
    for key in &reference.keys {
        let mut matches = Vec::new();
        for body in &scene.bodies {
            let Some(local) = key.strip_prefix(&format!("edge:{}:", body.id.0)) else {
                continue;
            };
            for face in &body.faces {
                if !face.edge_keys.iter().any(|k| k == local) {
                    continue;
                }
                let plane_normal = face.plane.map(|p| p.normal);
                let planar = plane_normal.is_some_and(|n| {
                    (dot(n, setup.wcs.z_axis).abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6
                });
                let conical = face.cone.is_some_and(|c| {
                    (dot(array(c.axis), setup.wcs.z_axis).abs() - 1.).abs() < 1e-6
                        && (c.semi_angle.abs() - std::f64::consts::FRAC_PI_4).abs() < 1e-6
                });
                if !planar && !conical {
                    continue;
                }
                let boundary = body
                    .edges
                    .iter()
                    .filter(|e| face.edge_keys.contains(&e.key))
                    .collect::<Vec<_>>();
                if boundary.is_empty()
                    || boundary.iter().any(|e| {
                        e.points.len() < 2
                            || e.points
                                .iter()
                                .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
                    })
                {
                    continue;
                }
                let heights = boundary
                    .iter()
                    .flat_map(|e| e.points.iter().map(|p| z(array(*p))))
                    .collect::<Vec<_>>();
                let lo = heights.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if !lo.is_finite()
                    || !hi.is_finite()
                    || hi - lo <= TOL
                    || ((level - lo).abs() > TOL && (level - hi).abs() > TOL)
                {
                    continue;
                }
                let upper = boundary
                    .iter()
                    .filter(|e| e.points.iter().all(|p| (z(array(*p)) - hi).abs() <= TOL))
                    .copied()
                    .collect::<Vec<_>>();
                let lower = boundary
                    .iter()
                    .filter(|e| e.points.iter().all(|p| (z(array(*p)) - lo).abs() <= TOL))
                    .copied()
                    .collect::<Vec<_>>();
                if upper.is_empty() || lower.is_empty() {
                    continue;
                }

                if !upper.iter().all(|edge| {
                    body.faces.iter().any(|adjacent| {
                        adjacent.key != face.key
                            && adjacent.edge_keys.contains(&edge.key)
                            && adjacent
                                .plane
                                .is_some_and(|p| dot(p.normal, setup.wcs.z_axis) > 1. - 1e-6)
                    })
                }) {
                    continue;
                }
                let hi = z(array(upper[0].points[0]));
                let lo = z(array(lower[0].points[0]));
                let plane_normal = plane_normal.map(|n| {
                    let middle = |e: &&limo_cad_solid::EdgeDto| {
                        let a = e.points[0];
                        let b = *e.points.last().unwrap();
                        [(a.x + b.x) * 0.5, (a.y + b.y) * 0.5, (a.z + b.z) * 0.5]
                    };
                    let u = middle(&upper[0]);
                    let l = middle(&lower[0]);
                    let d = std::array::from_fn(|i| l[i] - u[i]);
                    let planar_n = std::array::from_fn(|i| {
                        n[i] - dot(n, setup.wcs.z_axis) * setup.wcs.z_axis[i]
                    });
                    if dot(d, planar_n) < 0. {
                        n.map(|v| -v)
                    } else {
                        n
                    }
                });

                if conical {
                    let Some(u) = upper.iter().find_map(|e| e.circle) else {
                        continue;
                    };
                    let Some(l) = lower.iter().find_map(|e| e.circle) else {
                        continue;
                    };
                    let d = [
                        u.center.x - l.center.x,
                        u.center.y - l.center.y,
                        u.center.z - l.center.z,
                    ];
                    let axial = dot(d, setup.wcs.z_axis);
                    if !u.closed
                        || !l.closed
                        || (0..3).any(|i| (d[i] - axial * setup.wcs.z_axis[i]).abs() > TOL)
                    {
                        continue;
                    }
                    if (u.radius - l.radius).abs() <= TOL
                        || ((u.radius - l.radius).abs() - (hi - lo)).abs() > TOL
                    {
                        continue;
                    }
                    cone_material_inside = Some(l.radius > u.radius);
                }
                matches.push((body.id.0, hi, lo, upper, lower, plane_normal));
            }
        }
        if matches.len() != 1 {
            return Err("Cannot identify one accessible 45° modeled chamfer at every selected edge. Select the bevel's upper/lower rim, or use Sharp edge + width; unsupported/ambiguous surfaces are not guessed.".into());
        }
        let (body, hi, lo, upper, lower, normal) = matches.pop().unwrap();
        if top.is_some_and(|v| (v - hi).abs() > TOL) || bottom.is_some_and(|v| (v - lo).abs() > TOL)
        {
            return Err("The selected chamfers have different widths or heights. Program them as separate toolpaths.".into());
        }
        top = Some(hi);
        bottom = Some(lo);
        for edge in lower {
            let key = format!("edge:{body}:{}", edge.key);
            if !lower_keys.contains(&key) {
                lower_keys.push(key);
            }
        }
        for edge in upper {
            let key = format!("edge:{body}:{}", edge.key);
            if !upper_keys.contains(&key) {
                upper_keys.push(key.clone());
            }
            if let Some(n) = normal {
                outward.insert(key, n);
            }
        }
    }
    let corner_transitions = edge_chain::resolve(&edges, &lower_keys, false)
        .map_or(true, |chain| chain.closed != selected.closed);
    let mut result = finish(
        setup,
        reference,
        &edges,
        selected.closed,
        upper_keys,
        outward,
        cone_material_inside,
        top.unwrap(),
        bottom.unwrap(),
        level,
    )?;
    result.corner_transitions = corner_transitions;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn finish(
    setup: &CamSetupDto,
    reference: &CamChainRefDto,
    edges: &[edge_chain::Edge],
    selected_closed: bool,
    upper_keys: Vec<String>,
    outward: BTreeMap<String, [f64; 3]>,
    cone_material_inside: Option<bool>,
    top: f64,
    bottom: f64,
    selected_z: f64,
) -> Result<CamChamferGeometry, String> {
    let dot = |a: [f64; 3], b: [f64; 3]| (0..3).map(|i| a[i] * b[i]).sum::<f64>();
    let tangent = |e: &edge_chain::Edge| std::array::from_fn(|i| e.points[1][i] - e.points[0][i]);
    let selected_seed = edges.iter().find(|e| e.key == reference.keys[0]).unwrap();
    let upper_seed = edges.iter().find(|e| e.key == upper_keys[0]).unwrap();
    let reverse = (dot(tangent(selected_seed), tangent(upper_seed)) < 0.) ^ reference.reversed;
    let upper = edge_chain::resolve(edges, &upper_keys, reverse)?;
    if upper.closed != selected_closed {
        return Err("The selected chamfer does not resolve to one matching upper boundary.".into());
    }
    let path = upper
        .points
        .iter()
        .map(|p| {
            let d = [
                p[0] - setup.wcs.origin.x,
                p[1] - setup.wcs.origin.y,
                p[2] - setup.wcs.origin.z,
            ];
            Point2Dto::new(dot(d, setup.wcs.x_axis), dot(d, setup.wcs.y_axis))
        })
        .collect::<Vec<_>>();
    let tool_left = outward.get(&upper_keys[0]).map(|n| {
        let t = tangent(upper_seed).map(|v| if reverse { -v } else { v });
        dot(t, setup.wcs.x_axis) * dot(*n, setup.wcs.y_axis)
            - dot(t, setup.wcs.y_axis) * dot(*n, setup.wcs.x_axis)
            > 0.
    });
    let wall_side = if upper.closed {
        let area = (0..path.len())
            .map(|i| {
                let a = path[i];
                let b = path[(i + 1) % path.len()];
                a.x * b.y - a.y * b.x
            })
            .sum::<f64>();
        if area.abs() < TOL * TOL {
            return Err("The modeled chamfer upper boundary has zero area.".into());
        }
        let inside = if let Some(left) = tool_left {
            (area > 0.) != left
        } else {
            cone_material_inside.ok_or("Cannot establish the modeled chamfer material side.")?
        };
        if inside {
            ContourCompensation::Inside
        } else {
            ContourCompensation::Outside
        }
    } else {
        if tool_left
            .ok_or("Open conical chamfers are not supported; use a complete circular rim.")?
        {
            ContourCompensation::Right
        } else {
            ContourCompensation::Left
        }
    };
    Ok(CamChamferGeometry {
        path,
        closed: upper.closed,
        top_z: top,
        width: top - bottom,
        wall_side,
        selected_z,
        corner_transitions: false,
    })
}
