//! Narrow common-prefix deletion transaction. No legacy selection is resolved
//! against the final scene, and no live state changes until both branches prove.
use super::*;
use serde_json::{json, Value};

impl NativeEngineHost {
    pub(super) fn delete_finish_with_survivor_proof(&self, payload: &str) -> Option<String> {
        let request: DeleteFeatureRequest = serde_json::from_str(payload).ok()?;
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        let session = workspace.active_session_id.clone();
        let inner = workspace.active_mut();
        let result = (|| -> Result<Option<SolidUpdateDto>, String> {
            inner
                .manager
                .validate_sketch_session_retention_boundary()
                .map_err(|e| e.to_string())?;
            let snapshot = inner
                .manager
                .export_project_snapshot()
                .map_err(|e| e.to_string())?;
            let mut model: Value =
                serde_json::from_str(snapshot.model_json()).map_err(|e| e.to_string())?;
            let features = model["document"]["history"]["features"]
                .as_array()
                .ok_or("Missing history")?;
            let Some(at) = features
                .iter()
                .position(|f| f["id"] == json!(request.feature_id))
            else {
                return Ok(None);
            };
            let finish = |f: &Value| f["kind"] == "fillet" || f["kind"] == "chamfer";
            if !finish(&features[at]) || !features[at + 1..].iter().any(finish) {
                return Ok(None);
            }
            let finish_body = |id: &Value| {
                ["fillets", "chamfers"].into_iter().find_map(|key| {
                    model[key]
                        .as_array()?
                        .iter()
                        .find(|d| d["feature_id"] == *id)
                        .map(|d| d["body_id"].clone())
                })
            };
            let selected_body = finish_body(&features[at]["id"]);
            if selected_body.is_some()
                && !features[at + 1..]
                    .iter()
                    .any(|f| finish(f) && finish_body(&f["id"]) == selected_body)
            {
                return Ok(None);
            }
            // The prefix is unchanged. Only the final two active features may
            // differ; native correspondence determines prefix eligibility.
            let unsupported = || {
                "Delete remap requires a healthy unchanged prefix and two final active edge finishes, without drawing/assembly/CAM/domain references; other consumers require support reassociation".to_string()
            };
            let feature_count = features.len();
            if at + 2 != feature_count
                || !finish(&features[at + 1])
                || model["document"]["history"]["rollback_index"] != json!(feature_count)
                || features.iter().any(|f| f["suppressed"] != false)
                || !snapshot.intents().is_empty()
            {
                return Err(unsupported());
            }
            let defaults: Value = serde_json::from_str(
                &SketchManager::new()
                    .project_model_projection()
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            for key in ["drawings", "cam"] {
                if model[key] != defaults[key] {
                    return Err(format!("{} ({key})", unsupported()));
                }
            }
            // Every ordinary part has automatically synchronized body-only
            // components. They do not select topology. Preserve them, while
            // rejecting joints, saved motion/contact intent and all other
            // assembly fields unless they are exactly the empty defaults.
            let mut assembly = model["assembly"].clone();
            let mut empty_assembly = defaults["assembly"].clone();
            assembly
                .as_object_mut()
                .ok_or("Missing assembly")?
                .remove("component_structure");
            empty_assembly
                .as_object_mut()
                .ok_or("Missing default assembly")?
                .remove("component_structure");
            if assembly != empty_assembly {
                return Err(format!("{} (assembly)", unsupported()));
            }
            let mut old_manager = SketchManager::new();
            let old_plan = old_manager
                .prepare_load_project(snapshot.model_json().to_owned())
                .map_err(|e| e.to_string())?;
            let mut old_kernel = OcctKernel::new().map_err(|e| e.to_string())?;
            let old_scene = old_kernel.recompute(&old_plan).map_err(|e| e.to_string())?;
            if !old_scene.errors.is_empty() {
                return Err(format!("Delete remap requires healthy original history; repair or reselect the missing/ambiguous supports first: {:?}", old_scene.errors));
            }
            let consumer_name = features[at + 1]["name"]
                .as_str()
                .unwrap_or("unnamed finish")
                .to_owned();
            let (mut kernel, consumer, keys) = old_kernel
                .delete_penultimate_finish(request.feature_id)
                .map_err(|e| {
                    format!("Cannot preserve downstream feature '{consumer_name}': {e}")
                })?;
            for key in ["fillets", "chamfers"] {
                let definitions = model[key].as_array_mut().ok_or("Missing finishes")?;
                definitions.retain(|d| d["feature_id"] != json!(request.feature_id));
                for definition in definitions {
                    if definition["feature_id"] == json!(consumer) {
                        let body: limo_cad_core::BodyId =
                            serde_json::from_value(definition["body_id"].clone())
                                .map_err(|e| e.to_string())?;
                        definition["edge_ids"] = json!(keys
                            .iter()
                            .map(|key| limo_cad_solid::stable_edge_id(body, key))
                            .collect::<Vec<_>>());
                        definition["edge_keys"] = json!(keys);
                    }
                }
            }
            model["document"]["history"]["features"]
                .as_array_mut()
                .unwrap()
                .remove(at);
            model["document"]["history"]["rollback_index"] = json!(feature_count - 1);
            let mut manager = SketchManager::new();
            let plan = manager
                .prepare_load_project(model.to_string())
                .map_err(|e| e.to_string())?;
            if !kernel.completed_jobs_match(&plan) {
                return Err(
                    "Delete remap rewritten intent disagrees with proven native branch".into(),
                );
            }
            let (scene, verified) = kernel
                .recompute_with_supports(&plan, &manager.history_support_queries())
                .map_err(|e| e.to_string())?;
            if !scene.errors.is_empty() {
                return Err("Delete remap branch failed; previous history retained".into());
            }
            let update = manager
                .commit_solid_with_verified_supports(
                    limo_cad_solid::CommitKernelRequest {
                        transaction_id: plan.transaction_id,
                        scene,
                    },
                    &verified,
                )
                .map_err(|e| e.to_string())?;
            manager
                .adopt_matching_sketch_session_retention(&mut inner.manager)
                .map_err(|e| e.to_string())?;
            inner.manager = manager;
            inner.kernel = kernel;
            inner.geometry_proof = None;
            inner.sketch_history_view = None;
            inner.invalidate_viewport();
            inner.geometry_revision = inner.geometry_revision.wrapping_add(1);
            inner.record_geometry_commit(&session);
            Ok(Some(update))
        })();
        match result {
            Ok(None) => None,
            Ok(Some(update)) => Some(ok_json(update)),
            Err(error) => Some(err_json(error)),
        }
    }
}
