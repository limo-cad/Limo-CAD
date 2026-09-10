//! One authoritative drawing path for definition geometry and placed assemblies.
use super::*;
use nbcad_assembly::{AssemblyDocumentDto, InstanceBodyPoseDto, OccurrenceId};
use nbcad_sketch::{DrawingLineRefDto, DrawingTopologyAnchorRefDto, DrawingViewScope};
use nbcad_solid::BodyDto;

/// Resolve instance selection and project all selected exact shapes together,
/// so one occurrence can hide another. Used by desktop and MCP hosts alike.
pub fn project_drawing(
    kernel: &OcctKernel,
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
    request: &DrawingProjectionRequest,
) -> Result<DrawingProjectionDto, OcctError> {
    if !scene.errors.is_empty() {
        return Err(OcctError(
            "Resolve timeline errors before generating a drawing view".into(),
        ));
    }
    let request = resolve_request(scene, assembly, request)?;
    let mut projection = kernel.drawing_projection(&request)?;
    projection.anchors = drawing_projection_anchors(scene, &request, &projection)?;
    projection.circles = drawing_projection_circles(scene, &request, &projection)?;
    Ok(projection)
}

fn resolve_request(
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
    request: &DrawingProjectionRequest,
) -> Result<DrawingProjectionRequest, OcctError> {
    let mut resolved = request.clone();
    resolved.resolved_occurrences = None;
    let mut body_ids = HashSet::new();
    for id in &request.body_ids {
        if !body_ids.insert(*id) || !scene.bodies.iter().any(|body| body.id == *id) {
            return Err(OcctError(
                "Drawing body selection contains a missing or duplicate id".into(),
            ));
        }
    }
    if request.scope == DrawingViewScope::Definition {
        if !request.occurrence_ids.is_empty() {
            return Err(OcctError(
                "Definition drawing views cannot select assembly occurrences".into(),
            ));
        }
        return Ok(resolved);
    }
    let solution = assembly.solve(scene);
    if !solution.solved {
        return Err(OcctError(
            "Resolve assembly diagnostics before projecting its occurrences".into(),
        ));
    }
    let mut selected = HashSet::new();
    for id in &request.occurrence_ids {
        if id.0 == 0
            || !selected.insert(*id)
            || !assembly
                .component_structure
                .occurrences
                .iter()
                .any(|node| node.id == *id)
        {
            return Err(OcctError(
                "Drawing occurrence selection contains a missing or duplicate id".into(),
            ));
        }
    }
    // A subassembly selection contains every descendant instance, at its
    // current world placement, without copying any retained part geometry.
    loop {
        let before = selected.len();
        for node in &assembly.component_structure.occurrences {
            if node
                .parent_occurrence_id
                .is_some_and(|parent| selected.contains(&parent))
            {
                selected.insert(node.id);
            }
        }
        if selected.len() == before {
            break;
        }
    }
    let poses = solution
        .instance_body_poses
        .into_iter()
        .filter(|pose| {
            pose.visible
                && (request.occurrence_ids.is_empty() || selected.contains(&pose.occurrence_id))
                && (body_ids.is_empty() || body_ids.contains(&pose.body_id))
        })
        .collect::<Vec<_>>();
    if poses.is_empty() {
        return Err(OcctError(
            "The selected assembly drawing contains no visible body occurrences".into(),
        ));
    }
    resolved.resolved_occurrences = Some(poses);
    Ok(resolved)
}

pub(super) fn drawing_bodies<'a>(
    scene: &'a SolidSceneDto,
    request: &DrawingProjectionRequest,
) -> Result<Vec<(&'a BodyDto, Option<InstanceBodyPoseDto>)>, OcctError> {
    if request.scope == DrawingViewScope::Assembly {
        let poses = request.resolved_occurrences.as_ref().ok_or_else(|| {
            OcctError("Assembly drawing requires host-resolved occurrence placements".into())
        })?;
        return poses
            .iter()
            .map(|pose| {
                let body = scene
                    .bodies
                    .iter()
                    .find(|body| body.id == pose.body_id)
                    .ok_or_else(|| {
                        OcctError("Drawing occurrence references a missing body".into())
                    })?;
                Ok((body, Some(*pose)))
            })
            .collect();
    }
    Ok(scene
        .bodies
        .iter()
        .filter(|body| request.body_ids.is_empty() || request.body_ids.contains(&body.id))
        .map(|body| (body, None))
        .collect())
}

