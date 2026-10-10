//! Keep exact circular edges separate, including concentric model features.
use super::{super::drawing_paper, Stamp};
use limo_cad_occt::{DrawingProjectedCircleDto, DrawingProjectionDto};
use limo_cad_sketch::{
    drawing_commands::AddRadialDimension, DrawingCircularRefDto, DrawingRadialDimensionMode,
    DrawingViewDto,
};

#[derive(Clone)]
pub(super) struct Target {
    pub view_id: u64,
    pub reference: DrawingCircularRefDto,
    pub center: [f64; 2],
    pub radius: f64,
    pub hidden: bool,
    depth: f64,
}
pub(super) fn targets(
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    direction: [f64; 3],
    mode: DrawingRadialDimensionMode,
) -> Result<Vec<Target>, String> {
    let mut targets = Vec::new();
    for circle in &projection.circles {
        if (circle.hidden && !view.show_hidden_lines)
            || (mode == DrawingRadialDimensionMode::Diameter && !circle.closed)
        {
            continue;
        }
        targets.push(target(view, projection, circle, direction)?);
    }
    targets.sort_by_key(|t| {
        (
            t.reference.occurrence_id.map(|id| id.0),
            t.reference.body_id.0,
            t.reference.edge_id.0,
        )
    });
    Ok(targets)
}
pub(super) fn target(
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    circle: &DrawingProjectedCircleDto,
    direction: [f64; 3],
) -> Result<Target, String> {
    let center = drawing_paper::paper_point(view, circle.center, projection);
    let radius = circle.radius * view.scale;
    let depth = circle
        .center_model
        .iter()
        .zip(direction)
        .map(|(a, b)| a * b)
        .sum::<f64>();
    if center.iter().any(|n| !n.is_finite())
        || !radius.is_finite()
        || radius <= 0.
        || !depth.is_finite()
    {
        return Err("Invalid projected circular edge".into());
    }
    Ok(Target {
        view_id: view.id,
        center,
        radius,
        hidden: circle.hidden,
        depth,
        reference: DrawingCircularRefDto {
            topology_signature: projection
                .topology_signatures
                .get(&circle.body_id.0.to_string())
                .cloned(),
            occurrence_id: circle.occurrence_id,
            body_id: circle.body_id,
            edge_id: circle.edge_id,
            edge_key: circle.edge_key.clone(),
            fallback_center: circle.center_model,
            fallback_normal: circle.normal_model,
            fallback_radius: circle.radius,
            closed: circle.closed,
        },
    })
}
pub(super) fn request(
    stamp: &Stamp,
    target: &Target,
    mode: DrawingRadialDimensionMode,
) -> Result<AddRadialDimension, String> {
    if mode == DrawingRadialDimensionMode::Diameter && !target.reference.closed {
        return Err("Diameter requires a closed circular edge".into());
    }
    Ok(AddRadialDimension {
        sheet_id: stamp.sheet_id,
        view_id: target.view_id,
        feature: target.reference.clone(),
        mode,
        leader_angle_deg: -35.,
        offset: 14.,
        precision: 2,
        prefix: String::new(),
        suffix: String::new(),
        presentation: Default::default(),
    })
}
/// Pick the nearest actual circle perimeter. Rectangular UI bounds must never
/// erase a smaller concentric circle or turn empty corners into edge hits.
pub(super) fn hit(targets: &[Target], point: [f64; 2], tolerance: f64) -> Option<usize> {
    if point.iter().any(|n| !n.is_finite()) || !tolerance.is_finite() || tolerance <= 0. {
        return None;
    }
    targets
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let distance =
                ((point[0] - t.center[0]).hypot(point[1] - t.center[1]) - t.radius).abs();
            (distance <= tolerance).then_some((i, t, distance))
        })
        .min_by(|(ia, a, da), (ib, b, db)| {
            da.total_cmp(db)
                .then(a.hidden.cmp(&b.hidden))
                .then(b.depth.total_cmp(&a.depth))
                .then(ia.cmp(ib))
        })
        .map(|(i, _, _)| i)
}

#[cfg(test)]
mod tests;
