//! Independent diagnostic for captured fixed-axis flat-cutter roughing.
//! Clips the original target triangles above the lowest tip Z, then checks
//! whole line capsules / bounded arc capsules against their XY projection.
//! This deliberately overestimates sweeps: an unproven move is not necessarily
//! a collision. No planner raster, stock voxels or tessellation smoothing is
//! used. This certifies only the supplied mesh, not the exact B-rep or machine.
use limo_cad_cam::{
    CamArcPlane, CamCommandDto, CamDocumentDto, CamOperationDto, CamProgramDto, CamToolKind,
    Point3Dto,
};
use std::collections::HashMap;

type P = [f64; 2];
const EPS: f64 = 1e-9;
const ARC_SAGITTA_MM: f64 = 1e-5;
fn xy(p: Point3Dto) -> P {
    [p.x, p.y]
}
fn cross(a: P, b: P, c: P) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn point_segment(p: P, a: P, b: P) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let d2 = d[0] * d[0] + d[1] * d[1];
    let t = if d2 <= EPS * EPS {
        0.0
    } else {
        ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / d2
    }
    .clamp(0.0, 1.0);
    (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
}
fn segment_distance(a: P, b: P, c: P, d: P) -> f64 {
    if cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0 {
        return 0.0;
    }
    point_segment(a, c, d)
        .min(point_segment(b, c, d))
        .min(point_segment(c, a, b))
        .min(point_segment(d, a, b))
}
fn segment_polygon(a: P, b: P, poly: &[P]) -> f64 {
    let edges = || {
        poly.iter()
            .copied()
            .zip(poly.iter().copied().cycle().skip(1))
            .take(poly.len())
    };
    let area: f64 = edges().map(|(p, q)| p[0] * q[1] - p[1] * q[0]).sum();
    if area.abs() > EPS
        && [a, b]
            .iter()
            .any(|&p| edges().all(|(u, v)| cross(u, v, p) * area >= -EPS))
    {
        return 0.0;
    }
    edges()
        .map(|(c, d)| segment_distance(a, b, c, d))
        .fold(f64::INFINITY, f64::min)
}
fn clip_above(tri: &[Point3Dto; 3], z: f64) -> Vec<P> {
    if tri.iter().all(|p| p.z <= z + EPS) {
        return vec![];
    }
    let mut out = Vec::new();
    let mut a = tri[2];
    for &b in tri {
        if (a.z >= z) != (b.z >= z) {
            let t = (z - a.z) / (b.z - a.z);
            out.push([a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t]);
        }
        if b.z >= z {
            out.push(xy(b));
        }
        a = b;
    }
    out
}

type ArcCapsule = (P, P, f64);

/// Cover the complete finite arc by chord capsules enlarged by the exact
/// maximum sagitta. This is a continuous bound, not a point-sample verdict.
/// It remains independent of both the planner's hull and its arc-distance
/// helpers. The radius disagreement term also encloses tiny numeric drift.
fn arc_capsules(
    from: Point3Dto,
    to: Point3Dto,
    center: Point3Dto,
    clockwise: bool,
) -> Result<Vec<ArcCapsule>, Box<dyn std::error::Error>> {
    use std::f64::consts::TAU;
    let q = (from.x - center.x).hypot(from.y - center.y);
    if q <= EPS {
        return Err("Zero arc radius in target audit".into());
    }
    let start = (from.y - center.y).atan2(from.x - center.x);
    let end = (to.y - center.y).atan2(to.x - center.x);
    let sign = if clockwise { -1.0 } else { 1.0 };
    let mut sweep = (sign * (end - start)).rem_euclid(TAU);
    if sweep < EPS {
        sweep = TAU;
    }
    let angle_step =
        (2.0 * (1.0 - ARC_SAGITTA_MM / q).clamp(-1.0, 1.0).acos()).min(std::f64::consts::FRAC_PI_2);
    let steps = (sweep / angle_step).ceil().max(1.0) as usize;
    if steps > 100_000 {
        return Err("Arc capsule audit exceeds its subdivision budget".into());
    }
    let inflation = q * (1.0 - (sweep / steps as f64 * 0.5).cos())
        + ((to.x - center.x).hypot(to.y - center.y) - q).abs()
        + EPS;
    let point = |i: usize| {
        if i == 0 {
            return xy(from);
        }
        if i == steps {
            return xy(to);
        }
        let a = start + sign * sweep * i as f64 / steps as f64;
        [center.x + q * a.cos(), center.y + q * a.sin()]
    };
    Ok((0..steps)
        .map(|i| (point(i), point(i + 1), inflation))
        .collect())
}

