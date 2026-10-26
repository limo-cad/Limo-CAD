//! A sketch edit presents its creation-time inputs without changing the saved
//! history cursor, evaluated document, visibility, or assembly ownership.
use super::*;
use limo_cad_core::FeatureKind;
use limo_cad_project_file::ProjectSnapshot;
use serde_json::{json, Value};

pub(super) struct SketchHistoryView {
    name: String,
    pub(super) frame: NativeViewportFrame,
}

fn value(response: String) -> Result<Value, String> {
    let response: Value = serde_json::from_str(&response).map_err(|error| error.to_string())?;
    if response["ok"] != true {
        return Err(response["error"]
            .as_str()
            .unwrap_or("Cannot prepare the sketch history view")
            .into());
    }
    Ok(response["value"].clone())
}

pub(super) fn prepare(
    source: &NativeEngine,
    session_id: &str,
    payload: &str,
) -> Result<Option<SketchHistoryView>, String> {
    // Invalid requests and attempts to replace an active sketch keep the normal
    // command's validation and cannot retire an already presented history view.
    if source.manager.has_active_sketch() {
        return Ok(None);
    }
    let request: Value = match serde_json::from_str(payload) {
        Ok(request) => request,
        Err(_) => return Ok(None),
    };
    let Some(name) = request.as_str().or_else(|| request["name"].as_str()) else {
        return Ok(None);
    };
    let features = source.manager.document().features();
    let Some(index) = features
        .features
        .iter()
        .position(|feature| feature.kind == FeatureKind::Sketch && feature.name == name)
    else {
        return Ok(None);
    };
    let snapshot = source
        .manager
        .export_project_snapshot()
        .map_err(|error| error.to_string())?;
    let mut model: Value =
        serde_json::from_str(snapshot.model_json()).map_err(|error| error.to_string())?;
    model["document"]["history"]["rollback_index"] = json!(index);
    let snapshot = ProjectSnapshot::new(model.to_string(), snapshot.intents().to_vec())
        .map_err(|error| error.to_string())?;
    let stage = NativeEngineHost::new();
    if session_id != BOOTSTRAP_SESSION_ID {
        value(stage.bind_project_session(session_id))?;
    }
    value(stage.project_load_snapshot(&snapshot))?;
    let frame = stage.viewport_frame();
    Ok(Some(SketchHistoryView {
        name: name.into(),
        frame,
    }))
}

pub(super) fn filter_metadata(manager: &SketchManager, document: &mut NativeViewportDocument) {
    let features = manager.document().features();
    let active = features
        .features
        .iter()
        .take(features.rollback_index)
        .filter(|feature| !feature.suppressed)
        .map(|feature| feature.id)
        .collect::<BTreeSet<_>>();
    document.finished_sketches.retain(|sketch| {
        features.features.iter().any(|feature| {
            feature.kind == FeatureKind::Sketch
                && feature.name == sketch.name
                && active.contains(&feature.id)
        })
    });
    document
        .datum_planes
        .retain(|plane| active.contains(&plane.feature_id));
}

pub(super) fn placement(source: &NativeEngine) -> Option<CachedPlacement> {
    let history = source.sketch_history_view.as_ref()?;
    // Geometry stays at the sketch's input stage, while placement and visibility
    // follow the live assembly and Browser choices throughout the edit.
    let mut solution = source
        .manager
        .assembly_document_ref()
        .solve(history.frame.document.scene.as_ref());
    let hidden = source
        .manager
        .project_visibility()
        .hidden_body_ids
        .into_iter()
        .collect::<BTreeSet<_>>();
    for instance in &mut solution.instance_body_poses {
        instance.visible &= !hidden.contains(&instance.body_id.0);
    }
    Some(CachedPlacement {
        body_poses: Arc::new(solution.body_poses),
        instance_body_poses: Arc::new(solution.instance_body_poses),
    })
}

pub(super) fn document(source: &NativeEngine) -> Option<NativeViewportDocument> {
    let history = source.sketch_history_view.as_ref()?;
    let sketch = source.manager.active_snapshot()?;
    if sketch.name != history.name {
        return None;
    }
    let mut document = history.frame.document.as_ref().clone();
    document.metadata_revision = source.viewport_revision;
    document.active_sketch = Some(sketch);
    // Active profiles and dimensions follow each real sketch edit and Undo/Redo.
    document.profile_catalog = source
        .manager
        .profile_catalog()
        .into_iter()
        .filter(|profile| {
            profile.sketch_name == history.name
                || document
                    .finished_sketches
                    .iter()
                    .any(|sketch| sketch.name == profile.sketch_name)
        })
        .collect();
    Some(document)
}

#[cfg(test)]
#[path = "sketch_history/tests.rs"]
mod tests;
