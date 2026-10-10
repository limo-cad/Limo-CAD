//! Pick targets retain exact topology. Coincident endpoint/circle markers use
//! DrawingWorkspace's visibility/depth/radius and stable-ID tie breaking.
use limo_cad_occt::{
    DrawingProjectedCircleDto, DrawingProjectionAnchorDto, DrawingProjectionAnchorEndpoint,
    DrawingProjectionDto,
};
use limo_cad_sketch::{DrawingEdgeEndpoint, DrawingTopologyAnchorRefDto, DrawingViewDto};
use std::{cmp::Ordering, collections::BTreeMap};

fn point_key(point: [f64; 2]) -> Result<[u64; 2], String> {
    let rounded = point.map(|v| (v * 1e6 + 0.5).floor());
    if rounded.iter().any(|v| !v.is_finite()) {
        return Err("Drawing pick target has non-finite coordinates".into());
    }
    Ok(rounded.map(|v| if v == 0. { 0 } else { v.to_bits() }))
}
fn stable(a: &DrawingProjectionAnchorDto, b: &DrawingProjectionAnchorDto) -> Ordering {
    a.body_id
        .0
        .cmp(&b.body_id.0)
        .then(a.edge_id.0.cmp(&b.edge_id.0))
        .then_with(|| {
            (a.endpoint == DrawingProjectionAnchorEndpoint::End)
                .cmp(&(b.endpoint == DrawingProjectionAnchorEndpoint::End))
        })
}
pub(super) fn endpoints<'a>(
    view: &DrawingViewDto,
    projection: &'a DrawingProjectionDto,
    direction: [f64; 3],
) -> Result<Vec<&'a DrawingProjectionAnchorDto>, String> {
    let depth = |a: &DrawingProjectionAnchorDto| {
        a.model_point
            .iter()
            .zip(direction)
            .map(|(a, b)| a * b)
            .sum::<f64>()
    };
    let mut positions: BTreeMap<[u64; 2], usize> = BTreeMap::new();
    let mut targets: Vec<&DrawingProjectionAnchorDto> = Vec::new();
    for anchor in &projection.anchors {
        if anchor.hidden && !view.show_hidden_lines {
            continue;
        }
        let key = point_key(anchor.point)?;
        let z = depth(anchor);
        if !z.is_finite() {
            return Err("Drawing pick target has invalid model depth".into());
        }
        if let Some(&index) = positions.get(&key) {
            let current = targets[index];
            if (current.hidden && !anchor.hidden)
                || (current.hidden == anchor.hidden && z > depth(current) + 1e-7)
                || (current.hidden == anchor.hidden
                    && (z - depth(current)).abs() <= 1e-7
                    && stable(anchor, current).is_lt())
            {
                targets[index] = anchor;
            }
        } else {
            positions.insert(key, targets.len());
            targets.push(anchor);
        }
    }
    targets.sort_by(|a, b| stable(a, b));
    Ok(targets)
}
pub(super) fn circles<'a>(
    view: &DrawingViewDto,
    projection: &'a DrawingProjectionDto,
    direction: [f64; 3],
    closed_only: bool,
) -> Result<Vec<&'a DrawingProjectedCircleDto>, String> {
    let depth = |c: &DrawingProjectedCircleDto| {
        c.center_model
            .iter()
            .zip(direction)
            .map(|(a, b)| a * b)
            .sum::<f64>()
    };
    let mut positions: BTreeMap<[u64; 2], usize> = BTreeMap::new();
    let mut targets: Vec<&DrawingProjectedCircleDto> = Vec::new();
    for circle in &projection.circles {
        if (circle.hidden && !view.show_hidden_lines) || (closed_only && !circle.closed) {
            continue;
        }
        let key = point_key(circle.center)?;
        if !circle.radius.is_finite() || circle.radius <= 0. {
            return Err("Drawing circle target has invalid radius".into());
        }
        let z = depth(circle);
        if !z.is_finite() {
            return Err("Drawing circle target has invalid model depth".into());
        }
        if let Some(&index) = positions.get(&key) {
            let current = targets[index];
            if (current.hidden && !circle.hidden)
                || (current.hidden == circle.hidden && circle.radius > current.radius + 1e-7)
                || (current.hidden == circle.hidden
                    && (circle.radius - current.radius).abs() <= 1e-7
                    && (z > depth(current) + 1e-7
                        || ((z - depth(current)).abs() <= 1e-7
                            && (
                                circle.body_id.0,
                                circle.edge_id.0,
                                circle.occurrence_id.map(|id| id.0),
                            ) < (
                                current.body_id.0,
                                current.edge_id.0,
                                current.occurrence_id.map(|id| id.0),
                            ))))
            {
                targets[index] = circle;
            }
        } else {
            positions.insert(key, targets.len());
            targets.push(circle);
        }
    }
    targets.sort_by(|a, b| {
        a.center[0]
            .total_cmp(&b.center[0])
            .then(a.center[1].total_cmp(&b.center[1]))
            .then(a.body_id.0.cmp(&b.body_id.0))
            .then(a.edge_id.0.cmp(&b.edge_id.0))
    });
    Ok(targets)
}
pub(super) fn endpoint_ref(
    anchor: &DrawingProjectionAnchorDto,
    projection: &DrawingProjectionDto,
) -> DrawingTopologyAnchorRefDto {
    DrawingTopologyAnchorRefDto {
        topology_signature: projection
            .topology_signatures
            .get(&anchor.body_id.0.to_string())
            .cloned(),
        occurrence_id: anchor.occurrence_id,
        body_id: anchor.body_id,
        edge_id: anchor.edge_id,
        edge_key: anchor.edge_key.clone(),
        endpoint: match anchor.endpoint {
            DrawingProjectionAnchorEndpoint::Start => DrawingEdgeEndpoint::Start,
            DrawingProjectionAnchorEndpoint::End => DrawingEdgeEndpoint::End,
        },
        fallback_point: anchor.model_point,
        circle_center: false,
    }
}
pub(super) fn circle_ref(
    circle: &DrawingProjectedCircleDto,
    projection: &DrawingProjectionDto,
) -> DrawingTopologyAnchorRefDto {
    DrawingTopologyAnchorRefDto {
        topology_signature: projection
            .topology_signatures
            .get(&circle.body_id.0.to_string())
            .cloned(),
        occurrence_id: circle.occurrence_id,
        body_id: circle.body_id,
        edge_id: circle.edge_id,
        edge_key: circle.edge_key.clone(),
        endpoint: DrawingEdgeEndpoint::Start,
        fallback_point: circle.center_model,
        circle_center: true,
    }
}
pub(super) fn same_anchor(
    a: &DrawingTopologyAnchorRefDto,
    b: &DrawingTopologyAnchorRefDto,
) -> bool {
    a.occurrence_id == b.occurrence_id
        && a.body_id == b.body_id
        && a.edge_id == b.edge_id
        && a.edge_key == b.edge_key
        && a.circle_center == b.circle_center
        && (a.circle_center || a.endpoint == b.endpoint)
}
