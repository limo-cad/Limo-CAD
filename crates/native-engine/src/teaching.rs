//! One fail-closed DTO and validation contract for offline bundles and native playback.
//! No host is constructed and no native measurement is computed by decoding a frame.
use limo_cad_solid::SolidSceneDto;
use serde::{Deserialize, Serialize};
#[path = "teaching/assembly.rs"]
pub mod assembly;
#[path = "teaching/inspection.rs"]
mod inspection;
pub use inspection::{Inspection, Measurement, Selection};
const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub caption: String,
    pub scene: SolidSceneDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Package placement, mutually exclusive with inspection until both use
    /// one authoritative placement for topology, annotations and visibility.
    pub assembly: Option<limo_cad_sketch::AssemblySolutionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<limo_cad_sketch::SketchDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cue: Option<PreviewCue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<PreviewFinish>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspection: Option<Inspection>,
}

/// An opt-in teaching finish; never changes the main viewport's materials.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewFinish {
    SatinAluminum,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewCue {
    Plane,
    Dimension,
    Cut,
    Profile,
    Extrude,
    Depth,
    Apply,
    Body,
    Edges,
    Radius,
}

#[derive(Debug)]
pub struct Bounds {
    pub center: [f32; 3],
    pub radius: f32,
    pub bytes: usize,
}
pub fn validate_frames(frames: &[Frame]) -> Result<Bounds, String> {
    if frames.is_empty() || frames.len() > 16 {
        return Err("A feature preview needs between 1 and 16 frames".into());
    }
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    let mut triangles = 0;
    let mut points = 0;
    let mut bodies = 0;
    let mut faces = 0;
    let mut edges = 0;
    let (mut drawn_bodies, mut drawn_triangles, mut drawn_points, mut drawn_faces, mut drawn_edges) =
        (0, 0, 0, 0, 0);
    for frame in frames {
        if frame.assembly.is_some() && frame.inspection.is_some() {
            return Err("Keep assembly and inspection in separate preview frames".into());
        }
        assembly::validate(frame)?;
        if let Some(inspection) = &frame.inspection {
            inspection.validate(frame)?;
        }
        if frame.edges.len() > 16
            || frame.edges.iter().any(|id| {
                !frame
                    .scene
                    .bodies
                    .iter()
                    .any(|body| body.edges.iter().any(|edge| edge.id.0 == *id))
            })
        {
            return Err("Preview edge selection must reference at most 16 owned edges".into());
        }
        if frame
            .duration_ms
            .is_some_and(|delay| !(100..=5000).contains(&delay))
        {
            return Err("Preview frame timing must be between 100 and 5000 ms".into());
        }
        if let Some(profile) = &frame.profile {
            if profile.entities.len() > 128
                || profile.dimensions.len() > 32
                || !profile.projected_edges.is_empty()
                || !profile.reference_midpoints.is_empty()
            {
                return Err("Preview profiles must be small, independent sketches".into());
            }
            let mut point = |p: limo_cad_sketch::Vec2| -> Result<(), String> {
                let p = profile.basis.to_3d([p.x, p.y]);
                if p.iter().any(|v| !v.is_finite() || v.abs() > 10_000.) {
                    return Err("Invalid preview profile point".into());
                }
                let p = [p[0] as f32, p[1] as f32, p[2] as f32];
                for axis in 0..3 {
                    low[axis] = low[axis].min(p[axis]);
                }
                for axis in 0..3 {
                    high[axis] = high[axis].max(p[axis]);
                }
                Ok(())
            };
            for entity in &profile.entities {
                match entity {
                    limo_cad_sketch::EntityDto::Point { position, .. } => point(*position)?,
                    limo_cad_sketch::EntityDto::Line { start, end, .. } => {
                        point(*start)?;
                        point(*end)?;
                    }
                    limo_cad_sketch::EntityDto::Circle { center, radius, .. }
                    | limo_cad_sketch::EntityDto::Arc { center, radius, .. } => {
                        if !radius.is_finite() || *radius <= 0. || *radius > 10_000. {
                            return Err("Invalid preview circle radius".into());
                        }
                        if let limo_cad_sketch::EntityDto::Arc {
                            start_angle,
                            end_angle,
                            ..
                        } = entity
                        {
                            if !start_angle.is_finite() || !end_angle.is_finite() {
                                return Err("Invalid preview arc angles".into());
                            }
                        }
                        // The enclosing square bounds the entire native circle or arc
                        // on any sketch basis. Drawing keeps the original curve DTO.
                        for (x, y) in [(1., 1.), (-1., 1.), (1., -1.), (-1., -1.)] {
                            point(limo_cad_sketch::Vec2::new(
                                center.x + x * radius,
                                center.y + y * radius,
                            ))?;
                        }
                    }
                    limo_cad_sketch::EntityDto::Spline {
                        points: fit,
                        tessellation,
                        ..
                    } => {
                        if fit.len() < 2
                            || fit.len() > 128
                            || tessellation.len() < 2
                            || tessellation.len() > 4096
                        {
                            return Err("Invalid or unbounded native preview spline".into());
                        }
                        for p in fit.iter().chain(tessellation) {
                            point(*p)?;
                        }
                    }
                }
            }
        }
        if frame.caption.len() > 4000 || !frame.scene.errors.is_empty() {
            return Err("A preview frame has an invalid caption or failed geometry".into());
        }
        for body in &frame.scene.bodies {
            let mesh = &body.mesh;
            bodies += 1;
            faces += body.faces.len();
            edges += body.edges.len();
            triangles += mesh.indices.len() / 3;
            points += mesh.positions.len() / 3
                + body
                    .edges
                    .iter()
                    .map(|edge| edge.points.len())
                    .sum::<usize>();
            if bodies > 256
                || faces > 10_000
                || edges > 10_000
                || triangles > 100_000
                || mesh.positions.len() % 3 != 0
                || mesh.indices.len() < 3
                || points > 300_000
                || mesh.positions.len() < 9
                || mesh.normals.len() != mesh.positions.len()
                || mesh.normals.iter().any(|value| !value.is_finite())
                || mesh.indices.len() % 3 != 0
                || mesh
                    .indices
                    .iter()
                    .any(|index| *index as usize >= mesh.positions.len() / 3)
                || mesh
                    .positions
                    .iter()
                    .any(|value| !value.is_finite() || value.abs() > 10_000.0)
            {
                return Err("This geometry exceeds the bounded feature preview; run the script in a new design".into());
            }

            let mut ranges = body
                .faces
                .iter()
                .map(|face| (face.first_index as usize, face.index_count as usize))
                .collect::<Vec<_>>();
            ranges.sort_unstable();
            let mut previous_end = 0;
            for (start, count) in ranges {
                let end = start
                    .checked_add(count)
                    .ok_or("Invalid preview face range")?;
                if start % 3 != 0
                    || count % 3 != 0
                    || start < previous_end
                    || end > mesh.indices.len()
                {
                    return Err("Invalid preview face range".into());
                }
                previous_end = end;
            }
            if body
                .edges
                .iter()
                .flat_map(|edge| &edge.points)
                .any(|point| {
                    [point.x, point.y, point.z]
                        .iter()
                        .any(|value| !value.is_finite() || value.abs() > 10_000.0)
                })
            {
                return Err("Invalid preview edge points".into());
            }
            for pose in assembly::body_poses(frame.assembly.as_ref(), body.id.0) {
                drawn_bodies += 1;
                drawn_triangles += mesh.indices.len() / 3;
                drawn_faces += body.faces.len();
                drawn_edges += body.edges.len();
                drawn_points += mesh.positions.len() / 3
                    + body
                        .edges
                        .iter()
                        .map(|edge| edge.points.len())
                        .sum::<usize>();
                if drawn_bodies > 256
                    || drawn_triangles > 100_000
                    || drawn_points > 300_000
                    || drawn_faces > 10_000
                    || drawn_edges > 10_000
                {
                    return Err("Assembly instances exceed the bounded feature preview".into());
                }
                let mesh_points = mesh
                    .positions
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|p| [p[0], p[1], p[2]]);
                let edge_points = body
                    .edges
                    .iter()
                    .filter(|_| frame.assembly.is_some())
                    .flat_map(|edge| &edge.points)
                    .map(|p| [p.x as f32, p.y as f32, p.z as f32]);
                for point in mesh_points.chain(edge_points) {
                    let point = pose.transform_point(point);
                    if point.iter().any(|v| !v.is_finite() || v.abs() > 10_000.) {
                        return Err("Invalid transformed preview geometry".into());
                    }
                    for axis in 0..3 {
                        low[axis] = low[axis].min(point[axis]);
                        high[axis] = high[axis].max(point[axis]);
                    }
                }
            }
        }
    }
    let bytes = serde_json::to_vec(&frames)
        .map_err(|error| error.to_string())?
        .len();
    if bytes > MAX_DOCUMENT_BYTES {
        return Err("The feature preview is too large; run the script in a new design".into());
    }
    let (center, radius) = if low.iter().all(|v| v.is_finite()) {
        (
            std::array::from_fn(|i| (low[i] + high[i]) * 0.5),
            (0..3)
                .map(|i| (high[i] - low[i]).powi(2))
                .sum::<f32>()
                .sqrt()
                .max(2.0)
                * 0.5,
        )
    } else {
        ([0.; 3], 1.0)
    };
    Ok(Bounds {
        center,
        radius,
        bytes,
    })
}

#[cfg(test)]
#[path = "teaching/tests.rs"]
mod tests;