pub(super) fn placed_point(
    point: [f64; 3],
    pose: Option<&InstanceBodyPoseDto>,
) -> Result<[f64; 3], OcctError> {
    let Some(pose) = pose else {
        return Ok(point);
    };
    let norm = pose
        .rotation
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite()
        || norm <= 1.0e-12
        || pose.translation.iter().any(|value| !value.is_finite())
    {
        return Err(OcctError(
            "Drawing occurrence placement is not a finite rigid transform".into(),
        ));
    }
    let q = pose.rotation.map(|value| value / norm);
    let twice_cross = scale3(cross([q[0], q[1], q[2]], point), 2.0);
    Ok(add3(
        pose.translation,
        add3(
            point,
            add3(
                scale3(twice_cross, q[3]),
                cross([q[0], q[1], q[2]], twice_cross),
            ),
        ),
    ))
}

fn reference_pose(
    assembly: &AssemblyDocumentDto,
    scene: &SolidSceneDto,
    occurrence: Option<OccurrenceId>,
    body: BodyId,
) -> Result<Option<InstanceBodyPoseDto>, OcctError> {
    let Some(occurrence) = occurrence else {
        return Ok(None);
    };
    let solution = assembly.solve(scene);
    if !solution.solved {
        return Err(OcctError(
            "Drawing reference belongs to an unsolved assembly".into(),
        ));
    }
    solution
        .instance_body_poses
        .into_iter()
        .find(|pose| pose.occurrence_id == occurrence && pose.body_id == body)
        .map(Some)
        .ok_or_else(|| {
            OcctError("Drawing reference occurrence no longer contains its source body".into())
        })
}

/// Current model or assembly-world coordinate for derived cutting planes and
/// detail centers. Stale topology is an error, never a guessed fallback point.
pub fn resolve_drawing_anchor(
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
    reference: &DrawingTopologyAnchorRefDto,
) -> Result<[f64; 3], OcctError> {
    let edge = reference_edge(
        scene,
        reference.body_id,
        reference.edge_id,
        &reference.edge_key,
    )?;
    let point = if reference.circle_center {
        let points = edge
            .points
            .iter()
            .map(|p| [p.x, p.y, p.z])
            .collect::<Vec<_>>();
        fit_circle(&points)
            .map(|circle| circle.0)
            .ok_or_else(|| OcctError("Drawing circle reference is no longer circular".into()))?
    } else {
        let point = match reference.endpoint {
            nbcad_sketch::DrawingEdgeEndpoint::Start => edge.points.first(),
            nbcad_sketch::DrawingEdgeEndpoint::End => edge.points.last(),
        }
        .ok_or_else(|| OcctError("Drawing edge reference has no endpoints".into()))?;
        [point.x, point.y, point.z]
    };
    let pose = reference_pose(assembly, scene, reference.occurrence_id, reference.body_id)?;
    placed_point(point, pose.as_ref())
}

pub fn resolve_drawing_line(
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
    reference: &DrawingLineRefDto,
) -> Result<[[f64; 3]; 2], OcctError> {
    let edge = reference_edge(
        scene,
        reference.body_id,
        reference.edge_id,
        &reference.edge_key,
    )?;
    let first = edge
        .points
        .first()
        .ok_or_else(|| OcctError("Drawing edge reference has no endpoints".into()))?;
    let last = edge
        .points
        .last()
        .ok_or_else(|| OcctError("Drawing edge reference has no endpoints".into()))?;
    let pose = reference_pose(assembly, scene, reference.occurrence_id, reference.body_id)?;
    Ok([
        placed_point([first.x, first.y, first.z], pose.as_ref())?,
        placed_point([last.x, last.y, last.z], pose.as_ref())?,
    ])
}

fn reference_edge<'a>(
    scene: &'a SolidSceneDto,
    body: BodyId,
    edge: EdgeId,
    key: &str,
) -> Result<&'a nbcad_solid::EdgeDto, OcctError> {
    let body = scene
        .bodies
        .iter()
        .find(|value| value.id == body)
        .ok_or_else(|| OcctError("Drawing reference body is missing".into()))?;
    body.edges
        .iter()
        .find(|value| value.id == edge && value.key == key)
        .or_else(|| body.edges.iter().find(|value| value.key == key))
        .ok_or_else(|| OcctError("Drawing reference topology is stale".into()))
}
