//! Snapshot/export preparation stays on the mutation worker. The render
//! thread only installs an owned, already-built scene and reports its receipt.

use super::*;

#[derive(serde::Deserialize)]
pub(crate) struct NativeVisibility {
    hidden_body_ids: Vec<u64>,
    hidden_datum_plane_ids: Vec<u64>,
    hidden_sketch_names: Vec<String>,
}

#[derive(Resource)]
pub(crate) struct PreparedNativePresentation {
    pub owner: DocumentContext,
    pub revision: u64,
    pub scene: Result<PreparedNativeScene, String>,
    pub publication: Result<Value, String>,
}

pub(crate) enum PreparedNativeScene {
    Model(ViewportModel, NativeVisibility),
    /// Metadata changes retain geometry. The render thread must
    /// still prove that it holds this exact preceding document revision.
    Unchanged {
        from_revision: u64,
    },
}

#[derive(Resource)]
struct RenderedSceneStamp(u64);

pub(crate) fn can_retain_scene(world: &World) -> bool {
    world
        .get_resource::<RenderedSceneStamp>()
        .is_some_and(|stamp| stamp.0 == native_viewport::interface_model_revision(world))
}

pub(crate) fn prepare_native_presentation(
    engine: &AppState,
    bridge: &SessionBridgeState,
    result: &NativeMutationResult,
    operation: &str,
) -> PreparedNativePresentation {
    let scene = bridge.with_native_document_receipt(engine, &result.context, |revision| {
        if revision != result.engine_revision {
            return Err("A newer model revision superseded this scene".into());
        }
        if operation == "drawing_select_sheet" || is_print_intent_edit(operation) {
            let from_revision = revision
                .checked_sub(1)
                .ok_or("Invalid presentation revision")?;
            return Ok(PreparedNativeScene::Unchanged { from_revision });
        }
        let model = model_snapshot(engine);
        let visibility = read_visibility(engine)?;
        Ok(PreparedNativeScene::Model(model, visibility))
    });
    let focus = if scene
        .as_ref()
        .is_ok_and(|scene| matches!(scene, PreparedNativeScene::Model(model, _) if model.document.active_sketch.is_some()))
    {
        "sketch"
    } else {
        "solid"
    };
    let publication = bridge.publish_native_document(engine, &result.context, focus);
    PreparedNativePresentation {
        owner: result.context.clone(),
        revision: result.engine_revision,
        scene,
        publication,
    }
}

pub(crate) fn read_visibility(engine: &AppState) -> Result<NativeVisibility, String> {
    let visibility =
        super::super::parse_engine_envelope(engine.engine_call("project_visibility", ""))?;
    serde_json::from_value(visibility).map_err(|error| error.to_string())
}

/// Install immediate and worker-prepared native frames through the same reducer.
/// Keep rigid placements from this exact model when refreshing visibility.
pub(crate) fn apply_prepared_scene(
    world: &mut World,
    model: ViewportModel,
    visibility: NativeVisibility,
    reset_selection: bool,
) -> Result<Vec<(u64, String)>, String> {
    let rows = model
        .document
        .scene
        .bodies
        .iter()
        .map(|body| (body.id.0, body.name.clone()))
        .collect();
    let (_, _, mut presentation, _) = native_viewport::interface_view_snapshot(world);
    if reset_selection {
        view::clear_selection(&mut presentation);
    }
    use crate::native_viewport::ViewportMode;
    if model.document.active_sketch.is_some() {
        presentation.mode = ViewportMode::Sketch;
    } else if reset_selection || presentation.mode == ViewportMode::Sketch {
        presentation.mode = ViewportMode::Solid;
    }
    presentation.body_poses.clone_from(&model.body_poses);
    presentation
        .instance_body_poses
        .clone_from(&model.instance_body_poses);
    presentation.hidden_body_ids = visibility.hidden_body_ids;
    presentation.hidden_datum_plane_ids = visibility.hidden_datum_plane_ids;
    presentation.hidden_sketch_names = visibility.hidden_sketch_names;
    let session = model.session_id.clone();
    native_viewport::apply_interface_model(world, model)?;
    native_viewport::apply_interface_view(world, &session, None, Some(presentation))?;
    world.insert_resource(RenderedSceneStamp(
        native_viewport::interface_model_revision(world),
    ));
    Ok(rows)
}
