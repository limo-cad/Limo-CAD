//! Read-only measurements of native topology, in canonical millimetres.
use limo_cad_assembly::{BodyPoseDto, InstanceBodyPoseDto};
use limo_cad_core::{BodyId, EdgeId, FaceId};
use limo_cad_solid::SolidSceneDto;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeasurementTarget {
    Body,
    Face { face_id: FaceId },
    Edge { edge_id: EdgeId },
    Vertex { edge_id: EdgeId, end: bool },
    Center { edge_id: EdgeId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasurementSelection {
    pub body_id: BodyId,
    pub occurrence_id: Option<u64>,
    pub target: MeasurementTarget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasurementRequest {
    pub selections: Vec<MeasurementSelection>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MeasurementProperties {
    pub length: Option<f64>,
    pub area: Option<f64>,
    pub loop_length: Option<f64>,
    pub volume: Option<f64>,
    pub radius: Option<f64>,
    pub position: Option<[f64; 3]>,
    pub center: Option<[f64; 3]>,
    pub size: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MeasurementReport {
    pub selections: Vec<MeasurementProperties>,
    pub distance: Option<f64>,
    pub closest_points: Option<[[f64; 3]; 2]>,
    pub angle_degrees: Option<f64>,
    pub center_distance: Option<f64>,
    pub center_points: Option<[[f64; 3]; 2]>,
    pub maximum_distance: Option<f64>,
    pub maximum_points: Option<[[f64; 3]; 2]>,
}

/// Resolved only from a coherent host scene and presentation solution.
#[derive(Debug, Clone)]
pub struct ResolvedMeasurementSelection {
    pub body_id: BodyId,
    pub kind: u8,
    pub index: u32,
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
}

pub fn resolve(
    scene: &SolidSceneDto,
    bodies: &[BodyPoseDto],
    instances: &[InstanceBodyPoseDto],
    request: &MeasurementRequest,
) -> Result<Vec<ResolvedMeasurementSelection>, String> {
    if !(1..=2).contains(&request.selections.len()) {
        return Err("Select one or two objects to measure".into());
    }
    if request.selections.len() == 2 && request.selections[0] == request.selections[1] {
        return Err("Choose two different objects".into());
    }
    request
        .selections
        .iter()
        .map(|selection| {
            let body = scene
                .bodies
                .iter()
                .find(|b| b.id == selection.body_id)
                .ok_or("The measurement body no longer exists")?;
            let (kind, key) = match selection.target {
                MeasurementTarget::Body => (0, None),
                MeasurementTarget::Face { face_id } => (
                    1,
                    Some(
                        body.faces
                            .iter()
                            .find(|f| f.id == face_id)
                            .ok_or("The measurement face no longer exists")?
                            .key
                            .as_str(),
                    ),
                ),
                MeasurementTarget::Edge { edge_id }
                | MeasurementTarget::Vertex { edge_id, .. }
                | MeasurementTarget::Center { edge_id } => {
                    let edge = body
                        .edges
                        .iter()
                        .find(|e| e.id == edge_id)
                        .ok_or("The measurement edge no longer exists")?;
                    let kind = match selection.target {
                        MeasurementTarget::Vertex { end: false, .. } => 3,
                        MeasurementTarget::Vertex { end: true, .. } => 4,
                        MeasurementTarget::Center { .. } => 5,
                        _ => 2,
                    };
                    (kind, Some(edge.key.as_str()))
                }
            };
            let index = key
                .map(|key| {
                    key.strip_prefix(if kind == 1 { "face:" } else { "edge:" })
                        .and_then(|s| s.parse::<u32>().ok())
                        .ok_or("Unsupported measurement topology")
                })
                .transpose()?
                .unwrap_or(0);
            let (translation, rotation) = if let Some(id) = selection.occurrence_id {
                let pose = instances
                    .iter()
                    .find(|p| p.occurrence_id.0 == id && p.body_id == body.id && p.visible)
                    .ok_or("The measurement occurrence is no longer visible")?;
                (pose.translation, pose.rotation)
            } else {
                if !instances.is_empty() {
                    return Err("Choose a visible component occurrence".into());
                }
                bodies
                    .iter()
                    .find(|p| p.body_id == body.id)
                    .map(|p| (p.translation, p.rotation))
                    .unwrap_or(([0.; 3], [0., 0., 0., 1.]))
            };
            if !translation
                .iter()
                .chain(rotation.iter())
                .all(|v| v.is_finite())
                || rotation.iter().map(|v| v * v).sum::<f64>() < 1e-24
            {
                return Err("The measurement placement is invalid".into());
            }
            Ok(ResolvedMeasurementSelection {
                body_id: body.id,
                kind,
                index,
                translation,
                rotation,
            })
        })
        .collect()
}