pub fn audit(
    doc: &CamDocumentDto,
    program: &CamProgramDto,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let setup = doc.setup(program.setup_id).ok_or("No setup")?;
    if setup.work_offset_count != 1 {
        return Err("Audit requires one work offset".into());
    }
    let mut results = Vec::new();
    for op in &setup.operations {
        if !op.enabled() {
            continue;
        }
        let CamOperationDto::Adaptive3d {
            id,
            tool_id,
            geometry: Some(geometry),
            parameters,
            ..
        } = op
        else {
            continue;
        };
        if geometry.targets.is_empty() {
            return Err("Audit requires captured target triangles".into());
        }
        let tool = doc
            .tools
            .iter()
            .find(|t| t.id == *tool_id)
            .ok_or("No tool")?;
        if tool.kind != CamToolKind::FlatEndMill {
            return Err("Audit supports flat end mills only".into());
        }
        let mut triangles = Vec::new();
        for mesh in &geometry.targets {
            let points = mesh
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| {
                    let d = [
                        p[0] - setup.wcs.origin.x,
                        p[1] - setup.wcs.origin.y,
                        p[2] - setup.wcs.origin.z,
                    ];
                    let dot = |v: [f64; 3]| d[0] * v[0] + d[1] * v[1] + d[2] * v[2];
                    Point3Dto::new(
                        dot(setup.wcs.x_axis),
                        dot(setup.wcs.y_axis),
                        dot(setup.wcs.z_axis),
                    )
                })
                .collect::<Vec<_>>();
            for tri in mesh.indices.as_chunks::<3>().0 {
                triangles.push([
                    points[tri[0] as usize],
                    points[tri[1] as usize],
                    points[tri[2] as usize],
                ]);
            }
        }
        let mut projections: HashMap<u64, Vec<Vec<P>>> = HashMap::new();
        let mut position = None;
        let mut active = false;
        let mut checked = 0;
        let mut min_clearance = f64::INFINITY;
        let mut unproven = Vec::new();
        for (index, command) in program.commands.iter().enumerate() {
            match command {
                CamCommandDto::SectionStart { operation_id, .. } => active = *operation_id == *id,
                CamCommandDto::SectionEnd => active = false,
                CamCommandDto::CutterCompensationOn { .. } if active => {
                    return Err("Audit cannot assume compensated motion".into())
                }
                _ => {}
            }
            let Some(to) = command.endpoint() else {
                continue;
            };
            if let Some(from) = position.filter(|_| active) {
                let from: Point3Dto = from;
                let z = from.z.min(to.z);
                let polys = projections.entry(z.to_bits()).or_insert_with(|| {
                    triangles
                        .iter()
                        .map(|t| clip_above(t, z))
                        .filter(|p| !p.is_empty())
                        .collect()
                });
                let radius = tool.diameter * 0.5;
                let capsule_gap = |a: P, b: P, r: f64| {
                    polys
                        .iter()
                        .map(|p| segment_polygon(a, b, p) - r)
                        .fold(f64::INFINITY, f64::min)
                };
                let gap = match command {
                    CamCommandDto::Circular {
                        plane: CamArcPlane::Xy,
                        center,
                        clockwise,
                        ..
                    } => {
                        let q = (from.x - center.x)
                            .hypot(from.y - center.y)
                            .max((to.x - center.x).hypot(to.y - center.y));
                        let enclosing = capsule_gap(xy(*center), xy(*center), radius + q);
                        if enclosing >= parameters.radial_stock_to_leave {
                            enclosing
                        } else {
                            arc_capsules(from, to, *center, *clockwise)?
                                .into_iter()
                                .map(|(a, b, inflation)| capsule_gap(a, b, radius + inflation))
                                .fold(f64::INFINITY, f64::min)
                        }
                    }
                    CamCommandDto::Rapid { .. } | CamCommandDto::Linear { .. } => {
                        capsule_gap(xy(from), xy(to), radius)
                    }
                    _ => return Err("Unsupported move in diagnostic roughing audit".into()),
                };
                min_clearance = min_clearance.min(gap);
                checked += 1;
                if gap < parameters.radial_stock_to_leave - EPS {
                    unproven.push(index);
                }
            }
            position = Some(to);
        }
        if checked == 0 {
            return Err("No roughing moves were audited".into());
        }
        results.push(serde_json::json!({"operation_id":id,"moves_checked":checked,
            "minimum_enclosing_sweep_clearance_mm":min_clearance,
            "requested_radial_allowance_mm":parameters.radial_stock_to_leave,
            "unproven_move_count":unproven.len(),"first_unproven_commands":unproven.into_iter().take(10).collect::<Vec<_>>(),
            "scope":"captured target mesh; flat tool; sagitta-enlarged arc capsules / full line capsule; no fixtures, holder, machine or exact B-rep"}));
    }
    if results.is_empty() {
        return Err("No enabled roughing operation with captured geometry".into());
    }
    Ok(serde_json::json!(results))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_segments_not_only_endpoints() {
        let p = [[0.0, 0.0], [4.0, 0.0], [0.0, 4.0]];
        assert_eq!(segment_polygon([-1.0, 1.0], [5.0, 1.0], &p), 0.0);
        assert_eq!(segment_polygon([1.0, 1.0], [1.0, 1.0], &p), 0.0);
        assert!((segment_polygon([-2.0, -1.0], [-2.0, 5.0], &p) - 2.0).abs() < EPS);
        let vertical = [[0.0, 0.0], [0.0, 4.0], [0.0, 2.0]];
        assert!((segment_polygon([2.0, 1.0], [2.0, 3.0], &vertical) - 2.0).abs() < EPS);
    }
    #[test]
    fn sloped_triangle_clip_retains_intersections() {
        let t = [
            Point3Dto::new(0.0, 0.0, 0.0),
            Point3Dto::new(2.0, 0.0, 2.0),
            Point3Dto::new(0.0, 2.0, 2.0),
        ];
        assert_eq!(clip_above(&t, 2.0).len(), 0);
        let p = clip_above(&t, 1.0);
        assert_eq!(p.len(), 4);
        assert!(
            (segment_polygon([0.0, 0.0], [0.0, 0.0], &p) - std::f64::consts::FRAC_1_SQRT_2).abs()
                < EPS
        );
    }
    #[test]
    fn arc_capsules_cover_between_station_positions() {
        for clockwise in [true, false] {
            let sign = if clockwise { -1.0 } else { 1.0 };
            let capsules = arc_capsules(
                Point3Dto::new(10.0, 0.0, -1.0),
                Point3Dto::new(0.0, sign * 10.0, -1.0),
                Point3Dto::new(0.0, 0.0, -1.0),
                clockwise,
            )
            .unwrap();
            for (a, b, margin) in capsules {
                let angle = ((a[1].atan2(a[0])) + (b[1].atan2(b[0]))) * 0.5;
                let p = [10.0 * angle.cos(), 10.0 * angle.sin()];
                assert!(point_segment(p, a, b) <= margin + EPS);
                assert!(margin <= ARC_SAGITTA_MM + EPS * 2.0);
            }
        }
    }
}
