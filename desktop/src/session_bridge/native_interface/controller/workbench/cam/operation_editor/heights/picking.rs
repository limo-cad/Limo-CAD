//! Independent height picks retain topology identity in the operation draft.
use super::*;
use limo_cad_cam::CamHeightGeometryDto;
use limo_cad_sketch::EntityDto;
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

const MAX_POINTS: usize = 4096;
const MAX_VISITS: usize = 131_072;
const MAX_TEXT: usize = 1024 * 1024;

#[derive(Clone)]
pub(in super::super::super) struct Source {
    pub setup: CamSetupDto,
    pub scene: Arc<SolidSceneDto>,
    pub sketches: Arc<[SketchDto]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super::super) struct SelectionState {
    pub selection: Selection,
    pub row: &'static str,
    kind: String,
    epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super::super) struct Key {
    pub reference: String,
    level: u64,
}

pub(in super::super::super) struct Candidate {
    pub key: Key,
    pub point: [f64; 3],
}

pub(in super::super::super) fn button(row: &str) -> String {
    format!("{PREFIX}{row}/pick")
}

pub(in super::super::super) fn is_button(path: &str) -> bool {
    ROWS.iter().any(|(row, _, _)| path == button(row))
}

pub(in super::super::super) fn snapshot(
    draft: &Draft,
    path: &str,
) -> Result<SelectionState, String> {
    let row = ROWS
        .iter()
        .find(|(row, _, _)| path == button(row))
        .map(|(row, _, _)| *row)
        .ok_or("Choose a height geometry picker")?;
    if !matches!(draft.selection, Selection::Operation(_))
        || form::text(draft, "/native/ui/operation_section")? != "heights"
        || !visible(draft, path)
    {
        return Err("Open the operation's picked height reference before picking".into());
    }
    let source = source(draft)?;
    if !source.scene.errors.is_empty() {
        return Err("Resolve model errors before picking heights".into());
    }
    let kind = form::text(draft, &format!("/native/ui/height_kind/{row}"))?;
    if !["face", "edge", "vertex", "sketch_point", "sketch_line"].contains(&kind) {
        return Err("Choose a height geometry type".into());
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (Arc::as_ptr(&source) as usize).hash(&mut hash);
    if draft.fields.len() > 32_768 {
        return Err("Height picking exceeds the form budget".into());
    }
    let mut bytes = 0usize;
    for field in &draft.fields {
        if handles(&field.path) || field.path == "/native/ui/operation_section" {
            bytes = bytes
                .saturating_add(field.path.len())
                .saturating_add(field.text.len());
            if bytes > MAX_TEXT {
                return Err("Height picking exceeds the form text budget".into());
            }
            field.path.hash(&mut hash);
            field.text.hash(&mut hash);
        }
    }
    Ok(SelectionState {
        selection: draft.selection,
        row,
        kind: kind.into(),
        epoch: hash.finish(),
    })
}

pub(in super::super::super) fn source(draft: &Draft) -> Result<Arc<Source>, String> {
    draft
        .operation_edit
        .as_ref()
        .ok_or("Reopen the operation editor")?
        .heights
        .picker_source()
}

pub(in super::super::super) fn candidates(
    source: Arc<Source>,
    expected: &SelectionState,
) -> Result<Vec<Candidate>, String> {
    let mut visits = source
        .scene
        .bodies
        .len()
        .saturating_add(source.sketches.len());
    for body in &source.scene.bodies {
        if source.setup.body_ids.contains(&body.id) {
            match expected.kind.as_str() {
                "face" => visits = visits.saturating_add(body.faces.len()),
                "edge" | "vertex" => {
                    visits = visits.saturating_add(body.edges.len());
                    if expected.kind == "edge" {
                        for edge in &body.edges {
                            visits = visits.saturating_add(edge.points.len());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for sketch in source.sketches.iter() {
        visits = visits.saturating_add(sketch.entities.len());
    }
    if visits > MAX_VISITS {
        return Err("Height picking exceeds the geometry budget".into());
    }
    let mut result = Vec::new();
    let mut bytes = 0usize;
    let mut add = |geometry: CamHeightGeometryDto, point: [f64; 3]| -> Result<(), String> {
        if point
            .iter()
            .any(|v| !v.is_finite() || !(*v as f32).is_finite())
        {
            return Ok(());
        }
        let Ok(base) = limo_cad_sketch::resolve_cam_height_geometry(
            &geometry,
            &source.setup,
            &source.scene,
            &source.sketches,
        ) else {
            return Ok(());
        };
        let key = serde_json::to_string(&geometry).map_err(|error| error.to_string())?;
        bytes = bytes.saturating_add(key.len());
        if result.len() >= MAX_POINTS || bytes > MAX_TEXT {
            return Err("Height picking exceeds the candidate budget".into());
        }
        result.push(Candidate {
            key: Key {
                reference: key,
                level: base.to_bits(),
            },
            point,
        });
        Ok(())
    };
    for body in source
        .scene
        .bodies
        .iter()
        .filter(|body| source.setup.body_ids.contains(&body.id))
    {
        if expected.kind == "face" {
            for face in &body.faces {
                if let Some(plane) = &face.plane {
                    let mut point = plane.origin;
                    let start = face.first_index as usize;
                    if let Some(indices) = body
                        .mesh
                        .indices
                        .get(start..start.saturating_add(3))
                        .filter(|_| face.index_count >= 3)
                    {
                        let vertices: Option<Vec<_>> = indices
                            .iter()
                            .map(|index| {
                                let start = (*index as usize).checked_mul(3)?;
                                body.mesh.positions.get(start..start.checked_add(3)?)
                            })
                            .collect();
                        if let Some(vertices) = vertices {
                            point = std::array::from_fn(|axis| {
                                vertices
                                    .iter()
                                    .map(|vertex| f64::from(vertex[axis]))
                                    .sum::<f64>()
                                    / 3.
                            });
                        }
                    }
                    add(
                        CamHeightGeometryDto::Face {
                            body_id: body.id.0,
                            key: face.key.clone(),
                        },
                        point,
                    )?;
                }
            }
        } else if expected.kind == "edge" || expected.kind == "vertex" {
            for edge in &body.edges {
                let (Some(first), Some(last)) = (edge.points.first(), edge.points.last()) else {
                    continue;
                };
                let first = [first.x, first.y, first.z];
                let last = [last.x, last.y, last.z];
                if expected.kind == "edge" {
                    let middle = edge.points[edge.points.len() / 2];
                    add(
                        CamHeightGeometryDto::Edge {
                            body_id: body.id.0,
                            key: edge.key.clone(),
                        },
                        [middle.x, middle.y, middle.z],
                    )?;
                } else if first
                    .iter()
                    .zip(last)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    > 1e-8
                {
                    for (end, point) in [(false, first), (true, last)] {
                        add(
                            CamHeightGeometryDto::Vertex {
                                body_id: body.id.0,
                                key: edge.key.clone(),
                                end,
                            },
                            point,
                        )?;
                    }
                }
            }
        }
    }
    for sketch in source.sketches.iter() {
        let point = |uv: limo_cad_sketch::Vec2| -> [f64; 3] {
            std::array::from_fn(|i| {
                sketch.basis.origin[i] + sketch.basis.u[i] * uv.x + sketch.basis.v[i] * uv.y
            })
        };
        for entity in &sketch.entities {
            match entity {
                EntityDto::Point { id, position, .. } if expected.kind == "sketch_point" => {
                    add(
                        CamHeightGeometryDto::SketchPoint {
                            sketch: sketch.name.clone(),
                            entity_id: id.0,
                        },
                        point(*position),
                    )?;
                }
                EntityDto::Line {
                    id,
                    start,
                    end,
                    consumed: false,
                    ..
                } if expected.kind == "sketch_line" => {
                    let a = point(*start);
                    let b = point(*end);
                    add(
                        CamHeightGeometryDto::SketchLine {
                            sketch: sketch.name.clone(),
                            entity_id: id.0,
                        },
                        std::array::from_fn(|i| a[i] * 0.5 + b[i] * 0.5),
                    )?;
                }
                _ => {}
            }
        }
    }
    if result.is_empty() {
        return Err("There is no geometry of this type in one setup-Z plane".into());
    }
    Ok(result)
}

pub(in super::super::super) fn stage(
    draft: &mut Draft,
    expected: &SelectionState,
    key: &Key,
) -> Result<(), String> {
    if snapshot(draft, &button(expected.row))? != *expected {
        return Err("Height draft changed; start picking again".into());
    }
    let geometry: CamHeightGeometryDto =
        serde_json::from_str(&key.reference).map_err(|error| error.to_string())?;
    if kind(&geometry) != expected.kind {
        return Err("The height geometry type changed".into());
    }
    let base = f64::from_bits(key.level);
    if !base.is_finite() {
        return Err("The picked height is not finite".into());
    }
    let label = label(&geometry);
    draft
        .operation_edit
        .as_mut()
        .ok_or("Reopen the operation editor")?
        .heights
        .stage_geometry(expected.row, geometry, base)?;
    let path = button(expected.row);
    form::set(draft, &path, &key.reference);
    if let Some(field) = draft.fields.iter_mut().find(|field| field.path == path) {
        field.label = format!("Picked height: {label}");
    }
    Ok(())
}

pub(super) fn kind(geometry: &CamHeightGeometryDto) -> &'static str {
    match geometry {
        CamHeightGeometryDto::Face { .. } => "face",
        CamHeightGeometryDto::Edge { .. } => "edge",
        CamHeightGeometryDto::Vertex { .. } => "vertex",
        CamHeightGeometryDto::SketchPoint { .. } => "sketch_point",
        CamHeightGeometryDto::SketchLine { .. } => "sketch_line",
    }
}

pub(in super::super::super) fn label(geometry: &CamHeightGeometryDto) -> String {
    match geometry {
        CamHeightGeometryDto::Face { body_id, key } => format!("Body {body_id} face {key}"),
        CamHeightGeometryDto::Edge { body_id, key } => format!("Body {body_id} edge {key}"),
        CamHeightGeometryDto::Vertex { body_id, key, end } => format!(
            "Body {body_id} edge {key} {}",
            if *end { "end" } else { "start" }
        ),
        CamHeightGeometryDto::SketchPoint { sketch, entity_id } => {
            format!("{sketch} point {entity_id}")
        }
        CamHeightGeometryDto::SketchLine { sketch, entity_id } => {
            format!("{sketch} line {entity_id}")
        }
    }
}
