//! Conservative drawing association guards. OCCT edge keys are ordinals, not
//! historical names. Preserve dimensions across dimensional edits only while
//! the exact connectivity and final owning feature remain unchanged.
use crate::{DrawingDocumentDto, DrawingSheetDto};
use nbcad_core::BodyId;
use nbcad_solid::{BodyDto, SolidSceneDto};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub fn drawing_body_signature(body: &BodyDto) -> Option<String> {
    (!body.topology_signature.is_empty())
        .then(|| format!("feature:{}:{}", body.feature_id.0, body.topology_signature))
}

pub fn drawing_topology_signatures(scene: &SolidSceneDto) -> BTreeMap<String, String> {
    scene
        .bodies
        .iter()
        .filter_map(|body| {
            drawing_body_signature(body).map(|signature| (body.id.0.to_string(), signature))
        })
        .collect()
}

pub fn validate_drawing_reference_topology(
    scene: &SolidSceneDto,
    body: BodyId,
    captured: Option<&str>,
) -> Result<(), String> {
    let body = scene
        .bodies
        .iter()
        .find(|b| b.id == body)
        .ok_or("Drawing reference body is missing")?;
    match (drawing_body_signature(body), captured) {
        (Some(current), Some(captured)) if current == captured => Ok(()),
        (Some(_), None) => Err("Drawing reference is unverified; explicitly reassociate the legacy annotation or view.".into()),
        (None, None) => Ok(()), // Host geometry without exact native metadata.
        _ => Err("Drawing reference topology changed; explicitly reassociate the affected annotation or view.".into()),
    }
}

fn references(
    value: &mut Value,
    action: &mut impl FnMut(&mut serde_json::Map<String, Value>) -> Result<(), String>,
) -> Result<(), String> {
    match value {
        Value::Array(values) => {
            for value in values {
                references(value, action)?;
            }
        }
        Value::Object(object) => {
            if object.contains_key("body_id")
                && object.contains_key("edge_id")
                && object.contains_key("edge_key")
            {
                action(object)?;
            }
            for value in object.values_mut() {
                references(value, action)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// One canonical setter serves UI and MCP drawing mutations. Existing captured
/// guards are never replaced merely because the current body has changed.
pub fn capture_drawing_topology(
    document: DrawingDocumentDto,
    scene: &SolidSceneDto,
    previous: Option<&DrawingDocumentDto>,
) -> Result<DrawingDocumentDto, String> {
    let signatures = drawing_topology_signatures(scene);
    let mut legacy = BTreeSet::new();
    if let Some(previous) = previous {
        let mut prior = serde_json::to_value(previous).map_err(|e| e.to_string())?;
        references(&mut prior, &mut |reference| {
            if reference
                .get("topology_signature")
                .is_none_or(Value::is_null)
            {
                legacy.insert(serde_json::to_string(reference).map_err(|e| e.to_string())?);
            }
            Ok(())
        })?;
    }
    let mut value = serde_json::to_value(document).map_err(|e| e.to_string())?;
    references(&mut value, &mut |reference| {
        if reference
            .get("topology_signature")
            .is_none_or(Value::is_null)
        {
            // Merely adding a note must not bless old unguarded ordinal refs.
            if legacy.contains(&serde_json::to_string(reference).map_err(|e| e.to_string())?) {
                return Ok(());
            }
            if let Some(signature) = reference
                .get("body_id")
                .and_then(Value::as_u64)
                .and_then(|id| signatures.get(&id.to_string()))
            {
                reference.insert(
                    "topology_signature".into(),
                    Value::String(signature.clone()),
                );
            }
        }
        Ok(())
    })?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

pub fn validate_drawing_topology(
    document: &DrawingDocumentDto,
    scene: &SolidSceneDto,
) -> Result<(), String> {
    let mut value = serde_json::to_value(document).map_err(|e| e.to_string())?;
    validate_references(&mut value, scene)
}

pub fn validate_drawing_sheet_topology(
    sheet: &DrawingSheetDto,
    scene: &SolidSceneDto,
) -> Result<(), String> {
    let mut value = serde_json::to_value(sheet).map_err(|e| e.to_string())?;
    validate_references(&mut value, scene)
}

fn validate_references(value: &mut Value, scene: &SolidSceneDto) -> Result<(), String> {
    references(value, &mut |reference| {
        let body = reference
            .get("body_id")
            .and_then(Value::as_u64)
            .ok_or("Invalid drawing body reference")?;
        validate_drawing_reference_topology(
            scene,
            BodyId(body),
            reference.get("topology_signature").and_then(Value::as_str),
        )
    })
}
